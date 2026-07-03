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
    GroundPoint { range: f32, fallback: AcqFallback },
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
}

fn default_true() -> bool {
    true
}

/// When (whether) a window enters the world on its own.
#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq)]
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
}
