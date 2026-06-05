use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillPhase { Windup, Active, Recovery, Done }

/// Per-cast runtime state. Effective (speed-scaled) durations are snapshotted at cast start.
#[derive(Component, Debug)]
pub struct ActiveCast {
    pub skill_id: String,
    pub target: Entity,
    pub phase: SkillPhase,
    pub elapsed: f32,
    /// Effective phase durations (seconds), already divided by the caster's speed rate.
    pub windup: f32,
    pub active: f32,
    pub recovery: f32,
    /// Window ids already spawned this cast (so we open each once).
    pub fired_windows: Vec<String>,
}

impl ActiveCast {
    pub fn total_duration(&self) -> f32 { self.windup + self.active + self.recovery }
    /// Which phase a given elapsed time falls in.
    pub fn phase_at(&self, t: f32) -> SkillPhase {
        if t < self.windup { SkillPhase::Windup }
        else if t < self.windup + self.active { SkillPhase::Active }
        else if t < self.total_duration() { SkillPhase::Recovery }
        else { SkillPhase::Done }
    }
}
