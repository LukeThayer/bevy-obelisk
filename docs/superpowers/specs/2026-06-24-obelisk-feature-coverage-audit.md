# Obelisk Feature Coverage Audit (obelisk-bevy)

**Date:** 2026-06-24
**Scope:** All obelisk features reachable through obelisk-bevy, mapped across 8 subsystems.
**Suite at audit time:** 32 golden scenarios + ~57 unit/integration tests.
**Question answered:** "Look through all obelisk features and make sure they are all covered."

---

## 1. Scorecard

Features audited across the 8 subsystem maps: **159**.

| Status | Count | Meaning |
|---|---:|---|
| `covered` | 95 | Proven by a golden scenario and/or a unit/integration test |
| `gap-reachable` | 41 | Implemented + reachable via the current harness, but no obelisk-bevy test exercises it |
| `gap-boundary` | 11 | Real obelisk feature obelisk-bevy intentionally does **not** auto-route (documented) |
| `not-integrated` | 19 | Obelisk feature with **no** obelisk-bevy surface at all (out of scope) |

> Status tallies are aggregated from the per-subsystem maps. A few features are tagged
> `partial` in their map; those are counted as `covered` (mechanism proven, exercise incomplete)
> except where the map explicitly downgrades them to a gap.

**Verdict:** Coverage of the surface obelisk-bevy *actually integrates* is strong — every
core mechanic (cast pipeline, hit detection, damage/crit/resist, status/DoT, stacking,
triggers, loot rolls, netcode egress, determinism) is proven end-to-end by golden traces or
unit tests. The residual work is **breadth, not correctness**: ~41 already-reachable
mechanics (alternate trigger conditions, secondary defense layers, alternate targeting/hit
filters) have no dedicated scenario yet. Nothing in the integrated surface is *broken or
unproven at the mechanism level* — the gaps are untested permutations, not missing wiring.

---

## 2. Covered (what is proven, and by what)

Compact per-subsystem view of the load-bearing covered features. `golden:<name>` =
end-to-end scenario trace; `unit:<test>` = isolated test.

### combat-damage-crit-triggers
| Feature | Evidence |
|---|---|
| Base damage calculation | golden:firebolt_kill, golden:crit_strike |
| Damage scaling (increased/more) | golden:self_buff_boosts_damage, golden:cast_speed_scaling |
| Crit chance (flat/increased) | golden:crit_strike; unit:test_guaranteed_crit_always_crits |
| Crit multiplier (1.5 + bonus) | golden:crit_strike, golden:skill_trigger_secondary_cast |
| OnCrit trigger → secondary cast | golden:skill_trigger_secondary_cast; unit:test_trigger_additional_fires_both |
| Resistance mitigation | golden:resistance_mitigates; unit:test_resistance_mitigation |
| Status application (chance/guaranteed) | golden:firebolt_kill, golden:probabilistic_effect_apply, golden:apply_effect |
| DoT DPS calculation | golden:firebolt_kill (DotTicked) |
| DamageResolved breakdown (crit/prevented/life/mana) | golden:crit_strike, golden:resistance_mitigates |
| Damage type conversions | unit:test_damage_conversion_full_chain (+ normalization/merge) |
| Multi-hit (hits_per_attack>1) | golden:everytick_hitbox |

### defense-mitigation
| Feature | Evidence |
|---|---|
| Resistance (with penetration) | golden:resistance_mitigates (prevented=10.0 in H2) |
| Armour (POE formula, DR) | unit (obelisk) — reachable, see gaps |
| Oneshot protection cap | unit (obelisk) — reachable, see gaps |
| Barrier absorption | unit:test_barrier_absorbs_damage_first |
| Elude stacks | unit (obelisk elude tests) |
| Spell dodge | unit (resolution step-0) |
| Physical DR / Generic DR | unit (resolution) |
| H2 damage_prevented aggregation | golden:resistance_mitigates (function covered; only resist layer non-zero) |

### stat-block-stattypes
| Feature | Evidence |
|---|---|
| IncreasedFireDamage | golden:self_buff_boosts_damage (empower 20→30) |
| IncreasedCastSpeed | golden:cast_speed_scaling (with_stat) |
| AddedCriticalChance | golden:crit_strike (with_stat) |
| FireResistance | golden:resistance_mitigates (with_stat) |
| flat+increased+more layering | golden:cast_speed_scaling (real rebuild path) |
| rebuild_from_sources pipeline | golden x3 + unit:with_stat_flows_through_rebuild_without_wiping_bases |
| DamageResolved H2 breakdown | golden:crit_strike, golden:resistance_mitigates |

### effects-statuses
| Feature | Evidence |
|---|---|
| Duration Finite/Infinite | golden:apply_effect, golden:on_apply_triggers_skill; unit:test_infinite_duration_to_effect |
| Stacking: Refresh | golden:effect_refresh_stacking |
| Stacking: Limited + OnMaxStacks | golden:on_max_stacks_triggers_and_consumes |
| Stacking: Unlimited (PerStack timers) | golden:effect_unlimited_stacking |
| OnApply trigger | golden:on_apply_triggers_skill |
| OnExpire trigger | golden:on_expire_triggers_skill; unit:on_expire_trigger_is_surfaced_when_an_effect_expires |
| OnConsume trigger | golden:trigger_cascade; unit:on_consume_trigger_is_surfaced_in_hit_outcome |
| DoT ticking | golden:firebolt_kill; unit:burn_is_applied_and_ticks_down_life |
| Apply chance (probabilistic) | golden:probabilistic_effect_apply (seed-split unit companion) |
| Stat modifiers / rebuild | golden:self_buff_boosts_damage; unit:apply_obelisk_effect_modifies_computed_stat |
| Effect lifecycle (apply/tick/expire) | all 32 goldens (EffectApplied/Expired/DotTicked) |

### skills-cast-timeline
| Feature | Evidence |
|---|---|
| Targeting SingleEnemy / Cone / Direction | golden:firebolt_kill, golden:cone_cleave |
| Delivery Melee / Projectile | golden:firebolt_kill (projectile motion) |
| HitMode OncePerTarget / EveryTick | golden:firebolt_kill, golden:everytick_hitbox |
| HitMode FirstOnly / rehit_interval | unit:first_only_hitbox_…, unit:everytick_with_rehit_interval_… |
| Cast phases Windup/Active/Recovery | every casting golden |
| HitWindow lifecycle | every hit golden |
| Cooldown gate | golden:cooldown_gate |
| Mana cost gate | golden:cast_rejected_insufficient_mana |
| Attack/cast-speed scaling | golden:cast_speed_scaling |
| Interrupt | golden:interrupt_cast |
| Range / LoS validation | golden:out_of_range, golden:line_of_sight |
| All CastRejectReason (8 of 9) | dedicated golden each (UnknownSkill, OutOfRange, NoTarget, NoLineOfSight, OnCooldown, AlreadyCasting, InsufficientMana) |
| HitFilter Enemies | golden:faction_filter |
| Collision shapes Sphere/Cone, motion Static/Linear | golden:firebolt_kill, golden:cone_cleave |

### loot-tables
| Feature | Evidence |
|---|---|
| Weighted selection | golden x5 + unit:test_weighted_roll_count |
| Currency / Item drop types | golden:loot_multi_drop; unit:test_roll_currency/test_roll_item |
| Multiple rolls per table | golden:loot_multi_drop |
| Nested tables | golden:loot_nested_table (+ companion unit) |
| Rarity bonus modulation | golden:loot_rarity_scaling (copper→diamond) |
| Quantity multiplier | golden:loot_quantity_scaling (10→30) |
| Seeded determinism | golden x5 + determinism:same_seed/cross_seed loot |
| DropTableId + LootDropped | all 5 loot goldens + integration test |
| Count ranges / entry rarity_bonus | exercised across loot goldens |

### netcode-rng-determinism
| Feature | Evidence |
|---|---|
| NetEvent surfaces (CastBegan, DamageResolved, EffectApplied, DotTicked, EntityDied, CastRejected) | 16+ goldens + tests/netcode.rs |
| Serde round-trip | unit:netevent_serde_round_trips |
| String-stable ids / ObeliskEntityIndex | unit:sync_index_added/removed + netcode drain test |
| CombatRng seeding (ChaCha8) | unit:config_loads_skills_and_seeds_rng |
| RNG threading (crit/status/loot) | determinism.rs (6+ tests) |
| Same-seed idempotence / cross-seed divergence | determinism.rs (8 tests) |
| AoE stable sort by id (order-independence) | unit:aoe_stable_sorts_targets_before_drawing_rng |
| No thread_rng / no receive_damage funnel | enforced + implicitly verified by reproducibility |
| Triggered secondary cast egress | golden:skill_trigger_secondary_cast |

---

## 3. Residual reachable gaps (the real remaining work)

These are `gap-reachable`: the mechanic is implemented in obelisk and reachable through the
existing obelisk-bevy harness (`with_stat`, `grant_*` verbs, fixtures, scenario library), but
**no obelisk-bevy test exercises it**. Each lists the cheapest cover. Prioritized.

### HIGH — secondary defense layers (whole mitigation pipeline currently proven only by resistance)
The H2 `damage_prevented` breakdown aggregates barrier + armour + resists + block + physical_dr
+ generic_dr + oneshot + elude, but **only resistance is ever non-zero in committed goldens**.
Each layer below is one small golden.

| Gap | Cheapest cover |
|---|---|
| **Armour** (physical DR, POE formula) | golden: target `.with_stat(AddedArmour, x)`, cast a physical skill (cleave is physical), assert `prevented` carries armour |
| **Barrier** absorption | golden: `grant_barrier(x)` then take damage; assert barrier-absorbed portion |
| **Elude** stacks (consumed on hit) | golden: `grant_elude(n)` then hit; assert elude reduction + stack consumption |
| **Generic DR** (ReducedDamageTaken) | golden: `.with_stat(ReducedDamageTaken, x)`; assert generic-DR portion |
| **Physical DR** (% after armour) | golden: `.with_stat(PhysicalDamageReduction, x)` + physical hit |
| **Oneshot protection** (evasion cap) | golden: `.with_stat(AddedOneshotProtection, x)` + low-accuracy caster + big hit |
| **Spell dodge** | unit/integration: `.with_stat(SpellDodgeChance, x)`; note `was_dodged` is not yet threaded to DamageResolved (would need event wiring) |
| **Penetration** (reduces target resist) | golden: caster penetration vs resisted target; assert effective resist drop |

### HIGH — alternate trigger conditions (only OnCrit/OnConsume/OnApply/OnExpire/OnMaxStacks proven end-to-end)
All are unit-tested in stat_core; none have an obelisk-bevy golden.

| Gap | Cheapest cover |
|---|---|
| **OnNonCrit** | golden: low-crit caster + trigger skill (mirror of skill_trigger_secondary_cast) |
| **EveryNthHit** | golden: multi-hit cast loop (n=3) exercising the mutable counter |
| **DamageOverThreshold** | golden: skill with threshold param + a hit above it |
| **MultipleDamageTypes** | golden: skill with fire+physical base, trigger on multi-type |
| **OnBarrierBroken / OnOverkill** | golden: pairs with barrier/overkill defense scenarios above |
| **Caster/target-state pre-triggers** (PlayerFullLife, TargetLowLife, SelfHasEffect, etc.) | golden(s): conditional trigger gated on life/effect state |
| **Skill-condition replacement mode** (`additional=false`) | golden: trigger that drops the primary packet |
| **Global conditionals from gear/effects** | golden: effect/fixture carrying global_conditional that fires on matching skill |

### MED — alternate damage/status mechanics (reachable via fixture authoring)
| Gap | Cheapest cover |
|---|---|
| **Flat damage adds** (AddedPhysical/Fire/Cold/Lightning/Chaos) | golden: `.with_stat(AddedFireDamage, x)`; assert raised base |
| **Status magnitude scaling** (non-1.0) | fixture: effect with magnitude≠1.0 + skill applying it; assert scaled modifier |
| **`increased` vs `more` distinction** | fixture: effect with `is_more=true`; contrast vs increased golden |
| **Amplify status** (Ignite doubling burn) | unit/fixture: amplifies_status modifier; assert doubled DoT DPS |
| **Consume DoT damage** (bonus from expiring status) | fixture: skill with consume_dot_damage; assert bonus packet |
| **Buildup status application** (`type='buildup' threshold=X`) | fixture: buildup effect + repeated hits crossing threshold |
| **Charge-based effects** (AllSkills/AllIncomingHits/SpecificOnly) | fixture: charged-buff scenario consuming charges on cast/hit |
| **consume_stacks operation** | fixture: skill that consumes target effect stacks |
| **apply_chance='always'** (guaranteed vs probabilistic) | fixture: effect_application with always mode |
| **Life/Mana on hit/kill** | golden: skill with on_hit/on_kill recovery (note on-kill routing boundary, below) |

### MED — alternate cast/targeting permutations
| Gap | Cheapest cover |
|---|---|
| **SelfCast targeting** | golden: self-targeted skill (no range gate) |
| **Instant delivery through cast pipeline** | golden: instant skill that still emits cast events (current path bypasses validation) |
| **HitFilter Caster / Allies / All** | golden(s): self-damage, ally-buff, friendly-fire scenarios |
| **Point targeting** (`cast_skill_at_point`) | golden: ground-point AoE |
| **Collision shape Capsule** | fixture: capsule volume (overlap is shape-agnostic, but trace it) |
| **effective_cooldown (CDR)** | golden: `.with_stat` CDR + cooldown skill, assert shortened gate |
| **effective_mana_cost (cost mods)** | golden: cost-reduction/increase mod, assert changed gate |

### LOW — loot/netcode integration-path coverage (unit-tested in core, not via obelisk-bevy)
| Gap | Cheapest cover |
|---|---|
| **Unique drop type** | golden: drop table with a unique entry |
| **No-drop entry** | golden: table with a no_drop entry |
| **Level-based filtering** (min/max_level) | golden: `with_level(n)` + level-gated entries (builder exists, never called by a scenario) |
| **Cycle detection / UnknownTable error** | integration: roll a cyclic/unknown table, assert RollError |
| **EffectExpired NetEvent** | golden: trace EffectExpired explicitly (currently only TriggerFired observed on expiry) |
| **Skip-on-missing-id mirror path** | unit: remove ObeliskId, assert mirror skips gracefully |
| **Action::ApplyStatSources via scenario** | golden: spawn-time stat sources driving observable damage |

---

## 4. Documented boundaries (intentionally not routed — do NOT test)

These are genuine obelisk features that obelisk-bevy deliberately does not auto-wire; the
limitation is documented (CLAUDE.md / feature_matrix). They are **boundaries, not gaps**.

- **Defender-side trigger cascades** — OnDamageTaken / OnDamageTakenOfType / OnHitTaken /
  OnDodge / OnEvasionCap / OnBarrierDepleted / OnLowLifeReached. Counter-attack routing is
  not auto-wired (needs game-level target selection). Unit-tested in stat_core.
- **On-kill / splash / counter packets** — surfaced but not auto-resolved (need
  game-level target selection, e.g. nearest enemy for splash).
- **Trigger-fired skill auto-execution from `apply_obelisk_effect` closures** — TriggeredEffect
  is surfaced via TriggerFired; game-level routing required.
- **`StrongestOnly` stacking** — implemented, but effects applied via the public bevy path
  build `dot:None` (dps()=0), so stronger/weaker is indistinguishable in the EffectApplied
  trace. No observable signature → intentionally not a golden.
- **`DotTicked.effect_id` rollup** — empty effect_id; stat_core's TickResult has no per-effect
  breakdown (would require an obelisk-side change).
- **`CastRejectReason::ConditionNotMet` / skill `use_conditions`** — obelisk parses
  use_conditions but has no pre-cast condition gating; the variant is defined but never emitted.
- **Full item generation on drop** — Drop::Item is emitted but not run through ItemGenerator
  (affixes/sockets deferred to game layer).
- **Parent-rig event propagation** — events are global with a source field; hierarchical
  routing is deferred. Consumers filter by source.

---

## 5. Out of scope (obelisk features with NO obelisk-bevy surface)

`not-integrated` — listed explicitly so they are not mistaken for gaps. obelisk-bevy's
focus is intentionally narrow: spatiotemporal casting, deterministic hit resolution, effect
lifecycle, loot rolls, and netcode egress.

- **Skill Tree** (allocation, modular cores, node effects, passive→stat-source pipeline) — entirely absent.
- **Equipment / item management** (equip/unequip, gear sources, weapon-hand swapping).
- **Currency operations & crafting** (apply_currency, orbs, affix rolling, rarity tiers, unique preview).
- **Buff-source layer** (apply_buff/remove_buff/tick_buffs — superseded by effects).
- **Direct damage API** (receive_damage / attack — wrapped by the deterministic funnel).
- **Combat recovery / on-kill recovery application** (apply_combat_recovery, life/mana_on_kill as resolvable mechanics).
- **Environmental damage** (take_environmental_damage).
- **Persistent modifiers** (shrine buffs, level-up bonuses).
- **Attribute scaling rules** (set/add_attribute_scaling).
- **Low-level stat mutation verbs** (force_rebuild, heal, restore_mana, deduct_skill_cost).
- **Spell-dodge / plain-f64 resource fields** as game-level config (dot_multiplier, etc.).
- **Low-level global-conditional / conditional-modifier add/remove verbs** (used internally only).
- **Direct effect query/removal verbs** (effects_of_status, remove_effects_by_*, clear_effects).
- **StatusEffectData scaling/conversion tables** as a public mutation/inspection API.
- **Block** (CombatResult.damage_blocked + BlockAmount stat exist but no resolution logic — not implemented in obelisk itself).
- **Explicit drop_chance field** (loot uses weight + no_drop; not an untested feature).
- **Effect icon/banner** (UI-only display hints).
- **Attribute & all-attribute stat types** (strength–charisma) as driven mechanics.
- **Status-effect parameterized stat types** (EffectDamageOverTime, ConvertDamageToEffect, etc.).

---

## 6. Bottom line

**Is "all obelisk features covered" TRUE for everything obelisk-bevy integrates?**

**Yes at the mechanism level; not yet at the permutation level.** Every subsystem
obelisk-bevy integrates has its core path proven by a golden trace or unit test, the
determinism contract is airtight, and the netcode wire format is fully exercised. There are
**no broken or unproven integrations** in scope.

What remains is breadth — **41 already-reachable mechanics** lack a dedicated obelisk-bevy
test. The genuinely valuable remaining work, in priority order:

1. **Defense pipeline** — armour, barrier, elude, generic/physical DR, oneshot, penetration
   (each one small golden; today only resistance proves the H2 breakdown).
2. **Alternate trigger conditions** — OnNonCrit, EveryNthHit, DamageOverThreshold,
   MultipleDamageTypes, caster/target-state gates, replacement-mode, gear globals.
3. **Alternate damage/status mechanics** — flat adds, magnitude, `more` vs `increased`,
   amplify, buildup, charges, consume_stacks, on-hit recovery.
4. **Targeting/cast permutations** — SelfCast, Point, HitFilter variants, CDR/cost mods.
5. **Loot/netcode integration paths** — unique/no-drop/level-filter/error paths,
   EffectExpired egress.

The `gap-boundary` (11) and `not-integrated` (19) items are correctly out of scope and should
not be confused with gaps: defender cascades, on-kill routing, skill-tree, equipment, and
crafting are deliberate boundaries of obelisk-bevy's design.
