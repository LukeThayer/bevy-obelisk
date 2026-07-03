//! Task 10 — authored acquisition + cast-point preservation (spec §3.2). Replaces the range gate
//! that died with `CastTargeting` (Task 9) and fixes the historic "blizzard blocker": a
//! `CastAim::Point` no longer gets collapsed to a direction when the timeline authors a
//! `GroundPoint` acquisition — the point is PRESERVED into `ActiveCast::cast_point`, which
//! `WindowAnchor::CastPoint` windows spawn at.
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::assets::{
    AcqFallback, Acquisition, CastTimeline, CollisionShape, CollisionWindow, HitFilter, HitMode,
    PhaseDurations, VolumeMotion, WindowAnchor, WindowPhase, WindowSpawn,
};
use obelisk_bevy::events::CastRejectReason;
use obelisk_bevy::prelude::*;
use obelisk_bevy::spatial::boxes::Hitbox;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;
use std::collections::HashMap;

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = mana;
    b.current_mana = mana;
    b
}

/// A non-striking "storm cloud" window: anchored `CastPoint` + 8 units up. `strikes: false`
/// (a carrier volume — see `CollisionWindow::strikes`) keeps this suite focused purely on WHERE
/// the window spawns, not on overlap/damage machinery already covered elsewhere.
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
        active_duration: 1.0,
        shape: CollisionShape::Sphere { radius: 3.0 },
        motion: VolumeMotion::Static,
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::OncePerTarget,
        rehit_interval: None,
    }
}

/// The "blizzard" stub timeline: `GroundPoint` acquisition (range wide enough to cover both
/// tests below), falling back to `SelfPoint` when no point was aimed — so the SAME timeline
/// exercises both the point-preserved path and the self-point fallback path.
fn blizzard_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "blizzard".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 0.05,
            recovery: 0.05,
        },
        collision_windows: vec![storm_window()],
        acquisition: Acquisition::GroundPoint {
            range: 10.0,
            fallback: AcqFallback::Then(Box::new(Acquisition::SelfPoint)),
        },
        vfx_cues: HashMap::new(),
    }
}

/// Player-only harness (skill_id `blizzard`, `tests/fixtures/skills/blizzard.toml`) with the
/// stub timeline above registered in place of any real `.cast.ron` — mirrors the in-memory
/// timeline-override pattern used by `tests/beam_retarget.rs`'s `chain_timeline()`.
fn harness_with_blizzard_stub() -> (ObeliskTestApp, Entity) {
    let mut t = ObeliskTestApp::new(11);
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

/// World position of the first live `Hitbox` (any window) — this suite only ever has one at a
/// time.
fn first_hitbox_pos(t: &mut ObeliskTestApp) -> Vec3 {
    let mut q = t.app.world_mut().query::<(&Transform, &Hitbox)>();
    q.iter(t.app.world())
        .next()
        .map(|(tf, _)| tf.translation)
        .expect("a hitbox spawned")
}

fn caster_pos(t: &mut ObeliskTestApp, caster: Entity) -> Vec3 {
    t.app.world().get::<Transform>(caster).unwrap().translation
}

/// The historic "blizzard blocker" fix: a `GroundPoint`-acquired cast preserves its aimed point
/// into `ActiveCast::cast_point` — a `CastPoint`-anchored window spawns ABOVE THE POINT, not
/// collapsed to a direction from the caster.
#[test]
fn ground_point_is_preserved_to_cast_point_anchored_windows() {
    let (mut t, caster) = harness_with_blizzard_stub();
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at_point("blizzard", Vec3::new(5.0, 0.0, 3.0));
    t.advance_ticks(10);

    assert!(
        t.rec().cast_rejected.is_empty(),
        "a point within acquisition range must validate: {:?}",
        t.rec().cast_rejected.first().map(|r| &r.reason)
    );
    let w = first_hitbox_pos(&mut t);
    assert!(
        (w - Vec3::new(5.0, 8.0, 3.0)).length() < 0.1,
        "storm spawns ABOVE THE AIMED POINT (5,8,3), not collapsed to a direction: got {w:?}"
    );
}

/// No point provided (a direction-aimed cast of a `GroundPoint` skill): the authored
/// `Then(SelfPoint)` fallback resolves the cast point to the CASTER's own position.
#[test]
fn ground_point_falls_back_to_self_point() {
    let (mut t, caster) = harness_with_blizzard_stub();
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_dir("blizzard", Dir3::X);
    t.advance_ticks(10);

    assert!(
        t.rec().cast_rejected.is_empty(),
        "GroundPoint falls back to SelfPoint rather than rejecting: {:?}",
        t.rec().cast_rejected.first().map(|r| &r.reason)
    );
    let caster_p = caster_pos(&mut t, caster);
    let w = first_hitbox_pos(&mut t);
    assert!(
        (w - (caster_p + Vec3::Y * 8.0)).length() < 0.2,
        "storm spawns above the CASTER (fallback cast point): got {w:?}, caster {caster_p:?}"
    );
}

/// `HitscanEntity` + `Fizzle`, aimed by DIRECTION (no entity at all): the acquisition's own
/// requirement ("must be `CastAim::Entity`") is unmet, so the `Fizzle` fallback fires a paid
/// `CastRejected` — REJECTION-REASON MAPPING: reuses `NoTarget` ("no valid target of the kind
/// this branch needs"), not a new variant — see `resolve_acquisition`'s doc comment.
///
/// "Paid" mirrors `tests/beam_retarget.rs::direction_aimed_beam_is_a_paid_fizzle`'s semantics:
/// checked here (rather than assumed) that today's paid-fizzle spends NO mana — obelisk bills
/// mana per-hit only (`src/combat/system.rs`'s billing-rule doc), never at cast validation — so
/// this acquisition-level Fizzle, exactly like the beam's target-less miss, leaves the caster's
/// mana untouched. Unlike the beam case, though, this Fizzle happens IN `validate_casts` itself
/// (before any `ActiveCast`/`CastBegan`), so the cast never begins at all — no cooldown starts
/// either.
#[test]
fn hitscan_fizzle_is_a_paid_rejection() {
    let mut t = ObeliskTestApp::new(9);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(CastTimeline {
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
                motion: VolumeMotion::Linear { speed: 10.0 },
                hit_filter: HitFilter::Enemies,
                hit_mode: HitMode::FirstOnly,
                rehit_interval: None,
            }],
            acquisition: Acquisition::HitscanEntity {
                range: 20.0,
                filter: HitFilter::Enemies,
                fallback: AcqFallback::Fizzle,
            },
            vfx_cues: HashMap::new(),
        });
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("firebolt".into(), handle);
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
        .cast_skill_dir("firebolt", Dir3::X);
    t.advance_ticks(10);

    let rec = t.rec();
    assert!(
        rec.cast_began.is_empty(),
        "HitscanEntity+Fizzle rejects BEFORE the cast begins (no ActiveCast, no CastBegan)"
    );
    assert_eq!(
        rec.cast_rejected.len(),
        1,
        "exactly one CastRejected: {:?}",
        rec.cast_rejected
    );
    assert_eq!(
        rec.cast_rejected[0].reason,
        CastRejectReason::NoTarget,
        "wrong-aim-kind (direction, not Entity) maps to the reused NoTarget reason"
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
        "a paid fizzle still spends NO mana (billing is per-hit, and no hit ever happens here) \
         — same as the beam paid-fizzle's untouched mana"
    );
}

/// `validate_timeline` (Task 10 addition): a `CastPoint`-anchored window authored on a timeline
/// whose acquisition is `Aim` (the default — cannot ever produce a cast point) is a load-time
/// error, catching the authoring mistake instead of silently falling back to the caster's
/// position at spawn time.
#[test]
fn cast_point_anchor_in_aim_skill_fails_validation() {
    let tl = CastTimeline {
        skill_id: "bad".into(),
        phase_durations: PhaseDurations {
            windup: 0.1,
            active: 0.1,
            recovery: 0.1,
        },
        collision_windows: vec![storm_window()],
        acquisition: Acquisition::Aim, // the default — never produces a point
        vfx_cues: HashMap::new(),
    };
    let err = obelisk_bevy::assets::validate_timeline(&tl).unwrap_err();
    assert!(
        err.contains("storm") && err.contains("CastPoint"),
        "error names the offending window and anchor: {err}"
    );
}
