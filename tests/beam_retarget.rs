//! Increment 2 — beam windows + retarget hops (chain lightning). A `VolumeMotion::Beam` window
//! strikes its DESIGNATED target (the cast's entity aim / a retarget pick) with no overlap
//! test; `EndReaction::Retarget` seeks the nearest un-struck valid target around the end
//! position and spawns the next beam onto it, bounded by `max_hops`. Also pins the LOS
//! self-block fix: an entity-aimed cast by a caster with its OWN child hurtbox validates.
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::assets::{
    validate_timeline, CastDelivery, CastTargeting, CollisionShape, CollisionWindow, EndReaction,
    HitFilter, HitMode, OnEnd, PhaseDurations, VolumeMotion, WindowPhase,
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
        spawn_phase: phase,
        spawn_offset: 0.0,
        active_duration: 0.1,
        shape: CollisionShape::Sphere { radius: 0.3 },
        motion: VolumeMotion::Beam,
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::FirstOnly,
        rehit_interval: None,
        on_end: OnEnd {
            hit: Some(EndReaction::Retarget {
                window: "hop".into(),
                radius: 6.0,
                max_hops: 3,
            }),
            world: None,
            fuse: None,
        },
    }
}

/// arc (Active, entity-aimed) --hit--> Retarget("hop") --hit--> Retarget("hop") ... x3 hops.
fn chain_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "firebolt".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 0.05,
            recovery: 0.05,
        },
        collision_windows: vec![
            beam_window("arc", WindowPhase::Active),
            beam_window("hop", WindowPhase::Chained),
        ],
        targeting: CastTargeting::SingleEntity { range: 15.0 },
        delivery: CastDelivery::Instant,
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
    assert_eq!(rec.cast_began.len(), 1, "the cast itself succeeds (costs paid)");
    assert!(rec.hit_confirmed.is_empty(), "no designated target, no strikes");
    assert!(
        rec.hitbox_ended
            .iter()
            .any(|e| e.window_id == "arc" && e.reason == obelisk_bevy::events::EndReason::Fuse),
        "the un-targeted beam fuses out"
    );
}

#[test]
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

#[test]
fn validation_rules_for_retarget() {
    // Self-retarget (hop -> hop) is legal: the hop counter bounds it.
    assert!(validate_timeline(&chain_timeline()).is_ok());

    // max_hops 0 can never fire.
    let mut tl = chain_timeline();
    tl.collision_windows[0].on_end.hit = Some(EndReaction::Retarget {
        window: "hop".into(),
        radius: 6.0,
        max_hops: 0,
    });
    assert!(validate_timeline(&tl).unwrap_err().contains("positive"));

    // Retarget must point at a Chained window.
    let mut tl = chain_timeline();
    tl.collision_windows[1].spawn_phase = WindowPhase::Active;
    assert!(validate_timeline(&tl)
        .unwrap_err()
        .contains("spawn_phase: Chained"));
}
