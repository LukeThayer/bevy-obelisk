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
/// The dummy carries 50 life (> 10) so it SURVIVES the mitigated direct hit (kill=false) and the
/// Damage line records cleanly with the prevented amount, before the burn DoT (seeded from the full
/// pre-mitigation fire damage) finishes it. Uses `.with_stat` only — no new fixture needed.
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
        netcode_egress(),
        on_apply_triggers_skill(),
        on_expire_triggers_skill(),
        on_max_stacks_triggers_and_consumes(),
        self_buff_boosts_damage(),
        crit_strike(),
        resistance_mitigates(),
        cast_speed_scaling(),
        interrupt_cast(),
        cast_rejected_insufficient_mana(),
        cast_rejected_unknown_skill(),
        cast_rejected_no_target(),
    ]
}
