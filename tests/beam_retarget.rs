//! Beam windows (chain lightning's delivery). A `VolumeMotion::Beam` window strikes its
//! DESIGNATED target (the cast's entity aim) with no overlap test. Also pins the LOS
//! self-block fix: an entity-aimed cast by a caster with its OWN child hurtbox validates.
//!
//! Schema v2 (Task 9) deleted the authored `EndReaction::Retarget` hop machinery; Task 12
//! re-keys hop behavior to the rules `can_chain`/`chain_count` fields
//! (`tests/fixtures/skills/chain_bolt.toml`: `can_chain = true, chain_count = 3`). The hop
//! tests use the dedicated `chain_bolt` skill so they don't perturb the plain `firebolt`
//! fixture (`can_chain = false`) the other tests in this file — and the golden suite — rely on
//! staying single-hit.
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
        paints: None,
    }
}

/// arc (Active, entity-aimed beam). v1 authored `on_end: Retarget("hop")` here; the hop chain
/// now comes from rules `chain_count` on `skill_id` (Task 12).
fn chain_timeline(skill_id: &str) -> CastTimeline {
    CastTimeline {
        skill_id: skill_id.into(),
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
        // Layout below spans T0(2)..T4(7.5) with <=1.5 gaps; 6.0 comfortably covers every hop.
        chain_radius: 6.0,
        chargeable: false,
        max_hold: 1.0,
        cues: HashMap::new(),
        charge_cues: Vec::new(),
    }
}

/// Caster (WITH its own child hurtbox — the LOS self-block regression) + 5 enemies in a line.
fn setup(seed: u64, skill_id: &str) -> (ObeliskTestApp, Entity, Vec<Entity>) {
    let mut t = ObeliskTestApp::new(seed);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(chain_timeline(skill_id));
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert(skill_id.into(), handle);
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
    let (mut t, caster, dummies) = setup(3, "chain_bolt");
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at("chain_bolt", dummies[0]);
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

/// The two-anchor cue contract: an entity-aimed beam's `on_window_{id}` cue carries BOTH
/// anchors — from the beam origin (caster) to its designated victim. Uses the plain
/// (non-chaining) `firebolt` fixture so it stays a single strike.
#[test]
fn entity_aimed_beam_cue_is_two_point() {
    let (mut t, caster, dummies) = setup(3, "firebolt");
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
    let (mut t, caster, _dummies) = setup(5, "firebolt");
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
fn charge_scales_every_strike_in_the_chain() {
    let total = |charge: Option<u8>| {
        let (mut t, caster, dummies) = setup(9, "chain_bolt");
        let mut cmd = t.app.world_mut().commands();
        let mut ent = cmd.entity(caster);
        match charge {
            Some(c) => ent.cast_skill_at_charged("chain_bolt", dummies[0], c),
            None => ent.cast_skill_at("chain_bolt", dummies[0]),
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

// -------------------------------------------------------------------------------------------
// Final review, item 3: chain x hit-trigger composition. By code reading, a `can_chain` beam
// whose skill ALSO has an `always -> <explosion>` hit-phase condition explodes PER HOP — each
// hop's `HitConfirmed` (`end_hitboxes`' chain-hop arm, spec D5) re-enters `on_hit_confirmed` at
// its own hop count, and the observer's `partition_conditions`/timeline-target execution
// (Task 7) doesn't care whether the hit came from a scheduled window or a chain re-strike. No
// prior test pinned this — this one does, self-contained (skills registered programmatically,
// no fixture file changes, so it can't perturb the golden skill/timeline set).
// -------------------------------------------------------------------------------------------

/// `chain_zap` (`can_chain = true`, `chain_count = 3`, `always -> chain_zap_explosion`,
/// `mana_cost = 5.0`) + `chain_zap_explosion` (`mana_cost = 5.0` — nonzero on purpose: a
/// nonzero cost that still bills `mana_spent == 0.0` in play is the proof the sub-cast took the
/// depth>0 FREE billing path, not a paid one).
fn chain_with_explosion_toml() -> &'static str {
    r#"
[[skills]]
id = "chain_zap"
name = "Chain Zap"
tags = ["spell", "lightning"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0
[[skills.conditions]]
trigger_skill = "chain_zap_explosion"
type = "always"
additional = true
[skills.damage]
base_damages = [{ type = "lightning", min = 20.0, max = 20.0 }]
can_chain = true
chain_count = 3

[[skills]]
id = "chain_zap_explosion"
name = "Chain Zap Explosion"
tags = ["spell", "lightning"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0
[skills.damage]
base_damages = [{ type = "lightning", min = 5.0, max = 5.0 }]
"#
}

/// The explosion's own timeline: one offset-0 `Active` `Static` sphere anchored `CastPoint`
/// (resolves at the triggering hit's position — each hop's own victim). Radius 0.2 keeps
/// radius-sum-vs-gap (0.2 explosion + 0.6 hurtbox = 0.8) under the dummies' TIGHTEST spacing
/// (`setup`'s `zs`: T0-T1 is exactly 1.0), so each hop's explosion can only ever reach ITS OWN
/// victim, never a neighboring one — keeps the hop-count == explosion-count assertion exact.
fn chain_zap_explosion_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "chain_zap_explosion".into(),
        phase_durations: PhaseDurations {
            windup: 0.0,
            active: 1.0,
            recovery: 0.0,
        },
        collision_windows: vec![CollisionWindow {
            id: "blast".into(),
            spawn: WindowSpawn::Scheduled {
                phase: WindowPhase::Active,
                offset: 0.0,
            },
            anchor: WindowAnchor::CastPoint,
            anchor_offset: Vec3::ZERO,
            strikes: true,
            active_duration: 0.2,
            shape: CollisionShape::Sphere { radius: 0.2 },
            motion: VolumeMotion::Static,
            motion_direction: Default::default(),
            hit_filter: HitFilter::Enemies,
            hit_mode: HitMode::OncePerTarget,
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

/// Like `setup`, but also registers `chain_zap`/`chain_zap_explosion` in `SkillRegistry`
/// (programmatically — no fixture file changes) and both their timelines.
fn setup_chain_with_explosion(seed: u64) -> (ObeliskTestApp, Entity, Vec<Entity>) {
    let (mut t, caster, dummies) = setup(seed, "chain_zap");

    let skills = stat_core::config::parse_skills(chain_with_explosion_toml()).unwrap();
    t.app
        .world_mut()
        .resource_mut::<SkillRegistry>()
        .0
        .extend(skills);

    let blast_handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(chain_zap_explosion_timeline());
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("chain_zap_explosion".into(), blast_handle);
    t.app.update();

    (t, caster, dummies)
}

#[test]
fn chain_hops_each_trigger_their_own_hit_phase_explosion() {
    let (mut t, caster, dummies) = setup_chain_with_explosion(42);
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at("chain_zap", dummies[0]);
    t.advance_ticks(60);

    let rec = t.rec();
    assert!(
        rec.cast_rejected.is_empty(),
        "cast must validate: {:?}",
        rec.cast_rejected.first().map(|r| &r.reason)
    );

    let hop_strikes = rec
        .damage_resolved
        .iter()
        .filter(|d| d.skill_id == "chain_zap")
        .count();
    assert_eq!(hop_strikes, 4, "initial + 3 hops, same as the plain chain test");

    let explosions: Vec<_> = rec
        .damage_resolved
        .iter()
        .filter(|d| d.skill_id == "chain_zap_explosion")
        .collect();
    assert_eq!(
        explosions.len(),
        hop_strikes,
        "every hop's own HitConfirmed re-enters the partition and triggers its own explosion \
         sub-cast — count must match the strike count exactly, got {} strikes vs {} explosions",
        hop_strikes,
        explosions.len()
    );
    assert!(
        explosions.iter().all(|d| d.mana_spent == 0.0),
        "every explosion sub-cast must resolve mana-free (depth > 0), despite a nonzero \
         mana_cost on chain_zap_explosion — got mana_spent values {:?}",
        explosions.iter().map(|d| d.mana_spent).collect::<Vec<_>>()
    );
}

#[test]
fn same_seed_is_deterministic() {
    let run = || {
        let (mut t, caster, dummies) = setup(0xBEA1, "firebolt");
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
