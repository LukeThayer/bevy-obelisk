use crate::assets::{CastTimeline, CastTimelineHandles, VolumeMotion, WindowPhase};
use crate::core::components::Attributes;
use crate::core::config::SkillRegistry;
use crate::events::{CastBegan, CastPhaseChanged, CastRejectReason, CastRejected, HitWindowOpened};
use crate::spatial::boxes::Hitbox;
use crate::spatial::projectile::Projectile;
use crate::timeline::cast::PendingCast;
use crate::timeline::state::{effective_rate, scale_durations, ActiveCast, SkillPhase};
use bevy::prelude::*;

/// Validate pending casts: skill known? timeline loaded? mana/conditions ok? Then insert ActiveCast.
pub fn validate_casts(
    mut commands: Commands,
    pending: Query<(Entity, &PendingCast)>,
    casters: Query<&Attributes>,
    registry: Res<SkillRegistry>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
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
            target: req.target,
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
                let dir = Vec3::Z; // replaced by resolved aim in a later task
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
                    Transform::from_translation(caster_tf.translation),
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
        hb.remaining -= dt;
        if hb.remaining <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
