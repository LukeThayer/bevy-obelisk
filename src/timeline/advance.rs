use bevy::prelude::*;
use crate::assets::{CastTimeline, CastTimelineHandles};
use crate::core::components::Attributes;
use crate::core::config::SkillRegistry;
use crate::events::{CastBegan, CastRejected, CastRejectReason};
use crate::timeline::cast::PendingCast;
use crate::timeline::state::{effective_rate, scale_durations, ActiveCast, SkillPhase};

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
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::UnknownSkill });
            continue;
        };
        let Some(handle) = handles.0.get(&req.skill_id) else {
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::TimelineMissing });
            continue;
        };
        let Some(timeline) = timelines.get(handle) else {
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::TimelineMissing });
            continue;
        };
        let Ok(attrs) = casters.get(caster) else { continue };
        if !attrs.0.can_use_skill(skill) {
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::InsufficientMana });
            continue;
        }

        // Speed-scale the authored phase durations at cast start.
        let rate = effective_rate(&attrs.0, skill);
        let base = (timeline.phase_durations.windup, timeline.phase_durations.active, timeline.phase_durations.recovery);
        let (windup, active, recovery) = scale_durations(base, rate);

        commands.entity(caster).insert(ActiveCast {
            skill_id: req.skill_id.clone(),
            target: req.target,
            phase: SkillPhase::Windup,
            elapsed: 0.0,
            windup, active, recovery,
            fired_windows: Vec::new(),
        });
        commands.trigger(CastBegan { caster, skill_id: req.skill_id.clone(), total_duration: windup + active + recovery });
    }
}
