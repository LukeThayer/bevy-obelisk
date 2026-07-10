use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Authored skill timeline, schema v2 (spec D6, §3.2). v1's authored causality — `on_end`
/// `Chain`/`Retarget` reactions, `WindowPhase::Chained` — is DELETED: causality now lives in
/// rules triggers (hit-phase + lifecycle conditions, Tasks 7-8). `CastTargeting` (replaced by
/// acquisition, Task 10) and the never-read `CastDelivery` are gone too. `deny_unknown_fields`
/// makes stale v1 content (and author typos) fail LOUD at load instead of half-parsing.
#[derive(Asset, Reflect, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastTimeline {
    pub skill_id: String,
    pub phase_durations: PhaseDurations,
    #[serde(default)]
    pub collision_windows: Vec<CollisionWindow>,
    /// How a resolved `CastAim` (the HOST's raw-input resolution — verbs unchanged) is checked
    /// and, for point-producing branches, turned into this cast's CAST POINT (Task 10, spec
    /// §3.2). Defaults to `Aim` (any aim accepted, no cast point) so all pre-Task-10 content
    /// parses unchanged. `#[reflect(ignore)]`: `AcqFallback::Then(Box<Acquisition>)` is
    /// self-referential through a `Box`, which `bevy_reflect`'s derive can't close over (no
    /// blanket `Reflect` impl for `Box<T>` in this version) — the field stays fully
    /// serde-round-trippable, it's just invisible to the (unused-here) reflection APIs.
    #[serde(default)]
    #[reflect(ignore)]
    pub acquisition: Acquisition,
    #[serde(default)]
    pub vfx_cues: std::collections::HashMap<String, String>,
    /// Search radius (world units) a chain hop uses to find its next victim (spec D5, Task 12).
    /// Consumed only when a struck BEAM hitbox's skill has rules `can_chain = true` and hops
    /// remain (`hop < chain_count`) — see `end_hitboxes`. A `can_chain` skill with no beam
    /// windows simply never chains; this field is then dead data, by design (v1 scope: beams
    /// only). Defaults to 6.0 so every pre-Task-12 timeline round-trips unaffected.
    #[serde(default = "default_chain_radius")]
    pub chain_radius: f32,
    /// Whether the HOST may hold this cast to scale it (e.g. a longer windup for a bigger bolt).
    /// Data-only (Task 13, spec §3.2/§4): the sim never reads it — charging still happens purely
    /// through the existing `charge: Option<u8>` cast parameter (`charge_mult`,
    /// `cast_skill_*_charged`), which is independent of this flag. `chargeable` is authored
    /// metadata for the HOST to decide whether to offer hold-to-charge input at all. Defaults to
    /// `false` so every pre-Task-13 timeline round-trips unaffected.
    #[serde(default)]
    pub chargeable: bool,
    /// Seconds the HOST may hold a chargeable cast before it caps out. Data-only, same caveat as
    /// `chargeable` — never read by the sim. Defaults to 1.0.
    #[serde(default = "default_max_hold")]
    pub max_hold: f32,
    /// Presentation cue-binding map (Task 13, spec §3.2/§4): authored `cue key -> CueBinding`
    /// (VFX effect / attach mode / animation / charge-driven params) for the editor and any other
    /// presentation consumer to resolve cue keys against. Pure data — DISTINCT from `vfx_cues`
    /// (which the sim's cue-firing systems DO read to name the `CueEvent::cue_id` a key fires).
    ///
    /// The map's keys are the SIM's runtime cue-slot names — not the spec table's shorthand —
    /// because this is what `src/vfx.rs`'s cue-firing observers and `src/events.rs::CueEvent`
    /// actually match against. There are exactly five slot-name patterns, each firing at most
    /// once per its trigger (`window_id` is a `CollisionWindow::id`):
    ///   - `on_cast` — the cast begins (`vfx.rs::cue_on_cast`).
    ///   - `on_window_{window_id}` — a *scheduled* window or chain re-strike spawns
    ///     (`vfx.rs::cue_on_window`).
    ///   - `on_hit` — each hit confirm (`vfx.rs::cue_on_hit`).
    ///   - `on_end_{window_id}` — a window ends (`vfx.rs::cue_on_end`).
    ///   - `emit_{window_id}` — an emitter instantiates a `Template` window (emitted instances
    ///     fire this ONLY, never `on_window_{window_id}` — `vfx.rs::cue_on_window`).
    ///
    /// These map 1:1 onto the spec's cue table (design doc
    /// `docs/superpowers/specs/2026-07-02-skill-editor-reimplementation-design.md` §3.2, "Cue
    /// table") shorthand: `cast` = `on_cast`, `window_open:<id>` = `on_window_{id}`, `hit` =
    /// `on_hit`, `end:<id>` = `on_end_{id}`, `emit:<id>` = `emit_{id}`. That table is normative
    /// for which `CueBinding` fields are legal to author on each slot — see `CueBinding`.
    ///
    /// Nothing in this crate ever reads `cues`; a binding naming an effect/anim that doesn't
    /// exist anywhere is therefore inert by construction, never a panic (see
    /// `cue_binding_naming_a_nonexistent_effect_does_not_panic`, `src/scenario/library.rs`).
    /// `#[reflect(ignore)]`: sidesteps requiring `Reflect` on the whole `CueBinding` tree, same
    /// rationale as `acquisition` above. Defaults to empty so every pre-Task-13 timeline
    /// round-trips unaffected.
    #[serde(default)]
    #[reflect(ignore)]
    pub cues: std::collections::HashMap<String, CueBinding>,
    /// CHARGE-HOLD presentation tiers (chargeable skills): while the HOST's hold-to-charge input
    /// is down, the tier whose `threshold` is the highest one at or below the LIVE charge
    /// fraction is ACTIVE — its effect loops (emission stopped when the tier is left or the
    /// hold releases; live particles drain on their own lifetimes), its `anim` loops on the
    /// caster's rig, and its `params` stream the live charge every frame. Crossing into a higher
    /// tier drains the old tier's effect and starts the new one — "the glow grows, then ignites".
    ///
    /// PURE PRESENTATION, entirely host-side: the sim never reads this, no `CueEvent` fires for
    /// it (charging is visible to every peer through the host's own input replication, so hosts
    /// drive these cues locally per player with no wire traffic). `cue.duration` is ignored
    /// (tiers loop). Sorted ascending by `threshold`; `validate_timeline` enforces it. Empty for
    /// every pre-charge-cues timeline (defaulted). `#[reflect(ignore)]`: same rationale as
    /// `cues` above.
    #[serde(default)]
    #[reflect(ignore)]
    pub charge_cues: Vec<ChargeCue>,
}

/// One charge-hold presentation tier — active while the live charge fraction is within
/// [`threshold`, next tier's threshold). See [`CastTimeline::charge_cues`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChargeCue {
    /// Charge fraction (0.0..=1.0 of a FULL hold) at which this tier becomes active. The first
    /// tier is normatively 0.0 (a hold with no 0.0 tier shows nothing until the first
    /// threshold — legal, occasionally intended).
    pub threshold: f32,
    /// The tier's binding: effect/attach/anim/params. `duration` is ignored (tiers loop until
    /// left); `attach` is normatively `World` (at the caster) or `Bone` (a rig socket) — never
    /// `Follow` (there is no window to follow during a hold).
    pub cue: CueBinding,
}

fn default_chain_radius() -> f32 {
    6.0
}

fn default_max_hold() -> f32 {
    1.0
}

/// A presentation binding for one cue key (Task 13, spec §3.2/§4) — what VFX/anim/params the
/// HOST should play when that cue fires. Entirely inert to the sim; see `CastTimeline::cues`.
///
/// The spec's cue table (design doc §3.2, "Cue table") is **normative for each slot's legal
/// binding options** — quoting it verbatim: "attachment is authorable on `window_open`/`emit`
/// only; anim on `cast` only." In this crate's slot-name vocabulary (see `CastTimeline::cues`):
/// `attach` is only meaningful on `on_window_*`/`emit_*` bindings — `on_cast`/`on_hit`/`on_end_*`
/// effects are world-anchored (fixed position/orientation, no source to follow); `anim` is only
/// meaningful on `on_cast` bindings. This schema is a **deliberately permissive superset**: the
/// struct does not reject an `attach`/`anim` authored on the "wrong" slot at parse time —
/// enforcement of these constraints is editor-side validation (phase 3, `ValidationRegistry`),
/// not a sim-side concern.
///
/// `CueAttach::Follow` means: the host flies a proxy entity along the cue's motion data (the
/// window's projectile/beam trajectory), and the window's end **event** (`HitboxEnded`, fired
/// regardless of whether an `on_end_{window_id}` *binding* is authored) snaps the proxy to the
/// end position and terminates it. A `Follow` binding on `on_window_bolt` therefore needs no
/// matching `on_end_bolt` binding to clean up after itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueBinding {
    /// Name of a VFX effect/preset to play (editor/host-resolved; not validated here).
    #[serde(default)]
    pub effect: Option<String>,
    /// Where the effect attaches. Normatively authorable on `on_window_*`/`emit_*` slots only
    /// (see `CueBinding` docs) — `World` is always legal (and is the default), `Follow` is only
    /// meaningful there. Not enforced by this schema; see `CueBinding` docs.
    #[serde(default)]
    pub attach: CueAttach,
    /// Name of an animation clip to play alongside the effect (editor/host-resolved).
    /// Normatively authorable on the `on_cast` slot only (see `CueBinding` docs); not enforced
    /// by this schema.
    #[serde(default)]
    pub anim: Option<String>,
    /// Extra parameters fed to the effect/anim from cast-time data.
    #[serde(default)]
    pub params: Vec<CueParam>,
    /// How long the bound effect PLAYS (emits), in seconds, before the host stops emission and
    /// lets live particles drain out on their own authored lifetimes. `None` = the host's
    /// default chain (the effect preset's own duration if it has one, else the host default).
    /// Like `effect`, this is presentation-only — entirely inert to the sim.
    #[serde(default)]
    pub duration: Option<f32>,
}

/// Where a cue's effect attaches in the world.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum CueAttach {
    /// Plays at a fixed world position/orientation (does not track its source afterward).
    /// Legal on every slot; the only legal option on world-anchored slots (`on_hit`,
    /// `on_end_*` — see `CueBinding` docs).
    #[default]
    World,
    /// Follows its source entity for its lifetime: the host flies a proxy along the cue's motion
    /// data, and the window's end event (not the binding) terminates it — see `CueBinding` docs.
    /// Normatively authorable on `on_window_*`/`emit_*` slots only.
    Follow,
    /// Anchors to a NAMED BONE (socket) of the CASTER's rig, offset in the bone's local frame —
    /// charge sparks on the casting hand, a muzzle flash at the palm. The host resolves `socket`
    /// against its rig's named joints and parents the effect there (an unknown socket falls back
    /// to the rig root — never a panic; presentation is best-effort by contract). Normatively
    /// authorable on caster-anchored presentation slots: `on_cast` and the charge tiers
    /// (`CastTimeline::charge_cues`).
    Bone {
        socket: String,
        #[serde(default)]
        offset: Vec3,
    },
}

/// One cue parameter binding: `param` (the effect/anim parameter name) driven by `source`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueParam {
    pub param: String,
    pub source: ParamSource,
}

/// Where a cue parameter's value comes from. v1: only the cast's charge level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamSource {
    /// The cast's `charge: Option<u8>` level (0-255; `None` treated as unspecified by the host).
    Charge,
}

/// Authored aim-acquisition requirement (Task 10, spec §3.2). `validate_casts` checks the
/// HOST-resolved `CastAim` against this tree and, on failure, walks `AcqFallback` chains until
/// one is met or the chain bottoms out at `Fizzle` (a paid `CastRejected`). Only the fallible
/// branches (`HitscanEntity`/`GroundPoint`) carry a `fallback` field — `Aim`/`SelfPoint` cannot
/// fail, so a fallback on them would be dead data; this is enforced BY CONSTRUCTION (the fields
/// simply don't exist on those variants) rather than by a runtime check.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum Acquisition {
    /// Any resolved aim is accepted as-is (Entity/Point degrade to their direction for facing
    /// purposes, same as pre-Task-10 behavior). Produces no cast point.
    #[default]
    Aim,
    /// Any resolved aim is accepted; the cast point is always the CASTER's position at cast
    /// time (self-centered effects — e.g. a storm summoned over the caster's own head).
    SelfPoint,
    /// Requires the resolved aim to be `CastAim::Entity`, within `range` of the caster, and
    /// passing `filter` against the caster's faction. On success the cast proceeds with its
    /// existing entity-aim target/direction (no cast point produced — v1 doesn't need the
    /// target's position as a point). On failure (wrong aim kind, out of range, or filter
    /// mismatch) runs `fallback`.
    HitscanEntity {
        range: f32,
        filter: HitFilter,
        fallback: AcqFallback,
    },
    /// Requires the resolved aim to be `CastAim::Point`, within `range` of the caster. On
    /// success the cast point IS that point (preserved — the historic "blizzard blocker": a
    /// ground-targeted point is no longer collapsed to a direction). On failure runs `fallback`.
    GroundPoint {
        range: f32,
        fallback: AcqFallback,
        /// Optional surface gate (spec §5.1) — see [`SurfaceRequirement`].
        #[serde(default)]
        on_surface: Option<SurfaceRequirement>,
    },
}

/// What to do when an `Acquisition` branch's requirement isn't met by the resolved aim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AcqFallback {
    /// A paid rejection: `validate_casts` fires `CastRejected` (the cast never begins — no
    /// `ActiveCast`, no cooldown started) with the reason the LAST attempted branch failed for.
    Fizzle,
    /// Re-check the SAME resolved aim against another `Acquisition` node (chains e.g.
    /// `GroundPoint` -> `SelfPoint` so a ground-point skill still resolves to "above the
    /// caster" when no point was aimed).
    Then(Box<Acquisition>),
}

/// Surface gate on a point acquisition (spec §5.1): the aimed point must land ON a patch of
/// `surface` (XZ within `patch.radius + SURFACE_MATCH_SLACK`, Y within tolerance). `snap`
/// recenters the cast point on the matched patch; `consume` removes the patch at CAST-ACCEPT
/// (with mana — spec D7: an interrupted cast still spends the tile). Failure runs the normal
/// fallback chain (paid fizzle at `Fizzle`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRequirement {
    pub surface: String,
    #[serde(default = "default_true")]
    pub snap: bool,
    #[serde(default)]
    pub consume: bool,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct PhaseDurations {
    pub windup: f32,
    pub active: f32,
    pub recovery: f32,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionWindow {
    pub id: String,
    /// The window's spawn ROLE: `Scheduled` puts it on the phase schedule; `Template` keeps it
    /// off every schedule — it exists only to be instantiated by an emitter (Task 11).
    pub spawn: WindowSpawn,
    /// The frame the window's origin resolves against (see [`WindowAnchor`]).
    #[serde(default)]
    pub anchor: WindowAnchor,
    /// World-axis-aligned offset added to the resolved anchor position (e.g. `(0, 8, 0)` hangs
    /// a storm cloud 8 units above the cast point).
    #[serde(default)]
    pub anchor_offset: Vec3,
    /// `false` = a carrier volume: it flies, ends, and fires cues/events, but overlap detection
    /// skips it entirely — it can never produce a `HitConfirmed`. Default `true`.
    #[serde(default = "default_true")]
    pub strikes: bool,
    pub active_duration: f32,
    pub shape: CollisionShape,
    #[serde(default)]
    pub motion: VolumeMotion,
    /// Overrides `motion`'s launch direction (Task 11, spec §3.2 — see [`MotionDirection`]).
    /// Defaults to `Inherit`, so every pre-Task-11 window is byte-for-byte unaffected.
    #[serde(default)]
    pub motion_direction: MotionDirection,
    pub hit_filter: HitFilter,
    pub hit_mode: HitMode,
    #[serde(default)]
    pub rehit_interval: Option<f32>,
    /// Rains one instance of a `Template` window on a dedicated clock while a hitbox spawned
    /// from THIS window is alive (Task 11, spec §3.2 — see [`Emitter`]). `None` (the default) =
    /// no emission; every pre-Task-11 window omits this and is unaffected.
    #[serde(default)]
    pub emitter: Option<Emitter>,
    /// Paint surface patches while alive / on end (spec §5.1) — see [`PaintSpec`]. `None` (the
    /// default) = no painting; every pre-surfaces window omits this and is unaffected.
    #[serde(default)]
    pub paints: Option<PaintSpec>,
}

fn default_true() -> bool {
    true
}

/// When (whether) a window enters the world on its own. `deny_unknown_fields` (Task 13 review
/// finding, carried from Task 9): a typo'd `Scheduled` field (e.g. `phse`) must fail loud at
/// load, exactly like every other authored struct in this schema, rather than silently falling
/// back to defaults for the fields it DID recognize.
#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum WindowSpawn {
    /// On the phase schedule: eligible at `phase`'s start time plus `offset` seconds.
    Scheduled {
        phase: WindowPhase,
        #[serde(default)]
        offset: f32,
    },
    /// Never self-schedules — instantiated only by an emitter (Task 11). Until emitters land,
    /// authoring a `Template` window is a validation error (`validate_timeline`).
    Template,
}

/// Where a window's origin resolves. For a triggered execution, the CAST POINT is the trigger's
/// payload position (the hit / impact / expiry the trigger fired at); for a player cast, both
/// variants resolve to the caster's muzzle origin until acquisition (Task 10) gives casts a
/// distinct acquired point.
#[derive(Debug, Clone, Copy, Default, Reflect, Serialize, Deserialize, PartialEq, Eq)]
pub enum WindowAnchor {
    /// The caster entity's position at the moment the window spawns.
    #[default]
    Caster,
    /// The position this cast/execution was invoked at.
    CastPoint,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq, Eq)]
pub enum WindowPhase {
    Windup,
    Active,
    Recovery,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum CollisionShape {
    Sphere { radius: f32 },
    Capsule { radius: f32, height: f32 },
    Cone { angle: f32, range: f32 },
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize, Default)]
pub enum VolumeMotion {
    #[default]
    Static,
    Linear {
        speed: f32,
    },
    /// Ballistic arc: launched along the (free-look, possibly pitched) aim direction at `speed`,
    /// pulled down by `gravity` (world units/s²) each fixed step. Charge scales `speed` like
    /// `Linear`, so a charged lob flies flatter and further.
    Ballistic {
        speed: f32,
        gravity: f32,
    },
    /// Instantaneous link to the hitbox's DESIGNATED TARGET (the cast's entity aim; rules-
    /// driven chain hops re-key onto this in Task 12): no overlap test — the target IS the
    /// payload; the hitbox strikes it on resolve and ends `HitEntity` at the victim. With no designated
    /// target (e.g. a direction-aimed cast of a beam skill) it strikes nothing and fuses out —
    /// the paid-fizzle miss. `shape` is unused for hitting (kept for the editor gizmo).
    Beam,
}

/// Rains one instance of a `Template` window from a live hitbox on a dedicated clock (Task 11,
/// spec §3.2): every `1/rate` seconds, spawn ONE instance of the `Template` window named
/// `window` at the emitting hitbox's current position plus an xz-disc jitter offset (radius
/// `jitter`, sampled from the dedicated `SpawnRng` stream — never `CombatRng`). Validated by
/// `validate_timeline`: `window` must name an existing `Template` window; every `Template`
/// window must be referenced by at least one emitter; a `Template` window may not itself carry
/// an emitter (recursion guard); `rate` must be `> 0.0` and `jitter` must be `>= 0.0`.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct Emitter {
    /// Spawns per second (the emission period is `1.0 / rate` seconds).
    pub rate: f32,
    /// xz-disc jitter radius (world units), uniform-area sampled from `SpawnRng`.
    pub jitter: f32,
    /// Id of the `Template` window to instantiate.
    pub window: String,
}

/// Authored surface painting for a window (spec §5.1): while this window's hitbox is alive
/// (`Trail`) or when it ends (`OnEnd`), paint patches of `surface`. Painting is a window
/// PROPERTY (not a child window), so it composes with emitters without inheriting the parent
/// skill's lifecycle triggers — the exact trap that forced the arena's tile-drop poller.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintSpec {
    /// Surface-type id (resolved against the host's `SurfaceRegistry` at runtime; unknown ids
    /// warn-and-skip — editor validation is the blocking check, same split as cue effects).
    pub surface: String,
    /// Painted patch radius (world units).
    pub radius: f32,
    pub mode: PaintMode,
    /// Per-paint lifetime override (secs); `None` = the surface type's default.
    #[serde(default)]
    pub lifetime: Option<f32>,
}

/// When a painting window actually paints.
#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq)]
pub enum PaintMode {
    /// Paint every `step` meters of hitbox travel (the glacier-trail shape). Also paints once
    /// immediately at spawn.
    Trail { step: f32 },
    /// Paint once at the hitbox's end position, whatever the end reason (shards, blasts).
    OnEnd,
}

/// Overrides a window's `VolumeMotion` launch direction (Task 11, spec §3.2) — authored ON the
/// window whose direction is overridden (typically an emitted `Template`, e.g. a blizzard
/// shard's Down motion; nothing stops authoring it on a scheduled window too).
#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum MotionDirection {
    /// Keep the motion's normal direction: the spawning cast/execution's aim, or (for an
    /// emitted instance) the emitting hitbox's `aim`. The default — every pre-Task-11 window is
    /// unaffected.
    #[default]
    Inherit,
    /// Launch straight down (`-Y`) at the motion's authored `speed`, regardless of aim — a
    /// shard falling out of the sky rather than following the caster's facing.
    Down,
    /// Launch along the inherited direction FLATTENED to the ground plane (Y zeroed, then
    /// normalized) — a ground-roller/wave that continues along the aim without diving into or
    /// climbing away from the floor. A triggered execution's inherited direction is the ending
    /// window's travel direction (e.g. a ballistic bolt's DESCENDING impact velocity); this
    /// keeps the child hugging the ground instead of burying itself. A degenerate horizontal
    /// component (straight-down aim) falls back to `+Z`.
    Horizontal,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq, Eq)]
pub enum HitFilter {
    Caster,
    Allies,
    Enemies,
    All,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq, Eq)]
pub enum HitMode {
    OncePerTarget,
    FirstOnly,
    EveryTick,
}

/// RON loader for `*.cast.ron`.
#[derive(Default, TypePath)]
pub struct CastTimelineLoader;

impl AssetLoader for CastTimelineLoader {
    type Asset = CastTimeline;
    type Settings = ();
    type Error = CastLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<CastTimeline, CastLoadError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| CastLoadError::Io(e.to_string()))?;
        let tl = ron::de::from_bytes::<CastTimeline>(&bytes)
            .map_err(|e| CastLoadError::Ron(e.to_string()))?;
        validate_timeline(&tl).map_err(CastLoadError::Invalid)?;
        Ok(tl)
    }

    fn extensions(&self) -> &[&str] {
        &["cast.ron"]
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CastLoadError {
    #[error("io: {0}")]
    Io(String),
    #[error("ron: {0}")]
    Ron(String),
    #[error("invalid timeline: {0}")]
    Invalid(String),
}

/// Timeline validation, v2. The v1 chain-graph rules died with the authored `on_end` reaction
/// schema; what remains to check today:
/// - (Task 11) every `Emitter.window` must name a window that EXISTS and is `Template` — a
///   scheduled/non-existent emitter target would silently spawn nothing at runtime.
/// - (Task 11) every `Template` window must be referenced by at least one emitter — an
///   unreferenced `Template` window can never spawn (it's off every schedule by definition),
///   which is always an authoring mistake.
/// - (Task 11) a `Template` window may not itself carry an `emitter` (spec §3.2's
///   Template→Template recursion guard) — emission is a property of the window that's ALIVE and
///   ticking, and a `Template` window is never itself alive on its own (it only exists once
///   instantiated), so an emitter authored there could never tick.
/// - (Task 11) `Emitter::rate` must be `> 0.0` (a non-positive rate can never cross an emission
///   boundary — or divides by zero) and `Emitter::jitter` must be `>= 0.0` (a negative disc
///   radius is nonsensical).
/// - (Task 10) a `WindowAnchor::CastPoint` window must be reachable from a timeline whose
///   `acquisition` can actually produce a cast point — otherwise the window would silently fall
///   back to the caster's position at spawn time, masking an authoring mistake.
pub fn validate_timeline(tl: &CastTimeline) -> Result<(), String> {
    // Charge tiers: thresholds within [0, 1], strictly ascending (equal thresholds would make
    // "the active tier" ambiguous), Bone sockets non-empty. `Follow` is normatively illegal on
    // tiers (nothing to follow during a hold) — flagged here rather than silently no-oping.
    let mut prev_threshold: Option<f32> = None;
    for (i, tier) in tl.charge_cues.iter().enumerate() {
        if !(0.0..=1.0).contains(&tier.threshold) {
            return Err(format!(
                "charge tier {i} threshold {} must be within 0.0..=1.0",
                tier.threshold
            ));
        }
        if let Some(prev) = prev_threshold {
            if tier.threshold <= prev {
                return Err(format!(
                    "charge tier {i} threshold {} must be strictly greater than the previous \
                     tier's {prev} (tiers sort ascending)",
                    tier.threshold
                ));
            }
        }
        prev_threshold = Some(tier.threshold);
        match &tier.cue.attach {
            CueAttach::Follow => {
                return Err(format!(
                    "charge tier {i} may not use Follow attach (there is no window to follow \
                     during a hold) — use World or Bone"
                ));
            }
            CueAttach::Bone { socket, .. } if socket.is_empty() => {
                return Err(format!("charge tier {i} Bone attach needs a non-empty socket name"));
            }
            _ => {}
        }
    }

    let mut referenced: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for w in &tl.collision_windows {
        let Some(em) = &w.emitter else { continue };
        // Recursion guard (spec §3.2): a Template window may not itself carry an emitter — it
        // only exists to BE instantiated, never to instantiate something else.
        if w.spawn == WindowSpawn::Template {
            return Err(format!(
                "window '{}' is a Template and may not itself carry an emitter (spec §3.2 \
                 Template->Template recursion guard)",
                w.id
            ));
        }
        if em.rate <= 0.0 {
            return Err(format!(
                "window '{}' emitter rate must be > 0.0, got {}",
                w.id, em.rate
            ));
        }
        if em.jitter < 0.0 {
            return Err(format!(
                "window '{}' emitter jitter must be >= 0.0, got {}",
                w.id, em.jitter
            ));
        }
        match tl.collision_windows.iter().find(|t| t.id == em.window) {
            None => {
                return Err(format!(
                    "window '{}' emitter targets unknown window '{}'",
                    w.id, em.window
                ))
            }
            Some(target) if target.spawn != WindowSpawn::Template => {
                return Err(format!(
                    "window '{}' emitter targets window '{}', which is not a Template \
                     (spawn: {:?}) — an emitter may only instantiate a Template window",
                    w.id, em.window, target.spawn
                ))
            }
            Some(_) => {
                referenced.insert(em.window.as_str());
            }
        }
    }
    for w in &tl.collision_windows {
        if w.spawn == WindowSpawn::Template && !referenced.contains(w.id.as_str()) {
            return Err(format!(
                "window '{}' is a Template but is never referenced by an emitter — it can \
                 never spawn",
                w.id
            ));
        }
        if w.anchor == WindowAnchor::CastPoint && !acquisition_can_produce_point(&tl.acquisition) {
            return Err(format!(
                "window '{}' anchors on CastPoint, but this timeline's acquisition ({:?}) can \
                 never produce a cast point — add a GroundPoint/SelfPoint branch (directly or via \
                 a fallback chain) or change the window's anchor to Caster",
                w.id, tl.acquisition
            ));
        }
        if let Some(paints) = &w.paints {
            if paints.surface.is_empty() {
                return Err(format!("window '{}' paints an empty surface id", w.id));
            }
            if paints.radius <= 0.0 {
                return Err(format!(
                    "window '{}' paints radius must be > 0, got {}",
                    w.id, paints.radius
                ));
            }
            if let PaintMode::Trail { step } = paints.mode {
                if step <= 0.0 {
                    return Err(format!(
                        "window '{}' paints Trail step must be > 0, got {step}",
                        w.id
                    ));
                }
            }
            if let Some(lt) = paints.lifetime {
                if lt <= 0.0 {
                    return Err(format!(
                        "window '{}' paints lifetime override must be > 0, got {lt}",
                        w.id
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Whether `acq` (walking its fallback chain, if any) can EVER resolve to a cast point.
/// `SelfPoint` always can (it's unconditional); `GroundPoint`'s own success always produces one
/// too (only its fallback matters when it doesn't apply/fit); `Aim` never does; `HitscanEntity`'s
/// success never does (v1 doesn't surface the target's position), so only its fallback chain
/// can rescue a point.
fn acquisition_can_produce_point(acq: &Acquisition) -> bool {
    match acq {
        Acquisition::Aim => false,
        Acquisition::SelfPoint => true,
        Acquisition::GroundPoint { .. } => true,
        Acquisition::HitscanEntity { fallback, .. } => fallback_can_produce_point(fallback),
    }
}

fn fallback_can_produce_point(fb: &AcqFallback) -> bool {
    match fb {
        AcqFallback::Fizzle => false,
        AcqFallback::Then(next) => acquisition_can_produce_point(next),
    }
}

/// Maps skill_id -> loaded timeline handle.
#[derive(Resource, Default)]
pub struct CastTimelineHandles(pub std::collections::HashMap<String, Handle<CastTimeline>>);

pub struct ObeliskAssetsPlugin;
impl Plugin for ObeliskAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<CastTimeline>()
            .register_asset_loader(CastTimelineLoader)
            .init_resource::<CastTimelineHandles>();
        app.register_type::<CastTimeline>()
            .register_type::<PhaseDurations>()
            .register_type::<CollisionWindow>()
            .register_type::<WindowSpawn>()
            .register_type::<WindowAnchor>()
            .register_type::<WindowPhase>()
            .register_type::<CollisionShape>()
            .register_type::<VolumeMotion>()
            .register_type::<MotionDirection>()
            .register_type::<Emitter>()
            .register_type::<PaintSpec>()
            .register_type::<PaintMode>()
            .register_type::<HitFilter>()
            .register_type::<HitMode>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic_window(id: &str) -> CollisionWindow {
        CollisionWindow {
            id: id.into(),
            spawn: WindowSpawn::Scheduled {
                phase: WindowPhase::Active,
                offset: 0.0,
            },
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
            paints: None,
        }
    }

    fn timeline_with_windows(wins: Vec<CollisionWindow>) -> CastTimeline {
        CastTimeline {
            skill_id: "test".into(),
            phase_durations: PhaseDurations {
                windup: 0.1,
                active: 0.1,
                recovery: 0.1,
            },
            collision_windows: wins,
            acquisition: Acquisition::default(),
            vfx_cues: Default::default(),
            chain_radius: default_chain_radius(),
            chargeable: false,
            max_hold: default_max_hold(),
            cues: Default::default(),
            charge_cues: Vec::new(),
        }
    }

    fn timeline_with(win: CollisionWindow) -> CastTimeline {
        timeline_with_windows(vec![win])
    }

    #[test]
    fn v2_window_round_trips() {
        let tl = timeline_with(CollisionWindow {
            spawn: WindowSpawn::Scheduled {
                phase: WindowPhase::Active,
                offset: 0.0,
            },
            anchor: WindowAnchor::CastPoint,
            anchor_offset: Vec3::new(0.0, 8.0, 0.0),
            strikes: false,
            ..basic_window("storm")
        });
        let s = ron::ser::to_string_pretty(&tl, Default::default()).unwrap();
        let back: CastTimeline = ron::from_str(&s).unwrap();
        assert_eq!(back.collision_windows[0].anchor, WindowAnchor::CastPoint);
        assert_eq!(
            back.collision_windows[0].anchor_offset,
            Vec3::new(0.0, 8.0, 0.0)
        );
        assert!(!back.collision_windows[0].strikes);
        assert_eq!(
            back.collision_windows[0].spawn,
            WindowSpawn::Scheduled {
                phase: WindowPhase::Active,
                offset: 0.0
            }
        );
    }

    /// A v1 firebolt-style timeline: `spawn_phase`/`spawn_offset` fields, an authored
    /// `on_end: Chain` reaction, a `Chained` blast window, and the deleted
    /// `targeting`/`delivery` blocks. None of it may silently half-parse.
    const OLD_FIREBOLT_V1_RON: &str = r#"(
        skill_id: "firebolt",
        phase_durations: ( windup: 0.3, active: 0.1, recovery: 0.2 ),
        collision_windows: [
            ( id: "bolt", spawn_phase: Active, spawn_offset: 0.0, active_duration: 2.0,
              shape: Sphere( radius: 0.5 ), motion: Linear( speed: 20.0 ),
              hit_filter: Enemies, hit_mode: FirstOnly,
              on_end: ( hit: Some(Chain("blast")) ) ),
            ( id: "blast", spawn_phase: Chained, spawn_offset: 0.0, active_duration: 0.05,
              shape: Sphere( radius: 1.5 ), motion: Static,
              hit_filter: Enemies, hit_mode: OncePerTarget ),
        ],
        targeting: SingleEntity( range: 15.0 ),
        delivery: Projectile( speed: 20.0 ),
    )"#;

    #[test]
    fn old_chain_schema_fails_loud() {
        assert!(
            ron::from_str::<CastTimeline>(OLD_FIREBOLT_V1_RON).is_err(),
            "v1 on_end/Chained content must not silently half-parse"
        );
    }

    /// v2 content may omit `anchor` / `anchor_offset` / `strikes` / `offset`: they default to
    /// Caster / zero / true / 0.0.
    #[test]
    fn v2_defaults_apply_when_fields_are_omitted() {
        let src = r#"(
            skill_id: "minimal",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            collision_windows: [
                ( id: "w", spawn: Scheduled( phase: Active ), active_duration: 1.0,
                  shape: Sphere( radius: 0.5 ), hit_filter: Enemies, hit_mode: OncePerTarget ),
            ],
        )"#;
        let tl: CastTimeline = ron::from_str(src).expect("minimal v2 content parses");
        let w = &tl.collision_windows[0];
        assert_eq!(w.anchor, WindowAnchor::Caster);
        assert_eq!(w.anchor_offset, Vec3::ZERO);
        assert!(w.strikes, "strikes defaults to true");
        assert_eq!(
            w.spawn,
            WindowSpawn::Scheduled {
                phase: WindowPhase::Active,
                offset: 0.0
            }
        );
        assert!(w.emitter.is_none(), "emitter defaults to None");
        assert_eq!(
            w.motion_direction,
            MotionDirection::Inherit,
            "motion_direction defaults to Inherit"
        );
    }

    fn template_window(id: &str) -> CollisionWindow {
        CollisionWindow {
            spawn: WindowSpawn::Template,
            ..basic_window(id)
        }
    }

    fn emitting_window(id: &str, target: &str) -> CollisionWindow {
        CollisionWindow {
            emitter: Some(Emitter {
                rate: 2.0,
                jitter: 1.0,
                window: target.into(),
            }),
            ..basic_window(id)
        }
    }

    #[test]
    fn scheduled_windows_validate() {
        assert!(validate_timeline(&timeline_with(basic_window("w"))).is_ok());
    }

    /// (Task 11) A `Template` window referenced by exactly one emitter validates — the happy
    /// path every blizzard-style timeline needs.
    #[test]
    fn template_window_referenced_by_emitter_validates() {
        let tl = timeline_with_windows(vec![
            emitting_window("storm", "shard"),
            template_window("shard"),
        ]);
        assert!(
            validate_timeline(&tl).is_ok(),
            "{:?}",
            validate_timeline(&tl)
        );
    }

    /// (Task 11) An emitter naming a window id that doesn't exist anywhere in the timeline is a
    /// validation error naming both windows.
    #[test]
    fn emitter_targeting_unknown_window_fails_validation() {
        let tl = timeline_with_windows(vec![emitting_window("storm", "ghost")]);
        let err = validate_timeline(&tl).unwrap_err();
        assert!(
            err.contains("storm") && err.contains("ghost") && err.contains("unknown"),
            "error names both windows: {err}"
        );
    }

    /// (Task 11) An emitter naming a window that EXISTS but isn't `Template` (e.g. a scheduled
    /// window) is a validation error — an emitter may only instantiate a `Template`.
    #[test]
    fn emitter_targeting_non_template_window_fails_validation() {
        let tl = timeline_with_windows(vec![
            emitting_window("storm", "other"),
            basic_window("other"), // Scheduled, not Template
        ]);
        let err = validate_timeline(&tl).unwrap_err();
        assert!(
            err.contains("storm") && err.contains("other") && err.contains("not a Template"),
            "error names both windows and explains why: {err}"
        );
    }

    /// (Task 11) A `Template` window with no emitter referencing it can never spawn — a
    /// validation error naming the orphaned window.
    #[test]
    fn unreferenced_template_window_fails_validation() {
        let tl = timeline_with_windows(vec![template_window("shard")]);
        let err = validate_timeline(&tl).unwrap_err();
        assert!(
            err.contains("shard") && err.contains("Template") && err.contains("never referenced"),
            "error names the orphaned window: {err}"
        );
    }

    /// (Task 11, spec §3.2) A `Template` window may not itself carry an `emitter` — the
    /// Template->Template recursion guard.
    #[test]
    fn template_window_with_its_own_emitter_fails_validation() {
        let mut tpl = emitting_window("shard", "shard");
        tpl.spawn = WindowSpawn::Template;
        let tl = timeline_with_windows(vec![tpl]);
        let err = validate_timeline(&tl).unwrap_err();
        assert!(
            err.contains("shard") && err.contains("recursion"),
            "error names the window and the recursion guard: {err}"
        );
    }

    /// (Task 11) `Emitter::rate` must be strictly positive.
    #[test]
    fn emitter_rate_must_be_positive_fails_validation() {
        let mut storm = emitting_window("storm", "shard");
        storm.emitter.as_mut().unwrap().rate = 0.0;
        let tl = timeline_with_windows(vec![storm, template_window("shard")]);
        let err = validate_timeline(&tl).unwrap_err();
        assert!(
            err.contains("storm") && err.contains("rate"),
            "error names the window and the offending field: {err}"
        );
    }

    /// (Task 11) `Emitter::jitter` must be non-negative.
    #[test]
    fn emitter_jitter_must_be_non_negative_fails_validation() {
        let mut storm = emitting_window("storm", "shard");
        storm.emitter.as_mut().unwrap().jitter = -1.0;
        let tl = timeline_with_windows(vec![storm, template_window("shard")]);
        let err = validate_timeline(&tl).unwrap_err();
        assert!(
            err.contains("storm") && err.contains("jitter"),
            "error names the window and the offending field: {err}"
        );
    }

    fn cast_point_window(id: &str) -> CollisionWindow {
        CollisionWindow {
            anchor: WindowAnchor::CastPoint,
            ..basic_window(id)
        }
    }

    /// `GroundPoint`'s own success always produces a cast point, so a `CastPoint`-anchored
    /// window is reachable directly — no fallback needed.
    #[test]
    fn cast_point_window_with_ground_point_acquisition_validates() {
        let tl = CastTimeline {
            acquisition: Acquisition::GroundPoint {
                range: 30.0,
                fallback: AcqFallback::Fizzle,
                on_surface: None,
            },
            ..timeline_with(cast_point_window("storm"))
        };
        assert!(validate_timeline(&tl).is_ok());
    }

    /// `HitscanEntity`'s own success never produces a point, but its `Then` fallback chain
    /// bottoms out at `SelfPoint`, which always can — the point-producer is reached only by
    /// walking the chain, not the branch's own success.
    #[test]
    fn cast_point_window_reachable_via_fallback_chain_validates() {
        let tl = CastTimeline {
            acquisition: Acquisition::HitscanEntity {
                range: 15.0,
                filter: HitFilter::Enemies,
                fallback: AcqFallback::Then(Box::new(Acquisition::SelfPoint)),
            },
            ..timeline_with(cast_point_window("storm"))
        };
        assert!(validate_timeline(&tl).is_ok());
    }

    /// `SelfPoint` is unconditional — always reaches a `CastPoint`-anchored window.
    #[test]
    fn self_point_acquisition_validates() {
        let tl = CastTimeline {
            acquisition: Acquisition::SelfPoint,
            ..timeline_with(cast_point_window("storm"))
        };
        assert!(validate_timeline(&tl).is_ok());
    }

    #[test]
    fn loads_firebolt_cast_ron() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin {
                file_path: ".".into(),
                ..default()
            })
            .add_plugins(ObeliskAssetsPlugin);
        app.finish();
        app.cleanup();
        let handle: Handle<CastTimeline> = app
            .world()
            .resource::<AssetServer>()
            .load("assets/skills/firebolt.cast.ron");
        for _ in 0..1000 {
            app.update();
            if app
                .world()
                .resource::<Assets<CastTimeline>>()
                .get(&handle)
                .is_some()
            {
                break;
            }
        }
        let timeline = app
            .world()
            .resource::<Assets<CastTimeline>>()
            .get(&handle)
            .expect("loaded");
        assert_eq!(timeline.skill_id, "firebolt");
        assert_eq!(timeline.collision_windows.len(), 1);
    }

    #[test]
    fn cast_timeline_round_trips_through_ron() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/skills/firebolt.cast.ron"
        ))
        .expect("read firebolt.cast.ron");
        let parsed: CastTimeline = ron::de::from_str(&src).expect("parse original");
        let serialized = ron::ser::to_string(&parsed).expect("serialize");
        let reparsed: CastTimeline = ron::de::from_str(&serialized).expect("re-parse");
        assert_eq!(parsed.skill_id, reparsed.skill_id);
        assert_eq!(
            parsed.phase_durations.windup,
            reparsed.phase_durations.windup
        );
        assert_eq!(
            parsed.phase_durations.active,
            reparsed.phase_durations.active
        );
        assert_eq!(
            parsed.phase_durations.recovery,
            reparsed.phase_durations.recovery
        );
        assert_eq!(
            parsed.collision_windows.len(),
            reparsed.collision_windows.len()
        );
        assert_eq!(
            format!("{:?}", parsed.collision_windows[0].shape),
            format!("{:?}", reparsed.collision_windows[0].shape)
        );
        assert_eq!(parsed.vfx_cues, reparsed.vfx_cues);
        assert_eq!(parsed.chain_radius, reparsed.chain_radius);
    }

    /// `chain_radius` (Task 12) is absent from every pre-Task-12 `.cast.ron` fixture; the
    /// omitted field must default to 6.0 rather than fail `deny_unknown_fields`/missing-field
    /// parsing.
    #[test]
    fn chain_radius_defaults_when_omitted() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/skills/firebolt.cast.ron"
        ))
        .expect("read firebolt.cast.ron");
        assert!(
            !src.contains("chain_radius"),
            "fixture must not already author chain_radius for this default-coverage test"
        );
        let parsed: CastTimeline = ron::de::from_str(&src).expect("parse without chain_radius");
        assert_eq!(parsed.chain_radius, 6.0);
    }

    /// `chargeable`/`max_hold` (Task 13) are absent from every pre-Task-13 `.cast.ron` fixture;
    /// omitting them must default to `false`/`1.0` rather than fail parsing.
    #[test]
    fn chargeable_and_max_hold_default_when_omitted() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/skills/firebolt.cast.ron"
        ))
        .expect("read firebolt.cast.ron");
        assert!(
            !src.contains("chargeable") && !src.contains("max_hold"),
            "fixture must not already author chargeable/max_hold for this default-coverage test"
        );
        let parsed: CastTimeline =
            ron::de::from_str(&src).expect("parse without chargeable/max_hold");
        assert!(!parsed.chargeable);
        assert_eq!(parsed.max_hold, 1.0);
    }

    /// `chargeable`/`max_hold` round-trip through RON when authored non-default (Task 13).
    #[test]
    fn chargeable_and_max_hold_round_trip() {
        let tl = CastTimeline {
            chargeable: true,
            max_hold: 2.5,
            ..timeline_with(basic_window("bolt"))
        };
        let s = ron::ser::to_string_pretty(&tl, Default::default()).unwrap();
        let back: CastTimeline = ron::from_str(&s).unwrap();
        assert!(back.chargeable);
        assert_eq!(back.max_hold, 2.5);
    }

    /// The presentation `cues` map (Task 13, spec §3.2/§4) round-trips every field, including
    /// both `CueAttach` variants and the sole v1 `ParamSource::Charge` variant. Pure data — this
    /// test only proves the schema serializes faithfully; `cue_binding_naming_a_nonexistent_effect_
    /// does_not_panic` (`src/scenario/library.rs`) proves the sim never reads it.
    #[test]
    fn cue_binding_round_trips_through_ron() {
        let mut cues = std::collections::HashMap::new();
        cues.insert(
            "on_cast".to_string(),
            CueBinding {
                effect: Some("bolt_cast_fx".into()),
                attach: CueAttach::World,
                anim: Some("cast_anim".into()),
                params: vec![CueParam {
                    param: "scale".into(),
                    source: ParamSource::Charge,
                }],
                duration: Some(2.5),
            },
        );
        cues.insert(
            "on_hit".to_string(),
            CueBinding {
                effect: Some("bolt_impact_fx".into()),
                attach: CueAttach::Follow,
                anim: None,
                params: vec![],
                duration: None,
            },
        );
        let tl = CastTimeline {
            cues,
            ..timeline_with(basic_window("bolt"))
        };
        let s = ron::ser::to_string_pretty(&tl, Default::default()).unwrap();
        let back: CastTimeline = ron::from_str(&s).unwrap();
        assert_eq!(back.cues.len(), 2);
        let on_cast = &back.cues["on_cast"];
        assert_eq!(on_cast.effect.as_deref(), Some("bolt_cast_fx"));
        assert_eq!(on_cast.attach, CueAttach::World);
        assert_eq!(on_cast.anim.as_deref(), Some("cast_anim"));
        assert_eq!(on_cast.params.len(), 1);
        assert_eq!(on_cast.params[0].param, "scale");
        assert_eq!(on_cast.params[0].source, ParamSource::Charge);
        assert_eq!(on_cast.duration, Some(2.5));
        let on_hit = &back.cues["on_hit"];
        assert_eq!(on_hit.attach, CueAttach::Follow);
        assert!(on_hit.anim.is_none());
        // `duration` omitted from the RON (None serializes as nothing under serde default)
        // round-trips as None — every pre-duration binding parses unchanged.
        assert_eq!(on_hit.duration, None);
    }

    /// `cues` omitted entirely (every pre-Task-13 timeline) defaults to empty.
    #[test]
    fn cues_default_to_empty_when_omitted() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/skills/firebolt.cast.ron"
        ))
        .expect("read firebolt.cast.ron");
        let parsed: CastTimeline = ron::de::from_str(&src).expect("parse without cues");
        assert!(parsed.cues.is_empty());
    }

    /// `WindowSpawn`'s new `deny_unknown_fields` (Task 13 review finding, carried from Task 9)
    /// fails loud on a typo'd `Scheduled` field instead of silently ignoring it.
    #[test]
    fn window_spawn_rejects_a_typoed_field() {
        let src = r#"(
            skill_id: "typo",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            collision_windows: [
                ( id: "w", spawn: Scheduled( phse: Active ), active_duration: 1.0,
                  shape: Sphere( radius: 0.5 ), hit_filter: Enemies, hit_mode: OncePerTarget ),
            ],
        )"#;
        assert!(
            ron::from_str::<CastTimeline>(src).is_err(),
            "a typo'd WindowSpawn::Scheduled field must fail loud, not half-parse"
        );
    }

    /// Every shipped `.cast.ron` fixture still parses after the `WindowSpawn::deny_unknown_fields`
    /// hygiene fix — the carried Task 9 review item explicitly asked this be verified, not assumed.
    #[test]
    fn every_shipped_cast_asset_still_parses() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/skills");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).expect("read assets/skills") {
            let path = entry.expect("dir entry").path();
            if path.to_string_lossy().ends_with(".cast.ron") {
                let src =
                    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
                ron::de::from_str::<CastTimeline>(&src)
                    .unwrap_or_else(|e| panic!("{path:?} failed to parse: {e}"));
                checked += 1;
            }
        }
        assert!(
            checked >= 10,
            "expected to check every shipped skill (got {checked})"
        );
    }

    #[test]
    fn cast_timeline_type_is_registered_for_reflection() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin {
                file_path: ".".into(),
                ..default()
            })
            .add_plugins(ObeliskAssetsPlugin);
        app.finish();
        app.cleanup();
        let registry = app.world().resource::<AppTypeRegistry>().read();
        assert!(
            registry
                .get(std::any::TypeId::of::<CastTimeline>())
                .is_some(),
            "CastTimeline must be registered"
        );
    }

    /// Charge tiers + Bone attach round-trip through RON, old content parses unchanged
    /// (defaulted), and the validation rules hold: ascending thresholds in range, no Follow on
    /// tiers, non-empty sockets.
    #[test]
    fn charge_cues_parse_and_validate() {
        let ron_src = r#"(
            skill_id: "t",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            chargeable: true,
            charge_cues: [
                ( threshold: 0.0, cue: ( effect: Some("sparks"),
                    attach: Bone( socket: "right_hand", offset: (0.0, 0.05, 0.0) ),
                    anim: Some("casting_idle"),
                    params: [ ( param: "scale", source: Charge ) ] ) ),
                ( threshold: 0.6, cue: ( effect: Some("storm"),
                    attach: Bone( socket: "right_hand" ) ) ),
            ],
        )"#;
        let tl: CastTimeline = ron::from_str(ron_src).expect("charge_cues timeline parses");
        assert_eq!(tl.charge_cues.len(), 2);
        assert!(matches!(
            &tl.charge_cues[0].cue.attach,
            CueAttach::Bone { socket, offset } if socket == "right_hand" && offset.y == 0.05
        ));
        validate_timeline(&tl).expect("well-formed tiers validate");

        // Old content: no charge_cues field -> empty, still valid.
        let old: CastTimeline = ron::from_str(
            r#"( skill_id: "o", phase_durations: ( windup: 0.0, active: 0.1, recovery: 0.0 ) )"#,
        )
        .expect("pre-charge content parses");
        assert!(old.charge_cues.is_empty());

        // Rule failures.
        let mut bad = tl.clone();
        bad.charge_cues[1].threshold = 0.0; // duplicate of tier 0
        assert!(validate_timeline(&bad).is_err(), "non-ascending thresholds rejected");
        let mut bad = tl.clone();
        bad.charge_cues[0].cue.attach = CueAttach::Follow;
        assert!(validate_timeline(&bad).is_err(), "Follow illegal on tiers");
        let mut bad = tl.clone();
        bad.charge_cues[0].cue.attach = CueAttach::Bone {
            socket: String::new(),
            offset: Vec3::ZERO,
        };
        assert!(validate_timeline(&bad).is_err(), "empty socket rejected");
        let mut bad = tl;
        bad.charge_cues[0].threshold = 1.5;
        assert!(validate_timeline(&bad).is_err(), "threshold outside 0..=1 rejected");
    }

    /// (Surfaces) `paints` round-trips, defaults to None, and validates numeric sanity.
    #[test]
    fn paints_field_round_trips_and_defaults() {
        let src = r#"(
            skill_id: "painter",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            collision_windows: [
                ( id: "roll", spawn: Scheduled( phase: Active ), active_duration: 1.0,
                  shape: Sphere( radius: 0.3 ), motion: Linear( speed: 8.0 ),
                  hit_filter: Enemies, hit_mode: OncePerTarget,
                  paints: Some(( surface: "frost", radius: 0.45, mode: Trail( step: 0.8 ) )) ),
            ],
        )"#;
        let tl: CastTimeline = ron::from_str(src).expect("paints content parses");
        let paints = tl.collision_windows[0].paints.as_ref().expect("paints present");
        assert_eq!(paints.surface, "frost");
        assert_eq!(paints.radius, 0.45);
        assert!(matches!(paints.mode, PaintMode::Trail { step } if step == 0.8));
        assert!(paints.lifetime.is_none());
        validate_timeline(&tl).expect("valid paints validates");
        // Round-trip.
        let s = ron::ser::to_string_pretty(&tl, Default::default()).unwrap();
        let back: CastTimeline = ron::from_str(&s).unwrap();
        assert!(back.collision_windows[0].paints.is_some());
        // Omitted -> None (every existing .cast.ron parses unchanged).
        let plain = timeline_with(basic_window("w"));
        assert!(plain.collision_windows[0].paints.is_none());
    }

    /// (Surfaces) invalid paints fail validation loudly.
    #[test]
    fn paints_validation_rejects_bad_values() {
        let mut win = basic_window("w");
        win.paints = Some(PaintSpec {
            surface: "frost".into(),
            radius: 0.0,
            mode: PaintMode::OnEnd,
            lifetime: None,
        });
        assert!(validate_timeline(&timeline_with(win)).is_err(), "radius must be > 0");

        let mut win = basic_window("w");
        win.paints = Some(PaintSpec {
            surface: "frost".into(),
            radius: 0.4,
            mode: PaintMode::Trail { step: 0.0 },
            lifetime: None,
        });
        assert!(validate_timeline(&timeline_with(win)).is_err(), "trail step must be > 0");

        let mut win = basic_window("w");
        win.paints = Some(PaintSpec {
            surface: "".into(),
            radius: 0.4,
            mode: PaintMode::OnEnd,
            lifetime: None,
        });
        assert!(validate_timeline(&timeline_with(win)).is_err(), "surface must be non-empty");

        let mut win = basic_window("w");
        win.paints = Some(PaintSpec {
            surface: "frost".into(),
            radius: 0.4,
            mode: PaintMode::OnEnd,
            lifetime: Some(0.0),
        });
        assert!(
            validate_timeline(&timeline_with(win)).is_err(),
            "lifetime override must be > 0"
        );
    }

    /// (Surfaces) `on_surface` on GroundPoint round-trips and defaults.
    #[test]
    fn on_surface_round_trips() {
        let src = r#"(
            skill_id: "gated",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            acquisition: GroundPoint( range: 60.0, fallback: Fizzle,
                on_surface: Some(( surface: "frost", consume: true )) ),
        )"#;
        let tl: CastTimeline = ron::from_str(src).expect("on_surface parses");
        let Acquisition::GroundPoint { on_surface, .. } = &tl.acquisition else {
            panic!("GroundPoint expected");
        };
        let req = on_surface.as_ref().expect("requirement present");
        assert_eq!(req.surface, "frost");
        assert!(req.snap, "snap defaults to true");
        assert!(req.consume);
        // Old content (no on_surface) still parses.
        let old = r#"(
            skill_id: "plain",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            acquisition: GroundPoint( range: 30.0, fallback: Fizzle ),
        )"#;
        let tl: CastTimeline = ron::from_str(old).expect("pre-surfaces GroundPoint parses");
        let Acquisition::GroundPoint { on_surface, .. } = &tl.acquisition else {
            panic!()
        };
        assert!(on_surface.is_none());
    }
}
