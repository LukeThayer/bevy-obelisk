//! Window END EVENTS (spec: 2026-07-02-event-driven-skill-phases.md, schema v2 per Task 9).
//!
//! Every hitbox ends exactly once with a reason (`HitEntity` / `HitWorld` / `Fuse`) and a world
//! position, and the `on_end_{id}` cue fires AT that position. These tests drive the REAL sim
//! (`ObeliskTestApp`, seeded RNG, avian spatial) end to end for each reason, plus the schema-v2
//! `strikes: false` carrier-volume gate.
//!
//! The v1 `on_end: Chain` SPAWNING tests that used to live here are deleted with the authored
//! reaction schema: end-driven causality now runs through rules triggers, covered by
//! `tests/triggered_exec.rs` (Task 7 hit-phase execution; Task 8 OnImpact/OnExpire lifecycle
//! execution at the end position).
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::assets::{
    CollisionShape, CollisionWindow, HitFilter, HitMode, PhaseDurations, VolumeMotion,
    WindowAnchor, WindowPhase, WindowSpawn,
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
        spawn: WindowSpawn::Scheduled { phase, offset: 0.0 },
        anchor: WindowAnchor::Caster,
        anchor_offset: Vec3::ZERO,
        strikes: true,
        active_duration: duration,
        shape: CollisionShape::Sphere { radius: 0.5 },
        motion,
        motion_direction: Default::default(),
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::FirstOnly,
        rehit_interval: None,
        emitter: None,
        paints: None,
    }
}

/// One bolt window (Linear 10 u/s, fuse `bolt_duration`), with an `on_end_bolt` cue authored.
/// Registered under the fixture skill id "firebolt".
fn bolt_timeline(bolt_duration: f32) -> CastTimeline {
    let bolt = window(
        "bolt",
        WindowPhase::Active,
        VolumeMotion::Linear { speed: 10.0 },
        bolt_duration,
    );
    CastTimeline {
        skill_id: "firebolt".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 0.05,
            recovery: 0.05,
        },
        collision_windows: vec![bolt],
        acquisition: Default::default(),
        vfx_cues: HashMap::from([
            ("on_end_bolt".to_string(), "firebolt_boom".to_string()),
            ("on_cast".to_string(), "firebolt_cast".to_string()),
        ]),
        chain_radius: 6.0,
        chargeable: false,
        max_hold: 1.0,
        cues: HashMap::new(),
        charge_cues: Vec::new(),
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
fn fuse_end_fires_event_and_cue_at_the_fuse_position() {
    // Bolt aimed AWAY from the dummy (+X): nothing to hit, the 0.2 s fuse ends it mid-air at
    // x ≈ 2.0 — the event and the on_end_bolt cue both carry THAT position.
    let (mut t, player, _dummy) = setup(7, bolt_timeline(0.2));
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
    // The on_end_bolt cue fired AT the fuse position with the OnEnd kind.
    let cue = rec
        .cues
        .iter()
        .find(|c| c.cue_id == "firebolt_boom")
        .expect("on_end_bolt cue fires");
    assert_eq!(cue.kind, CueKind::OnEnd);
    assert!((cue.position.x - 2.0).abs() < 0.25);
    assert!(rec.damage_resolved.is_empty(), "the bolt hit nothing");
}

#[test]
fn entity_hit_end_carries_the_contact_position() {
    // Bolt aimed AT the dummy (z = 2): FirstOnly hit ends it at the contact point.
    let (mut t, player, dummy) = setup(11, bolt_timeline(2.0));
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
    // z ≈ 2.0 − 1.1 — the impact surface, exactly where a triggered explosion would belong.
    assert!(
        ended[0].position.z > 0.8 && ended[0].position.z < 2.0,
        "ends at the contact point short of the victim center: {:?}",
        ended[0].position
    );
    let bolt_hits = rec
        .hit_confirmed
        .iter()
        .filter(|h| h.window_id == "bolt" && h.target == dummy)
        .count();
    assert_eq!(bolt_hits, 1, "FirstOnly bolt hits once");
    assert_eq!(rec.damage_resolved.len(), 1, "the direct hit resolves");
    assert!(
        rec.damage_resolved.iter().all(|d| d.caster == player),
        "damage keeps the caster attribution"
    );
}

#[test]
fn host_world_hit_ends_with_hit_world_at_the_impact_point() {
    // Long fuse, aimed away; the HOST reports a world impact (what arena's floor plane does).
    let (mut t, player, _dummy) = setup(13, bolt_timeline(5.0));
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
}

#[test]
fn hit_and_end_events_carry_position_and_depth() {
    // Bolt aimed AT the dummy: a plain, un-triggered cast — depth 0, hop 0 — but still carries
    // a real world position from the hitbox transform at the moment of the hit.
    let (mut t, player, dummy) = setup(19, bolt_timeline(2.0));
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("firebolt", Dir3::Z);
    t.advance_ticks(120);

    let rec = t.rec();
    let hit = rec
        .hit_confirmed
        .iter()
        .find(|h| h.target == dummy)
        .expect("a hit");
    assert!(
        hit.position.length() > 0.0,
        "hit carries the hitbox position"
    );
    assert_eq!(hit.depth, 0, "a player cast is depth 0");
    assert_eq!(hit.hop, 0, "a plain cast has no chain hops");
    let ended = rec
        .hitbox_ended
        .iter()
        .find(|e| e.window_id == "bolt")
        .expect("an end");
    assert_eq!(ended.depth, 0);
}

/// Task 1 (phase-3 prerequisite): every cue slot carries the cast's charge, and the `on_end_{id}`
/// cue additionally carries the terminating `EndReason`. Full-charge cast (255) + a long fuse
/// aimed away so the HOST reports a world impact — mirrors
/// `host_world_hit_ends_with_hit_world_at_the_impact_point`'s pattern exactly, just charged.
#[test]
fn on_cast_cue_carries_charge_and_on_end_cue_carries_charge_and_end_reason() {
    let (mut t, player, _dummy) = setup(29, bolt_timeline(5.0));
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir_charged("firebolt", Dir3::X, 255);
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
    let on_cast_cue = rec
        .cues
        .iter()
        .find(|c| c.cue_id == "firebolt_cast")
        .expect("on_cast cue fires");
    assert_eq!(
        on_cast_cue.charge,
        Some(255),
        "the on_cast cue carries the cast's charge"
    );
    assert_eq!(
        on_cast_cue.end_reason, None,
        "a non-OnEnd cue never carries an end reason"
    );

    let end_cue = rec
        .cues
        .iter()
        .find(|c| c.cue_id == "firebolt_boom")
        .expect("on_end_bolt cue fires");
    assert_eq!(
        end_cue.charge,
        Some(255),
        "the on_end cue keeps carrying the cast's charge"
    );
    assert_eq!(
        end_cue.end_reason,
        Some(EndReason::HitWorld),
        "the on_end cue carries the terminating EndReason"
    );
}

/// Schema v2 `strikes` gate: a `strikes: false` zone sitting ON TOP of a dummy never produces a
/// `HitConfirmed` (it's a carrier volume — it still opens, ends, and fires events); the same
/// zone with `strikes: true` hits. Brief Step 1's `non_striking_windows_never_hit`.
#[test]
fn non_striking_windows_never_hit() {
    let run = |strikes: bool| {
        let mut zone = window("zone", WindowPhase::Active, VolumeMotion::Static, 1.0);
        zone.shape = CollisionShape::Sphere { radius: 3.0 };
        zone.hit_mode = HitMode::OncePerTarget;
        zone.strikes = strikes;
        let tl = CastTimeline {
            skill_id: "firebolt".into(),
            phase_durations: PhaseDurations {
                windup: 0.05,
                active: 0.05,
                recovery: 0.05,
            },
            collision_windows: vec![zone],
            acquisition: Default::default(),
            vfx_cues: HashMap::new(),
            chain_radius: 6.0,
            chargeable: false,
            max_hold: 1.0,
            cues: HashMap::new(),
            charge_cues: Vec::new(),
        };
        let (mut t, player, _dummy) = setup(23, tl);
        t.app
            .world_mut()
            .commands()
            .entity(player)
            .cast_skill_dir("firebolt", Dir3::Z);
        t.advance_ticks(90); // 1.5 s: past the zone's whole life (0.05 s windup + 1 s fuse)
        (
            t.rec().hit_confirmed.len(),
            t.rec()
                .hitbox_ended
                .iter()
                .filter(|e| e.window_id == "zone")
                .count(),
        )
    };
    let (hits_striking, ended_striking) = run(true);
    assert!(
        hits_striking > 0,
        "control: the striking zone hits the dummy"
    );
    assert_eq!(ended_striking, 1, "control zone still ends once");

    let (hits_carrier, ended_carrier) = run(false);
    assert_eq!(
        hits_carrier, 0,
        "a strikes:false zone over a dummy must never confirm a hit"
    );
    assert_eq!(ended_carrier, 1, "the carrier zone still ends (fuse) once");
}

/// REGRESSION (zero-boundary window spawn): a window whose start time is exactly 0.0
/// (windup 0.0 + `Scheduled { Active, offset: 0.0 }` — an instant blast like obelisk-arena's
/// firebolt_explosion) must open on the FIRST advance tick of a DIRECT cast. The crossing test
/// `prev_elapsed < start` alone can never be true for `start == 0.0`, so such windows silently
/// never spawned (no hitbox, no cue, no damage) when cast directly — the editor preview's
/// direct-cast path. The triggered executor was unaffected, which masked this in-game where
/// firebolt_explosion is only ever triggered.
#[test]
fn zero_start_window_opens_on_direct_cast() {
    let tl = CastTimeline {
        skill_id: "firebolt".into(),
        phase_durations: PhaseDurations {
            windup: 0.0,
            active: 0.05,
            recovery: 0.0,
        },
        collision_windows: vec![window(
            "blast",
            WindowPhase::Active,
            VolumeMotion::Static,
            0.05,
        )],
        acquisition: Default::default(),
        vfx_cues: HashMap::new(),
        chain_radius: 6.0,
        chargeable: false,
        max_hold: 1.0,
        cues: HashMap::new(),
        charge_cues: Vec::new(),
    };
    let (mut t, player, _dummy) = setup(11, tl);
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("firebolt", Dir3::X);
    t.advance_ticks(30);

    let rec = t.rec();
    assert!(!rec.cast_began.is_empty(), "cast begins");
    assert!(
        rec.hit_window_opened.iter().any(|w| w.window_id == "blast"),
        "a start==0.0 scheduled window must open on the first tick of a direct cast; opened: {:?}",
        rec.hit_window_opened
    );
}
