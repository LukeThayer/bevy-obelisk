//! Beam windows (chain lightning's delivery). A `VolumeMotion::Beam` window strikes its
//! DESIGNATED target (the cast's entity aim) with no overlap test. Also pins the LOS
//! self-block fix: an entity-aimed cast by a caster with its OWN child hurtbox validates.
//!
//! Schema v2 (Task 9) deleted the authored `EndReaction::Retarget` hop machinery; the hop
//! tests below are `#[ignore]`d as Task 12's RED fixtures — Task 12 re-keys hop behavior to
//! the rules `can_chain`/`chain_count` fields and un-ignores them.
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::assets::{
    CollisionShape, CollisionWindow, HitFilter, HitMode, PhaseDurations, VolumeMotion,
    WindowAnchor, WindowPhase, WindowSpawn,
};
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

fn beam_window(id: &str, phase: WindowPhase) -> CollisionWindow {
    CollisionWindow {
        id: id.into(),
        spawn: WindowSpawn::Scheduled { phase, offset: 0.0 },
        anchor: WindowAnchor::Caster,
        anchor_offset: Vec3::ZERO,
        strikes: true,
        active_duration: 0.1,
        shape: CollisionShape::Sphere { radius: 0.3 },
        motion: VolumeMotion::Beam,
        motion_direction: Default::default(),
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::FirstOnly,
        rehit_interval: None,
        emitter: None,
    }
}

/// arc (Active, entity-aimed beam). v1 authored `on_end: Retarget("hop")` here; the hop chain
/// returns as rules `chain_count` in Task 12.
fn chain_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "firebolt".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 0.05,
            recovery: 0.05,
        },
        collision_windows: vec![beam_window("arc", WindowPhase::Active)],
        acquisition: Default::default(),
        vfx_cues: HashMap::from([
            ("on_window_arc".to_string(), "cl_arc".to_string()),
            ("on_window_hop".to_string(), "cl_hop".to_string()),
        ]),
    }
}

/// Caster (WITH its own child hurtbox — the LOS self-block regression) + 5 enemies in a line.
fn setup(seed: u64) -> (ObeliskTestApp, Entity, Vec<Entity>) {
    let mut t = ObeliskTestApp::new(seed);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(chain_timeline());
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
    {
        let mut commands = t.app.world_mut().commands();
        // The caster's OWN hurtbox: before the LOS fix this self-blocked every entity cast.
        insert_hurtbox(&mut commands, caster, 0.6, Vec3::ZERO);
    }
    // Enemies along +Z: chain layout T0(2) T1(3) T2(4.5) T3(6) T4(7.5). From each victim the
    // nearest candidate INCLUDES already-struck ones — the visited exclusion is what forces
    // the chain forward (from T1, T0 at 1.0 is closer than T2 at 1.5).
    let zs = [2.0, 3.0, 4.5, 6.0, 7.5];
    let mut dummies = Vec::new();
    for (i, z) in zs.iter().enumerate() {
        let d = t
            .app
            .world_mut()
            .spawn((
                Combatant,
                Attributes(make_block(&format!("t{i}"), 500.0, 0.0)),
                Faction::Enemy,
                ObeliskId(format!("t{i}")),
                Transform::from_xyz(0.0, 0.0, *z),
            ))
            .id();
        let mut commands = t.app.world_mut().commands();
        insert_hurtbox(&mut commands, d, 0.6, Vec3::new(0.0, 0.0, *z));
        dummies.push(d);
    }
    t.app.update();
    (t, caster, dummies)
}

#[test]
#[ignore = "re-keyed to rules chain_count in Task 12"]
fn chain_hops_nearest_unvisited_and_stops_at_max_hops() {
    let (mut t, caster, dummies) = setup(3);
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at("firebolt", dummies[0]);
    t.advance_ticks(60);

    let rec = t.rec();
    assert!(
        rec.cast_rejected.is_empty(),
        "entity cast with own hurtbox must validate (LOS fix): {:?}",
        rec.cast_rejected.first().map(|r| &r.reason)
    );
    let victims: Vec<Entity> = rec.hit_confirmed.iter().map(|h| h.target).collect();
    assert_eq!(
        victims,
        vec![dummies[0], dummies[1], dummies[2], dummies[3]],
        "initial + 3 hops, nearest-unvisited order (visited exclusion forces forward)"
    );
    assert_eq!(rec.damage_resolved.len(), 4, "full damage each strike");
    assert!(
        rec.damage_resolved.iter().all(|d| d.caster == caster),
        "hops keep the original caster attribution"
    );
    assert!(
        !victims.contains(&dummies[4]),
        "T4 in radius of T3 but the chain is out of hops"
    );
    // Two-point window cues: every beam window cue carries both anchors.
    let beam_cues: Vec<_> = rec
        .cues
        .iter()
        .filter(|c| c.cue_id == "cl_arc" || c.cue_id == "cl_hop")
        .collect();
    assert_eq!(beam_cues.len(), 4, "arc + 3 hops");
    assert!(
        beam_cues.iter().all(|c| c.position_from.is_some()),
        "beam window cues are two-point"
    );
    // The first hop's arc runs T0 -> T1.
    let hop1 = beam_cues[1];
    assert!((hop1.position_from.unwrap().z - 2.0).abs() < 0.5);
    assert!((hop1.position.z - 3.0).abs() < 0.5);
}

/// The two-anchor cue contract, kept GREEN while the hop tests above wait for Task 12: an
/// entity-aimed beam's `on_window_{id}` cue carries BOTH anchors — from the beam origin
/// (caster) to its designated victim.
#[test]
fn entity_aimed_beam_cue_is_two_point() {
    let (mut t, caster, dummies) = setup(3);
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at("firebolt", dummies[0]);
    t.advance_ticks(60);

    let rec = t.rec();
    assert!(
        rec.cast_rejected.is_empty(),
        "entity cast with own hurtbox must validate (LOS fix): {:?}",
        rec.cast_rejected.first().map(|r| &r.reason)
    );
    let victims: Vec<Entity> = rec.hit_confirmed.iter().map(|h| h.target).collect();
    assert_eq!(victims, vec![dummies[0]], "the beam strikes its entity aim");
    let cue = rec
        .cues
        .iter()
        .find(|c| c.cue_id == "cl_arc")
        .expect("the arc window cue fires");
    let from = cue.position_from.expect("beam window cues are two-point");
    assert!(from.z.abs() < 0.5, "arc starts at the caster: {from:?}");
    assert!(
        (cue.position.z - 2.0).abs() < 0.5,
        "arc ends at the victim: {:?}",
        cue.position
    );
}

#[test]
fn direction_aimed_beam_is_a_paid_fizzle() {
    let (mut t, caster, _dummies) = setup(5);
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_dir("firebolt", Dir3::X);
    t.advance_ticks(60);

    let rec = t.rec();
    assert_eq!(
        rec.cast_began.len(),
        1,
        "the cast itself succeeds (costs paid)"
    );
    assert!(
        rec.hit_confirmed.is_empty(),
        "no designated target, no strikes"
    );
    assert!(
        rec.hitbox_ended
            .iter()
            .any(|e| e.window_id == "arc" && e.reason == obelisk_bevy::events::EndReason::Fuse),
        "the un-targeted beam fuses out"
    );
}

#[test]
#[ignore = "re-keyed to rules chain_count in Task 12"]
fn charge_scales_every_strike_in_the_chain() {
    let total = |charge: Option<u8>| {
        let (mut t, caster, dummies) = setup(9);
        let mut cmd = t.app.world_mut().commands();
        let mut ent = cmd.entity(caster);
        match charge {
            Some(c) => ent.cast_skill_at_charged("firebolt", dummies[0], c),
            None => ent.cast_skill_at("firebolt", dummies[0]),
        };
        t.advance_ticks(60);
        t.rec()
            .damage_resolved
            .iter()
            .map(|d| d.total_damage)
            .collect::<Vec<_>>()
    };
    let base = total(None);
    let charged = total(Some(255));
    assert_eq!(base.len(), 4);
    assert_eq!(charged.len(), 4);
    for (b, c) in base.iter().zip(&charged) {
        assert!(
            (c / b - 2.0).abs() < 1e-6,
            "full charge doubles every strike: {b} -> {c}"
        );
    }
}

#[test]
fn same_seed_is_deterministic() {
    let run = || {
        let (mut t, caster, dummies) = setup(0xBEA1);
        t.app
            .world_mut()
            .commands()
            .entity(caster)
            .cast_skill_at("firebolt", dummies[0]);
        t.advance_ticks(60);
        t.rec()
            .damage_resolved
            .iter()
            .map(|d| (d.total_damage, d.life_after))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
