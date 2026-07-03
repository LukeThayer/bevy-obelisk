use crate::assets::{
    AcqFallback, Acquisition, CastTimeline, CastTimelineHandles, CollisionWindow, MotionDirection,
    PhaseDurations, VolumeMotion, WindowAnchor, WindowPhase, WindowSpawn,
};
use crate::combat::system::is_invalid_lifecycle_target;
use crate::core::components::{Attributes, Faction};
use crate::core::config::SkillRegistry;
use crate::core::cooldown::Cooldowns;
use crate::core::spawn_rng::SpawnRng;
use crate::events::{
    CastBegan, CastPhaseChanged, CastRejectReason, CastRejected, CooldownStarted, EndReason,
    HitWindowOpened, HitboxEnded, HitboxWorldHit,
};
use crate::spatial::boxes::{Hitbox, Hurtbox};
use crate::spatial::filter::passes_filter;
use crate::spatial::projectile::Projectile;
use crate::timeline::cast::{charge_mult, CastAim, PendingCast};
use crate::timeline::state::{effective_rate, scale_durations, ActiveCast, SkillPhase};
use crate::timeline::triggered::{execute_skill_timeline, ExecPayload};
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use rand::Rng;
use stat_core::TriggerCondition;

/// Validate pending casts: skill known? timeline loaded? mana/conditions ok? Then insert ActiveCast.
///
/// (Task 10) the authored `CastTargeting` range gate that died with the v1 schema is restored,
/// re-keyed to the timeline's `Acquisition`: `validate_casts` checks the HOST-resolved `CastAim`
/// against it (walking `AcqFallback` chains) and rejects `OutOfRange`/`NoTarget` per
/// [`resolve_acquisition`] when no branch is met. Line-of-sight gating on entity aims is a
/// SEPARATE, unchanged check.
#[allow(clippy::too_many_arguments)]
pub fn validate_casts(
    mut commands: Commands,
    pending: Query<(Entity, &PendingCast)>,
    active: Query<(), With<ActiveCast>>,
    casters: Query<&Attributes>,
    registry: Res<SkillRegistry>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
    factions: Query<&Faction>,
    spatial: SpatialQuery,
    hurtboxes: Query<(Entity, &Hurtbox)>,
    mut cooldowns: ResMut<Cooldowns>,
) {
    for (caster, req) in &pending {
        commands.entity(caster).remove::<PendingCast>();

        if active.get(caster).is_ok() {
            commands.trigger(CastRejected {
                caster,
                skill_id: req.skill_id.clone(),
                reason: CastRejectReason::AlreadyCasting,
            });
            continue;
        }

        let Some(skill) = registry.0.get(&req.skill_id) else {
            commands.trigger(CastRejected {
                caster,
                skill_id: req.skill_id.clone(),
                reason: CastRejectReason::UnknownSkill,
            });
            continue;
        };
        let Some(handle) = handles.0.get(&req.skill_id) else {
            commands.trigger(CastRejected {
                caster,
                skill_id: req.skill_id.clone(),
                reason: CastRejectReason::TimelineMissing,
            });
            continue;
        };
        let Some(timeline) = timelines.get(handle) else {
            commands.trigger(CastRejected {
                caster,
                skill_id: req.skill_id.clone(),
                reason: CastRejectReason::TimelineMissing,
            });
            continue;
        };
        let Ok(attrs) = casters.get(caster) else {
            continue;
        };
        if !attrs.0.can_use_skill(skill) {
            commands.trigger(CastRejected {
                caster,
                skill_id: req.skill_id.clone(),
                reason: CastRejectReason::InsufficientMana,
            });
            continue;
        }
        if !cooldowns.is_ready(caster, &req.skill_id) {
            commands.trigger(CastRejected {
                caster,
                skill_id: req.skill_id.clone(),
                reason: CastRejectReason::OnCooldown,
            });
            continue;
        }

        // Resolve the aim to a facing direction.
        let caster_pos = transforms
            .get(caster)
            .map(|t| t.translation)
            .unwrap_or(Vec3::ZERO);
        let (aim_dir, target) = match req.aim {
            CastAim::Entity(e) => {
                let Ok(tf) = transforms.get(e) else {
                    commands.trigger(CastRejected {
                        caster,
                        skill_id: req.skill_id.clone(),
                        reason: CastRejectReason::NoTarget,
                    });
                    continue;
                };
                let delta = tf.translation - caster_pos;
                (delta.normalize_or_zero(), Some(e))
            }
            CastAim::Point(p) => ((p - caster_pos).normalize_or_zero(), None),
            CastAim::Direction(d) => (d.as_vec3(), None),
        };
        let aim_dir = if aim_dir == Vec3::ZERO {
            Vec3::Z
        } else {
            aim_dir
        };

        // Line-of-sight check for entity-aimed casts (fail-open: no hit = clear).
        if let CastAim::Entity(e) = req.aim {
            if let Ok(tf) = transforms.get(e) {
                let to_target = tf.translation - caster_pos;
                let dist = to_target.length();
                if dist > f32::EPSILON {
                    let dir = Dir3::new(to_target).unwrap_or(Dir3::Z);
                    // Exclude the caster AND its own hurtbox entities: the hurtbox is a CHILD
                    // sensor collider, so excluding only the caster body let the ray hit the
                    // caster's own hurtbox first and self-block every entity-aimed cast.
                    let own_hurtboxes = hurtboxes
                        .iter()
                        .filter(|(_, h)| h.owner == caster)
                        .map(|(e, _)| e);
                    let filter = SpatialQueryFilter::default()
                        .with_excluded_entities(own_hurtboxes.chain([caster]));
                    if let Some(hit) = spatial.cast_ray(caster_pos, dir, dist, true, &filter) {
                        // The target counts whether the ray meets its HURTBOX (owner == e) or
                        // its BODY collider directly (hit.entity == e) — hosts with compound
                        // bodies (arena: Dynamic capsule + child hurtbox sensor) can return
                        // either collider first at the same surface.
                        let hit_is_target = hit.entity == e
                            || hurtboxes.get(hit.entity).map(|(_, h)| h.owner) == Ok(e);
                        if !hit_is_target {
                            commands.trigger(CastRejected {
                                caster,
                                skill_id: req.skill_id.clone(),
                                reason: CastRejectReason::NoLineOfSight,
                            });
                            continue;
                        }
                    }
                }
            }
        }

        // Authored acquisition gate (Task 10): does this resolved aim satisfy the timeline's
        // `Acquisition` (walking `AcqFallback` chains)? Rejects (paid — no `ActiveCast`, no
        // cooldown started, same as every other early reject above) if the chain bottoms out at
        // `Fizzle`; otherwise yields this cast's CAST POINT, if the winning branch produces one.
        let cast_point = match resolve_acquisition(
            &timeline.acquisition,
            req.aim,
            caster_pos,
            &transforms,
            &factions,
            caster,
        ) {
            Ok(cast_point) => cast_point,
            Err(reason) => {
                commands.trigger(CastRejected {
                    caster,
                    skill_id: req.skill_id.clone(),
                    reason,
                });
                continue;
            }
        };

        // Speed-scale the authored phase durations at cast start.
        let rate = effective_rate(&attrs.0, skill);
        let base = (
            timeline.phase_durations.windup,
            timeline.phase_durations.active,
            timeline.phase_durations.recovery,
        );
        let (windup, active, recovery) = scale_durations(base, rate);

        commands.entity(caster).insert(ActiveCast {
            skill_id: req.skill_id.clone(),
            target,
            aim_dir,
            phase: SkillPhase::Windup,
            elapsed: 0.0,
            windup,
            active,
            recovery,
            fired_windows: Vec::new(),
            charge: req.charge,
            muzzle_offset: req.muzzle_offset,
            cast_point,
        });
        commands.trigger(CastBegan {
            caster,
            skill_id: req.skill_id.clone(),
            total_duration: windup + active + recovery,
        });
        let cd = skill.effective_cooldown(attrs.0.cooldown_reduction) as f32;
        if cd > 0.0 {
            cooldowns.start(caster, &req.skill_id, cd);
            commands.trigger(CooldownStarted {
                caster,
                skill_id: req.skill_id.clone(),
                duration: cd,
            });
        }
    }
}

/// Check a HOST-resolved `CastAim` against an authored `Acquisition` (Task 10, spec §3.2),
/// walking `AcqFallback::Then` chains against the SAME `aim` until a branch is met or the chain
/// bottoms out at `Fizzle`. Returns the resulting CAST POINT (`Some` only for `SelfPoint`/a
/// satisfied `GroundPoint` — `Aim`/`HitscanEntity` never produce one in v1) on success, or the
/// rejection reason the LAST attempted branch failed for.
///
/// REJECTION-REASON MAPPING (reuses the existing `CastRejectReason` enum — no new variant): a
/// branch that fails because the aim is the WRONG KIND (e.g. a direction/point aim checked
/// against `HitscanEntity`, or a non-point aim checked against `GroundPoint`) or an `Entity` aim
/// that fails the faction `filter` maps to `NoTarget` ("no valid target of the kind this branch
/// needs"); a branch that fails because a genuine distance check came up short (`HitscanEntity`
/// or `GroundPoint` beyond `range`) maps to `OutOfRange`. Both variants already exist and read
/// honestly for every failure mode this schema can produce, so no new variant was added.
fn resolve_acquisition(
    acq: &Acquisition,
    aim: CastAim,
    caster_pos: Vec3,
    transforms: &Query<&Transform>,
    factions: &Query<&Faction>,
    caster: Entity,
) -> Result<Option<Vec3>, CastRejectReason> {
    match acq {
        Acquisition::Aim => Ok(None),
        Acquisition::SelfPoint => Ok(Some(caster_pos)),
        Acquisition::HitscanEntity {
            range,
            filter,
            fallback,
        } => match check_hitscan_entity(
            aim, *range, *filter, caster_pos, transforms, factions, caster,
        ) {
            Ok(()) => Ok(None),
            Err(reason) => resolve_fallback(
                fallback, aim, caster_pos, transforms, factions, caster, reason,
            ),
        },
        Acquisition::GroundPoint { range, fallback } => {
            match check_ground_point(aim, *range, caster_pos) {
                Ok(point) => Ok(Some(point)),
                Err(reason) => resolve_fallback(
                    fallback, aim, caster_pos, transforms, factions, caster, reason,
                ),
            }
        }
    }
}

/// Apply an `AcqFallback`: `Fizzle` surfaces `reason` (the failure the branch that fell back
/// bottomed out on); `Then` re-checks the SAME `aim` against the next `Acquisition` node.
#[allow(clippy::too_many_arguments)]
fn resolve_fallback(
    fallback: &AcqFallback,
    aim: CastAim,
    caster_pos: Vec3,
    transforms: &Query<&Transform>,
    factions: &Query<&Faction>,
    caster: Entity,
    reason: CastRejectReason,
) -> Result<Option<Vec3>, CastRejectReason> {
    match fallback {
        AcqFallback::Fizzle => Err(reason),
        AcqFallback::Then(next) => {
            resolve_acquisition(next, aim, caster_pos, transforms, factions, caster)
        }
    }
}

/// `HitscanEntity`'s own requirement: `aim` must be `CastAim::Entity`, resolvable (the caller —
/// `validate_casts` — already rejected `NoTarget` upstream for an entity aim whose transform is
/// gone, so `transforms.get` failing here is a defensive `NoTarget`, not a normally-reached
/// path), within `range`, and pass `filter` against the caster's faction.
fn check_hitscan_entity(
    aim: CastAim,
    range: f32,
    filter: crate::assets::HitFilter,
    caster_pos: Vec3,
    transforms: &Query<&Transform>,
    factions: &Query<&Faction>,
    caster: Entity,
) -> Result<(), CastRejectReason> {
    let CastAim::Entity(e) = aim else {
        return Err(CastRejectReason::NoTarget); // wrong aim kind
    };
    let Ok(tf) = transforms.get(e) else {
        return Err(CastRejectReason::NoTarget);
    };
    if tf.translation.distance(caster_pos) > range {
        return Err(CastRejectReason::OutOfRange);
    }
    let caster_faction = factions.get(caster).copied().unwrap_or_default();
    let target_faction = factions.get(e).copied().unwrap_or_default();
    if !passes_filter(filter, caster_faction, target_faction, e == caster) {
        return Err(CastRejectReason::NoTarget);
    }
    Ok(())
}

/// `GroundPoint`'s own requirement: `aim` must be `CastAim::Point`, within `range` of the caster.
fn check_ground_point(
    aim: CastAim,
    range: f32,
    caster_pos: Vec3,
) -> Result<Vec3, CastRejectReason> {
    let CastAim::Point(p) = aim else {
        return Err(CastRejectReason::NoTarget); // wrong aim kind
    };
    if p.distance(caster_pos) > range {
        return Err(CastRejectReason::OutOfRange);
    }
    Ok(p)
}

pub fn advance_casts(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut casts: Query<(Entity, &mut ActiveCast, &Transform)>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
) {
    let dt = time.delta_secs();
    for (caster, mut cast, caster_tf) in &mut casts {
        let prev_phase = cast.phase;
        let prev_elapsed = cast.elapsed;
        cast.elapsed += dt;
        let new_phase = cast.phase_at(cast.elapsed);
        if new_phase != prev_phase {
            cast.phase = new_phase;
            commands.trigger(CastPhaseChanged {
                caster,
                skill_id: cast.skill_id.clone(),
                from: prev_phase,
                to: new_phase,
                elapsed: cast.elapsed,
            });
        }

        // Spawn collision windows whose start time was crossed this tick.
        let Some(handle) = handles.0.get(&cast.skill_id) else {
            continue;
        };
        let Some(timeline) = timelines.get(handle) else {
            continue;
        };
        for win in &timeline.collision_windows {
            if cast.fired_windows.contains(&win.id) {
                continue;
            }
            // Template windows are never on the phase schedule — they exist only to be
            // instantiated by an emitter (Task 11).
            if win.spawn == WindowSpawn::Template {
                continue;
            }
            // The cast's phase durations are already speed-scaled (`ActiveCast::windup` etc. —
            // snapshotted at cast start via `scale_durations`); `window_start_time` just adds
            // the (unscaled) authored `spawn_offset` on top, exactly as this math did inline.
            let effective = PhaseDurations {
                windup: cast.windup,
                active: cast.active,
                recovery: cast.recovery,
            };
            let start = window_start_time(&effective, win);
            if prev_elapsed < start && cast.elapsed >= start {
                cast.fired_windows.push(win.id.clone());
                // Anchor resolution for a player cast (Task 10): `Caster` spawns at the caster's
                // origin + muzzle offset (unchanged); `CastPoint` spawns at the cast's ACQUIRED
                // point (`ActiveCast::cast_point`, resolved once at validation time by
                // `resolve_acquisition` — e.g. a `GroundPoint` cast's aimed position, PRESERVED
                // rather than collapsed to a direction). `cast_point` is `None` only for
                // `Aim`/`HitscanEntity` casts, which `validate_timeline` guarantees never own a
                // `CastPoint`-anchored window — the caster-position fallback below is therefore
                // dead in valid content, kept only as a defensive default. Either way
                // `anchor_offset` applies on top in world axes.
                let anchor_base = match win.anchor {
                    WindowAnchor::Caster => caster_tf.translation + cast.muzzle_offset,
                    WindowAnchor::CastPoint => cast.cast_point.unwrap_or(caster_tf.translation),
                };
                spawn_window_hitbox(
                    &mut commands,
                    win,
                    caster,
                    &cast.skill_id,
                    anchor_base + win.anchor_offset,
                    cast.aim_dir,
                    cast.charge,
                    ChainPayload {
                        // A scheduled beam strikes the cast's entity aim; None (e.g. a
                        // direction-aimed cast of a beam skill) is the paid fizzle.
                        beam_target: cast.target,
                        ..Default::default()
                    },
                );
            }
        }

        // End the cast.
        if cast.phase == SkillPhase::Done {
            commands.entity(caster).remove::<ActiveCast>();
        }
    }
}

/// The elapsed-time at which an authored collision window becomes eligible to spawn, given a
/// cast's phase durations. Pure extraction of `advance_casts`' inline spawn-time math (used
/// there with the cast's SPEED-SCALED effective durations) — also used by the triggered-timeline
/// executor (`advance_triggered_execs`, Task 6), which passes the UNSCALED authored durations: a
/// triggered explosion runs on its own virtual clock and does not inherit the caster's cast/
/// attack speed. Callers must skip `WindowSpawn::Template` windows before calling this (they
/// are never on the phase schedule — they only spawn via an emitter, Task 11).
pub(crate) fn window_start_time(durations: &PhaseDurations, win: &CollisionWindow) -> f32 {
    let WindowSpawn::Scheduled { phase, offset } = win.spawn else {
        debug_assert!(false, "window_start_time called on a Template window");
        return 0.0;
    };
    let base = match phase {
        WindowPhase::Windup => 0.0,
        WindowPhase::Active => durations.windup,
        WindowPhase::Recovery => durations.windup + durations.active,
    };
    base + offset
}

/// The chain payload a spawned window inherits: who to strike (beams), how many hops came
/// before, and which entities the chain has already struck. `hop`/`visited` are populated by
/// `end_hitboxes`' chain-hop arm (Task 12, spec D5), keyed by rules `can_chain`/`chain_count`
/// (the authored `Retarget` reaction that populated them pre-schema-v2 is gone).
#[derive(Default)]
pub(crate) struct ChainPayload {
    pub beam_target: Option<Entity>,
    pub hop: u8,
    pub visited: Vec<Entity>,
    /// Trigger-generation depth the spawned `Hitbox` inherits (0 = a player cast).
    pub depth: u8,
    /// `true` when this spawn comes from an emitter (Task 11) — stamped onto the spawned
    /// `Hitbox.emitted` and `HitWindowOpened.emitted`. Emission is the SAME trigger generation
    /// as the emitting hitbox (this does NOT bump `depth`) — see `tick_emitters`.
    pub emitted: bool,
}

/// Spawn one collision window's `Hitbox` at `origin` facing `dir`, inserting its `Projectile`
/// motion (charge-scaled speed; gravity NOT charge-scaled — a charged lob flies flatter), and
/// fire `HitWindowOpened`. Shared by the phase schedule (`advance_casts`, origin = caster +
/// muzzle + anchor offset), the triggered-timeline executor (`advance_triggered_execs`, origin =
/// the resolved window anchor), and the emitter tick (`tick_emitters`, Task 11 — origin = the
/// emitting hitbox's position + jitter).
///
/// `win.motion_direction` (Task 11) overrides the LAUNCH direction independent of `dir`:
/// `Inherit` (every pre-Task-11 window; byte-identical to the old always-use-`dir` behavior) uses
/// `dir` unchanged; `Down` launches along `-Y` regardless of `dir` — a shard falling out of the
/// sky rather than following the caster's/emitting hitbox's facing. Both the visual rotation and
/// `Hitbox.aim` (cone axis / projectile heading) follow the SAME resolved direction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_window_hitbox(
    commands: &mut Commands,
    win: &CollisionWindow,
    caster: Entity,
    skill_id: &str,
    origin: Vec3,
    dir: Vec3,
    charge: Option<u8>,
    payload: ChainPayload,
) -> Entity {
    let motion_dir = match win.motion_direction {
        MotionDirection::Inherit => dir,
        MotionDirection::Down => Vec3::NEG_Y,
    };
    let emitted = payload.emitted;
    let rot = Quat::from_rotation_arc(Vec3::Z, motion_dir);
    let mut ent = commands.spawn((
        Hitbox {
            caster,
            skill_id: skill_id.to_string(),
            window_id: win.id.clone(),
            filter: win.hit_filter,
            mode: win.hit_mode,
            shape: win.shape,
            aim: motion_dir,
            age: 0.0,
            rehit_interval: win.rehit_interval,
            remaining: win.active_duration,
            hit_log: std::collections::HashMap::new(),
            done: false,
            charge,
            strikes: win.strikes,
            is_beam: matches!(win.motion, VolumeMotion::Beam),
            beam_target: payload.beam_target,
            hop: payload.hop,
            visited: payload.visited,
            depth: payload.depth,
            emit_elapsed: 0.0,
            emitted,
        },
        Transform::from_translation(origin).with_rotation(rot),
    ));
    match win.motion {
        VolumeMotion::Linear { speed } => {
            ent.insert(Projectile {
                velocity: motion_dir * (speed * charge_mult(charge)),
                gravity: 0.0,
            });
        }
        VolumeMotion::Ballistic { speed, gravity } => {
            ent.insert(Projectile {
                velocity: motion_dir * (speed * charge_mult(charge)),
                gravity,
            });
        }
        VolumeMotion::Static | VolumeMotion::Beam => {}
    }
    let hitbox_entity = ent.id();
    commands.trigger(HitWindowOpened {
        caster,
        skill_id: skill_id.to_string(),
        window_id: win.id.clone(),
        hitbox: hitbox_entity,
        emitted,
    });
    hitbox_entity
}

/// Emitter tick (Task 11, spec §3.2). Every live `Hitbox` whose window authors an `Emitter`
/// accumulates elapsed time (`Hitbox.emit_elapsed`) and, each time it crosses a `1/rate` second
/// boundary, spawns ONE instance of the named `Template` window — looping to catch up if
/// multiple boundaries are crossed in one tick (`rate` exceeds the fixed tick rate). The spawn
/// position is the emitting hitbox's CURRENT position plus an xz-disc jitter offset, uniform-area
/// sampled from the dedicated [`SpawnRng`] stream (NEVER [`crate::core::config::CombatRng`] —
/// see `SpawnRng`'s doc): `r = jitter * sqrt(u1)`, `theta = u2 * TAU`, `offset = (r*cos(theta),
/// 0, r*sin(theta))`. The emitted instance inherits the emitting hitbox's `caster`, `skill_id`,
/// `charge`, `aim` (subject to the Template's own `motion_direction` override), and SAME
/// `depth` (emission is not a further trigger generation) — see `ChainPayload::emitted`.
///
/// A hitbox whose window isn't found (stale/unloaded timeline) or has no `emitter` is skipped
/// entirely — `emit_elapsed` stays `0.0` and untouched, so this system is a complete no-op for
/// every pre-Task-11 window (no existing golden is perturbed).
pub fn tick_emitters(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut hitboxes: Query<(&mut Hitbox, &Transform)>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    mut spawn_rng: ResMut<SpawnRng>,
) {
    let dt = time.delta_secs();
    for (mut hb, tf) in &mut hitboxes {
        let Some(handle) = handles.0.get(&hb.skill_id) else {
            continue;
        };
        let Some(timeline) = timelines.get(handle) else {
            continue;
        };
        let Some(win) = timeline
            .collision_windows
            .iter()
            .find(|w| w.id == hb.window_id)
        else {
            continue;
        };
        let Some(em) = &win.emitter else {
            continue;
        };
        // `validate_timeline` guarantees `em.window` names an existing Template window; a
        // missing target here would mean a timeline that skipped validation (e.g. an in-memory
        // test override) — defensively skip rather than panic.
        let Some(target) = timeline
            .collision_windows
            .iter()
            .find(|w| w.id == em.window)
        else {
            continue;
        };
        let period = 1.0 / em.rate;
        hb.emit_elapsed += dt;
        while hb.emit_elapsed >= period {
            hb.emit_elapsed -= period;
            // `r#gen` (final review, item 4): `gen` is reserved in edition 2024 and trips
            // rust-analyzer today; this rand version (0.8) has no `random()` alias for it, so
            // the raw identifier is the clean rename — same method (`Rng::gen`), same draw
            // order, zero behavior change (goldens stay byte-identical).
            let u1: f32 = spawn_rng.0.r#gen::<f32>();
            let u2: f32 = spawn_rng.0.r#gen::<f32>();
            let r = em.jitter * u1.sqrt();
            let theta = u2 * std::f32::consts::TAU;
            let offset = Vec3::new(r * theta.cos(), 0.0, r * theta.sin());
            spawn_window_hitbox(
                &mut commands,
                target,
                hb.caster,
                &hb.skill_id,
                tf.translation + offset,
                hb.aim,
                hb.charge,
                ChainPayload {
                    depth: hb.depth,
                    emitted: true,
                    ..Default::default()
                },
            );
        }
    }
}

/// Marker the [`HitboxWorldHit`] observer stamps on the struck hitbox; consumed by
/// [`end_hitboxes`] as the `HitWorld` end reason (with the impact position).
#[derive(Component, Debug)]
pub struct WorldHit(pub Vec3);

/// Observer for the HOST-fired [`HitboxWorldHit`]: mark the hitbox so the next `end_hitboxes`
/// run terminates it. (An observer + marker rather than direct termination keeps ALL endings
/// in one deterministic funnel with one reason-priority rule.)
pub fn on_hitbox_world_hit(ev: On<HitboxWorldHit>, mut commands: Commands) {
    let e = ev.event();
    commands.entity(e.hitbox).try_insert(WorldHit(e.position));
}

/// The hitbox termination funnel — the ONLY place hitboxes die. Ticks age/remaining, then ends
/// any hitbox that is done (`HitMode::FirstOnly` consumed → `HitEntity`), world-struck
/// (`WorldHit` marker → `HitWorld`), or out of time (`Fuse`) — priority in that order. Ending
/// = chain-hop re-strike (Task 12, below) if this is a `HitEntity` end of a `can_chain` beam
/// with hops remaining, then evaluate the ending skill's Lifecycle conditions (Task 8, below) AT
/// THE END POSITION, fire [`HitboxEnded`], despawn. (Schema v2: the authored `on_end`
/// Chain/Retarget reaction arms are DELETED — end-driven causality now runs entirely through
/// rules triggers; the chain hop below is one such trigger, keyed by rules
/// `can_chain`/`chain_count` rather than an authored reaction.)
///
/// Replaces the old `expire_hitboxes` (whose fuse-despawn was the only death) — a `FirstOnly`
/// hitbox now also ends immediately on its hit instead of lingering inert until the fuse
/// (damage-neutral: `done` already blocked every further hit).
///
/// Task 12 — chain-from-rules (spec D5): this is the deleted `EndReaction::Retarget` arm's
/// home, re-keyed off authored data instead of an authored reaction. A `HitEntity` end of a
/// BEAM hitbox (`hb.is_beam`) whose skill has `damage.can_chain` AND `hb.hop` is still under
/// `damage.chain_count` searches [`nearest_retarget_candidate`] from the strike position, within
/// this timeline's `chain_radius`, excluding everything the chain has already struck
/// (`hb.visited` ∪ this hitbox's own `hit_log`). A hit re-spawns the SAME window (`hb.window_id`
/// looked up on the timeline) aimed at the new victim, `hop + 1`, same `depth` (a chain hop is
/// NOT a deeper trigger generation) — mirroring exactly how the deleted `Retarget` arm worked,
/// modulo the trigger condition. No candidate in radius, hops exhausted, `can_chain` false, or a
/// non-beam ending: the chain just ends (v1 scope is beams only, spec D5 — silent no-op for a
/// `can_chain` non-beam skill).
///
/// Task 8 — lifecycle evaluation: evaluates the ending skill's
/// RULES conditions (`SkillRegistry`, keyed by `skill_id`) whose `TriggerCondition` is
/// `OnImpact`/`OnExpire` — `HitWorld → OnImpact`, `Fuse → OnExpire`, `HitEntity → nothing` (that
/// hit already ran Task 7's hit-phase evaluation in `on_hit_confirmed`). Lifecycle conditions
/// carry no stat context (`eval_condition_obelisk_side` always returns `false` for them during
/// hit resolution) — a direct `TriggerCondition` variant match against the mapped reason IS the
/// evaluation, no `TriggerConditionEval` call. A match executes the named skill's timeline AT
/// THE END POSITION, one trigger-depth deeper; a match with no registered timeline
/// (`is_invalid_lifecycle_target`) warns and is skipped — there's no defending entity to resolve
/// a packet against at a world impact or a fuse-out in empty air, so unlike a hit-phase
/// timeline-target condition there is no packet fallback to fall back to.
#[allow(clippy::too_many_arguments)]
pub fn end_hitboxes(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Hitbox, &Transform, Option<&WorldHit>)>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    registry: Res<SkillRegistry>,
    hurtboxes: Query<(Entity, &Hurtbox)>,
    combatants: Query<(&Transform, &Faction), Without<Hitbox>>,
    factions: Query<&Faction>,
) {
    let dt = time.delta_secs();
    for (e, mut hb, tf, world_hit) in &mut q {
        hb.age += dt;
        hb.remaining -= dt;
        let reason = if hb.done {
            EndReason::HitEntity
        } else if world_hit.is_some() {
            EndReason::HitWorld
        } else if hb.remaining <= 0.0 {
            EndReason::Fuse
        } else {
            continue;
        };
        let position = match (reason, world_hit) {
            (EndReason::HitWorld, Some(w)) => w.0,
            _ => tf.translation,
        };
        // Task 12 — chain-from-rules (spec D5): re-strike the SAME beam window at the nearest
        // unvisited victim within `chain_radius`. See the fn doc above for the full contract.
        if reason == EndReason::HitEntity && hb.is_beam {
            if let Some(skill) = registry.0.get(&hb.skill_id) {
                let dmg = &skill.damage;
                if dmg.can_chain && u32::from(hb.hop) < dmg.chain_count {
                    if let Some(timeline) =
                        handles.0.get(&hb.skill_id).and_then(|h| timelines.get(h))
                    {
                        if let Some(win) = timeline
                            .collision_windows
                            .iter()
                            .find(|w| w.id == hb.window_id)
                        {
                            // Everything this chain has struck so far (earlier hitboxes'
                            // victims + this one's own) is off-limits.
                            let mut struck = hb.visited.clone();
                            struck.extend(hb.hit_log.keys().copied());
                            if let Some((victim, victim_pos)) = nearest_retarget_candidate(
                                position,
                                timeline.chain_radius,
                                win.hit_filter,
                                hb.caster,
                                &struck,
                                &hurtboxes,
                                &combatants,
                                &factions,
                            ) {
                                let dir = (victim_pos - position).normalize_or(hb.aim);
                                spawn_window_hitbox(
                                    &mut commands,
                                    win,
                                    hb.caster,
                                    &hb.skill_id,
                                    position,
                                    dir,
                                    hb.charge,
                                    ChainPayload {
                                        beam_target: Some(victim),
                                        hop: hb.hop + 1,
                                        // The new hitbox's own strike lands in ITS hit_log;
                                        // visited carries everything before it.
                                        visited: struck,
                                        depth: hb.depth,
                                        emitted: false,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        // Task 8 — lifecycle evaluation (spec §3.2): runs for every ending. `HitEntity` maps
        // to nothing — the hit path (`on_hit_confirmed`) already ran Task 7's evaluation for
        // that ending.
        let lifecycle_cond = match reason {
            EndReason::HitWorld => Some(TriggerCondition::OnImpact),
            EndReason::Fuse => Some(TriggerCondition::OnExpire),
            EndReason::HitEntity => None,
        };
        if let Some(target_cond) = lifecycle_cond {
            if let Some(skill) = registry.0.get(&hb.skill_id) {
                for cond in &skill.conditions {
                    if cond.condition != target_cond {
                        continue;
                    }
                    if is_invalid_lifecycle_target(cond, &handles) {
                        warn!(
                            "skill '{}' has a lifecycle condition ({:?}) naming trigger_skill \
                             '{}' with no registered timeline — nothing to execute at this \
                             {:?} end (position {:?})",
                            hb.skill_id, cond.condition, cond.trigger_skill, reason, position
                        );
                        continue;
                    }
                    execute_skill_timeline(
                        &mut commands,
                        hb.caster,
                        &cond.trigger_skill,
                        ExecPayload {
                            position,
                            // No facing context at a world impact / fuse expiry — same
                            // documented placeholder Task 7 uses for `HitConfirmed`-driven
                            // triggers (see `on_hit_confirmed`'s `ExecPayload::direction` doc).
                            direction: Vec3::X,
                            target: None,
                            charge: hb.charge,
                            depth: hb.depth.saturating_add(1),
                        },
                    );
                }
            }
        }
        commands.trigger(HitboxEnded {
            caster: hb.caster,
            skill_id: hb.skill_id.clone(),
            window_id: hb.window_id.clone(),
            position,
            reason,
            charge: hb.charge,
            depth: hb.depth,
        });
        commands.entity(e).despawn();
    }
}

/// The nearest combatant to `from` within `radius` that passes `filter` against the caster's
/// faction and is not in `exclude` — the chain hop's victim. DETERMINISTIC: candidates sort
/// by (squared distance, entity index) so equidistant ties never depend on iteration order.
/// Searches hurtbox OWNERS (the combatant entities), deduped.
///
/// Called from `end_hitboxes`' chain-hop arm (Task 12, spec D5), keyed by rules
/// `can_chain`/`chain_count` rather than the deleted authored `Retarget` reaction.
#[allow(clippy::too_many_arguments)]
fn nearest_retarget_candidate(
    from: Vec3,
    radius: f32,
    filter: crate::assets::HitFilter,
    caster: Entity,
    exclude: &[Entity],
    hurtboxes: &Query<(Entity, &Hurtbox)>,
    combatants: &Query<(&Transform, &crate::core::components::Faction), Without<Hitbox>>,
    factions: &Query<&crate::core::components::Faction>,
) -> Option<(Entity, Vec3)> {
    let caster_faction = factions.get(caster).ok()?;
    let r2 = radius * radius;
    let mut seen = std::collections::HashSet::new();
    let mut best: Option<(f32, Entity, Vec3)> = None;
    for (_, hurtbox) in hurtboxes.iter() {
        let owner = hurtbox.owner;
        if !seen.insert(owner) || exclude.contains(&owner) {
            continue;
        }
        let Ok((tf, faction)) = combatants.get(owner) else {
            continue;
        };
        if !crate::spatial::filter::passes_filter(
            filter,
            *caster_faction,
            *faction,
            owner == caster,
        ) {
            continue;
        }
        let d2 = tf.translation.distance_squared(from);
        if d2 > r2 {
            continue;
        }
        let better = match &best {
            None => true,
            Some((bd2, be, _)) => d2 < *bd2 || (d2 == *bd2 && owner.index() < be.index()),
        };
        if better {
            best = Some((d2, owner, tf.translation));
        }
    }
    best.map(|(_, e, p)| (e, p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{CollisionShape, HitFilter, HitMode, WindowAnchor};

    fn durations() -> PhaseDurations {
        PhaseDurations {
            windup: 0.3,
            active: 0.5,
            recovery: 0.2,
        }
    }

    fn window(phase: WindowPhase, offset: f32) -> CollisionWindow {
        CollisionWindow {
            id: "w".into(),
            spawn: WindowSpawn::Scheduled { phase, offset },
            anchor: WindowAnchor::Caster,
            anchor_offset: Vec3::ZERO,
            strikes: true,
            active_duration: 1.0,
            shape: CollisionShape::Sphere { radius: 0.5 },
            motion: VolumeMotion::Static,
            motion_direction: MotionDirection::Inherit,
            hit_filter: HitFilter::Enemies,
            hit_mode: HitMode::OncePerTarget,
            rehit_interval: None,
            emitter: None,
        }
    }

    #[test]
    fn window_start_time_windup_is_zero_plus_offset() {
        assert_eq!(
            window_start_time(&durations(), &window(WindowPhase::Windup, 0.0)),
            0.0
        );
        assert_eq!(
            window_start_time(&durations(), &window(WindowPhase::Windup, 0.1)),
            0.1
        );
    }

    #[test]
    fn window_start_time_active_starts_after_windup() {
        let d = durations();
        assert_eq!(
            window_start_time(&d, &window(WindowPhase::Active, 0.0)),
            d.windup
        );
        assert_eq!(
            window_start_time(&d, &window(WindowPhase::Active, 0.05)),
            d.windup + 0.05
        );
    }

    #[test]
    fn window_start_time_recovery_starts_after_windup_plus_active() {
        let d = durations();
        assert_eq!(
            window_start_time(&d, &window(WindowPhase::Recovery, 0.0)),
            d.windup + d.active
        );
        assert_eq!(
            window_start_time(&d, &window(WindowPhase::Recovery, 0.25)),
            d.windup + d.active + 0.25
        );
    }
}
