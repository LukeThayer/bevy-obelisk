# Test-Coverage Expansion Spec — obelisk-bevy

**Date:** 2026-06-20
**Status:** Proposed
**Scope:** Behavior-validation expansion for the `obelisk-bevy` crate, synthesized from 8 parallel
subsystem coverage-maps (effect-triggers, damage-triggers, effects/buffs/statuses, stat-system,
skill-damage-mechanics, loot-tables, netcode-determinism, cast-timeline-speed).

Current baseline: **11 golden scenarios** (`firebolt_kill`, `cone_cleave`, `faction_filter`,
`out_of_range`, `line_of_sight`, `already_casting`, `apply_effect`, `cooldown_gate`,
`trigger_cascade`, `loot_on_death`, `netcode_egress`) + **~50 unit/integration tests**.

The user asked for comprehensive behavior validation and explicitly named: *triggering skills from
different conditions*, *applying buffs to self*, *rebuilding stat blocks from stats*, "and much
more." Those three map directly to Batches 2–4 below and are the spine of this plan.

---

## 1. Summary

- **Behaviors mapped across the 8 subsystems:** ~190 (de-duplicated). Many overlap between maps
  (the four `EffectTrigger` variants appear in 3 maps; crit/resist/speed-scaling appear in 3–4).
- **Currently covered (golden or unit):** ~95.
- **Currently UNCOVERED (`coverage: none`) or only partially exercised end-to-end:** ~95, of which:
  - **~35 are reachable today** through the existing public API + scenario harness — they just lack
    a fixture and/or a golden. These are the cheap, high-value wins.
  - **~40 are NOT reachable** because the harness cannot express the required state (caster stats,
    self-applied buffs via skills, defender-side triggers, level/rarity loot params) or because the
    `Trace` cannot observe the result (no damage-breakdown fields). These are blocked on **Batch 1
    harness work**.
  - **~20 are deferred by design** (unimplemented mechanics or documented routing boundaries — see
    §6). These should be noted, not tested.

### Headline gaps (the honest top 5)

1. **`OnExpire` effect triggers are silently dropped.** `tick_effects_system`
   (`src/core/tick.rs`) reads `TickResult` but never emits `result.triggered_effects` as
   `TriggerFired`. This **violates the CLAUDE.md "triggers are never silently dropped" invariant**.
   This is a real bug surfaced by the audit, not just a missing test. Fix + cover first.

2. **Self-buffs through the skill pipeline are zero-coverage and unreachable.** No fixture skill
   applies a `ApplicationTarget::Caster` stat-modifier effect, and no test verifies the
   apply → `rebuild()` → computed-stat-changed round trip. This is the user's "applying buffs to
   self" + "rebuilding stat blocks from stats" request and is the single highest-value gap.

3. **`ActorSpec` cannot carry caster/defender stats.** Crit chance, resistances, cast/attack speed,
   on-hit/on-kill recovery, leech, culling, barrier-at-spawn, defender trigger conditionals — all
   well-tested in obelisk's `stat_core` but **unreachable end-to-end** because the scenario actor
   model has no stat-source hook. Blocks ~25 behaviors.

4. **The `Trace` records no damage breakdown.** `DamageResolved` is traced as
   `dmg/kill/life_after` only — no `is_critical`, no per-type damage, no `damage_prevented`, no
   `life_gained`. Defense layers (armor, resist, evasion cap, RDT/IDT) and crit/recovery therefore
   cannot be asserted in a golden even once the stats are attachable.

5. **Defender-side and on-kill trigger cascades are not routed.** `OnHitTaken`, `OnDamageTaken*`,
   `OnDodge`, `OnBarrierDepleted`, `OnLowLifeReached`, `OnKill`, `OnOverkill` are evaluated in
   `stat_core` but `obelisk-bevy` returns `defender_packets`/`on_kill_packets` unresolved (a
   documented boundary). Most are **out of scope** for this sprint (§6); a few become testable only
   if a default routing policy is added (call that out, don't sneak it in).

---

## 2. Coverage matrix (per subsystem)

Legend — coverage: **G** golden, **U** unit, **P** partial (exercised indirectly, not isolated),
**NONE** = uncovered. Proposed test names use `golden:<name>` / `unit:<module>`. Priority H/M/L.
**Rows marked `[UNCOVERED]` are the work.** Rows marked `[reachable]` need only a fixture+test;
rows marked `[BLOCKED: …]` need a Batch-1 harness extension first.

### 2.1 effect-triggers

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| OnApply fires on add_effect | P (surfaced, no test) | `golden:on_apply_triggers_skill` `[reachable]` | H |
| **OnExpire fires during tick_effects** | **NONE — silently dropped** | `unit:core::tick::on_expire_surfaces_trigger_fired` + `golden:on_expire_triggers_skill` `[BLOCKED: tick.rs fix]` | H |
| **OnMaxStacks fires (Limited stacking)** | **NONE** | `golden:on_max_stacks_triggers_and_consumes` `[reachable]` | H |
| OnConsume fires (consumes_self_effect) | G (`trigger_cascade`) | — already covered | L |
| OnApply surfaced from `apply_obelisk_effect` verb | P | `golden:apply_obelisk_effect_surfaces_triggers` `[reachable]` | M |
| `TriggeredEffect::to_damage_packet` scaling | U (via golden) | `unit:combat::resolve::triggered_effect_damage_scaling` | M |
| All four `EffectTrigger` variants covered | P | (rollup of the above) | H |

### 2.2 damage-triggers (skill-authored `TriggerCondition`s)

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| **OnCrit (skill condition)** | U (stat_core only) | `golden:on_crit_trigger` `[BLOCKED: crit on ActorSpec]` | H |
| OnNonCrit | U | `golden:on_non_crit_trigger` `[BLOCKED: crit on ActorSpec]` | M |
| **DamageTypeDealt (e.g. Fire)** | U | `golden:on_fire_damage_dealt_trigger` `[reachable]` | M |
| **DamageOverThreshold** | U | `golden:on_big_hit_trigger` `[reachable]` | M |
| MultipleDamageTypes | U | `golden:on_multi_damage_type_trigger` `[reachable]` | L |
| OnBarrierBroken (attacker-side) | U | `golden:on_barrier_broken_trigger` `[BLOCKED: barrier on target spawn]` | M |
| Skill-authored PostCalc conditions (path) | P | covered by `on_crit_trigger` / `on_fire_damage_dealt_trigger` | H |
| Global conditionals (attacker StatBlock) | P | `golden:global_conditional_trigger` `[BLOCKED: global_conditionals on ActorSpec]` | M |
| Pre-calc state conditions (PlayerLowLife, TargetHasEffect, …) | U | `golden:on_crit_while_low_life` `[BLOCKED: stat/state on ActorSpec]` | M |
| EveryNthHit (stateful pre-calc) | U | `unit:` hit-counter logic `[BLOCKED: no hit-counter state]` | L |
| OnKill / OnOverkill (on_kill_packets) | P/U | **out of scope** (no routing) — §6 | L |
| OnHitTaken / OnDamageTaken* / OnDodge / OnEvasionCap / OnBarrierDepleted / OnLowLifeReached (defender) | NONE | **out of scope** (defender_packets unresolved) — §6 | M/L |
| OnEffectConsumed / OnEffectChargeUsed (defender) | NONE | **out of scope** — §6 | L |

### 2.3 effects-buffs-statuses

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| **Self-buff via skill `effect_applications` (Caster target)** | **NONE** | `golden:self_buff_applies_and_modifies_stats` `[BLOCKED: caster-target skill fixture + speed observability]` | H |
| **Stat-modifier aggregation in `rebuild()` (end-to-end)** | U (core) | `unit:verbs::apply_obelisk_effect_modifies_computed_stat` `[reachable]` | H |
| Refresh stacking (refresh + add stack) | U | `golden:effect_stacking_refresh` or `unit:` `[reachable]` | H |
| Limited stacking → OnMaxStacks | NONE | `golden:effect_limited_stacking_on_max_stacks` `[reachable]` | M |
| Unlimited stacking (independent decay) | U | `golden:effect_unlimited_independent_decay` `[reachable]` | M |
| StrongestOnly stacking (refresh, replace if stronger) | U | `golden:effect_strongest_only_refresh` `[reachable]` | M |
| DoT ticking (basic) | G (`firebolt_kill`) | — covered | L |
| Finite vs Infinite duration | G | — covered (`apply_effect` / `trigger_cascade`) | L |
| Charge-based (AllSkills / AllIncomingHits / SpecificOnly) | P (SpecificOnly only) | `golden:effect_charge_allskills_consumption` `[reachable]` | M |
| Magnitude scaling (`value*stacks*magnitude`) | U | `unit:` or `golden:effect_magnitude_scales_modifiers` `[reachable]` | L |
| Self-buff removal reverts stat | U (core) | `golden:self_buff_removal_reverts_boost` `[BLOCKED: same as self-buff]` | M |
| Global/conditional modifiers on active effect | NONE | `golden:effect_conditional_modifier_bonus` `[reachable, needs fixtures]` | L |
| Mixed buff-to-self + debuff-to-target in one skill | NONE | `golden:skill_buffs_self_debuffs_target` `[BLOCKED: caster+target effect_applications fixture]` | M |

### 2.4 stat-system

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| Flat / increased / more stacking | U | `unit:` multi-source accumulator test | H |
| Life/mana flat+increased | U | `unit:` (extend existing, add mana) | H |
| **Attribute scaling rules** | NONE | `golden:` / `unit:` `[BLOCKED: scaling-rule attach]` | H |
| **Armor mitigation** | U (core) | `unit:` resolve_damage OR `golden:armor_vs_burst` `[BLOCKED: stat attach + Trace breakdown]` | H |
| **Evasion / oneshot cap** | U | `[BLOCKED: stat attach + Trace breakdown]` | H |
| **Elemental resistances** | U | `golden:fire_resistance_mitigates` `[BLOCKED: resist on ActorSpec + Trace]` | H |
| Elude stacks reduction | U | `[BLOCKED: Trace breakdown]` (grant_elude verb exists) | M |
| **Life/mana on-hit recovery** | NONE | `golden:life_on_hit_recovers_attacker` `[BLOCKED: stat attach + Trace recovery field]` | H |
| **Life/mana on-kill recovery** | NONE | `[BLOCKED: stat attach + Trace recovery field]` | H |
| **Crit chance + multiplier** | U | `golden:crit_boost_scales_damage` `[BLOCKED: crit on ActorSpec + Trace is_critical]` | H |
| **Cooldown reduction** | U | `golden:cooldown_reduction_speeds_cast` `[BLOCKED: stat attach]` (observable via CooldownStarted dur) | H |
| **Cast/attack speed scaling** | U | `golden:cast_speed_doubles_windup` `[BLOCKED: stat attach]` (observable via CastBegan dur / phase ticks) | H |
| Mana cost scaling (reduced/increased) | NONE | `[BLOCKED: stat attach; verify scaling is wired at validate]` | M |
| Penetration / accuracy / conversions / global dmg / RDT-IDT / phys-reduction / culling / leech / status-stat scaling | U/NONE | `[BLOCKED: stat attach + Trace breakdown]` | M |
| Barrier absorption in combat | U (grant verb) | `golden:barrier_absorbs_damage` `[BLOCKED: Trace breakdown to see barrier vs life]` | M |
| Spell dodge / regen / AoE-scale / proj-speed / multi-proj / movement / block / item rarity-qty | NONE | **deferred — unimplemented** (§6) | L |

### 2.5 skill-damage-mechanics

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| Deterministic seeded damage | U + G | — covered | H |
| Fire damage + burn DoT scaling | G (`firebolt_kill`) | — covered | H |
| Cone targeting (angle + range) | G (`cone_cleave`) | — covered | H |
| Projectile / Melee delivery | G | — covered | H |
| HitFilter faction filter | G (`faction_filter`) | — covered | H |
| Range validation | G (`out_of_range`) | — covered | H |
| OncePerTarget / FirstOnly hit modes | U | — covered (`spatial/boxes.rs`) | H |
| **EveryTick re-hit mode** | U | `golden:everytick_multicast` `[reachable, needs EveryTick .cast.ron]` | M |
| **rehit_interval re-hit** | U | `golden:rehit_interval_allows_retrigger` `[reachable, needs .cast.ron]` | M |
| **Probabilistic status apply (`percentage`)** | NONE | `golden:probabilistic_status` `[reachable, needs fixture]` | M |
| **Crit through cast pipeline** | NONE | `golden:crit_strike_bonus_damage` `[BLOCKED: crit attach + Trace]` | H |
| **Resistance through cast pipeline** | NONE | `golden:resist_reduces_damage` `[BLOCKED: resist attach + Trace]` | H |
| Damage conversion (phys→fire) | P | `golden:damage_conversion_physical_to_fire` `[BLOCKED: conversion attach + Trace]` | M |
| Instant delivery | U | `golden:instant_aoe_cast` `[reachable, needs fixture]` | L |
| Capsule collision shape | NONE | `golden:capsule_melee_sweep` `[reachable, needs .cast.ron]` | L |
| Cone range boundary / projectile expiry-at-range | P | `golden:cone_range_boundary` / `golden:projectile_expires_at_range` `[reachable]` | L |
| Killing blow + EntityDied | G | — covered | H |
| `resolve_aoe` determinism + ordering | U | — covered (`facade/combat.rs`) | H |
| Mana deduction on cast | U | — covered (`combat/resolve.rs`) | H |

### 2.6 loot-tables

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| Core roll types (no_drop / currency / item / unique) | U | — covered (`tables_core`) | H |
| Weighted selection / roll count / count range | U | — covered | H |
| LootDropped on death | G (`loot_on_death`) | — covered | H |
| Deterministic seeded roll | G | `unit:loot::roll_determinism_same_seed` (add explicit) | M |
| **Rarity bonus (variable rarity_mult)** | U | `golden:rarity_bonus_scenario` `[BLOCKED: rarity_mult is hardcoded 1.0 in roll_drops_on_death]` | M |
| **Level filtering** | U | `golden:level_gated_loot` `[BLOCKED: ActorSpec has no level field]` | M |
| **Quantity multiplier (variable)** | U | `golden:quantity_mult_scenario` `[BLOCKED: hardcoded 1.0]` | M |
| **Nested table references** | U | `golden:nested_drop_tables` `[BLOCKED: fixture loading hardcoded to goblin.toml]` | M |
| Cycle detection / Unknown table / Invalid entry-type | U / NONE | `unit:tables_core` (add InvalidEntryType + config-parse edge) | L |
| ItemGenerator materialization | NONE | **deferred — not wired** (§6) | L |

### 2.7 netcode-determinism

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| NetEvent mirror: CastBegan/Damage/EffectApplied/DotTicked/EntityDied | G (`netcode_egress`) | — covered | H |
| **EffectExpired mirror** | P (no scenario expires w/o kill) | `golden:effect_expiry_no_kill` `[reachable]` | M |
| CastRejectReason: OutOfRange / NoLineOfSight / OnCooldown / AlreadyCasting | G | — covered | H |
| **CastRejectReason: InsufficientMana** | U | `golden:cast_rejected_insufficient_mana` `[reachable]` | H |
| **CastRejectReason: UnknownSkill** | U | `golden:cast_rejected_unknown_skill` `[reachable]` | M |
| **CastRejectReason: NoTarget** | U | `golden:cast_rejected_no_target` (despawn-before-cast) `[reachable]` | M |
| CastRejectReason: TimelineMissing | U | `[BLOCKED: harness preloads all .cast.ron]` — note only | L |
| CastRejectReason: ConditionNotMet | NONE | **deferred — no skill conditions in fixtures/ActorSpec** (§6) | L |
| Entity↔id bimap stability | U | — covered (`ids.rs`) | H |
| CombatRng seeding / same-seed idempotence | U | — covered (`vertical_slice.rs`) | H |
| **Cross-seed divergence** | NONE | `unit:determinism::cross_seed_diverges` `[reachable]` | M |
| Crit / status-apply-chance / loot / trigger-cascade roll determinism | P/NONE | `unit:` same-seed idempotence tests `[reachable once fixtures exist]` | M |
| Dodge/resistance roll determinism | NONE | `[BLOCKED: stat attach]` | L |
| NetEvent serde round-trip / buffering | U | — covered (`net.rs`, `netcode.rs`) | H |
| Net mirror skip+warn on missing ObeliskId | U (implicit) | `unit:net::mirror_skips_missing_id` `[reachable]` | L |

### 2.8 cast-timeline-speed

| Behavior | Cov | Proposed test | Pri |
|---|---|---|---|
| Phase transitions (Windup→Active→Recovery→Done) | G | — covered | L |
| HitWindow spawn timing (spawn_phase + offset) | G | `unit:timeline::advance` direct test (optional) | M |
| Cooldown duration / effective_cooldown | G + U | — covered (extend for CDR — see stat-system) | M |
| Already-casting gate | G | — covered | L |
| **Interrupt cast (`interrupt_cast` verb)** | NONE | `golden:interrupt_cast` `[BLOCKED: needs Action::Interrupt]` | H |
| **Cast speed scaling end-to-end (spell)** | U | `golden:cast_speed_scaling_spell` `[BLOCKED: stat attach]` | H |
| **Attack speed scaling end-to-end** | U | `golden:cast_speed_scaling_attack` `[BLOCKED: stat attach]` | H |
| `attack_speed_modifier` (skill-side) combinations | U | `unit:timeline::state::modifier_combinations` `[reachable]` | M |
| Hitbox lifetime / despawn | G (implicit) | `unit:timeline::advance::expire_hitboxes_despawns` `[reachable]` | M |
| Speed scaling via stat_sources buff | U (life only) | `golden:speed_source_buff` `[BLOCKED: needs Action::ApplyStatSources]` | M |
| Instant (zero-duration) cast | NONE | `golden:instant_cast` `[reachable, needs fixture]` | L |
| effective_rate negative-clamp guard | U (untested branch) | `unit:timeline::state::negative_rate_clamped` `[reachable]` | L |

---

## 3. Harness extensions required (build these FIRST)

These are the concrete additions that unblock the `[BLOCKED]` rows above. Ordered by how many
behaviors they unblock. **Nothing in Batches 2–4 should be started before the relevant extension
lands**, otherwise the tests get authored as unit-only stopgaps and the end-to-end gap persists.

### H1 — Stat-source / stat-override hook on `ActorSpec` (unblocks ~25 behaviors)

`ActorSpec` currently carries only `id, faction, life, mana, pos, skills, drop_table,
hurtbox_radius` and `stat_block()` sets only life/mana base. There is no way to give a caster crit
chance, resistances, cast/attack speed, on-hit/on-kill recovery, leech, conversions, etc.

Add an optional stat-source attachment that `spawn_actor` applies via the **existing**
`apply_stat_sources` verb (no new resolution path needed):

```rust
pub struct ActorSpec {
    // …existing…
    pub stat_sources: Vec<Box<dyn stat_core::source::StatSource>>, // applied at spawn
    pub barrier: Option<f64>,    // via grant_barrier
    pub elude: Option<u32>,      // via grant_elude (verb exists, never exercised in golden)
}
```

Because `Box<dyn StatSource>` is not `Clone`/`Debug`, either (a) drop `#[derive(Clone, Debug)]`
from `ActorSpec`/`Scenario` and adjust the playground/screenshot consumers, or (b) store a
**builder closure** `Arc<dyn Fn() -> Vec<Box<dyn StatSource>>>` so `Scenario` stays `Clone`. Option
(b) is preferred — it keeps `feature_matrix()` cloneable. Provide ergonomic builder methods:
`.with_crit(0.5)`, `.with_resistance(DamageType::Fire, 50.0)`, `.with_cast_speed(2.0)`,
`.with_life_on_hit(5.0)`, `.with_barrier(50.0)`, backed by small `StatSource` structs (mirror the
existing `LifeSource` pattern in `verbs.rs`).

### H2 — `DamageResolved` damage-breakdown fields + Trace (unblocks ~15 behaviors)

The `Trace` records `dmg/kill/life_after` only. Defense layers, crit, and recovery produce no
observable difference a golden can assert. Extend the `DamageResolved` event (it already exists; the
underlying `CombatResult` from obelisk carries the data) with the fields the resolution already
computes, and surface them in `trace.rs`:

- `is_critical: bool`
- `damage_prevented: f64` (armor/resist/evasion-cap/RDT cumulative; or a small struct)
- `life_gained: f64` / `mana_gained: f64` (on-hit + on-kill)
- optionally a per-type post-mitigation map (Fire/Cold/…)

Keep these **additive** to the trace line so the existing 11 goldens regenerate with a reviewed diff
(`UPDATE_GOLDEN=1`, then `git diff tests/golden/`). Do this in one commit so the golden churn is a
single, reviewable event.

### H3 — New `Action` variants for the scenario script (unblocks 3 behaviors)

- `Action::Interrupt { id }` → calls the existing `interrupt_cast` verb. Unblocks
  `golden:interrupt_cast`.
- `Action::ApplyStatSources { id, build: <closure/key> }` → calls `apply_stat_sources` mid-scenario.
  Unblocks `golden:speed_source_buff` (and lets stat changes be applied *after* spawn, not just at
  it). If H1 uses the closure approach, reuse the same mechanism.

### H4 — Loot roll parameters reachable from a scenario (unblocks 4 behaviors)

`roll_drops_on_death` (`src/loot.rs`) hardcodes `rarity_mult=1.0, quantity_mult=1.0, level=1`, and
`run.rs` hardcodes the `goblin.toml` fixture. To reach rarity/level/quantity/nested-table paths:

- Add `ActorSpec.level: Option<u32>` and `rarity_mult` / `quantity_mult` (on `ActorSpec` or
  `DropTableId`); thread them into the `roll()` call in the observer.
- Generalize fixture loading in `run.rs` from hardcoded `goblin.toml` to a fixture map
  (e.g. `Scenario.drop_table_fixtures: Vec<(name, toml)>`), so nested-table chains can load.

### H5 — `core/tick.rs` OnExpire surfacing (a real fix, not just a test)

`tick_effects_system` must iterate `result.triggered_effects` and emit `TriggerFired` for each
(mirroring `apply_obelisk_effect` in `verbs.rs` lines ~105–116). **Surfacing is non-negotiable per
the CLAUDE.md invariant; auto-firing the expired-effect's skill is a design decision — default to
surface-only (consistent with `apply_obelisk_effect`) and flag it for the maintainer.** This is the
prerequisite for any `OnExpire` golden.

> **Maintainer checkpoints (per the CLAUDE.md working agreement, do NOT guess):**
> - H2: which damage-breakdown fields to expose, and the trace line format (golden churn).
> - H5: surface-only vs auto-fire for `OnExpire` (cascade routing is an escalation-worthy fork).
> - Defender/on-kill routing (§6): stays out of scope unless the maintainer wants a default policy.

---

## 4. New fixtures needed

**Effects** (`tests/fixtures/effects/*.toml` — schema: `id, name, duration, is_debuff, stacking,
max_stacks, [[modifiers]] stat=… value=…, [[conditions]] type=… trigger_skill=… consume=…`):

- `haste.toml` — stat-buff: `[[modifiers]] stat="increased_cast_speed" value=…` (the canonical
  self-buff effect for stat-rebuild tests). Infinite or finite.
- `on_apply_proc.toml` — `[[conditions]] type="on_apply" trigger_skill="static_discharge"`.
- `on_expire_proc.toml` — finite short `duration`, `[[conditions]] type="on_expire" trigger_skill=…`.
- `rage.toml` — `stacking="limited"`, `max_stacks=3`, `[[conditions]] type="on_max_stacks"
  trigger_skill=… consume=true`.
- `bleed_unlimited.toml` — `stacking="unlimited"`, finite `duration` (~2.0s), small `tick_rate`
  (independent per-stack decay).
- `ward.toml` (optional) — `[[conditional_modifiers]]` / `[[global_conditionals]]` for the
  effect-conditional-bonus test.

**Skills** (`tests/fixtures/skills/*.toml` + matching `assets/skills/*.cast.ron`):

- `haste_stance.toml` + `.cast.ron` — `[[effect_applications]] effect_id="haste" target="caster"`,
  instant/short timeline. (Self-buff pipeline.)
- `war_cry.toml` — mixed: one `effect_applications` `target="caster"` (buff) + one `target="target"`
  (debuff). (Buff-self/debuff-target.)
- `crit_firebolt.toml` — firebolt variant with a skill-authored `OnCrit` trigger condition (pairs
  with caster crit via H1).
- `fire_proc.toml` — `DamageTypeDealt(Fire)` and/or `DamageOverThreshold` condition → trigger skill.
- `unreliable_burn.toml` — firebolt variant, `apply_chance="percentage"` (e.g. 50%).
- `beam.cast.ron` — Cone, `HitMode::EveryTick`, longer active phase (EveryTick + rehit_interval).
- `piercing.cast.ron` — `FirstOnly` + `rehit_interval` set.
- `instant_blast.toml` + `.cast.ron` — `CastDelivery::Instant`.
- `instant_buff.toml` + `.cast.ron` — phase_durations `(0,0,0)` (instant-cast edge case).
- `sweep.cast.ron` — `CollisionShape::Capsule`.
- `fast_attack.toml` + `.cast.ron` — `attack_speed_modifier > 1.0`.

**Loot** (`tests/fixtures/loot/*.toml`):

- `rarity_table.toml` — mixed entries with `rarity_bonus`.
- `leveled_drops.toml` — entries with `min_level`/`max_level`.
- `nested_outer.toml` + `nested_inner.toml` — nested-table chain.

---

## 5. Proposed implementation batches

Dependency-ordered. Each batch is a coherent commit-sized unit. "New tests" counts are estimates.
Run `cargo test --features test-support --lib --tests` + the golden suite after every batch; review
any golden diff before `UPDATE_GOLDEN=1`.

### Batch 0 — `OnExpire` surfacing fix (the bug)  ·  ~2 tests
Fix `core/tick.rs` to emit `TriggerFired` from `result.triggered_effects` (H5). Add
`unit:core::tick::on_expire_surfaces_trigger_fired`. This is independent of the harness work and
closes a CLAUDE.md invariant violation, so it goes first. **Maintainer checkpoint on
surface-vs-auto-fire.**

### Batch 1 — Harness extensions  ·  ~4 small unit/smoke tests
H1 (stat-source hook + builder methods + `barrier`/`elude` on `ActorSpec`), H2 (DamageResolved
breakdown + trace fields; regenerate the 11 goldens with a reviewed diff), H3 (`Action::Interrupt`,
`Action::ApplyStatSources`), H4 (loot params + fixture-map loading). Add a couple of smoke tests
that the new builder methods actually change the computed stat / trace line. **This batch unblocks
everything downstream; do not interleave feature tests into it.**

### Batch 2 — Effect-trigger scenarios (OnApply / OnExpire / OnMaxStacks)  ·  ~6 tests
Author `on_apply_proc`, `on_expire_proc`, `rage` fixtures. Goldens: `on_apply_triggers_skill`,
`on_expire_triggers_skill`, `on_max_stacks_triggers_and_consumes`,
`apply_obelisk_effect_surfaces_triggers`. Unit: `triggered_effect_damage_scaling`. Directly answers
the user's "trigger skills from different conditions." (Depends on Batch 0 for OnExpire.)

### Batch 3 — Self-buff + stat-rebuild  ·  ~6 tests
Author `haste` effect, `haste_stance` + `war_cry` skills. Tests:
`unit:verbs::apply_obelisk_effect_modifies_computed_stat`, `golden:self_buff_applies_and_modifies_stats`,
`golden:self_buff_removal_reverts_boost`, `golden:skill_buffs_self_debuffs_target`. Plus the
multi-source accumulator unit tests (flat/increased/more, mana variants). Directly answers "applying
buffs to self" + "rebuilding stat blocks from stats." (Depends on Batch 1 H1; self-buff-via-skill
observability uses CastBegan-duration for speed buffs, or the H2 trace fields for damage buffs.)

### Batch 4 — Effect stacking + DoT variants  ·  ~5 tests
`bleed_unlimited` fixture. Goldens/units: `effect_stacking_refresh`,
`effect_limited_stacking_on_max_stacks` (overlaps Batch 2's `rage`), `effect_unlimited_independent_decay`,
`effect_strongest_only_refresh`, `effect_charge_allskills_consumption`. (Depends on Batch 1.)

### Batch 5 — Crit, resistance, speed scaling (the stat-attach payoff)  ·  ~7 tests
Using H1 + H2: `golden:crit_boost_scales_damage`, `golden:fire_resistance_mitigates`,
`golden:cast_speed_doubles_windup`, `golden:cast_speed_scaling_attack`,
`golden:cooldown_reduction_speeds_cast`, `golden:on_crit_trigger`, `golden:armor_vs_burst`. Cast/
attack-speed and cooldown are observable via CastBegan/CooldownStarted durations even without H2;
crit/resist/armor need H2. (Depends on Batches 1.)

### Batch 6 — Damage-trigger conditions (attacker-side, reachable)  ·  ~4 tests
`fire_proc` fixture. Goldens: `on_fire_damage_dealt_trigger`, `on_big_hit_trigger`,
`on_barrier_broken_trigger` (needs target barrier via H1), `global_conditional_trigger` (needs
global-conditional attach — a small H1 extension). (Depends on Batch 1.)

### Batch 7 — Hit modes, delivery, probabilistic apply  ·  ~6 tests
`beam`/`piercing`/`instant_blast`/`sweep`/`unreliable_burn`/`instant_buff` fixtures. Goldens:
`everytick_multicast`, `rehit_interval_allows_retrigger`, `instant_aoe_cast`, `capsule_melee_sweep`,
`probabilistic_status`, `instant_cast`. Mostly reachable today; independent of the stat work.

### Batch 8 — Cast-reject scenarios + interrupt + netcode edges  ·  ~6 tests
Goldens: `cast_rejected_insufficient_mana`, `cast_rejected_unknown_skill`, `cast_rejected_no_target`,
`effect_expiry_no_kill`, `interrupt_cast` (needs Batch 1 H3). Units:
`net::mirror_skips_missing_id`, `timeline::advance::expire_hitboxes_despawns`,
`timeline::state::negative_rate_clamped`, `timeline::state::modifier_combinations`.

### Batch 9 — Determinism hardening  ·  ~4 tests (UNIT)
`unit:determinism::cross_seed_diverges`, `unit:loot::roll_determinism_same_seed`,
`unit:combat::trigger_cascade_determinism`, `unit:combat::status_apply_chance_determinism`.

### Batch 10 — Loot table feature matrix  ·  ~4 tests (mixed)
Using H4: `golden:rarity_bonus_scenario`, `golden:level_gated_loot`, `golden:quantity_mult_scenario`,
`golden:nested_drop_tables`. Plus `unit:tables_core` InvalidEntryType + config-parse edge cases.

**Behaviors that must stay UNIT tests (not goldens), with reason:**
- *Pure stat math* (flat/increased/more stacking, accumulator, `to_damage_packet` scaling,
  `effective_rate` clamp, `attack_speed_modifier` combos): no cast-pipeline event stream to trace;
  a golden would add noise without added signal.
- *Determinism idempotence/divergence* (Batch 9): requires running a scenario *twice* and comparing
  aggregates — a golden trace captures one run, so the comparison must live in a unit test.
- *Net mirror skip-on-missing-id*: requires spawning an entity deliberately without `ObeliskId`,
  which the scenario harness cannot express (every spawned actor gets one by construction).
- *Hitbox despawn*: no recorded despawn event exists; assert directly on entity absence in a unit
  test (or add a despawn event first — out of scope here).
- *`tables_core` roll/error paths*: already unit-tested in the upstream crate; only add the missing
  `InvalidEntryType` / config-parse-edge unit tests, no golden.

Rough total: **~52 new tests** (~30 goldens, ~22 unit), plus the 11 existing goldens regenerated
once (Batch 1 H2).

---

## 6. Out of scope / boundaries (note, don't test)

These are deliberate `obelisk-bevy` boundaries or unimplemented mechanics. Document them in the
spec/commit; do **not** author tests that would require building the mechanic, unless the maintainer
explicitly greenlights the routing policy.

1. **On-kill / splash / overkill cascades** — `resolve_damage_with_triggers` returns
   `on_kill_packets` unresolved; they need game-level target selection (splash-to-all, at-corpse).
   Documented limitation in CLAUDE.md. `OnKill` / `OnOverkill` triggers therefore can't auto-fire.
2. **Defender-side trigger cascades** — `defender_packets` (OnHitTaken, OnDamageTaken*, OnDodge,
   OnEvasionCap, OnBarrierDepleted, OnLowLifeReached, OnEffectConsumed, OnEffectChargeUsed) are
   evaluated in `stat_core` but not auto-fired; counter-attack routing is a game policy decision.
3. **`apply_obelisk_effect` auto-firing** — its triggers are *surfaced* via `TriggerFired` but
   intentionally **not** auto-fired from the command closure (deferred to the game/engine). Test the
   surfacing (Batch 2), not auto-firing.
4. **`EveryNthHit`** — needs a hit-counter on `StatBlock` not present in `obelisk-bevy`; external
   gating logic is unimplemented.
5. **`CastRejectReason::TimelineMissing` / `ConditionNotMet`** — TimelineMissing can't be simulated
   (harness preloads all `.cast.ron`); ConditionNotMet has no skill-condition fixtures or ActorSpec
   caster-state. Both deferred.
6. **Unimplemented stat mechanics** — spell dodge, passive life/mana regen, AoE-radius scaling,
   projectile-speed scaling, additional projectiles, skill-duration scaling, movement speed, block
   amount, item rarity/quantity → ItemGenerator materialization. Fields exist; no wiring. These are
   *design* gaps, not test gaps.
7. **`DotTicked.effect_id` empty** — `TickResult` has no per-effect breakdown; the event is
   correctly replicated, the empty id is an upstream-obelisk limitation, not a netcode bug.
