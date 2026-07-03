use crate::assets::{
    CastTargeting, CastTimeline, CastTimelineHandles, CollisionWindow, EndReaction, VolumeMotion,
    WindowPhase,
};
use crate::core::components::Attributes;
use crate::core::config::SkillRegistry;
use crate::core::cooldown::Cooldowns;
use crate::events::{
    CastBegan, CastPhaseChanged, CastRejectReason, CastRejected, CooldownStarted, EndReason,
    HitWindowOpened, HitboxEnded, HitboxWorldHit,
};
use crate::spatial::boxes::{Hitbox, Hurtbox};
use crate::spatial::projectile::Projectile;
use crate::timeline::cast::{charge_mult, CastAim, PendingCast};
use crate::timeline::state::{effective_rate, scale_durations, ActiveCast, SkillPhase};
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;

/// The max cast range for a targeting mode, if it gates on range. `None` = no range gate.
pub fn targeting_range(targeting: &CastTargeting) -> Option<f32> {
    match targeting {
        CastTargeting::SelfCast => None,
        CastTargeting::SingleEntity { range }
        | CastTargeting::Direction { range }
        | CastTargeting::Cone { range, .. } => Some(*range),
    }
}

/// Validate pending casts: skill known? timeline loaded? mana/conditions ok? Then insert ActiveCast.
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

        if let Some(max_range) = targeting_range(&timeline.targeting) {
            let aim_point = match req.aim {
                CastAim::Entity(e) => transforms.get(e).ok().map(|t| t.translation),
                CastAim::Point(p) => Some(p),
                CastAim::Direction(_) => None, // no distance to gate on
            };
            if let Some(point) = aim_point {
                if point.distance(caster_pos) > max_range {
                    commands.trigger(CastRejected {
                        caster,
                        skill_id: req.skill_id.clone(),
                        reason: CastRejectReason::OutOfRange,
                    });
                    continue;
                }
            }
        }

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

pub fn advance_casts(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut casts: Query<(Entity, &mut ActiveCast, &Transform)>,
    registry: Res<SkillRegistry>,
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
            let base = match win.spawn_phase {
                WindowPhase::Windup => 0.0,
                WindowPhase::Active => cast.windup,
                WindowPhase::Recovery => cast.windup + cast.active,
                // Chained windows are never on the phase schedule — they spawn from a parent
                // window's `on_end` (see `end_hitboxes`).
                WindowPhase::Chained => continue,
            };
            let start = base + win.spawn_offset;
            if prev_elapsed < start && cast.elapsed >= start {
                cast.fired_windows.push(win.id.clone());
                spawn_window_hitbox(
                    &mut commands,
                    win,
                    caster,
                    &cast.skill_id,
                    caster_tf.translation + cast.muzzle_offset,
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
        let _ = &registry; // kept for future use_conditions re-checks
    }
}

/// The chain payload a spawned window inherits: who to strike (beams), how many hops came
/// before, and which entities the chain has already struck.
#[derive(Default)]
pub(crate) struct ChainPayload {
    pub beam_target: Option<Entity>,
    pub hop: u8,
    pub visited: Vec<Entity>,
    /// Trigger-generation depth the spawned `Hitbox` inherits (0 = a player cast).
    pub depth: u8,
}

/// Spawn one collision window's `Hitbox` at `origin` facing `dir`, inserting its `Projectile`
/// motion (charge-scaled speed; gravity NOT charge-scaled — a charged lob flies flatter), and
/// fire `HitWindowOpened`. Shared by the phase schedule (`advance_casts`, origin = caster +
/// muzzle) and end reactions (`end_hitboxes`, origin = the parent's END position).
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
    let rot = Quat::from_rotation_arc(Vec3::Z, dir);
    let mut ent = commands.spawn((
        Hitbox {
            caster,
            skill_id: skill_id.to_string(),
            window_id: win.id.clone(),
            filter: win.hit_filter,
            mode: win.hit_mode,
            shape: win.shape,
            aim: dir,
            age: 0.0,
            rehit_interval: win.rehit_interval,
            remaining: win.active_duration,
            hit_log: std::collections::HashMap::new(),
            done: false,
            charge,
            is_beam: matches!(win.motion, VolumeMotion::Beam),
            beam_target: payload.beam_target,
            hop: payload.hop,
            visited: payload.visited,
            depth: payload.depth,
        },
        Transform::from_translation(origin).with_rotation(rot),
    ));
    match win.motion {
        VolumeMotion::Linear { speed } => {
            ent.insert(Projectile {
                velocity: dir * (speed * charge_mult(charge)),
                gravity: 0.0,
            });
        }
        VolumeMotion::Ballistic { speed, gravity } => {
            ent.insert(Projectile {
                velocity: dir * (speed * charge_mult(charge)),
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
    });
    hitbox_entity
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
/// = spawn the window's authored `on_end` reaction AT THE END POSITION (chained windows carry
/// the original caster, aim, and charge), fire [`HitboxEnded`], despawn.
///
/// Replaces the old `expire_hitboxes` (whose fuse-despawn was the only death) — a `FirstOnly`
/// hitbox now also ends immediately on its hit instead of lingering inert until the fuse
/// (damage-neutral: `done` already blocked every further hit).
#[allow(clippy::too_many_arguments)]
pub fn end_hitboxes(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Hitbox, &Transform, Option<&WorldHit>)>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    hurtboxes: Query<(Entity, &Hurtbox)>,
    combatants: Query<(&Transform, &crate::core::components::Faction), Without<Hitbox>>,
    factions: Query<&crate::core::components::Faction>,
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
        // Authored end reaction: spawn at the end position (Chain), or seek the next victim
        // around it (Retarget).
        if let Some(timeline) = handles.0.get(&hb.skill_id).and_then(|h| timelines.get(h)) {
            let reaction = timeline
                .collision_windows
                .iter()
                .find(|w| w.id == hb.window_id)
                .and_then(|w| w.on_end.for_reason(reason));
            match reaction {
                Some(EndReaction::Chain(next_id)) => {
                    if let Some(next) = timeline
                        .collision_windows
                        .iter()
                        .find(|w| &w.id == next_id)
                    {
                        spawn_window_hitbox(
                            &mut commands,
                            next,
                            hb.caster,
                            &hb.skill_id,
                            position,
                            hb.aim,
                            hb.charge,
                            ChainPayload {
                                hop: hb.hop,
                                visited: hb.visited.clone(),
                                depth: hb.depth,
                                ..Default::default()
                            },
                        );
                    }
                }
                Some(EndReaction::Retarget {
                    window,
                    radius,
                    max_hops,
                }) if hb.hop < *max_hops => {
                    if let Some(next) = timeline
                        .collision_windows
                        .iter()
                        .find(|w| &w.id == window)
                    {
                        // Everything this chain has struck so far (earlier hitboxes'
                        // victims + this one's own) is off-limits.
                        let mut struck = hb.visited.clone();
                        struck.extend(hb.hit_log.keys().copied());
                        if let Some((victim, victim_pos)) = nearest_retarget_candidate(
                            position,
                            *radius,
                            next.hit_filter,
                            hb.caster,
                            &struck,
                            &hurtboxes,
                            &combatants,
                            &factions,
                        ) {
                            let dir = (victim_pos - position).normalize_or(hb.aim);
                            spawn_window_hitbox(
                                &mut commands,
                                next,
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
                                },
                            );
                        }
                    }
                }
                // Retarget with hops exhausted, or no reaction: the chain just ends.
                _ => {}
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
/// faction and is not in `exclude` — the retarget hop's victim. DETERMINISTIC: candidates sort
/// by (squared distance, entity index) so equidistant ties never depend on iteration order.
/// Searches hurtbox OWNERS (the combatant entities), deduped.
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
    use crate::assets::CastTargeting;

    #[test]
    fn self_cast_has_no_range_gate() {
        assert_eq!(targeting_range(&CastTargeting::SelfCast), None);
    }
    #[test]
    fn single_entity_range_is_extracted() {
        assert_eq!(
            targeting_range(&CastTargeting::SingleEntity { range: 12.0 }),
            Some(12.0)
        );
    }
    #[test]
    fn cone_range_is_extracted() {
        assert_eq!(
            targeting_range(&CastTargeting::Cone {
                angle: 90.0,
                range: 4.0
            }),
            Some(4.0)
        );
    }
}
