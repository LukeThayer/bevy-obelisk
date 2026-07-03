//! Task 5 — free sub-cast resolution (spec §3.2 billing rule): only hits from the cast's
//! scheduled windows bill mana. Chain re-strikes (`hop > 0`) and triggered sub-casts
//! (`depth > 0`) resolve mana-free — they never pass through cast validation, so a caster at
//! zero mana must NOT fizzle on them, and NOTHING gets billed or put on cooldown.
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = mana;
    b.current_mana = mana;
    b
}

/// Spawns a player + dummy pair (mirrors `tests/end_events.rs`'s `setup`), no cast — this suite
/// drives `HitConfirmed` directly since it's testing the observer's billing logic, not the
/// timeline/spatial machinery that produces the event in real play.
fn setup(seed: u64) -> (ObeliskTestApp, Entity, Entity) {
    let mut t = ObeliskTestApp::new(seed);
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
    t.app.update();
    (t, player, dummy)
}

fn drain_caster_mana(t: &mut ObeliskTestApp, caster: Entity) {
    t.app
        .world_mut()
        .entity_mut(caster)
        .get_mut::<Attributes>()
        .expect("caster has Attributes")
        .0
        .current_mana = 0.0;
}

fn caster_mana(t: &ObeliskTestApp, caster: Entity) -> f64 {
    t.app
        .world()
        .entity(caster)
        .get::<Attributes>()
        .unwrap()
        .0
        .current_mana
}

/// Manually fires a `HitConfirmed` for "firebolt" (fixture skill, `mana_cost = 5.0`) against the
/// dummy, bypassing cast validation entirely — exactly the shape a triggered sub-cast or a
/// chain-hop re-strike takes in real play (both skip `advance.rs`'s cast pipeline).
fn fire_hit(t: &mut ObeliskTestApp, caster: Entity, target: Entity, depth: u8, hop: u8) {
    t.app.world_mut().trigger(HitConfirmed {
        caster,
        target,
        skill_id: "firebolt".to_string(),
        window_id: "bolt".to_string(),
        charge: None,
        position: Vec3::ZERO,
        depth,
        hop,
    });
    // Flush the observer's queued `commands.trigger(DamageResolved ...)` so the recorder sees it.
    t.app.world_mut().flush();
}

#[test]
fn depth_gt_zero_hits_do_not_bill_or_fizzle_on_mana() {
    let (mut t, player, dummy) = setup(1);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 1, 0);

    assert!(
        !t.rec().damage_resolved.is_empty(),
        "zero mana must not fizzle a depth>0 sub-cast hit"
    );
    assert_eq!(
        caster_mana(&t, player),
        0.0,
        "a free hit must not bill any mana"
    );
}

#[test]
fn hop_gt_zero_hits_do_not_bill_or_fizzle_on_mana() {
    let (mut t, player, dummy) = setup(2);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 0, 1);

    assert!(
        !t.rec().damage_resolved.is_empty(),
        "zero mana must not fizzle a hop>0 chain re-strike hit"
    );
    assert_eq!(
        caster_mana(&t, player),
        0.0,
        "a free hit must not bill any mana"
    );
}

/// Paired control: today's (unchanged) behavior. A depth-0, hop-0 hit is a normal scheduled-
/// window hit — it bills mana per-hit, and at zero mana it fizzles (no `DamageResolved`).
#[test]
fn depth_zero_hop_zero_hits_still_fizzle_on_zero_mana() {
    let (mut t, player, dummy) = setup(3);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 0, 0);

    assert!(
        t.rec().damage_resolved.is_empty(),
        "a paid hit at zero mana must still fizzle (today's preserved behavior)"
    );
}

/// Sub-casts never pass through `advance.rs`'s cast validation (the only place that calls
/// `Cooldowns::start`), so a free hit must leave the skill's cooldown untouched.
#[test]
fn free_sub_cast_hit_does_not_start_a_cooldown() {
    let (mut t, player, dummy) = setup(4);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 1, 0);

    let cooldowns = t.app.world().resource::<Cooldowns>();
    assert!(
        cooldowns.is_ready(player, "firebolt"),
        "a free sub-cast hit must never start a cooldown"
    );
}
