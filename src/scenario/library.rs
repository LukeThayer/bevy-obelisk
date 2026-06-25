use crate::core::components::Faction;
use crate::scenario::{Action, Aim, Scenario};
use bevy::prelude::Vec3;
use stat_core::StatType;

pub fn firebolt_kill() -> Scenario {
    Scenario::new("firebolt_kill", 42, 600)
        .describe("Single-target projectile: the bolt flies, hits for 20, applies a burn DoT that ticks the target to death.")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Cone cleave: player + 3 enemies (two inside the 120° arc, one directly behind).
/// Mirrors `tests/spatial_targeting.rs::cone_cleave_hits_multiple_enemies_in_arc_but_not_behind`.
/// Expect exactly two Damage lines (front_a, front_b); the entity behind is outside the cone.
pub fn cone_cleave() -> Scenario {
    Scenario::new("cone_cleave", 7, 120)
        .describe(
            "Directional cone (cleave) hits the two enemies inside the arc but not the one behind.",
        )
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor(
            "front_a",
            Faction::Enemy,
            50.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .actor(
            "front_b",
            Faction::Enemy,
            50.0,
            0.0,
            Vec3::new(1.0, 0.0, 1.5),
        )
        .actor(
            "behind",
            Faction::Enemy,
            50.0,
            0.0,
            Vec3::new(0.0, 0.0, -2.0),
        )
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Dir(Vec3::Z),
            },
        )
}

/// Faction filter: player + a same-faction ally directly in front of the cleave.
/// Mirrors `tests/spatial_targeting.rs::cleave_does_not_hit_allies`.
/// The `Enemies` hit filter must NOT damage the ally (zero Damage lines).
pub fn faction_filter() -> Scenario {
    Scenario::new("faction_filter", 7, 120)
        .describe("Cleave across an ally and an enemy only damages the enemy - faction hit-filtering spares allies.")
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("ally", Faction::Player, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Dir(Vec3::Z),
            },
        )
}

/// Out of range: an enemy 10 units away, cleave range is 3.
/// Mirrors `tests/spatial_targeting.rs::out_of_range_cast_is_rejected`.
/// Expect `CastRejected reason=OutOfRange` and no Damage.
pub fn out_of_range() -> Scenario {
    Scenario::new("out_of_range", 7, 30)
        .describe(
            "A cast at a target beyond the skill's range is rejected (OutOfRange) with no damage.",
        )
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("far", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 10.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Entity("far".into()),
            },
        )
}

/// Line of sight (blocked): a static obstacle between the player and the target.
/// Mirrors the blocked half of `tests/spatial_targeting.rs::cast_blocked_by_obstacle_is_rejected_then_clears`.
/// The obstacle is spawned at tick 0 so its static collider is registered (visible to
/// `SpatialQuery` from the 2nd update) before the firebolt cast at tick 3; expect
/// `CastRejected reason=NoLineOfSight` and nothing else (a clean, blocked-only golden).
pub fn line_of_sight() -> Scenario {
    Scenario::new("line_of_sight", 1, 30)
        .describe("An obstacle between caster and target blocks the cast (NoLineOfSight).")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor(
            "target",
            Faction::Enemy,
            100.0,
            0.0,
            Vec3::new(0.0, 0.0, 6.0),
        )
        .at(
            0,
            Action::Obstacle {
                pos: Vec3::new(0.0, 0.0, 3.0),
                radius: 1.0,
            },
        )
        .at(
            3,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("target".into()),
            },
        )
}

/// Already casting: cast firebolt, then cast again while the first is still in windup.
/// Mirrors `tests/vertical_slice.rs::second_cast_while_casting_is_rejected`.
/// The firebolt windup is ~18 ticks, so a second cast a few ticks later is rejected
/// `AlreadyCasting`. The dummy has high life so it survives long enough for a clean golden.
pub fn already_casting() -> Scenario {
    Scenario::new("already_casting", 1, 60)
        .describe("A second cast issued mid-windup is rejected (AlreadyCasting).")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor(
            "dummy",
            Faction::Enemy,
            200.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
        .at(
            4,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Apply effect: directly apply `burn` to an enemy (no cast pipeline).
/// Expect `EffectApplied burn`. NOTE: a *directly*-applied burn carries no triggering
/// damage, so its DoT DPS (= `base_damage_percent * status_damage * …`, obelisk
/// `calculate_status_dot_dps`) is 0 — hence no `DotTicked` lines (correct engine behavior;
/// damage-driven DoTs are exercised by `firebolt_kill`, where the bolt's hit seeds the burn).
pub fn apply_effect() -> Scenario {
    Scenario::new("apply_effect", 1, 120)
        .describe("Directly applying a burn (no triggering hit) emits EffectApplied with no DoT - zero status-damage means no ticks.")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .actor(
            "dummy",
            Faction::Enemy,
            100.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .at(
            1,
            Action::ApplyEffect {
                target: "dummy".into(),
                effect: "burn".into(),
            },
        )
}

/// Cooldown gate: the player casts a cooldown-bearing firebolt variant (`firebolt_cd`,
/// `cooldown = 2.0` → 120 ticks at 60 Hz), then casts again AFTER the first cast finishes
/// (the firebolt cast resolves to Done by ~tick 37) but while the skill is still on cooldown.
/// Expect a `CooldownStarted dur=2.000` on the first cast and a `CastRejected reason=OnCooldown`
/// on the second. (The second cast is intentionally scheduled past the first cast's recovery so
/// the gate that fires is `OnCooldown`, not the earlier `AlreadyCasting` concurrent-cast gate.)
/// The first cast still kills the dummy (firebolt_cd is mechanically identical to firebolt aside
/// from the cooldown), so the golden also carries the usual hit/damage/death trace — the cooldown
/// rejection is the new behavior. `firebolt_cd` is a dedicated fixture so adding a cooldown does
/// NOT perturb the `firebolt` goldens (firebolt_kill / netcode_egress), which stay cooldown-free.
pub fn cooldown_gate() -> Scenario {
    Scenario::new("cooldown_gate", 42, 90)
        .describe("After the first cast starts the cooldown, a second cast within it is rejected (OnCooldown).")
        .cast_asset("firebolt_cd")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt_cd")
        .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt_cd".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
        .at(
            45,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt_cd".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Trigger cascade: the player carries a `charged` self-effect whose `OnConsume` condition
/// triggers `static_discharge`, and casts `discharge_strike` (which `consumes_self_effect`
/// `charged`) at a dummy. When the strike's hit confirms, the resolve funnel consumes the
/// `charged` effect, surfaces the trigger as `TriggerFired source=player target=dummy
/// skill=static_discharge effect=charged`, AND auto-fires `static_discharge` as a second
/// (lightning) damage hit against the same target — the natural on-hit triggered-skill routing
/// (`src/combat/system.rs::on_hit_confirmed`). This exercises the *surfaced + auto-fired on-hit*
/// trigger path end-to-end through the cast pipeline; it does NOT touch the documented
/// `on_kill`/splash/counter routing boundary, which stays deferred.
///
/// Fixtures authored for this scenario (no pre-existing trigger fixture exists in this repo):
/// `discharge_strike.toml` (+ `.cast.ron`), `static_discharge.toml`, and the `charged.toml`
/// effect with the `on_consume` condition.
pub fn trigger_cascade() -> Scenario {
    Scenario::new("trigger_cascade", 5, 60)
        .describe("Hitting while 'charged' consumes the buff and fires a triggered skill (TriggerFired -> static_discharge bonus damage).")
        .cast_asset("discharge_strike")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("discharge_strike")
        .actor(
            "dummy",
            Faction::Enemy,
            200.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .at(
            1,
            Action::ApplyEffect {
                target: "player".into(),
                effect: "charged".into(),
            },
        )
        .at(
            3,
            Action::Cast {
                caster: "player".into(),
                skill: "discharge_strike".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Loot on death: a goblin enemy carrying `with_drop_table("goblin")` is killed by firebolt.
/// `run_scenario` loads the `goblin` drop table into `DropTables` (because an actor declares a
/// drop table), and `roll_drops_on_death` rolls it on the goblin's death — deterministically,
/// off the seeded `CombatRng` — emitting a `Loot source=goblin drops=[...]` line. Mirrors
/// `tests/vfx_content.rs::dead_enemy_with_a_drop_table_drops_loot` but driven through the full
/// cast pipeline (a real firebolt kill) rather than a synthetic `EntityDied` trigger.
pub fn loot_on_death() -> Scenario {
    Scenario::new("loot_on_death", 7, 600)
        .describe("Killing an enemy with a drop table rolls loot deterministically (LootDropped).")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor(
            "goblin",
            Faction::Enemy,
            25.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .with_drop_table("goblin")
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("goblin".into()),
            },
        )
}

/// Shared spawn skeleton for the loot drop-table scenarios: a firebolt-caster player + a goblin
/// (same 25-life / 0-mana / `Vec3(0,0,2)` placement as `loot_on_death`) carrying `drop_table`,
/// with the cast scripted at tick 1. The goblin dies to the firebolt hit + burn DoT and rolls its
/// table on death — deterministically, off the seeded `CombatRng` — emitting one `Loot` line. The
/// caller picks the seed + roll-param builders + drop-table fixtures.
fn loot_scenario(name: &str, seed: u64, drop_table: &str) -> Scenario {
    Scenario::new(name, seed, 600)
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor(
            "goblin",
            Faction::Enemy,
            25.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .with_drop_table(drop_table)
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("goblin".into()),
            },
        )
}

/// Loot: MULTIPLE guaranteed drops. The `multi_drop` table has a single roll-count option of
/// `count = 2` (so it deterministically rolls its entry selection TWICE) over a two-entry pool
/// (an item + a currency). The two rolls list 2 drops, so the `Loot source=goblin drops=[...]`
/// line carries 2+ entries — proving multiple-drops-per-kill resolution (vs `loot_on_death`'s
/// single currency). Faithfully records whichever pair the seed's two weighted selections produce.
pub fn loot_multi_drop() -> Scenario {
    loot_scenario("loot_multi_drop", 7, "multi_drop")
        .describe("A drop table with two rolls over an item+currency pool lists 2+ drops on the Loot line.")
        .with_drop_table_fixture("multi_drop", "tests/fixtures/loot/multi_drop.toml")
}

/// Loot: QUANTITY scaling. The `coin_pile` table drops a fixed `count = [10, 10]` currency; the
/// goblin carries `.with_quantity_mult(3.0)`, threaded into the roll via `DropRollParams`.
/// `tables_core`'s `apply_quantity_mult` scales the currency count by the multiplier, so the
/// `Loot` line's `count` is ~3x the unscaled baseline (a count near 30 vs 10). Fixed `[10,10]`
/// range so the multiplier is the only variable on the count (deterministic under the seed).
/// The contrast is pinned by `loot_quantity_scaling_multiplier_raises_currency_count` below.
pub fn loot_quantity_scaling() -> Scenario {
    loot_scenario("loot_quantity_scaling", 7, "coin_pile")
        .describe("An enemy with quantity_mult=3 on a fixed-count currency table drops ~3x the base count.")
        .with_quantity_mult(3.0)
        .with_drop_table_fixture("coin_pile", "tests/fixtures/loot/coin_pile.toml")
}

/// Loot: RARITY scaling. The `rarity_tiers` table has a heavily-weighted common currency
/// (`copper`, weight 100, rarity_bonus 0) and a rare currency (`diamond`, weight 1, rarity_bonus
/// 75). The roll's effective weight is `weight + rarity_bonus * rarity_mult`. The bonus is sized so
/// that at the BASELINE rarity_mult=1.0 diamond is the UNDERDOG (1 + 75*1 = 76 vs copper's 100) AND
/// seed 7's deciding draw genuinely lands `copper`; the goblin's `.with_rarity_mult(50.0)` lifts
/// diamond's effective weight to 1 + 75*50 = 3751 (vs copper's 100, ~97%), flipping that SAME seed's
/// pick to the higher-rarity tier -> the `Loot` line shows `diamond`. So the multiplier is
/// LOAD-BEARING (it flips the pick), not a coincidental baseline minority draw. The baseline-picks-
/// `copper` / scaled-picks-`diamond` contrast is pinned by
/// `loot_rarity_scaling_multiplier_flips_to_rare_tier` below.
pub fn loot_rarity_scaling() -> Scenario {
    loot_scenario("loot_rarity_scaling", 7, "rarity_tiers")
        .describe("An enemy with rarity_mult=50 on a rarity-gated table drops the higher-rarity tier (diamond).")
        .with_rarity_mult(50.0)
        .with_drop_table_fixture("rarity_tiers", "tests/fixtures/loot/rarity_tiers.toml")
}

/// Loot: NESTED table resolution. The goblin carries `nested_chest`, whose only entry is a
/// `type = "table"` reference to `treasure_inner` (loaded via a SECOND `with_drop_table_fixture`).
/// `tables_core`'s roll resolves the reference through the registry and splices the inner table's
/// drops into the result, so the `Loot` line carries the INNER table's `ruby` currency — proving
/// nested-table resolution across two fixtures. `count = [2, 2]` pulls the inner table twice.
pub fn loot_nested_table() -> Scenario {
    loot_scenario("loot_nested_table", 7, "nested_chest")
        .describe("A drop table that references another table resolves the nested table's drops on the Loot line.")
        .with_drop_table_fixture("nested_chest", "tests/fixtures/loot/nested_chest.toml")
        .with_drop_table_fixture("treasure_inner", "tests/fixtures/loot/treasure_inner.toml")
}

/// Netcode egress: identical to `firebolt_kill` but recording the buffered `NetEvent`
/// wire stream into the trace. The golden additionally contains `Net` lines carrying the
/// stable String ids (the replication egress).
pub fn netcode_egress() -> Scenario {
    Scenario::new("netcode_egress", 42, 600)
        .describe("Like firebolt_kill but recording the buffered NetEvent egress (Net lines, stable string ids).")
        .cast_asset("firebolt")
        .recording_net()
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// On-apply trigger: `on_apply_proc` is an infinite-duration buff whose `OnApply` condition triggers
/// `static_discharge`. We apply it to a target via `Action::ApplyEffect` at tick 1 (after the entity is
/// registered in `ObeliskEntityIndex`, so the trace carries resolved string ids rather than `?`). The
/// apply routes through `apply_obelisk_effect` -> `StatBlock::add_effect`, which collects every `OnApply`
/// condition on the freshly-added effect and returns it as a `TriggeredEffect`; the verb surfaces each as
/// `TriggerFired`. The golden shows `EffectApplied(on_apply_proc)` AND, at the same tick, `TriggerFired
/// skill=static_discharge effect=on_apply_proc` (source == target, a self-sourced effect). The triggered
/// skill is SURFACED only — not auto-fired from the command closure (the documented apply-path boundary),
/// so there is no second damage line. Reuses the existing `static_discharge` skill as the trigger target.
pub fn on_apply_triggers_skill() -> Scenario {
    Scenario::new("on_apply_triggers_skill", 5, 12)
        .describe("Applying an OnApply-trigger buff surfaces TriggerFired(static_discharge) at apply time alongside EffectApplied.")
        .actor("target", Faction::Enemy, 100.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "on_apply_proc".into(),
            },
        )
}

/// On-expire trigger (end-to-end proof of the H5 fix): a target starts with `on_expire_proc`, a short
/// 0.1s finite-duration debuff whose `OnExpire` condition triggers `static_discharge`. The effect is
/// applied at scenario start (`.with_self_effect`), and the scenario runs long enough (60 ticks ≈ 1.0s)
/// for `tick_effects_system` to expire it. Before the H5 fix, `tick_effects_system` discarded the
/// `TickResult.triggered_effects`, so the OnExpire trigger was silently dropped. The golden shows
/// `EffectApplied(on_expire_proc)` -> ... -> `EffectExpired(on_expire_proc)` AND `TriggerFired
/// skill=static_discharge effect=on_expire_proc` at expiry (surfaced, not auto-fired). Mirrors the
/// `src/core/tick.rs::on_expire_trigger_is_surfaced_when_an_effect_expires` unit test through the full
/// scenario pipeline.
pub fn on_expire_triggers_skill() -> Scenario {
    Scenario::new("on_expire_triggers_skill", 7, 60)
        .describe("A short OnExpire-trigger debuff fires TriggerFired(static_discharge) when it expires (end-to-end proof of the tick-path OnExpire surfacing).")
        .actor("target", Faction::Enemy, 100.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .with_self_effect("on_expire_proc")
}

/// On-max-stacks trigger + consume: `rage` is an infinite-duration `limited`-stacking buff (max_stacks
/// = 3) whose `OnMaxStacks { consume = true }` condition triggers `static_discharge`. We apply it to a
/// target three times (a few ticks apart) via `Action::ApplyEffect`. Each application routes through
/// `apply_obelisk_effect` -> `StatBlock::add_effect`'s `Limited` branch, which `add_stack`s and, only
/// once `stacks >= max_stacks`, returns the `OnMaxStacks` trigger AND (because `consume = true`) removes
/// the effect. So the golden shows `EffectApplied stacks=1`, then `stacks=2`, then on the 3rd application
/// `TriggerFired skill=static_discharge effect=rage` (the surfaced trigger) — and the 3rd `EffectApplied`
/// reads `dur=0.000 stacks=1` because the verb re-reads the effect AFTER `add_effect` already consumed
/// (removed) it (the genuine post-consume read; not a fabricated value). The triggered skill is surfaced
/// only, not auto-fired from the command closure (the documented apply-path boundary). Reaching max-stacks
/// IS elicited through the public apply pipeline, so no routing-boundary drop is needed here.
pub fn on_max_stacks_triggers_and_consumes() -> Scenario {
    Scenario::new("on_max_stacks_triggers_and_consumes", 5, 30)
        .describe("Applying a limited max_stacks=3 buff three times reaches max stacks on the 3rd, firing TriggerFired(static_discharge) and consuming (removing) the effect.")
        .actor("target", Faction::Enemy, 100.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "rage".into(),
            },
        )
        .at(
            5,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "rage".into(),
            },
        )
        .at(
            9,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "rage".into(),
            },
        )
}

/// Refresh stacking (Batch 4): `refresh_dot` is a finite-duration (5.0s) `refresh`-stacking debuff with
/// `max_stacks = 3`. We apply it four times a few ticks apart via `Action::ApplyEffect`. Each
/// re-application routes through `apply_obelisk_effect` -> `StatBlock::add_effect`'s `Refresh` branch,
/// which `add_stack()`s (capped at `max_stacks`) and `refresh()`es the SHARED duration timer back to
/// full. The verb re-reads the effect after `add_effect`, so the golden's `EffectApplied` line shows
/// the Refresh signature: `stacks` climbs 1 -> 2 -> 3 and then PLATEAUS at the 3-stack cap on the 4th
/// application, while `dur` stays pinned at the full `5.000` on every re-application (the duration is
/// refreshed, never decaying in the trace). These exact values were captured empirically from obelisk's
/// real `add_effect` (`stacks=1,2,3,3` / `dur=5.000`), not fabricated.
///
/// NOTE ON THE "DoT" NAME / NO `DotTicked` LINES: the public apply pipeline (`apply_obelisk_effect`)
/// builds the effect via `EffectConfig::to_effect`, which constructs a stat-modifier `Effect` with
/// `dot: None` — obelisk only attaches DoT data (`dot_dps`) when an effect is applied through the
/// *damage* pipeline (`to_ailment_effect_with_values`, the firebolt path), where the DPS is computed
/// from the hit. So a directly-applied effect NEVER ticks DoT damage (cf. `apply_effect`, where a
/// directly-applied `burn` also emits zero `DotTicked`). The faithful, observable signature of the
/// Refresh *stacking* mode through this public path is therefore the `EffectApplied` stacks/duration
/// progression above — the golden does not (and must not) fabricate `DotTicked` lines.
pub fn effect_refresh_stacking() -> Scenario {
    Scenario::new("effect_refresh_stacking", 5, 30)
        .describe("Re-applying a refresh-stacking debuff (max_stacks=3) adds a stack and refreshes duration: EffectApplied stacks climb 1->2->3 then plateau at the cap, dur stays pinned at 5.000.")
        .actor("target", Faction::Enemy, 100.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "refresh_dot".into(),
            },
        )
        .at(
            5,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "refresh_dot".into(),
            },
        )
        .at(
            9,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "refresh_dot".into(),
            },
        )
        .at(
            13,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "refresh_dot".into(),
            },
        )
}

/// Unlimited stacking (Batch 4): `unlimited_dot` is a finite-duration (5.0s) `unlimited`-stacking
/// debuff with NO `max_stacks` cap. We apply it four times a few ticks apart via `Action::ApplyEffect`.
/// Each re-application routes through `apply_obelisk_effect` -> `StatBlock::add_effect`'s `Unlimited`
/// branch, which promotes the timer to `StackTimers::PerStack` and PUSHES an independent per-stack
/// timer, setting `stacks = timers.len()`. So the golden's `EffectApplied` line shows the Unlimited
/// signature: `stacks` climbs WITHOUT bound — 1 -> 2 -> 3 -> 4 — visibly diverging from the refresh
/// fixture (which plateaus at 3 on its 4th application). `dur` stays at `5.000` (the Unlimited branch
/// never calls `refresh`, but `total_duration` was set to the full 5.0 at creation and is unchanged).
/// These exact values were captured empirically from obelisk's real `add_effect` (`stacks=1,2,3,4` /
/// `dur=5.000`), not fabricated.
///
/// (Same "DoT name / no `DotTicked`" caveat as `effect_refresh_stacking`: the public apply pipeline
/// builds the effect with `dot: None`, so the observable signature is the climbing-stacks
/// `EffectApplied` progression, not DoT ticks.)
pub fn effect_unlimited_stacking() -> Scenario {
    Scenario::new("effect_unlimited_stacking", 5, 30)
        .describe("Re-applying an unlimited-stacking debuff adds independent stacks with no cap: EffectApplied stacks climb 1->2->3->4 unbounded (vs the refresh fixture plateauing at its max_stacks).")
        .actor("target", Faction::Enemy, 100.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "unlimited_dot".into(),
            },
        )
        .at(
            5,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "unlimited_dot".into(),
            },
        )
        .at(
            9,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "unlimited_dot".into(),
            },
        )
        .at(
            13,
            Action::ApplyEffect {
                target: "target".into(),
                effect: "unlimited_dot".into(),
            },
        )
}

/// Self-buff boosts damage (Batch 3, end-to-end proof of "applying buffs to self"): the player
/// starts with the `empower` self-buff (`+50% increased fire damage`, applied at spawn via
/// `.with_self_effect` -> the public `apply_obelisk_effect` verb, sourced from the caster itself),
/// then casts firebolt at a dummy. Because `use_skill_against`'s `calculate_damage` reads the
/// caster's `global_fire_damage.increased` layer, the firebolt's 20 base fire damage is scaled to
/// `20 * (1 + 0.50) = 30.000` — the golden's Damage line shows `dmg=30.000`, visibly HIGHER than
/// the un-buffed baseline of `20.000` (see `firebolt_kill`). That higher number IS the observable
/// end-to-end proof that a self-applied buff boosts outgoing damage; the computed-stat VALUE behind
/// it is unit-tested directly in `src/verbs.rs` (`apply_obelisk_effect_modifies_computed_stat` /
/// `self_buff_removal_reverts_stat`), since stat values are not carried in the event trace.
///
/// The dummy carries enough life (50) to SURVIVE the boosted 30 direct hit, so the Damage line
/// records cleanly (kill=false) before the burn DoT finishes it — keeping the boosted number front
/// and centre. Ships the new `empower.toml` effect fixture (no existing scenario references it, so
/// the existing goldens are untouched).
///
/// NOTE: the spec offered an OPTIONAL caster-target self-buff *skill* (`empower_self`) as an
/// alternative trigger. obelisk's skill config DOES support `target = "caster"` effect applications
/// (`stat_core::skill::ApplicationTarget::Caster`, applied in `use_skill_against` Step 1), but a
/// pure no-damage buff skill does not flow through the bevy cast/hit pipeline cleanly (the resolve
/// funnel `resolve_one_hit` is hit-driven and requires a target). The `.with_self_effect` path —
/// explicitly endorsed by the spec as still satisfying "applying buffs to self" — is the robust,
/// deterministic choice and is used here.
pub fn self_buff_boosts_damage() -> Scenario {
    Scenario::new("self_buff_boosts_damage", 42, 600)
        .describe("Self-buffing with 'empower' (+50% fire damage) before casting firebolt boosts the bolt's damage from 20 to 30 - applying a buff to self raises outgoing damage.")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .with_self_effect("empower")
        .actor("dummy", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Crit strike (Batch 5, the stat-attach payoff): a caster with a 100% flat critical-strike chance
/// (`AddedCriticalChance` = 100, flowed through obelisk's real stat-rebuild path via `.with_stat`)
/// casts firebolt at a dummy. `AddedCriticalChance` lands on `StatBlock::critical_chance.flat`
/// (`StatAccumulator::apply_stat_type` → `critical_chance_flat`); `calculate_crit_chance` then yields
/// `(0 base + 100 flat) * 1.0 * 1.0 = 100%`, so the seeded crit roll (`rng.gen::<f64>() < 1.0`) ALWAYS
/// passes — a deterministic, guaranteed crit. On crit obelisk multiplies every damage by the
/// attacker's `computed_crit_multiplier()` (default base `1.5`, no bonuses here), so the firebolt's
/// 20 base fire damage becomes `20 * 1.5 = 30.000`. The golden's Damage line shows `crit=true` AND
/// `dmg=30.000`, visibly higher than the un-buffed `20.000` baseline (`firebolt_kill`).
///
/// The dummy carries 50 life (> 30) so it SURVIVES the boosted crit hit (kill=false) and the Damage
/// line records cleanly with the crit number front and centre, before the burn DoT finishes it.
/// Uses `.with_stat` only — no new fixture needed (precedent: the `FireResistance` round-trip unit
/// test in `src/scenario/mod.rs`).
pub fn crit_strike() -> Scenario {
    Scenario::new("crit_strike", 42, 600)
        .describe("A caster with 100% critical-strike chance always crits firebolt: the Damage line reads crit=true and 30.000 (20 base x the 1.5 crit multiplier), up from the un-crit 20.000.")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .with_stat(StatType::AddedCriticalChance, 100.0)
        .actor("dummy", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Resistance mitigates (Batch 5): a DUMMY target with 50% fire resistance (`FireResistance` = 50,
/// flowed through obelisk's real stat-rebuild path via `.with_stat` — `FireResistance` is a flat add,
/// landing on `StatBlock::fire_resistance`) is hit by firebolt (fire damage). obelisk's resolution
/// mitigates fire damage by resistance (`calculate_resistance_mitigation`: `damage * (1 - resist/100)`
/// with no penetration), so the 20 base fire damage is reduced to `20 * (1 - 0.50) = 10.000` and
/// `10.000` is prevented. The Damage line shows `prevented=10.000` (> 0) AND `dmg=10.000`, visibly
/// lower than the un-resisted `20.000` baseline (`firebolt_kill`). `damage_prevented` is summed from
/// the real obelisk `CombatResult.damage_reduced_by_resists` (see `combat_result_prevented`).
///
/// The dummy carries 50 life (> 10) so it SURVIVES both the mitigated direct hit (kill=false) and
/// the subsequent burn DoT (the burn expires without a kill); the Damage line records cleanly with
/// the prevented amount. Uses `.with_stat` only — no new fixture needed.
pub fn resistance_mitigates() -> Scenario {
    Scenario::new("resistance_mitigates", 42, 600)
        .describe("A target with 50% fire resistance halves firebolt's fire damage: the Damage line reads prevented=10.000 and dmg=10.000, down from the un-resisted 20.000.")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .with_stat(StatType::FireResistance, 50.0)
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Cast-speed scaling (Batch 5, validating the timeline's speed-scaled phase durations): a caster
/// with +100% increased cast speed (`IncreasedCastSpeed` = 100, flowed through obelisk's real
/// stat-rebuild path via `.with_stat` — `StatAccumulator::apply_stat_type` adds `100/100 = 1.0`
/// increased to `cast_speed`) casts firebolt. firebolt is a spell, so `effective_rate` reads
/// `cast_speed.compute() = 1.0 * (1 + 1.0) = 2.0`; `scale_durations` then divides every authored
/// phase duration by 2 (`validate_casts`). The authored windup/active/recovery (0.3/0.1/0.2 s =
/// 18/6/12 ticks at 60 Hz) become 0.15/0.05/0.10 s = 9/3/6 ticks, so the cast phases and the bolt's
/// hit window all fire at EARLIER trace ticks than the un-scaled `firebolt_kill` baseline
/// (Windup->Active + HitWindow at tick 19, HitConfirmed at 21). The shortened ticks are computed
/// deterministically off the fixed timestep + the rebuilt cast_speed; this is the end-to-end proof
/// that increased cast speed shortens the timeline. Uses `.with_stat` only — no new fixture needed.
pub fn cast_speed_scaling() -> Scenario {
    Scenario::new("cast_speed_scaling", 42, 600)
        .describe("A caster with +100% increased cast speed casts firebolt at 2x rate: the cast phases / hit window / hit-confirm all land at earlier (roughly halved) trace ticks than the un-scaled firebolt_kill baseline.")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .with_stat(StatType::IncreasedCastSpeed, 100.0)
        .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Armour mitigates (Batch 11, secondary-defense layer): a DUMMY target with 500 armour
/// (`AddedArmour` = 500, flowed through obelisk's real stat-rebuild path via `.with_stat` —
/// `AddedArmour` lands on `StatBlock::armour` as flat) is hit by `cleave` (100 PHYSICAL damage).
/// Physical damage is mitigated by ARMOUR, not resistance: obelisk's `calculate_armour_reduction`
/// applies the PoE diminishing-returns formula `reduction% = armour / (armour + C * damage)` with
/// the default `armour.damage_constant = 5.0`, so `500 / (500 + 5 * 100) = 500 / 1000 = 50%`. The
/// 100 physical is reduced to `100 * (1 - 0.50) = 50.000` and `50.000` is prevented. The Damage line
/// shows `prevented=50.000` (> 0) AND `dmg=50.000`, half the un-armoured `100.000` baseline
/// (`cone_cleave`). `damage_prevented` is summed from the real obelisk
/// `CombatResult.damage_reduced_by_armour` (see `combat_result_prevented`). This is the armour code
/// path (`src/defense/armour.rs`), a DISTINCT mitigation layer from `resistance_mitigates`.
///
/// The dummy carries 200 life (> 50) so it SURVIVES the mitigated hit (kill=false); cleave applies no
/// status effect, so the Damage line records cleanly with the prevented amount and no DoT follows.
/// Uses `.with_stat` only — no new fixture or production change needed.
pub fn armour_mitigates() -> Scenario {
    Scenario::new("armour_mitigates", 42, 60)
        .describe("A target with 500 armour halves cleave's physical hit (PoE diminishing-returns formula): the Damage line reads prevented=50.000 and dmg=50.000, down from the un-armoured 100.000.")
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .with_stat(StatType::AddedArmour, 500.0)
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Physical damage reduction mitigates (Batch 11, secondary-defense layer): a DUMMY target with 40%
/// PHYSICAL damage reduction (`PhysicalDamageReduction` = 40, flowed through obelisk's real
/// stat-rebuild path via `.with_stat` — landing on `StatBlock::physical_damage_reduction`) is hit by
/// `cleave` (100 PHYSICAL damage). This is a SEPARATE flat-percent physical layer applied AFTER
/// armour in obelisk's resolution (`resolution.rs` Step 2b: `reduced = phys * dr` with
/// `dr = clamp(value, 0, 90) / 100`); here the target has NO armour, so ONLY this channel engages.
/// The 100 physical is reduced to `100 * (1 - 0.40) = 60.000` and `40.000` is prevented. The Damage
/// line shows `prevented=40.000` (> 0) AND `dmg=60.000`, below the un-reduced `100.000` baseline
/// (`cone_cleave`). `damage_prevented` is summed from the real obelisk
/// `CombatResult.damage_reduced_by_physical_dr` (see `combat_result_prevented`) — a DISTINCT channel
/// from armour (`damage_reduced_by_armour`) and the generic reduction (`damage_reduced_by_dr`).
///
/// The dummy carries 200 life (> 60) so it SURVIVES the mitigated hit (kill=false). Uses `.with_stat`
/// only — no new fixture or production change needed.
pub fn physical_damage_reduction_mitigates() -> Scenario {
    Scenario::new("physical_damage_reduction_mitigates", 42, 60)
        .describe("A target with 40% physical damage reduction cuts cleave's physical hit: the Damage line reads prevented=40.000 and dmg=60.000, down from the un-reduced 100.000.")
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .with_stat(StatType::PhysicalDamageReduction, 40.0)
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Generic damage reduction mitigates (Batch 11, secondary-defense layer): a DUMMY target whose
/// reduced-damage-taken layer is set to a 30% global cut, hit by `cleave` (100 damage). Unlike
/// armour/physical-DR (which only touch physical), `reduced_damage_taken` is the GLOBAL final
/// multiplier obelisk applies to ALL damage types (`resolution.rs` Step 3b:
/// `reduced = final * clamp(field, 0, 90) / 100`). The 100 is reduced to `100 * (1 - 0.30) = 70.000`
/// and `30.000` is prevented. The Damage line shows `prevented=30.000` (> 0) AND `dmg=70.000`, below
/// the un-reduced `100.000` baseline (`cone_cleave`). `damage_prevented` is summed from the real
/// obelisk `CombatResult.damage_reduced_by_dr` (see `combat_result_prevented`) — the generic-DR
/// channel, DISTINCT from armour, physical-DR, resistance, and oneshot.
///
/// SCALING NOTE (faithful to a real obelisk quirk): for `ReducedDamageTaken` the StatType→field path
/// already divides by 100 (`apply_stat_type`: `reduced_damage_taken += value / 100`), but the
/// resolution step consumes the field as a raw PERCENT and divides by 100 AGAIN
/// (`dr = field.clamp(0, 90) / 100`). The net reduction fraction is therefore `value / 10000`, so a
/// 30% cut needs `value = 3000` (→ field 30.0 → `30/100 = 0.30`). (Contrast `FireResistance` = 50 in
/// `resistance_mitigates`, whose field is consumed directly as a percent — no double divide.) The
/// golden records obelisk's REAL behavior under this value; nothing is faked.
///
/// The dummy carries 200 life (> 70) so it SURVIVES the mitigated hit (kill=false). Uses `.with_stat`
/// only — no new fixture or production change needed.
pub fn damage_reduction_mitigates() -> Scenario {
    Scenario::new("damage_reduction_mitigates", 42, 60)
        .describe("A target with 30% reduced damage taken (global) cuts cleave's hit: the Damage line reads prevented=30.000 and dmg=70.000, down from the un-reduced 100.000.")
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .with_stat(StatType::ReducedDamageTaken, 3000.0)
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Oneshot protection caps (Batch 11, secondary-defense layer): a DUMMY target with 19000 oneshot
/// protection (`AddedOneshotProtection` = 19000, flowed through obelisk's real stat-rebuild path via
/// `.with_stat` — landing on `StatBlock::oneshot_protection`) is hit by `cleave` (100 damage). Oneshot
/// protection imposes a per-hit DAMAGE CAP that engages only when the rating exceeds the attacker's
/// accuracy (default 1000): `cap = accuracy / (1 + oneshot_protection / scale_factor)` with the
/// default `evasion.scale_factor = 1000` → `1000 / (1 + 19000 / 1000) = 1000 / 20 = 50`. The 100 hit
/// EXCEEDS the 50 cap, so it is capped to `50.000` and `50.000` is prevented (`resolution.rs` Step 3,
/// `apply_oneshot_protection`). The Damage line shows `prevented=50.000` (> 0) AND `dmg=50.000`, below
/// the un-capped `100.000` baseline (`cone_cleave`). `damage_prevented` is summed from the real
/// obelisk `CombatResult.damage_prevented_by_oneshot` (see `combat_result_prevented`) — a DISTINCT
/// mitigation layer (`src/defense/evasion.rs`) from armour, physical-DR, generic-DR, and resistance.
///
/// The dummy carries 200 life (> 50) so it SURVIVES the capped hit (kill=false). Uses `.with_stat`
/// only — no new fixture or production change needed.
pub fn oneshot_protection_caps() -> Scenario {
    Scenario::new("oneshot_protection_caps", 42, 60)
        .describe("A target with high oneshot protection caps cleave's per-hit damage (accuracy/(1+osp/scale) = 50): the Damage line reads prevented=50.000 and dmg=50.000, down from the un-capped 100.000.")
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .with_stat(StatType::AddedOneshotProtection, 19000.0)
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

// Barrier (`barrier_absorbs`) and elude (`elude_reduces`) are DEFERRED, not faked. Both soak via the
// runtime CURRENT pools (`StatBlock::current_barrier` / `current_elude_stacks`), which obelisk's
// stat-rebuild path leaves at 0 even when `.with_stat` raises the MAX (`rebuild()` does
// `current_barrier = current_barrier.min(max_barrier)` and explicitly preserves `current_elude_stacks`
// — see `stat_block/mod.rs`). Reaching them requires the `grant_barrier` / `grant_elude` verbs, which
// would need a new additive `ActorSpec` field wired through `spawn_actor`. With four `.with_stat`-only
// layers already delivered here (armour, physical-DR, generic-DR, oneshot — exceeding the >=3 target),
// those two are left as a clean follow-up rather than expanding the scenario harness in this batch.
//
// Spell-dodge (`was_dodged`) is also DROPPED: a dodged hit returns early in obelisk's resolution with
// no damage and no mitigation-channel write, and the dodge flag (`CombatResult.was_dodged`) is NOT
// threaded into `DamageResolved` today — surfacing it is a production change explicitly out of scope
// for this batch, so it is documented here rather than faked.

/// Interrupt mid-windup (Batch 8, the H3 `Action::Interrupt` payoff): the player casts firebolt at a
/// dummy at tick 1, then an `Action::Interrupt { id: "player" }` fires at tick 5 — mid-windup, BEFORE
/// the bolt's hit window opens (firebolt windup runs to ~tick 19; see `already_casting`/`firebolt_kill`).
/// `Action::Interrupt` routes through the public `interrupt_cast` verb (`remove::<(ActiveCast,
/// PendingCast)>`), so the in-flight cast is genuinely cancelled. The golden shows `CastBegan` (the cast
/// did start) and then NOTHING further for that cast: NO `CastPhase Windup->Active`, NO `HitWindow`, NO
/// `HitConfirmed`, NO `Damage`, NO `Died` — proof the `ActiveCast` was removed before `advance_casts`
/// could cross into the Active phase and spawn the bolt window. (The only post-cancel lines are the
/// `OnCast` cue + the cooldown trace from the original cast-begin, which fire at cast START, before the
/// interrupt.) Mirrors the `src/scenario/mod.rs::interrupt_action_cancels_an_in_flight_cast` unit test
/// through the full golden pipeline.
pub fn interrupt_cast() -> Scenario {
    Scenario::new("interrupt_cast", 42, 60)
        .describe("Interrupting a cast mid-windup (Action::Interrupt) cancels it: CastBegan with NO hit window, NO HitConfirmed, NO Damage, NO death afterward.")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
        .at(
            5,
            Action::Interrupt {
                id: "player".into(),
            },
        )
}

/// Insufficient mana (Batch 8): a player spawned with ZERO mana (firebolt costs 5 mana) casts firebolt
/// at a dummy. `validate_casts` calls `attrs.0.can_use_skill(skill)`, which fails the mana gate, so the
/// cast is rejected `InsufficientMana` and never enters an `ActiveCast` — the golden shows
/// `CastRejected reason=InsufficientMana` and NO `Damage`. Mirrors
/// `tests/vertical_slice.rs::cast_without_mana_is_rejected` as a golden (mana set via the ActorSpec mana
/// field, the simplest reachable form; the spec's `Action::SetMana` alternative would produce the same
/// rejection).
pub fn cast_rejected_insufficient_mana() -> Scenario {
    Scenario::new("cast_rejected_insufficient_mana", 1, 30)
        .describe("A caster with too little mana for firebolt (cost 5, mana 0) is rejected (InsufficientMana) with no damage.")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 0.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Unknown skill (Batch 8): the player casts a skill id (`phantom`) that is NOT in the global
/// `SkillRegistry` (the fixtures register only firebolt/cleave/discharge_strike/static_discharge/
/// firebolt_cd). `apply_action`'s `Cast` only inserts a `PendingCast { skill_id: "phantom" }` — the cast
/// verb does NOT require the skill to be granted/registered — so the request reaches `validate_casts`,
/// where `registry.0.get("phantom")` returns `None` and the cast is rejected `UnknownSkill` (this is the
/// FIRST gate after the already-casting check, ahead of the timeline/mana/range checks). The golden shows
/// a single `CastRejected reason=UnknownSkill` and nothing else. Cleanly reachable through the public
/// scenario API.
pub fn cast_rejected_unknown_skill() -> Scenario {
    Scenario::new("cast_rejected_unknown_skill", 1, 30)
        .describe("Casting a skill id absent from the SkillRegistry is rejected (UnknownSkill).")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .actor("dummy", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "phantom".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// No target (Batch 8): the player casts firebolt at a dummy AND the dummy is despawned at the SAME tick.
/// The two actions apply in script order within the tick (`run_scenario` collects them in order, then
/// runs one `app.update()`): the `Cast` action resolves the dummy's entity via `ObeliskEntityIndex` and
/// QUEUES a `PendingCast { aim: CastAim::Entity(dummy_entity) }` (a command, flushed at the next update);
/// the `Despawn` action then IMMEDIATELY despawns the dummy (`world_mut().entity_mut(e).despawn()`). So
/// when `validate_casts` runs on the next update, the `PendingCast` still holds the now-stale dummy
/// `Entity`, `transforms.get(e)` fails, and the cast is rejected `NoTarget`. This is the despawn-between-
/// cast-request-and-resolution path: the `Cast` action must run while the target still resolves (so the
/// `Entity` is captured), with the despawn landing before `validate_casts` reads its transform — which the
/// queued-insert-vs-immediate-despawn ordering guarantees. The golden shows a single
/// `CastRejected reason=NoTarget` (the caster id resolves; the despawned target id is not in the line).
/// Cleanly reachable through the public scenario API.
pub fn cast_rejected_no_target() -> Scenario {
    Scenario::new("cast_rejected_no_target", 1, 30)
        .describe("A cast whose entity target despawns the same tick (before validation reads its transform) is rejected (NoTarget).")
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
        .at(
            1,
            Action::Despawn {
                id: "dummy".into(),
            },
        )
}

/// Multi-hit hitbox (`EveryTick`): a melee `pummel` skill whose collision window is authored with
/// `hit_mode: EveryTick` and a `Static` (non-moving) sphere that stays active for `active_duration =
/// 0.1s` (6 fixed ticks at 60 Hz). A single stationary, high-life enemy stands inside the sphere for
/// the whole window. Because `Hitbox::can_hit` returns `true` unconditionally for `EveryTick` (see
/// `src/spatial/boxes.rs`), `detect_overlaps` re-confirms the SAME target on every FixedUpdate the
/// box is alive, so the golden shows MULTIPLE `Damage` lines for the same `target=dummy` (one per
/// tick the box overlaps it) — the observable signature of a multi-hit hitbox, vs the single Damage
/// line of the `OncePerTarget` cleave / `FirstOnly` firebolt. The dummy carries 10_000 life so it
/// survives every tick (no `Died` truncating the repeats), and `pummel` is a zero-mana melee skill so
/// the per-tick `use_skill_against` never gates on mana. Ships the `pummel.toml` (+ `.cast.ron`)
/// fixtures; no existing scenario references them, so existing goldens are untouched.
pub fn everytick_hitbox() -> Scenario {
    Scenario::new("everytick_hitbox", 5, 30)
        .describe("A persistent EveryTick melee hitbox re-hits the same stationary enemy every tick it stays inside: the golden shows multiple Damage lines for the one target.")
        .cast_asset("pummel")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("pummel")
        .actor(
            "dummy",
            Faction::Enemy,
            10000.0,
            0.0,
            Vec3::new(0.0, 0.0, 1.0),
        )
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "pummel".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Instant cast (near-zero windup): `quickjab` is a melee skill authored with a near-zero windup
/// (`windup: 0.001s` ≈ a single fixed tick at 60 Hz) — vs firebolt's 0.3s / ~18-tick windup. Its hit
/// window therefore opens almost immediately after `CastBegan`: the golden's `HitWindow` /
/// `HitConfirmed` land at a VERY EARLY tick (≈ tick 2-3) rather than firebolt's ~19/21
/// (cf. `firebolt_kill`). This validates that the timeline faithfully resolves a (near-)instant cast
/// to an immediate hit — the authoring-controlled windup is what gates *when* the box spawns. The
/// dummy carries enough life (50) to survive the 15 jab so the Damage line records cleanly. Ships the
/// `quickjab.toml` (+ `.cast.ron`) fixtures; no existing scenario references them.
pub fn instant_cast() -> Scenario {
    Scenario::new("instant_cast", 5, 20)
        .describe("A near-zero-windup (instant) melee cast resolves to a hit almost immediately: HitWindow/HitConfirmed land at a very early tick vs firebolt's ~19.")
        .cast_asset("quickjab")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("quickjab")
        .actor("dummy", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 1.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "quickjab".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Probabilistic effect apply (seeded ⇒ deterministic): `chancebolt` is a firebolt-shaped projectile
/// that applies the `chill` debuff with a SUB-100% chance. obelisk computes the apply chance as
/// `status_damage / target_max_health` (`stat_core` `PendingStatusEffect::calculate_apply_chance`),
/// here `status_damage = 20 base fire * 1.0 conversion = 20` against the dummy's `40` max life, so the
/// chance is `20 / 40 = 0.50`. The roll (`rng.gen::<f64>() < 0.5` in `combat/resolution.rs`) is drawn
/// off the seeded `CombatRng`, so whether `chill` lands is FULLY DETERMINISTIC for this `(seed = 11,
/// 40-HP)` pair. The golden faithfully records the deterministic outcome for that seed: an
/// `EffectApplied chill` line is present iff the seeded roll passed (captured empirically from the
/// real resolve path — NOT fabricated; the companion unit test
/// `src/scenario/library.rs::probabilistic_apply_is_deterministic_and_split_by_seed` pins both a
/// PASS-seed and a FAIL-seed at the resolve layer so the absence/presence is proven, not assumed).
/// The dummy's 40 life survives the 20 fire hit (kill=false) so the Damage line — and the
/// applied-or-not `chill` — record cleanly without a `Died` truncating the trace. Ships the
/// `chancebolt.toml` (+ `.cast.ron`) skill and the `chill.toml` effect fixtures; no existing scenario
/// references them. The `apply_chance.damage_scaled` form (no `apply_chance = "always"`) is what makes
/// this a real chance roll rather than a guaranteed application.
pub fn probabilistic_effect_apply() -> Scenario {
    Scenario::new("probabilistic_effect_apply", 11, 60)
        .describe("A chancebolt applies 'chill' with a 50% (status_damage/max_life) chance; the seeded CombatRng makes the roll deterministic, so the golden records whether chill applied for this seed.")
        .cast_asset("chancebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("chancebolt")
        .actor("dummy", Faction::Enemy, 40.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "chancebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Skill-condition damage trigger casts its secondary skill (the follow-up to `trigger_cascade`,
/// which covers the EFFECT-condition path). The player casts `critzap` — a fire bolt carrying an
/// `on_crit` `SkillCondition` (`additional = true`) naming `trigger_skill = "static_discharge"` —
/// with a 100% flat critical-strike chance (`AddedCriticalChance` = 100, via `.with_stat`, the same
/// stat-rebuild path as `crit_strike`). The seeded crit roll always passes, so critzap crits and the
/// `on_crit` condition fires.
///
/// obelisk's `calculate_damage_with_triggers` builds a primary critzap packet PLUS an
/// `is_triggered` static_discharge packet, and `resolve_damage_with_triggers` resolves BOTH inline
/// against the dummy. The fix (`resolve_one_hit`'s partition + `on_hit_confirmed`'s emit) surfaces
/// the secondary skill as its OWN cast rather than folding its damage into the primary total. The
/// 100% crit chance applies to every packet, so the triggered static_discharge also crits. The
/// golden shows the proven three-line shape (parity with `trigger_cascade`):
///   `Damage skill=critzap dmg=30.000 crit=true` (20 base x the 1.5 crit multiplier — NOT inflated
///   by the 37.5 static_discharge, the double-count fix) ->
///   `TriggerFired skill=static_discharge effect=` (empty effect_id: a skill condition has no
///   originating effect) ->
///   `Damage skill=static_discharge dmg=37.500 crit=true` (25 base x 1.5 crit).
///
/// The dummy carries 200 life (> 30 + 37.5 = 67.5) so it SURVIVES both hits (kill=false on each),
/// recording cleanly. A resolve-layer unit test (`tests::skill_trigger_excludes_triggered_from_primary`)
/// pins that the primary `outcome.total_damage` EXCLUDES the triggered damage and the triggered hit
/// is surfaced in `triggered_skill_hits`.
pub fn skill_trigger_secondary_cast() -> Scenario {
    Scenario::new("skill_trigger_secondary_cast", 42, 60)
        .describe("Crit-triggering critzap casts its secondary skill: the on_crit damage condition surfaces static_discharge as its OWN TriggerFired + Damage line (37.5, also crit) instead of inflating the primary crit (30) — Damage(critzap,30,crit) -> TriggerFired(static_discharge) -> Damage(static_discharge,37.5,crit).")
        .cast_asset("critzap")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("critzap")
        .with_stat(StatType::AddedCriticalChance, 100.0)
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "critzap".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Batch 12 — `damage_type_dealt` (fire) skill-condition trigger casting its secondary skill (a
/// sibling of `skill_trigger_secondary_cast`, which is the `on_crit` template). The player casts
/// `firezap` — a 20-base FIRE bolt carrying a `damage_type_dealt`/`damage_type = "fire"`
/// `SkillCondition` (`additional = true`) naming `trigger_skill = "static_discharge"`. The caster
/// has NO crit stat, so the hit is a plain non-crit fire hit. obelisk's `calculate_damage_with_triggers`
/// evaluates the post-calculation `DamageTypeDealt { Fire }` condition against the primary packet
/// (`primary.damage_of_type(Fire) > 0.0` — TRUE, the bolt deals 20 fire), so it builds the primary
/// firezap packet PLUS an `is_triggered` static_discharge packet; `resolve_one_hit`'s partition
/// surfaces the secondary cast as its OWN `TriggerFired` + `Damage` line (the re-bucket feature),
/// distinct from the primary fire hit, exactly as `skill_trigger_secondary_cast` does for `on_crit`.
/// No crit, so neither hit crits — the golden shows the three-line shape:
///   `Damage skill=firezap dmg=20.000 crit=false` (the fire hit; condition fires BECAUSE it dealt fire) ->
///   `TriggerFired skill=static_discharge effect=` (empty effect_id: a skill condition has no
///   originating effect) ->
///   `Damage skill=static_discharge dmg=25.000 crit=false` (25 lightning base, no crit).
///
/// The dummy carries 200 life (> 20 + 25 = 45) so it SURVIVES both hits (kill=false on each),
/// recording cleanly. firezap deals fire (so the condition is always met) — the contrast that would
/// NOT fire is a non-fire skill (e.g. `cleave`'s physical), proven by the resolve-layer
/// `tests::firezap_fires_only_on_fire_damage` test below.
pub fn on_fire_damage_dealt_trigger() -> Scenario {
    Scenario::new("on_fire_damage_dealt_trigger", 42, 60)
        .describe("A fire bolt carrying a damage_type_dealt=fire condition casts its secondary skill: the fire hit satisfies the condition, surfacing static_discharge as its OWN TriggerFired + Damage line — Damage(firezap,20,fire) -> TriggerFired(static_discharge) -> Damage(static_discharge,25).")
        .cast_asset("firezap")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firezap")
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firezap".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Batch 12 — `damage_over_threshold` skill-condition trigger casting its secondary skill (a sibling
/// of `skill_trigger_secondary_cast`). The player casts `bigzap` — a 50-base fire bolt carrying a
/// `damage_over_threshold`/`threshold = 30.0` `SkillCondition` (`additional = true`) naming
/// `trigger_skill = "static_discharge"`. The threshold (30) is set BELOW the skill's damage (50), so
/// obelisk's post-calculation `DamageOverThreshold` condition (`primary.total_damage() > 30.0` — TRUE,
/// the bolt deals 50) fires and builds an `is_triggered` static_discharge packet alongside the primary;
/// `resolve_one_hit`'s partition surfaces the secondary cast as its OWN `TriggerFired` + `Damage` line.
/// No crit caster, so neither hit crits — the golden shows the three-line shape:
///   `Damage skill=bigzap dmg=50.000 crit=false` (the big hit; 50 > the 30 threshold) ->
///   `TriggerFired skill=static_discharge effect=` ->
///   `Damage skill=static_discharge dmg=25.000 crit=false`.
///
/// The dummy carries 200 life (> 50 + 25 = 75) so it SURVIVES both hits (kill=false on each). The
/// resolve-layer `tests::bigzap_fires_only_over_threshold` test pins the contrast that a hit UNDER the
/// threshold does NOT fire (a 20-damage hit vs the same 30 threshold surfaces no triggered skill).
pub fn on_big_hit_trigger() -> Scenario {
    Scenario::new("on_big_hit_trigger", 42, 60)
        .describe("A 50-damage bolt carrying a damage_over_threshold=30 condition casts its secondary skill: the big hit (50 > 30) satisfies the condition, surfacing static_discharge as its OWN TriggerFired + Damage line — Damage(bigzap,50) -> TriggerFired(static_discharge) -> Damage(static_discharge,25).")
        .cast_asset("bigzap")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("bigzap")
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "bigzap".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Batch 12 — `on_non_crit` skill-condition trigger casting its secondary skill (the deliberate
/// CONTRAST to `skill_trigger_secondary_cast`, which fires on the CRIT branch). The player casts
/// `steadyzap` — a 20-base fire bolt carrying an `on_non_crit` `SkillCondition` (`additional = true`)
/// naming `trigger_skill = "static_discharge"` — with NO crit stat (no `.with_stat(AddedCriticalChance)`),
/// so the seeded crit roll never passes and every hit is a NON-crit. obelisk's post-calculation
/// `OnNonCrit` condition (`!primary.is_critical` — TRUE) fires and builds an `is_triggered`
/// static_discharge packet alongside the primary; `resolve_one_hit`'s partition surfaces the secondary
/// cast as its OWN `TriggerFired` + `Damage` line. The golden shows the three-line shape, all non-crit:
///   `Damage skill=steadyzap dmg=20.000 crit=false` (the non-crit hit fires the condition) ->
///   `TriggerFired skill=static_discharge effect=` ->
///   `Damage skill=static_discharge dmg=25.000 crit=false`.
///
/// The dummy carries 200 life (> 20 + 25 = 45) so it SURVIVES both hits (kill=false on each). This is
/// the mirror of `skill_trigger_secondary_cast`: there a 100%-crit caster fires the `on_crit` condition
/// (crit=true, 30 + 37.5); here a NO-crit caster fires the `on_non_crit` condition (crit=false, 20 + 25).
pub fn on_non_crit_trigger() -> Scenario {
    Scenario::new("on_non_crit_trigger", 42, 60)
        .describe("A non-crit fire bolt carrying an on_non_crit condition casts its secondary skill: the non-crit hit satisfies the condition (the contrast to critzap's on_crit), surfacing static_discharge as its OWN TriggerFired + Damage line — Damage(steadyzap,20,crit=false) -> TriggerFired(static_discharge) -> Damage(static_discharge,25,crit=false).")
        .cast_asset("steadyzap")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("steadyzap")
        .actor("dummy", Faction::Enemy, 200.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "steadyzap".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// The full regression matrix.
///
/// Intentionally-excluded scenarios (covered elsewhere, not omissions):
/// - `aoe_fan`: `ObeliskCombat::resolve_aoe` is not driven through the cast/event pipeline
///   (it is a programmatic SystemParam call), so a golden event-trace is awkward. It is
///   covered by a direct `#[test]` in `src/facade/combat.rs` instead.
/// - `vfx_cues`: the `Cue` lines (firebolt_cast/firebolt_impact) are already exercised by
///   `firebolt_kill` (and `netcode_egress`), so a dedicated scenario would be redundant.
/// - `stat_sources`: covered by the existing `src/verbs.rs` unit test.
///
/// Batch B (fixture-dependent) scenarios — `cooldown_gate`, `trigger_cascade`, `loot_on_death` —
/// are now included; they each ship dedicated fixtures (`firebolt_cd`, `discharge_strike` +
/// `static_discharge` + `charged`, and the `goblin` drop table) so existing batch-A goldens are
/// untouched.
///
/// Effect-trigger scenarios — `on_apply_triggers_skill`, `on_expire_triggers_skill`,
/// `on_max_stacks_triggers_and_consumes` — cover the three `EffectTrigger` variants left uncovered by
/// `trigger_cascade` (which is the `OnConsume` template). They ship the `on_apply_proc` / `rage`
/// effect fixtures (and reuse the existing `on_expire_proc` + `static_discharge`); adding new effect
/// fixtures does NOT perturb the existing goldens (no prior scenario references them).
///
/// Stacking-mode scenarios (Batch 4) — `effect_refresh_stacking`, `effect_unlimited_stacking` —
/// cover the `StackingBehavior::{Refresh, Unlimited}` variants left uncovered by
/// `on_max_stacks_triggers_and_consumes` (the `Limited` template, via `rage`). They ship the
/// `refresh_dot` / `unlimited_dot` fixtures. The fourth variant, `StackingBehavior::StrongestOnly`,
/// is INTENTIONALLY NOT a scenario: through the public apply pipeline (`apply_obelisk_effect` ->
/// `EffectConfig::to_effect`) an effect is built with `dot: None`, so its `dps()` is always `0`. The
/// `StrongestOnly` branch's only discriminator is `new.dps() >= existing.dps()` (`0 >= 0` is always
/// true), so a "stronger" re-application is INDISTINGUISHABLE from a "weaker" one — the magnitude /
/// modifier-value difference that would make one "stronger" is not carried in the `EffectApplied`
/// trace line (which exposes only `effect`, `dur`, `stacks`). The mode therefore has no DISTINCT,
/// observable trace through the public event pipeline, so it is DROPPED rather than committed as a
/// golden that does not actually demonstrate "stronger replaces weaker". (Also note: neither stacking
/// golden carries `DotTicked` lines — `to_effect` produces no DoT data, so the observable signature is
/// the `EffectApplied` stacks/duration progression, exactly as for `apply_effect`.)
pub fn feature_matrix() -> Vec<Scenario> {
    vec![
        firebolt_kill(),
        cone_cleave(),
        faction_filter(),
        out_of_range(),
        line_of_sight(),
        already_casting(),
        apply_effect(),
        cooldown_gate(),
        trigger_cascade(),
        loot_on_death(),
        loot_multi_drop(),
        loot_quantity_scaling(),
        loot_rarity_scaling(),
        loot_nested_table(),
        netcode_egress(),
        on_apply_triggers_skill(),
        on_expire_triggers_skill(),
        on_max_stacks_triggers_and_consumes(),
        effect_refresh_stacking(),
        effect_unlimited_stacking(),
        self_buff_boosts_damage(),
        crit_strike(),
        resistance_mitigates(),
        armour_mitigates(),
        physical_damage_reduction_mitigates(),
        damage_reduction_mitigates(),
        oneshot_protection_caps(),
        cast_speed_scaling(),
        interrupt_cast(),
        cast_rejected_insufficient_mana(),
        cast_rejected_unknown_skill(),
        cast_rejected_no_target(),
        everytick_hitbox(),
        instant_cast(),
        probabilistic_effect_apply(),
        skill_trigger_secondary_cast(),
        on_fire_damage_dealt_trigger(),
        on_big_hit_trigger(),
        on_non_crit_trigger(),
    ]
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;
    use stat_core::StatBlock;

    /// Companion to the `probabilistic_effect_apply` golden: proves the seeded apply-chance roll is
    /// (a) DETERMINISTIC and (b) genuinely SPLIT by seed (so the golden's recorded outcome is a real
    /// roll, not a guaranteed application). `chancebolt` applies `chill` with chance
    /// `status_damage / target_max_health = 20 / 40 = 0.50`; we resolve one hit through the SAME
    /// deterministic funnel the scenario uses (`resolve_one_hit`, seeded `ChaCha8Rng`) and assert:
    ///
    /// - seed 11 (the golden's seed) lands chill (PASS) — matches the golden's `EffectApplied chill`;
    /// - seed 2 does NOT land chill (FAIL) — the same skill/HP, different seed, opposite outcome;
    /// - re-running a seed reproduces its outcome bit-for-bit.
    ///
    /// The PASS/FAIL split off identical inputs is what makes this a probabilistic (sub-100%) apply
    /// rather than `apply_chance = "always"`. (Note: at the scenario level no rng is drawn before the
    /// hit, so the scenario's seed-11 outcome equals this resolve-layer seed-11 outcome — hence the
    /// golden shows `chill` applied.)
    fn resolve_chill(seed: u64) -> bool {
        crate::testkit::init_test_obelisk();
        let registry =
            stat_core::config::load_skills_dir(std::path::Path::new("tests/fixtures/skills"))
                .unwrap();
        let skill = registry.get("chancebolt").unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0;
        caster.current_mana = 100.0;
        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 40.0;
        target.current_life = 40.0;

        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let outcome = crate::combat::resolve::resolve_one_hit(
            &mut caster,
            &mut target,
            skill,
            &registry,
            &mut rng,
        )
        .unwrap();
        // The 20 fire hit leaves the 40-life dummy alive (max_life still 40 ⇒ chance 0.5).
        assert!(
            (target.current_life - 20.0).abs() < 1e-9,
            "dummy should survive the 20 fire hit (got {})",
            target.current_life
        );
        outcome.effects_applied.iter().any(|e| e.id == "chill")
    }

    #[test]
    fn probabilistic_apply_is_deterministic_and_split_by_seed() {
        // The golden's seed lands chill (PASS) — matches `EffectApplied chill` in the golden.
        assert!(
            resolve_chill(11),
            "seed 11 must land chill (the probabilistic_effect_apply golden records EffectApplied)"
        );
        // A different seed, same skill + same 40-HP target, does NOT land chill (FAIL) — proving the
        // application is a real sub-100% roll, not guaranteed.
        assert!(
            !resolve_chill(2),
            "seed 2 must NOT land chill (the same 50% roll, opposite outcome — proves it's probabilistic)"
        );
        // Determinism: re-running a seed reproduces its outcome bit-for-bit.
        assert_eq!(
            resolve_chill(11),
            resolve_chill(11),
            "PASS seed must reproduce"
        );
        assert_eq!(
            resolve_chill(2),
            resolve_chill(2),
            "FAIL seed must reproduce"
        );
    }

    /// Extract the single `Loot` line's detail (the `source=… drops=[…]` text) from a scenario's
    /// recorded trace. Panics if there is no `Loot` line (a loot scenario that dropped nothing would
    /// be a non-feature golden — we want the test to fail loudly).
    fn loot_line(scenario: &super::Scenario) -> String {
        let trace = crate::scenario::run::run_scenario(scenario);
        trace
            .lines
            .iter()
            .find(|l| l.kind == "Loot")
            .map(|l| l.detail.clone())
            .unwrap_or_else(|| panic!("scenario {} produced no Loot line", scenario.name))
    }

    /// Companion to `loot_multi_drop`: the `multi_drop` table's `count = 2` roll option lists 2+
    /// drops, vs `loot_on_death`'s single-currency goblin table (1 drop). Pins that the golden
    /// genuinely demonstrates "multiple guaranteed drops", not a coincidental single drop.
    #[test]
    fn loot_multi_drop_lists_two_or_more_drops() {
        let detail = loot_line(&super::loot_multi_drop());
        let count = detail.matches("id:").count() + detail.matches("base_type:").count();
        assert!(
            count >= 2,
            "multi_drop must list 2+ drops (got {count}): {detail}"
        );
        // The single-currency goblin table lists exactly one drop — the contrast.
        let baseline = loot_line(&super::loot_on_death());
        let base_count = baseline.matches("id:").count() + baseline.matches("base_type:").count();
        assert_eq!(
            base_count, 1,
            "loot_on_death baseline lists one drop: {baseline}"
        );
    }

    /// Companion to `loot_quantity_scaling`: `.with_quantity_mult(3.0)` on the fixed `[10,10]`
    /// `coin_pile` table must raise the dropped currency `count` well above the unscaled (mult 1.0)
    /// baseline. Runs the SAME scenario with and without the multiplier through the real
    /// `run_scenario` path and compares the parsed counts — proving the multiplier is what moves
    /// the count, not the seed.
    #[test]
    fn loot_quantity_scaling_multiplier_raises_currency_count() {
        fn gold_count(detail: &str) -> u32 {
            // detail looks like: source=goblin drops=[Currency { id: "gold", count: 30 }]
            let after = detail
                .split("count: ")
                .nth(1)
                .expect("a count in the Loot line");
            after
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .expect("numeric count")
        }
        let scaled = gold_count(&loot_line(&super::loot_quantity_scaling()));
        // Same scenario, same seed, but NO quantity_mult (falls back to the loot default 1.0).
        let baseline_scenario = super::loot_scenario("loot_quantity_baseline", 7, "coin_pile")
            .with_drop_table_fixture("coin_pile", "tests/fixtures/loot/coin_pile.toml");
        let baseline = gold_count(&loot_line(&baseline_scenario));
        assert_eq!(
            baseline, 10,
            "unscaled coin_pile drops the base count 10: {baseline}"
        );
        assert!(
            scaled > baseline,
            "quantity_mult=3 must raise the count above the base 10 (scaled={scaled}, base={baseline})"
        );
    }

    /// Companion to `loot_rarity_scaling`: `.with_rarity_mult(50.0)` on the `rarity_tiers` table
    /// flips the weighted selection to the higher-rarity `diamond` tier, whereas the unscaled
    /// (mult 1.0) baseline picks the common `copper`. Proves the rarity multiplier — not the seed —
    /// drives the tier, by running the same seeded scenario with and without it.
    #[test]
    fn loot_rarity_scaling_multiplier_flips_to_rare_tier() {
        let scaled = loot_line(&super::loot_rarity_scaling());
        assert!(
            scaled.contains("diamond"),
            "rarity_mult=50 must drop the rare tier (diamond): {scaled}"
        );
        // Same scenario, same seed, but NO rarity_mult (defaults to 1.0) -> the common tier.
        let baseline_scenario = super::loot_scenario("loot_rarity_baseline", 7, "rarity_tiers")
            .with_drop_table_fixture("rarity_tiers", "tests/fixtures/loot/rarity_tiers.toml");
        let baseline = loot_line(&baseline_scenario);
        assert!(
            baseline.contains("copper") && !baseline.contains("diamond"),
            "unscaled rarity_tiers drops the common tier (copper): {baseline}"
        );
    }

    /// Companion to `loot_nested_table`: the `nested_chest` table's only entry is a `type = "table"`
    /// reference to `treasure_inner`; the resolved `Loot` line must carry the INNER table's `ruby`
    /// currency, proving nested resolution across two `with_drop_table_fixture` loads.
    #[test]
    fn loot_nested_table_resolves_inner_table_drops() {
        let detail = loot_line(&super::loot_nested_table());
        assert!(
            detail.contains("ruby"),
            "nested_chest must resolve treasure_inner's ruby currency: {detail}"
        );
    }

    /// Resolve one hit of `skill_id` (from the fixtures dir) against a fresh high-life dummy with a
    /// NON-crit caster, returning the number of skill-condition-triggered secondary hits surfaced.
    /// Shared by the Batch-12 condition-gating contrast tests below: a fired condition surfaces
    /// exactly one `triggered_skill_hits` entry, a non-fired condition surfaces zero.
    fn triggered_skill_hits_for(skill_id: &str) -> usize {
        crate::testkit::init_test_obelisk();
        let registry =
            stat_core::config::load_skills_dir(std::path::Path::new("tests/fixtures/skills"))
                .unwrap();
        let skill = registry.get(skill_id).unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0;
        caster.current_mana = 100.0;
        // No crit stat -> non-crit hits (so `on_non_crit` fires and `on_crit` would not).

        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 200.0;
        target.current_life = 200.0;

        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let outcome = crate::combat::resolve::resolve_one_hit(
            &mut caster,
            &mut target,
            skill,
            &registry,
            &mut rng,
        )
        .unwrap();
        outcome.triggered_skill_hits.len()
    }

    /// Companion to `on_fire_damage_dealt_trigger`: the `damage_type_dealt`/fire condition on `firezap`
    /// fires BECAUSE the bolt deals fire (one triggered static_discharge hit). The CONTRAST — a skill
    /// whose hit deals NO fire — must NOT fire it: `cleave` (100 physical) carries no condition at all,
    /// but more to the point a `damage_type_dealt=fire` condition is only met when `damage_of_type(Fire)
    /// > 0`. Pins that firezap surfaces exactly one secondary hit and a physical skill surfaces none.
    #[test]
    fn firezap_fires_only_on_fire_damage() {
        assert_eq!(
            triggered_skill_hits_for("firezap"),
            1,
            "firezap deals fire, so the damage_type_dealt=fire condition fires once"
        );
        // `cleave` is pure physical and carries no skill condition -> zero triggered secondary hits,
        // the natural contrast to a fire-dealing skill that DOES satisfy a fire condition.
        assert_eq!(
            triggered_skill_hits_for("cleave"),
            0,
            "a non-fire skill surfaces no fire-condition trigger"
        );
    }

    /// Companion to `on_big_hit_trigger`: the `damage_over_threshold`/30 condition on `bigzap` (50
    /// damage) fires BECAUSE 50 > 30 (one triggered static_discharge hit). The CONTRAST — a hit UNDER
    /// the threshold — must NOT fire: `firezap` (20 damage) carries a fire condition, not a threshold
    /// one, but the key proof is the threshold gate itself, which we pin directly against the obelisk
    /// post-calc evaluator: a 20-damage packet does NOT exceed the 30 threshold, a 50-damage one does.
    #[test]
    fn bigzap_fires_only_over_threshold() {
        use stat_core::damage::{
            DamagePacket, FinalDamage, TriggerCondition, TriggerConditionEval,
        };
        use stat_core::DamageType;

        assert_eq!(
            triggered_skill_hits_for("bigzap"),
            1,
            "bigzap deals 50 (> the 30 threshold), so the damage_over_threshold condition fires once"
        );

        // Directly pin the threshold gate on obelisk's post-calc evaluator: 50 > 30 fires, 20 does not.
        let cond = TriggerCondition::DamageOverThreshold { threshold: 30.0 };
        let mut big = DamagePacket::new("player".to_string(), "bigzap".to_string());
        big.damages.push(FinalDamage::new(DamageType::Fire, 50.0));
        assert!(
            cond.evaluate_post_calc(&big),
            "a 50-damage hit must exceed the 30 threshold"
        );
        let mut small = DamagePacket::new("player".to_string(), "firezap".to_string());
        small.damages.push(FinalDamage::new(DamageType::Fire, 20.0));
        assert!(
            !cond.evaluate_post_calc(&small),
            "a 20-damage hit must NOT exceed the 30 threshold (the under-threshold contrast)"
        );
    }

    /// Companion to `on_non_crit_trigger`: the `on_non_crit` condition on `steadyzap` fires for a
    /// NO-crit caster (one triggered static_discharge hit) — the deliberate contrast to `critzap`,
    /// whose `on_crit` condition needs a crit. With no crit stat, the seeded hit is a non-crit, so
    /// `steadyzap` fires while `critzap` (same no-crit caster) does NOT.
    #[test]
    fn steadyzap_fires_on_non_crit_and_critzap_does_not() {
        assert_eq!(
            triggered_skill_hits_for("steadyzap"),
            1,
            "a non-crit hit fires the on_non_crit condition once"
        );
        // The mirror: critzap's `on_crit` condition does NOT fire for the same no-crit caster.
        assert_eq!(
            triggered_skill_hits_for("critzap"),
            0,
            "critzap's on_crit condition does not fire without a crit (the contrast)"
        );
    }
}
