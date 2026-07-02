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
    hurtboxes: Query<&Hurtbox>,
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
                    let filter = SpatialQueryFilter::default().with_excluded_entities([caster]);
                    if let Some(hit) = spatial.cast_ray(caster_pos, dir, dist, true, &filter) {
                        let hit_is_target = hurtboxes.get(hit.entity).map(|h| h.owner) == Ok(e);
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

/// Spawn one collision window's `Hitbox` at `origin` facing `dir`, inserting its `Projectile`
/// motion (charge-scaled speed; gravity NOT charge-scaled — a charged lob flies flatter), and
/// fire `HitWindowOpened`. Shared by the phase schedule (`advance_casts`, origin = caster +
/// muzzle) and end-reaction chaining (`end_hitboxes`, origin = the parent's END position).
pub(crate) fn spawn_window_hitbox(
    commands: &mut Commands,
    win: &CollisionWindow,
    caster: Entity,
    skill_id: &str,
    origin: Vec3,
    dir: Vec3,
    charge: Option<u8>,
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
        VolumeMotion::Static => {}
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
pub fn end_hitboxes(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Hitbox, &Transform, Option<&WorldHit>)>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
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
        // Authored chain reaction: spawn the named Chained window at the end position.
        if let Some(timeline) = handles.0.get(&hb.skill_id).and_then(|h| timelines.get(h)) {
            let reaction = timeline
                .collision_windows
                .iter()
                .find(|w| w.id == hb.window_id)
                .and_then(|w| w.on_end.for_reason(reason));
            if let Some(EndReaction::Chain(next_id)) = reaction {
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
        });
        commands.entity(e).despawn();
    }
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
