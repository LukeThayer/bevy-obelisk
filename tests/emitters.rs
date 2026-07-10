//! Task 11 — emitters + `SpawnRng` + motion override (spec §3.2, "blizzard's machinery"): a
//! live `Scheduled` window (a storm cloud) rains `Template`-window instances (shards) on a
//! deterministic clock, jittered in position via a RNG stream dedicated to emission
//! (`SpawnRng`) that must NEVER perturb `CombatRng`'s draw sequence. `MotionDirection::Down`
//! lets a shard fall straight out of the sky regardless of the storm's own facing.
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::assets::{
    AcqFallback, Acquisition, CastTimeline, CollisionShape, CollisionWindow, Emitter, HitFilter,
    HitMode, MotionDirection, PhaseDurations, VolumeMotion, WindowAnchor, WindowPhase, WindowSpawn,
};
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use obelisk_bevy::vfx::ObeliskCueExt;
use stat_core::StatBlock;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = mana;
    b.current_mana = mana;
    b
}

/// The Template shard: falls straight down (`MotionDirection::Down`, independent of the storm's
/// own facing) at 8 u/s, wide enough to catch a jittered landing near the dummy below.
fn shard_window() -> CollisionWindow {
    CollisionWindow {
        id: "shard".into(),
        spawn: WindowSpawn::Template,
        anchor: WindowAnchor::Caster,
        anchor_offset: Vec3::ZERO,
        strikes: true,
        active_duration: 3.0,
        shape: CollisionShape::Sphere { radius: 1.0 },
        motion: VolumeMotion::Linear { speed: 8.0 },
        motion_direction: MotionDirection::Down,
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::OncePerTarget,
        rehit_interval: None,
        emitter: None,
        paints: None,
    }
}

/// The storm cloud: a non-striking carrier (`strikes: false` — it's atmosphere, not a hitbox)
/// anchored 8 units above the cast point, emitting one `shard` every 0.25s (`rate: 4.0`) jittered
/// within a 2-unit xz disc.
fn storm_window() -> CollisionWindow {
    CollisionWindow {
        id: "storm".into(),
        spawn: WindowSpawn::Scheduled {
            phase: WindowPhase::Active,
            offset: 0.0,
        },
        anchor: WindowAnchor::CastPoint,
        anchor_offset: Vec3::new(0.0, 8.0, 0.0),
        strikes: false,
        active_duration: 12.0,
        shape: CollisionShape::Sphere { radius: 3.0 },
        motion: VolumeMotion::Static,
        motion_direction: MotionDirection::Inherit,
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::OncePerTarget,
        rehit_interval: None,
        emitter: Some(Emitter {
            rate: 4.0,
            jitter: 2.0,
            window: "shard".into(),
        }),
        paints: None,
    }
}

/// `GroundPoint` (wide range — the determinism test casts far away on purpose) falling back to
/// `SelfPoint`, mirroring `tests/acquisition.rs`'s blizzard stub. `emit_shard` is the ONLY
/// authored cue: pins the "emitted instances fire emit_{id}, never on_window_{id}" rule (nothing
/// is authored under `on_window_shard`/`on_window_storm`, so those firing would show up as a
/// missing-cue silence, not a wrong-cue assertion — the dedicated cue-branch test below covers
/// the positive half).
fn blizzard_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "blizzard".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 0.05,
            recovery: 0.05,
        },
        collision_windows: vec![storm_window(), shard_window()],
        acquisition: Acquisition::GroundPoint {
            range: 2000.0,
            fallback: AcqFallback::Then(Box::new(Acquisition::SelfPoint)),
        },
        vfx_cues: HashMap::from([("emit_shard".to_string(), "blizzard_emit_shard".to_string())]),
        chain_radius: 6.0,
        chargeable: false,
        max_hold: 1.0,
        cues: HashMap::new(),
        charge_cues: Vec::new(),
    }
}

/// Player-only harness (skill_id `blizzard`, `tests/fixtures/skills/blizzard.toml`) with the
/// storm+shard timeline above registered in place of any real `.cast.ron`.
fn harness_with_blizzard(seed: u64) -> (ObeliskTestApp, Entity) {
    let mut t = ObeliskTestApp::new(seed);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(blizzard_timeline());
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("blizzard".into(), handle);
    let caster = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("caster", 100.0, 100.0)),
            Faction::Player,
            ObeliskId("caster".into()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    t.app.update();
    (t, caster)
}

/// Advance roughly `secs` seconds at the harness's 60Hz fixed tick.
fn app_step_secs(t: &mut ObeliskTestApp, secs: f32) {
    t.advance_ticks((secs * 60.0).round() as usize);
}

/// Casts blizzard at `point`, steps ~1.05s (4 emission boundaries at rate 4.0), and returns every
/// `emit_shard` cue's spawn position (recorded via the SAME mechanism `vfx_content.rs` uses for
/// other cue slots — an `observe_cue` handler pushed into a shared `Vec`).
fn shard_spawn_positions(seed: u64, point: Vec3) -> Vec<Vec3> {
    let (mut t, caster) = harness_with_blizzard(seed);
    let positions: Arc<Mutex<Vec<Vec3>>> = Arc::new(Mutex::new(Vec::new()));
    let p = positions.clone();
    t.app.observe_cue("blizzard_emit_shard", move |cue, _cmds| {
        p.lock().unwrap().push(cue.position);
    });
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at_point("blizzard", point);
    app_step_secs(&mut t, 1.05);
    let out = positions.lock().unwrap().clone();
    out
}

/// The brief's determinism test: rate honored, jitter bounded, and re-running the SAME seed
/// reproduces the SAME spawn positions (SpawnRng is deterministic) while a DIFFERENT seed
/// diverges (the seed genuinely drives the jitter, not a hardcoded constant).
#[test]
fn emitter_rains_template_windows_deterministically() {
    let point = Vec3::new(5.0, 0.0, 3.0);

    let positions_a = shard_spawn_positions(11, point);
    assert_eq!(
        positions_a.len(),
        4,
        "rate honored: 4.0/s over 1.05s crosses exactly 4 boundaries, got {:?}",
        positions_a
    );
    assert!(
        positions_a
            .iter()
            .all(|p| (p.xz() - Vec2::new(5.0, 3.0)).length() <= 2.01),
        "jitter bounded to the authored radius (2.0): {:?}",
        positions_a
    );

    let positions_b = shard_spawn_positions(11, point);
    assert_eq!(
        positions_a, positions_b,
        "SpawnRng is deterministic for a fixed seed"
    );

    let positions_c = shard_spawn_positions(12, point);
    assert_ne!(
        positions_a, positions_c,
        "a different seed must draw different jitter — sanity check that the seed genuinely \
         drives the result rather than being ignored"
    );
}

/// Real firebolt collision window (mirrors `assets/skills/firebolt.cast.ron`'s `bolt` window,
/// in-memory so this test doesn't need to wait on an async asset load).
fn firebolt_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "firebolt".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 0.05,
            recovery: 0.05,
        },
        collision_windows: vec![CollisionWindow {
            id: "bolt".into(),
            spawn: WindowSpawn::Scheduled {
                phase: WindowPhase::Active,
                offset: 0.0,
            },
            anchor: WindowAnchor::Caster,
            anchor_offset: Vec3::ZERO,
            strikes: true,
            active_duration: 1.0,
            shape: CollisionShape::Sphere { radius: 0.5 },
            motion: VolumeMotion::Linear { speed: 20.0 },
            motion_direction: MotionDirection::Inherit,
            hit_filter: HitFilter::Enemies,
            hit_mode: HitMode::FirstOnly,
            rehit_interval: None,
            emitter: None,
            paints: None,
        }],
        acquisition: Default::default(),
        vfx_cues: HashMap::new(),
        chain_radius: 6.0,
        chargeable: false,
        max_hold: 1.0,
        cues: HashMap::new(),
        charge_cues: Vec::new(),
    }
}

/// A 50% crit chance makes `firebolt`'s `total_damage` genuinely depend on WHICH value
/// `CombatRng` draws for the crit roll (not just whether a draw happens) — the sensitive
/// probe this test needs.
fn crit_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = make_block(id, life, mana);
    b.critical_chance.base = 50.0;
    b
}

/// Casts `firebolt` at a stationary dummy 6 times (optionally with `blizzard` ALSO cast, far
/// away, so its shards draw `SpawnRng` jitter but never overlap anything — isolating "does
/// emission perturb `CombatRng`" from "do shard hits themselves draw `CombatRng`", which would be
/// a separate, legitimate combat draw and not what this test is about). Returns each
/// `firebolt` hit's `total_damage`, in order.
fn run_firebolt_totals(seed: u64, also_cast_blizzard: bool) -> Vec<f64> {
    let mut t = ObeliskTestApp::new(seed);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(firebolt_timeline());
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("firebolt".into(), handle);
    if also_cast_blizzard {
        let bh = t
            .app
            .world_mut()
            .resource_mut::<Assets<CastTimeline>>()
            .add(blizzard_timeline());
        t.app
            .world_mut()
            .resource_mut::<CastTimelineHandles>()
            .0
            .insert("blizzard".into(), bh);
    }

    let caster = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(crit_block("caster", 100.0, 1000.0)),
            Faction::Player,
            ObeliskId("caster".into()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    let dummy = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("dummy", 1_000_000.0, 0.0)),
            Faction::Enemy,
            ObeliskId("dummy".into()),
            Transform::from_xyz(0.0, 0.0, 2.0),
        ))
        .id();
    {
        let mut c = t.app.world_mut().commands();
        insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();

    if also_cast_blizzard {
        t.app
            .world_mut()
            .commands()
            .entity(caster)
            .cast_skill_at_point("blizzard", Vec3::new(500.0, 0.0, 500.0));
        app_step_secs(&mut t, 0.5);
    }

    for _ in 0..6 {
        t.app
            .world_mut()
            .commands()
            .entity(caster)
            .cast_skill_at("firebolt", dummy);
        t.advance_ticks(90);
    }

    t.rec()
        .damage_resolved
        .iter()
        .filter(|d| d.skill_id == "firebolt")
        .map(|d| d.total_damage)
        .collect()
}

/// The brief's golden-guard-in-miniature: identical `DamageResolved` totals for an UNRELATED
/// skill's hits whether or not an emitter skill is ALSO active — `SpawnRng`'s draws must never
/// leak into `CombatRng`'s sequence.
#[test]
fn spawn_rng_does_not_perturb_combat_rng() {
    let seed = 77;
    let without = run_firebolt_totals(seed, false);
    let with = run_firebolt_totals(seed, true);
    assert!(
        !without.is_empty(),
        "firebolt must actually land hits for this comparison to mean anything"
    );
    assert_eq!(
        without, with,
        "casting an emitter skill (blizzard, far away, never striking anything) alongside \
         firebolt must not perturb CombatRng's draw sequence for firebolt's own hits"
    );
}

/// Blizzard end-to-end: `GroundPoint` acquisition + a `Scheduled` storm (non-striking,
/// `strikes: false`) emitting `Template` shards (`Down` motion) that rain onto a dummy standing
/// under the storm. Every shard hit must land AND be mana-free (`Hitbox.emitted` extending
/// `is_free_hit`) — since the storm itself never strikes (it's a carrier) and every shard hit is
/// free, the WHOLE cast spends zero mana end to end.
#[test]
fn blizzard_shards_rain_and_hit_the_dummy_mana_free() {
    let (mut t, caster) = harness_with_blizzard(21);
    let point = Vec3::new(5.0, 0.0, 3.0);
    let dummy = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("dummy", 1_000_000.0, 0.0)),
            Faction::Enemy,
            ObeliskId("dummy".into()),
            Transform::from_xyz(point.x, point.y, point.z),
        ))
        .id();
    {
        let mut c = t.app.world_mut().commands();
        // Wide enough to catch a shard landing anywhere within the storm's 2-unit jitter disc
        // plus the shard's own 1.0 radius.
        insert_hurtbox(&mut c, dummy, 3.5, point);
    }
    t.app.update();

    let mana_before = t
        .app
        .world()
        .get::<Attributes>(caster)
        .unwrap()
        .0
        .current_mana;

    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at_point("blizzard", point);
    app_step_secs(&mut t, 3.0);

    let shard_hits: Vec<_> = t
        .rec()
        .hit_confirmed
        .iter()
        .filter(|h| h.skill_id == "blizzard" && h.window_id == "shard")
        .collect();
    assert!(
        !shard_hits.is_empty(),
        "shards must actually fall onto and hit the dummy standing under the storm"
    );
    assert!(
        shard_hits.iter().all(|h| h.emitted),
        "every shard hit is flagged emitted: {:?}",
        shard_hits
    );

    let mana_after = t
        .app
        .world()
        .get::<Attributes>(caster)
        .unwrap()
        .0
        .current_mana;
    assert_eq!(
        mana_after, mana_before,
        "the storm never strikes (strikes: false) and every shard hit is free (emitted) — the \
         whole cast bills NO mana end to end"
    );
}
