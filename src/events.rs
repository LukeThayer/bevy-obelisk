use bevy::prelude::*;

#[derive(Event, Clone, Debug)]
pub struct CastBegan {
    pub caster: Entity,
    pub skill_id: String,
    pub total_duration: f32,
    /// Optional per-cast charge (see `crate::timeline::cast::charge_mult`). Carried so the
    /// `on_cast` cue (`src/vfx.rs::cue_on_cast`) can scale its cosmetics the same way the cast
    /// itself scaled damage/speed. `None` = uncharged.
    pub charge: Option<u8>,
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
    AlreadyCasting,
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
    /// `true` when this window was spawned BY an emitter (Task 11) rather than the phase
    /// schedule or a triggered execution. `src/vfx.rs::cue_on_window` branches on this to fire
    /// `emit_{window_id}` instead of `on_window_{window_id}` — an emitted instance NEVER fires
    /// the ordinary window-open cue (spec §3.2: emit only).
    pub emitted: bool,
}

#[derive(Event, Clone, Debug)]
pub struct HitConfirmed {
    pub caster: Entity,
    pub target: Entity,
    pub skill_id: String,
    pub window_id: String,
    /// Optional per-cast charge forwarded from the originating `Hitbox`, used by the resolve to
    /// scale damage. `None` = uncharged (1.0x).
    pub charge: Option<u8>,
    /// World position of the hitbox at the moment of the hit (its transform translation).
    pub position: Vec3,
    /// Trigger-generation depth this hit's hitbox was spawned at (0 = a player cast).
    pub depth: u8,
    /// How many retarget hops preceded the hitbox that landed this hit (0 = the initial window).
    pub hop: u8,
    /// `true` when the hitbox that landed this hit was spawned by an emitter (Task 11) — extends
    /// the free-hit billing rule (`src/combat/system.rs::is_free_hit`) so an emitted shard's
    /// hits never bill mana, same as a chain re-strike or triggered sub-cast.
    pub emitted: bool,
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
    /// Whether any packet of this hit was a critical strike (from `DamagePacket.is_critical`).
    pub is_critical: bool,
    /// Total damage prevented by all mitigation on the defender (armour + resists + barrier +
    /// block + physical/generic DR + evasion-cap oneshot protection + elude). Summed across every
    /// `CombatResult` produced by this hit.
    pub damage_prevented: f64,
    /// Life the caster gained from this hit (life-on-hit + life-on-kill leech).
    pub life_gained: f64,
    /// Mana the caster gained from this hit (mana-on-hit + mana-on-kill leech).
    pub mana_gained: f64,
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

/// Why a hitbox terminated. Every hitbox ends exactly once with exactly one reason; when
/// several apply on the same tick the priority is `HitEntity > HitWorld > Fuse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// Terminal entity hit (`HitMode::FirstOnly` after its single confirm).
    HitEntity,
    /// The host reported a world impact (see [`HitboxWorldHit`]).
    HitWorld,
    /// The window's `active_duration` elapsed — the fuse ran out wherever the hitbox was.
    Fuse,
}

/// A hitbox terminated. Fired (with the world position where it happened) for EVERY hitbox by
/// the `end_hitboxes` funnel, which also evaluates the ending skill's lifecycle triggers at
/// that position. This is the event that makes skills physics-reactive: the explosion
/// happens where the bolt actually stopped — enemy, dirt, or mid-air fuse.
#[derive(Event, Clone, Debug)]
pub struct HitboxEnded {
    pub caster: Entity,
    pub skill_id: String,
    pub window_id: String,
    /// World position of the termination (world-impact point, or the hitbox's last transform).
    pub position: Vec3,
    pub reason: EndReason,
    /// The cast's charge, carried so chained damage/cosmetics keep scaling.
    pub charge: Option<u8>,
    /// Trigger-generation depth of the hitbox that ended (0 = a player cast).
    pub depth: u8,
}

/// HOST-fired trigger: `hitbox` struck world geometry at `position`. Obelisk deliberately
/// knows nothing about the world (floors/walls are the host's job, like physics); the host
/// detects the impact and fires this — obelisk's `end_hitboxes` funnel then terminates the
/// hitbox with [`EndReason::HitWorld`] on the next `Advance`.
#[derive(Event, Clone, Debug)]
pub struct HitboxWorldHit {
    pub hitbox: Entity,
    pub position: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueKind {
    OnCast,
    OnWindow,
    OnHit,
    /// A window ended (any [`EndReason`]) — the cue fires AT the end position.
    OnEnd,
    /// An emitter (Task 11) instantiated a `Template` window — fires AT the emitted instance's
    /// spawn position, INSTEAD OF `OnWindow` (spec §3.2: emit only, never the ordinary
    /// window-open cue).
    OnEmit,
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
    /// Second anchor for TWO-POINT cues (a beam window's open cue: `position_from` = the beam's
    /// origin, `position` = the victim). `None` for ordinary single-point cues.
    pub position_from: Option<Vec3>,
    pub kind: CueKind,
    /// The originating cast's charge, forwarded to EVERY cue slot (cast/window/hit/end/emit) so
    /// presentation can scale cosmetics the same way the cast itself scaled damage/speed
    /// (phase-3 prerequisite: `ParamSource::Charge` cue bindings). `None` = uncharged.
    pub charge: Option<u8>,
    /// Set ONLY on `CueKind::OnEnd` cues (from the terminating `HitboxEnded.reason`) so
    /// presentation can react differently to `HitEntity`/`HitWorld`/`Fuse` (phase-3
    /// prerequisite: reason-aware presentation). `None` for every other cue kind.
    pub end_reason: Option<EndReason>,
    /// The originating skill id, so a multi-skill presentation host can resolve the right
    /// timeline's `cues` binding for this cue (cue_id == slot is not unique across skills).
    pub skill_id: String,
}
