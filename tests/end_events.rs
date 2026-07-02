//! Increment 1 — window end events + chaining (spec: 2026-07-02-event-driven-skill-phases.md).
//!
//! Every hitbox ends exactly once with a reason (`HitEntity` / `HitWorld` / `Fuse`) and a world
//! position; the window's authored `on_end` reaction chains the next window AT that position,
//! carrying the original caster + charge. These tests drive the REAL sim (`ObeliskTestApp`,
//! seeded RNG, avian spatial) end to end for each reason, plus the `on_end_{id}` cue and the
//! loader validation rules.
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::assets::{
    validate_timeline, CastDelivery, CastTargeting, CollisionShape, CollisionWindow, EndReaction,
    HitFilter, HitMode, OnEnd, PhaseDurations, VolumeMotion, WindowPhase,
};
use obelisk_bevy::events::{EndReason, HitboxWorldHit};
use obelisk_bevy::prelude::*;
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

fn window(id: &str, phase: WindowPhase, motion: VolumeMotion, duration: f32) -> CollisionWindow {
    CollisionWindow {
        id: id.into(),
        spawn_phase: phase,
        spawn_offset: 0.0,
        active_duration: duration,
        shape: CollisionShape::Sphere { radius: 0.5 },
        motion,
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::FirstOnly,
        rehit_interval: None,
        on_end: OnEnd::default(),
    }
}

/// bolt (Linear 10 u/s, fuse `bolt_duration`) --on_end(all three reasons)--> blast
/// (Chained sphere 1.5, instant-ish). Registered under the fixture skill id "firebolt".
fn chaining_timeline(bolt_duration: f32) -> CastTimeline {
    let mut bolt = window(
        "bolt",
        WindowPhase::Active,
        VolumeMotion::Linear { speed: 10.0 },
        bolt_duration,
    );
    bolt.on_end = OnEnd {
        hit: Some(EndReaction::Chain("blast".into())),
        world: Some(EndReaction::Chain("blast".into())),
        fuse: Some(EndReaction::Chain("blast".into())),
    };
    let mut blast = window(
        "blast",
        WindowPhase::Chained,
        VolumeMotion::Static,
        0.05,
    );
    blast.shape = CollisionShape::Sphere { radius: 1.5 };
    blast.hit_mode = HitMode::OncePerTarget;
    CastTimeline {
        skill_id: "firebolt".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 0.05,
            recovery: 0.05,
        },
        collision_windows: vec![bolt, blast],
        targeting: CastTargeting::SingleEntity { range: 15.0 },
        delivery: CastDelivery::Projectile { speed: 10.0 },
        vfx_cues: HashMap::from([("on_end_bolt".to_string(), "firebolt_boom".to_string())]),
    }
}

fn setup(seed: u64, tl: CastTimeline) -> (ObeliskTestApp, Entity, Entity) {
    let mut t = ObeliskTestApp::new(seed);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(tl);
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("firebolt".into(), handle);
    let player = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("player", 100.0, 100.0)),
            Faction::Player,
            ObeliskId("player".into()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    let dummy = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("dummy", 200.0, 0.0)),
            Faction::Enemy,
            ObeliskId("dummy".into()),
            Transform::from_xyz(0.0, 0.0, 2.0),
        ))
        .id();
    {
        let mut commands = t.app.world_mut().commands();
        insert_hurtbox(&mut commands, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();
    (t, player, dummy)
}

#[test]
fn fuse_end_fires_event_and_chains_at_the_fuse_position() {
    // Bolt aimed AWAY from the dummy (+X): nothing to hit, the 0.2 s fuse ends it mid-air at
    // x ≈ 2.0, and the chained blast spawns THERE (far from the dummy → no damage).
    let (mut t, player, _dummy) = setup(7, chaining_timeline(0.2));
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("firebolt", Dir3::X);
    t.advance_ticks(60);

    let rec = t.rec();
    let ended: Vec<_> = rec
        .hitbox_ended
        .iter()
        .filter(|e| e.window_id == "bolt")
        .collect();
    assert_eq!(ended.len(), 1, "the bolt ends exactly once");
    assert_eq!(ended[0].reason, EndReason::Fuse);
    assert!(
        (ended[0].position.x - 2.0).abs() < 0.25,
        "fuse position ~10 u/s * 0.2 s: {:?}",
        ended[0].position
    );
    assert!(
        rec.hit_window_opened.iter().any(|w| w.window_id == "blast"),
        "fuse chains the blast"
    );
    // The blast itself then fuse-ends (no reaction authored on it).
    assert!(rec
        .hitbox_ended
        .iter()
        .any(|e| e.window_id == "blast" && e.reason == EndReason::Fuse));
    // The on_end_bolt cue fired AT the fuse position with the OnEnd kind.
    let cue = rec
        .cues
        .iter()
        .find(|c| c.cue_id == "firebolt_boom")
        .expect("on_end_bolt cue fires");
    assert_eq!(cue.kind, CueKind::OnEnd);
    assert!((cue.position.x - 2.0).abs() < 0.25);
    assert!(rec.damage_resolved.is_empty(), "nothing was near the blast");
}

#[test]
fn entity_hit_end_chains_the_blast_onto_the_victim() {
    // Bolt aimed AT the dummy (z = 2): FirstOnly hit ends it, the blast spawns at the impact
    // and (radius 1.5) hits the dummy too → bolt damage + blast damage.
    let (mut t, player, dummy) = setup(11, chaining_timeline(2.0));
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("firebolt", Dir3::Z);
    t.advance_ticks(120);

    let rec = t.rec();
    let ended: Vec<_> = rec
        .hitbox_ended
        .iter()
        .filter(|e| e.window_id == "bolt")
        .collect();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].reason, EndReason::HitEntity);
    // The bolt ends at first CONTACT: its sphere (r 0.5) meets the hurtbox (r 0.6) at
    // z ≈ 2.0 − 1.1 — the impact surface, which is exactly where the explosion belongs.
    assert!(
        ended[0].position.z > 0.8 && ended[0].position.z < 2.0,
        "ends at the contact point short of the victim center: {:?}",
        ended[0].position
    );
    let bolt_hits = rec
        .hit_confirmed
        .iter()
        .filter(|h| h.window_id == "bolt")
        .count();
    let blast_hits = rec
        .hit_confirmed
        .iter()
        .filter(|h| h.window_id == "blast" && h.target == dummy)
        .count();
    assert_eq!(bolt_hits, 1, "FirstOnly bolt hits once");
    assert_eq!(blast_hits, 1, "chained blast splashes the victim");
    assert_eq!(rec.damage_resolved.len(), 2, "direct hit + splash resolve");
    assert!(
        rec.damage_resolved.iter().all(|d| d.caster == player),
        "chained damage keeps the original caster attribution"
    );
}

#[test]
fn host_world_hit_ends_with_hit_world_at_the_impact_point() {
    // Long fuse, aimed away; the HOST reports a world impact (what arena's floor plane does).
    let (mut t, player, _dummy) = setup(13, chaining_timeline(5.0));
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("firebolt", Dir3::X);
    t.advance_ticks(10); // window is open, bolt in flight

    let hitbox = t
        .app
        .world_mut()
        .query_filtered::<Entity, With<obelisk_bevy::prelude::Hitbox>>()
        .single(t.app.world())
        .expect("bolt hitbox in flight");
    let impact = Vec3::new(1.2, 0.0, 0.0);
    t.app.world_mut().trigger(HitboxWorldHit {
        hitbox,
        position: impact,
    });
    t.advance_ticks(5);

    let rec = t.rec();
    let ended = rec
        .hitbox_ended
        .iter()
        .find(|e| e.window_id == "bolt")
        .expect("bolt ended");
    assert_eq!(ended.reason, EndReason::HitWorld);
    assert_eq!(ended.position, impact, "ends at the host-reported point");
    assert!(
        rec.hit_window_opened.iter().any(|w| w.window_id == "blast"),
        "world impact chains the blast there"
    );
}

#[test]
fn windows_without_on_end_just_end() {
    let mut tl = chaining_timeline(0.2);
    tl.collision_windows[0].on_end = OnEnd::default();
    tl.collision_windows.truncate(1);
    let (mut t, player, _dummy) = setup(17, tl);
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("firebolt", Dir3::X);
    t.advance_ticks(60);

    let rec = t.rec();
    assert!(rec
        .hitbox_ended
        .iter()
        .any(|e| e.window_id == "bolt" && e.reason == EndReason::Fuse));
    assert!(
        !rec.hit_window_opened.iter().any(|w| w.window_id == "blast"),
        "no reaction authored, nothing chains"
    );
}

#[test]
fn validation_rejects_bad_chain_graphs() {
    // Unknown target.
    let mut tl = chaining_timeline(0.2);
    tl.collision_windows[0].on_end.hit = Some(EndReaction::Chain("nope".into()));
    assert!(validate_timeline(&tl).unwrap_err().contains("unknown"));

    // Target not Chained.
    let mut tl = chaining_timeline(0.2);
    tl.collision_windows[1].spawn_phase = WindowPhase::Active;
    assert!(validate_timeline(&tl)
        .unwrap_err()
        .contains("spawn_phase: Chained"));

    // Cycle (blast chains back to itself).
    let mut tl = chaining_timeline(0.2);
    tl.collision_windows[1].on_end.fuse = Some(EndReaction::Chain("blast".into()));
    assert!(validate_timeline(&tl).unwrap_err().contains("cycle"));

    // The good graph passes.
    assert!(validate_timeline(&chaining_timeline(0.2)).is_ok());
}
