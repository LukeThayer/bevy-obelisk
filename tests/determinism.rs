//! Determinism hardening (Batch 9) — UNIT/integration tests that pin the seeded-RNG contract.
//!
//! obelisk-bevy's whole value rests on determinism: every combat/loot roll flows through a single
//! seeded `ChaCha8` `CombatRng`, NEVER `thread_rng`. The existing suite already covers same-seed
//! reproducibility (`vertical_slice.rs::slice_is_deterministic*`, several resolve-layer
//! `resolution_is_deterministic_for_a_fixed_seed` tests) and one seed-SPLIT case
//! (`library.rs::probabilistic_apply_is_deterministic_and_split_by_seed`). This file fills the
//! remaining GAPS across the THREE seeded-RNG draw sites:
//!
//!   1. the crit roll       (`StatBlock` crit chance < 100% → `rng.gen::<f64>() < p`),
//!   2. the loot roll       (`tables_core::DropTableRegistry::roll`, weighted pick),
//!   3. the status-apply roll(`PendingStatusEffect` apply-chance < 100%).
//!
//! For each it asserts BOTH halves of the determinism contract:
//!   - `cross_seed_diverges`: the SAME scenario under TWO different seeds produces DIFFERENT
//!     observable outcomes (so the seed genuinely drives the result — it is not a constant);
//!   - `same_seed_idempotent`: running TWICE with the SAME seed yields the IDENTICAL result.
//!
//! It also pins `resolve_aoe`'s STABLE-ORDER contract (order-independence under a fixed seed).
//!
//! The diverging seed pairs below were found by sweeping seeds (the same technique the
//! probabilistic-apply seed-split test used) so each assertion is GENUINELY diverging for the
//! committed pair, not flaky. These tests add NO new goldens.
#![cfg(feature = "test-support")]

use bevy::ecs::system::RunSystemOnce;
use bevy::math::Vec3;
use obelisk_bevy::combat::resolve_one_hit;
use obelisk_bevy::prelude::*;
use obelisk_bevy::scenario::run::run_scenario;
use obelisk_bevy::scenario::{library, Action, Aim, Scenario};
use obelisk_bevy::testkit::ObeliskTestApp;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use stat_core::config::load_skills_dir;
use stat_core::{Skill, StatBlock};
use std::collections::HashMap;
use std::path::Path;

// --------------------------------------------------------------------------------------------
// Shared resolve-layer helpers (the deterministic funnel the whole sim uses: `resolve_one_hit`
// threading a seeded `ChaCha8Rng`, NEVER `thread_rng`).
// --------------------------------------------------------------------------------------------

fn fixture_registry() -> HashMap<String, Skill> {
    obelisk_bevy::testkit::init_test_obelisk();
    load_skills_dir(Path::new("tests/fixtures/skills")).expect("load skill fixtures")
}

/// Resolve one firebolt hit under `seed` with `crit_chance`% flat critical-strike chance against a
/// high-life dummy (so it survives and the crit roll is the only varying signal). Returns
/// `(was_crit, total_damage)`. Firebolt deals 20 base fire; a crit multiplies by 1.5 → 30.
fn crit_outcome(seed: u64, crit_chance: f64) -> (bool, f64) {
    let registry = fixture_registry();
    let skill = registry.get("firebolt").expect("firebolt fixture");

    let mut caster = StatBlock::with_id("player");
    caster.max_mana.base = 100.0;
    caster.current_mana = 100.0;
    caster.critical_chance.flat = crit_chance;

    let mut target = StatBlock::with_id("dummy");
    target.max_life.base = 5000.0;
    target.current_life = 5000.0;

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let outcome = resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng)
        .expect("resolve firebolt");
    (outcome.is_critical, outcome.total_damage)
}

/// Resolve one chancebolt hit under `seed`: applies `chill` with chance
/// `status_damage / max_life = 20 / 40 = 0.50`. Returns whether `chill` landed.
fn chill_landed(seed: u64) -> bool {
    let registry = fixture_registry();
    let skill = registry.get("chancebolt").expect("chancebolt fixture");

    let mut caster = StatBlock::with_id("player");
    caster.max_mana.base = 100.0;
    caster.current_mana = 100.0;

    let mut target = StatBlock::with_id("dummy");
    target.max_life.base = 40.0;
    target.current_life = 40.0;

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let outcome = resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng)
        .expect("resolve chancebolt");
    outcome.effects_applied.iter().any(|e| e.id == "chill")
}

/// A rarity-gated loot scenario at the BASELINE `rarity_mult = 1.0` (so the weighted pick between
/// the common `copper` (~57%) and the rare `diamond` (~43%) genuinely depends on the seed). Returns
/// the `Loot` line's detail text (which currency dropped) from the full `run_scenario` path.
fn loot_detail(seed: u64) -> String {
    let s = Scenario::new("determinism_loot", seed, 600)
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .with_drop_table_fixture("rarity_tiers", "tests/fixtures/loot/rarity_tiers.toml")
        .with_drop_table("rarity_tiers")
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        );
    run_scenario(&s)
        .lines
        .iter()
        .find(|l| l.kind == "Loot")
        .map(|l| l.detail.clone())
        .unwrap_or_else(|| panic!("loot scenario (seed {seed}) produced no Loot line"))
}

// ============================================================================================
// 1. cross_seed_diverges — the SAME scenario under TWO different seeds produces a DIFFERENT
//    observable outcome. Proves the seed genuinely DRIVES the roll (it is not a baked constant).
//    Diverging pairs found by a seed sweep; the chosen pairs are committed and stable.
// ============================================================================================

/// Full-scenario divergence through the PUBLIC `run_scenario` path: the same `probabilistic_effect_apply`
/// scenario (chancebolt, 50% chill apply) under two different seeds yields DIFFERENT trace text — seed 0
/// lands `chill`, seed 2 does not. This is the integration-level twin of the resolve-layer seed split,
/// proving the seed flows all the way through the cast/hitbox/resolve pipeline to the trace.
#[test]
fn cross_seed_diverges_full_scenario_status_apply() {
    let with_seed = |seed: u64| {
        let mut s = library::probabilistic_effect_apply();
        s.seed = seed;
        run_scenario(&s).to_text()
    };
    let trace_pass = with_seed(0);
    let trace_fail = with_seed(2);

    assert!(
        trace_pass.contains("chill"),
        "seed 0 must land chill (EffectApplied chill):\n{trace_pass}"
    );
    assert!(
        !trace_fail.contains("chill"),
        "seed 2 must NOT land chill (same scenario, opposite outcome):\n{trace_fail}"
    );
    assert_ne!(
        trace_pass, trace_fail,
        "the same scenario under different seeds must produce a DIFFERENT trace (the seed drives the 50% chill roll, it is not a constant)"
    );
}

/// Resolve-layer crit-roll divergence: firebolt at a 50% crit chance crits under seed 1 (30 dmg) but
/// not under seed 0 (20 dmg). The crit bool AND the damage differ — the seeded `rng.gen::<f64>() < 0.5`
/// crit roll genuinely depends on the seed.
#[test]
fn cross_seed_diverges_crit_roll() {
    let (crit_a, dmg_a) = crit_outcome(0, 50.0);
    let (crit_b, dmg_b) = crit_outcome(1, 50.0);

    assert!(
        !crit_a,
        "seed 0 (crit 50%) must NOT crit, got crit (dmg {dmg_a})"
    );
    assert!(
        crit_b,
        "seed 1 (crit 50%) must crit, got non-crit (dmg {dmg_b})"
    );
    assert_ne!(
        (crit_a, dmg_a),
        (crit_b, dmg_b),
        "the 50% crit roll must DIFFER across seeds (20 base vs 30 crit) — proves the seed drives the crit roll"
    );
}

/// Loot-roll divergence through the full `run_scenario` path: the rarity-gated table at baseline
/// `rarity_mult = 1.0` (copper ~57% / diamond ~43%) drops `copper` under seed 0 but `diamond` under
/// seed 4. The weighted pick genuinely depends on the seed.
#[test]
fn cross_seed_diverges_loot_roll() {
    let drop_a = loot_detail(0);
    let drop_b = loot_detail(4);

    assert!(
        drop_a.contains("copper"),
        "seed 0 should drop copper (the common majority pick): {drop_a}"
    );
    assert!(
        drop_b.contains("diamond"),
        "seed 4 should drop diamond (the rare pick): {drop_b}"
    );
    assert_ne!(
        drop_a, drop_b,
        "the weighted loot roll must DIFFER across seeds — proves the seed drives the drop-table pick"
    );
}

// ============================================================================================
// 2. same_seed_idempotent — running TWICE with the SAME seed yields the IDENTICAL result, for
//    EACH of the three RNG-drawing paths (crit roll, loot roll, status-apply roll). Some paths
//    have partial coverage elsewhere; this pins all three uniformly in one place.
// ============================================================================================

/// The crit roll is idempotent for a fixed seed. Checked at BOTH a diverging seed that crits and one
/// that does not, so the idempotence holds on either branch of the roll.
#[test]
fn same_seed_idempotent_crit_roll() {
    // Seed that crits (30 dmg) reproduces; seed that misses (20 dmg) reproduces.
    assert_eq!(
        crit_outcome(1, 50.0),
        crit_outcome(1, 50.0),
        "same seed (crit branch) must reproduce the crit roll bit-for-bit"
    );
    assert_eq!(
        crit_outcome(0, 50.0),
        crit_outcome(0, 50.0),
        "same seed (no-crit branch) must reproduce the crit roll bit-for-bit"
    );
}

/// The loot roll is idempotent for a fixed seed, through the full `run_scenario` path — including the
/// `tables_core::DropTableRegistry::roll` weighted pick. Checked at both a copper-dropping and a
/// diamond-dropping seed.
#[test]
fn same_seed_idempotent_loot_roll() {
    assert_eq!(
        loot_detail(0),
        loot_detail(0),
        "same seed (copper branch) must reproduce the loot roll"
    );
    assert_eq!(
        loot_detail(4),
        loot_detail(4),
        "same seed (diamond branch) must reproduce the loot roll"
    );
}

/// The status apply-chance roll is idempotent for a fixed seed. Checked at both a PASS seed (chill
/// lands) and a FAIL seed (chill does not), so idempotence holds on either branch.
#[test]
fn same_seed_idempotent_status_apply_roll() {
    assert_eq!(
        chill_landed(0),
        chill_landed(0),
        "same seed (chill PASS branch) must reproduce the apply roll"
    );
    assert_eq!(
        chill_landed(2),
        chill_landed(2),
        "same seed (chill FAIL branch) must reproduce the apply roll"
    );
    // Sanity: the two seeds genuinely straddle the 50% roll (so the idempotence above is
    // exercising both branches, not the same one twice).
    assert!(chill_landed(0), "seed 0 lands chill");
    assert!(!chill_landed(2), "seed 2 does not land chill");
}

// ============================================================================================
// 3. order_independence_or_stable_order — `ObeliskCombat::resolve_aoe` STABLE-sorts targets by
//    `StatBlock.id` before drawing from the seeded RNG, so the per-target rolls are assigned
//    deterministically REGARDLESS of the spawn/iteration order handed in. We perturb the input
//    target order and assert the per-target outcome is IDENTICAL under the same seed.
// ============================================================================================

/// Spawn a combatant with the given id/faction/life and return its entity.
fn spawn(t: &mut ObeliskTestApp, id: &str, faction: Faction, life: f64) -> bevy::prelude::Entity {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = 100.0;
    b.current_mana = 100.0;
    t.app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(b),
            faction,
            ObeliskId(id.into()),
            bevy::prelude::Transform::default(),
        ))
        .id()
}

/// Run `resolve_aoe` over a list of enemies whose INPUT order is given by `order` (indices into the
/// freshly-spawned [a, b, c, d]); return each enemy's remaining life keyed by its STABLE string id.
/// Crit-capable so the seeded per-target crit roll is exercised (the rolls are the thing the stable
/// sort must pin to the right target regardless of input order).
fn aoe_life_by_id(seed: u64, order: [usize; 4]) -> Vec<(String, f64)> {
    let mut t = ObeliskTestApp::new(seed);
    let caster = spawn(&mut t, "caster", Faction::Player, 1000.0);
    // Distinct ids; their STABLE sort order (by id string) is a, b, c, d regardless of spawn order.
    let enemies = [
        spawn(&mut t, "enemy_a", Faction::Enemy, 1000.0),
        spawn(&mut t, "enemy_b", Faction::Enemy, 1000.0),
        spawn(&mut t, "enemy_c", Faction::Enemy, 1000.0),
        spawn(&mut t, "enemy_d", Faction::Enemy, 1000.0),
    ];
    // Give the caster a non-trivial crit chance so each target draws a crit roll off the shared RNG.
    {
        let mut caster_entity = t.app.world_mut().entity_mut(caster);
        let mut attrs = caster_entity.get_mut::<Attributes>().unwrap();
        attrs.0.critical_chance.flat = 50.0;
    }
    t.app.update();

    let targets: Vec<bevy::prelude::Entity> = order.iter().map(|&i| enemies[i]).collect();
    let _hits = t
        .app
        .world_mut()
        .run_system_once(move |mut combat: ObeliskCombat| {
            combat.resolve_aoe(caster, &targets, "firebolt")
        })
        .unwrap();

    // Read each enemy's life back, keyed by its stable id (so the comparison is order-agnostic).
    let mut out: Vec<(String, f64)> = enemies
        .iter()
        .map(|&e| {
            let a = t.app.world().entity(e).get::<Attributes>().unwrap();
            (a.0.id.clone(), a.0.current_life)
        })
        .collect();
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

/// `resolve_aoe`'s stable target order: perturbing the INPUT order of the target slice must NOT
/// change which seeded roll each target receives. Two runs under the SAME seed but with different
/// input orders (natural order vs reversed) must leave EACH target (keyed by stable id) with the
/// IDENTICAL remaining life. This proves the per-target crit/damage rolls are assigned by the
/// stable `StatBlock.id` sort, not by spawn/iteration order.
#[test]
fn aoe_stable_order_is_input_order_independent() {
    let natural = aoe_life_by_id(7, [0, 1, 2, 3]);
    let reversed = aoe_life_by_id(7, [3, 2, 1, 0]);
    let shuffled = aoe_life_by_id(7, [2, 0, 3, 1]);

    assert_eq!(
        natural, reversed,
        "perturbing the AoE target input order must NOT change any per-target result (stable id sort)"
    );
    assert_eq!(
        natural, shuffled,
        "an arbitrary AoE target input order must yield the IDENTICAL per-target result (stable id sort)"
    );
    // Sanity: the AoE actually did something (every enemy took damage; at least one crit varies the
    // numbers), so the equality above is meaningful rather than comparing two no-ops.
    assert!(
        natural.iter().all(|(_, life)| *life < 1000.0),
        "every enemy should have taken AoE damage: {natural:?}"
    );
}

/// Same-seed idempotence of `resolve_aoe` itself (the order-FIXED twin of the test above): two runs
/// with the same seed and the same input order produce the identical per-target outcome.
#[test]
fn same_seed_idempotent_resolve_aoe() {
    assert_eq!(
        aoe_life_by_id(7, [0, 1, 2, 3]),
        aoe_life_by_id(7, [0, 1, 2, 3]),
        "resolve_aoe under a fixed seed must be reproducible"
    );
}
