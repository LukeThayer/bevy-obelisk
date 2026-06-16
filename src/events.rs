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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CastRejectReason {
    UnknownSkill,
    TimelineMissing,
    InsufficientMana,
    ConditionNotMet,
    OutOfRange,
    NoTarget,
    NoLineOfSight,
    OnCooldown,
}

#[derive(Event, Clone, Debug)]
pub struct CooldownStarted {
    pub caster: Entity,
    pub skill_id: String,
    pub duration: f32,
}

#[derive(Event, Clone, Debug)]
pub struct CooldownReady {
    pub caster: Entity,
    pub skill_id: String,
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

#[derive(Event, Clone, Debug)]
pub struct LootDropped {
    /// The entity that died and dropped loot.
    pub source: Entity,
    /// The rolled drops (item base types, currencies, uniques).
    pub drops: Vec<tables_core::Drop>,
}

/// Fired whenever an obelisk effect-condition trigger fires (OnApply/OnMaxStacks/OnConsume/
/// OnExpire/OnKill). Emitted unconditionally so triggers are NEVER silently dropped — even
/// when the trigger is not (yet) auto-resolved into a triggered skill packet by the engine.
/// The game can observe this to drive bespoke routing (splash, on-kill targeting, etc.).
#[derive(Event, Clone, Debug)]
pub struct TriggerFired {
    pub source: Entity,
    pub target: Entity,
    pub skill_id: String,
    pub effect_id: String,
}

pub use stat_core::Effect as ObeliskEffect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueKind {
    OnCast,
    OnWindow,
    OnHit,
}

/// A VFX/audio cue fired by a skill at a moment in its timeline. The presentation layer
/// (or game) binds `cue_id` to a handler via `App::observe_cue`.
#[derive(Event, Clone, Debug)]
pub struct CueEvent {
    pub cue_id: String,
    /// The entity the cue is anchored to (caster for OnCast/OnWindow, target for OnHit).
    pub source: Entity,
    /// World position to spawn the effect at.
    pub position: Vec3,
    pub kind: CueKind,
}
