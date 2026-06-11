use crate::assets::{CastTargeting, CastTimeline, CastTimelineHandles, VolumeMotion, WindowPhase};
use crate::core::components::Attributes;
use crate::core::config::SkillRegistry;
use crate::core::cooldown::Cooldowns;
use crate::events::{CastBegan, CastPhaseChanged, CastRejectReason, CastRejected, CooldownStarted, HitWindowOpened};
use crate::spatial::boxes::{Hitbox, Hurtbox};
use crate::spatial::projectile::Projectile;
use crate::timeline::cast::{CastAim, PendingCast};
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
        });
        commands.trigger(CastBegan {
            caster,
            skill_id: req.skill_id.clone(),
            total_duration: windup + active + recovery,
        });
        let cd = skill.effective_cooldown(attrs.0.cooldown_reduction) as f32;
        if cd > 0.0 {
            cooldowns.start(caster, &req.skill_id, cd);
            commands.trigger(CooldownStarted { caster, skill_id: req.skill_id.clone(), duration: cd });
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
            };
            let start = base + win.spawn_offset;
            if prev_elapsed < start && cast.elapsed >= start {
                cast.fired_windows.push(win.id.clone());
                let dir = cast.aim_dir;
                let rot = Quat::from_rotation_arc(Vec3::Z, dir);
                let mut ent = commands.spawn((
                    Hitbox {
                        caster,
                        skill_id: cast.skill_id.clone(),
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
                    },
                    Transform::from_translation(caster_tf.translation).with_rotation(rot),
                ));
                if let VolumeMotion::Linear { speed } = win.motion {
                    ent.insert(Projectile {
                        velocity: dir * speed,
                    });
                }
                let hitbox_entity = ent.id();
                commands.trigger(HitWindowOpened {
                    caster,
                    skill_id: cast.skill_id.clone(),
                    window_id: win.id.clone(),
                    hitbox: hitbox_entity,
                });
            }
        }

        // End the cast.
        if cast.phase == SkillPhase::Done {
            commands.entity(caster).remove::<ActiveCast>();
        }
        let _ = &registry; // kept for future use_conditions re-checks
    }
}

/// Despawn hitboxes whose active window elapsed.
pub fn expire_hitboxes(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Hitbox)>,
) {
    let dt = time.delta_secs();
    for (e, mut hb) in &mut q {
        hb.age += dt;
        hb.remaining -= dt;
        if hb.remaining <= 0.0 {
            commands.entity(e).despawn();
        }
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
