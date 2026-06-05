use bevy::prelude::*;

#[derive(Event, Clone, Debug)]
pub struct CastBegan {
    pub caster: Entity,
    pub skill_id: String,
    pub total_duration: f32,
}

#[derive(Event, Clone, Debug)]
pub struct CastRejected {
    pub caster: Entity,
    pub skill_id: String,
    pub reason: CastRejectReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastRejectReason {
    UnknownSkill,
    TimelineMissing,
    InsufficientMana,
    ConditionNotMet,
    OutOfRange,
    NoTarget,
}

#[derive(Event, Clone, Debug)]
pub struct CastPhaseChanged {
    pub caster: Entity,
    pub skill_id: String,
    pub from: crate::timeline::SkillPhase,
    pub to: crate::timeline::SkillPhase,
    pub elapsed: f32,
}

#[derive(Event, Clone, Debug)]
pub struct HitWindowOpened {
    pub caster: Entity,
    pub skill_id: String,
    pub window_id: String,
    pub hitbox: Entity,
}

#[derive(Event, Clone, Debug)]
pub struct HitConfirmed {
    pub caster: Entity,
    pub target: Entity,
    pub skill_id: String,
    pub window_id: String,
}

#[derive(Event, Clone, Debug)]
pub struct DamageResolved {
    pub caster: Entity,
    pub target: Entity,
    pub skill_id: String,
    pub total_damage: f64,
    pub is_killing_blow: bool,
    pub life_after: f64,
    pub mana_spent: f64,
}

#[derive(Event, Clone, Debug)]
pub struct EffectApplied {
    pub target: Entity,
    pub effect_id: String,
    pub total_duration: f64,
    pub stacks: u32,
}

#[derive(Event, Clone, Debug)]
pub struct DotTicked {
    pub target: Entity,
    pub effect_id: String,
    pub dot_damage: f64,
    pub life_remaining: f64,
}

#[derive(Event, Clone, Debug)]
pub struct EffectExpired {
    pub target: Entity,
    pub effect_id: String,
}

#[derive(Event, Clone, Debug)]
pub struct EntityDied {
    pub target: Entity,
    pub killer: Option<Entity>,
}

pub use stat_core::Effect as ObeliskEffect;
