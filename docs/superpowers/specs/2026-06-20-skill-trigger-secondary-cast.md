# Skill-Condition Damage Triggers as Secondary Casts

**Date:** 2026-06-20
**Status:** Design proposal (awaiting maintainer decision on the cross-repo obelisk change)
**Repos touched:** `obelisk-bevy` (primary) + `obelisk/stat_core` (one surgical change — see §3/§6)

---

## 1. Problem

A skill author can attach a **damage-trigger condition** to a skill via a `SkillCondition`
(`../obelisk/stat_core/src/damage/triggers.rs:16-27`):

```rust
pub struct SkillCondition {
    pub trigger_skill: String,        // the skill ID to fire, e.g. "explosion"
    pub additional: bool,             // fire IN ADDITION to the primary (vs replace it)
    #[serde(flatten)] pub condition: TriggerCondition,  // OnCrit, EveryNthHit, ...
}
```

Intent: when (say) `fireball` crits, it should **cast `explosion` as a real secondary skill** —
producing its own observable cast/trigger event and its own distinct damage line, exactly the way
the effect-condition cascade already works.

### Current (wrong) behavior — silently summed

Today the trigger fires, but obelisk-bevy folds the triggered skill's damage into the **same**
`DamageResolved` total as the primary hit. The flow:

1. `calculate_damage_with_triggers` (`calculation.rs:465-607`) builds a `Vec<DamagePacket>`: the
   primary packet plus one `is_triggered=true` packet per matched condition (lines 590-604).
2. `resolve_damage_with_triggers` (`resolution.rs:390-494`) resolves **all** packets inline against
   the live defender (the `for packet in packets` loop, lines 402-412) and returns a flat
   `Vec<CombatResult>` — one result per packet.
3. obelisk-bevy `resolve_one_hit` (`src/combat/resolve.rs:55-112`) **sums every result** into one
   number: `let total_damage: f64 = tr.results.iter().map(|r| r.total_damage).sum();` (line 72).
4. `on_hit_confirmed` (`src/combat/system.rs:46-58`) emits **one** `DamageResolved` for the whole
   hit. There is no `TriggerFired` for the secondary skill and no separate damage line.

So `fireball` that triggers `explosion` shows up as a single inflated `fireball` damage number, with
zero indication that `explosion` ever cast. The author's `trigger_skill` is invisible in the trace.

### The working template — effect-condition cascade

The **effect-condition** trigger path (OnConsume / OnApply / OnMaxStacks / OnExpire) already does
the right thing, and the `trigger_cascade` golden proves it
(`tests/golden/trigger_cascade.trace`):

```
9  Damage        caster=player target=dummy skill=discharge_strike dmg=10.000 ...
9  TriggerFired  source=player target=dummy skill=static_discharge effect=charged
9  Damage        caster=player target=dummy skill=static_discharge dmg=25.000 ...
```

Two **separate** `Damage` lines + a `TriggerFired` naming the secondary skill. That is the exact
shape we want for skill-condition triggers. The mechanism that produces it: obelisk surfaces effect
triggers as `TriggeredEffect` objects on `CombatResult.triggered_effects`
(`result.rs:79-82`); obelisk-bevy lifts them into `HitOutcome.triggered`
(`resolve.rs:93-96`) and the `on_hit_confirmed` **worklist** (`system.rs:85-134`) fires each one:
`TriggerFired` (line 97) → `to_damage_packet` (line 108) → `resolve_damage_with_rng` (line 110) →
a **distinct** `DamageResolved` (line 113).

**The goal of this spec: make skill-condition damage triggers travel that same worklist path so they
produce the same `TriggerFired` + separate `Damage` line, instead of being summed.** This is
fundamentally a *visibility* problem (obelisk-bevy can't tell a triggered `CombatResult` from a
primary one), not a *computation* problem (obelisk already rolls/resolves the packet correctly).

---

## 2. Mechanism — where the `trigger_skill` info lives and dies

Precise data flow for a skill condition `fireball --OnCrit--> explosion`:

| # | Location | What happens | Has `trigger_skill`? |
|---|----------|--------------|----------------------|
| 1 | `SkillCondition.trigger_skill` (`triggers.rs:19`) | author writes `"explosion"` | ✅ source of truth |
| 2 | `calculation.rs:518/528/546/562/572` | matched conditions push `(trigger_skill, additional)` tuples into `pre_calc_triggers` / `post_calc_triggers` | ✅ in the tuple |
| 3 | `calculation.rs:590-591` | `for (trigger_id, _) ... skill_registry.get(&trigger_id)` looks up the triggered `Skill` | ✅ `trigger_id` in scope |
| 4 | `calculation.rs:592-602` | builds `triggered_packet`; sets `is_triggered = true` (600) and `triggered_by = Some(skill.id)` (601) | ⚠️ **partial loss** — see below |
| 5 | `resolution.rs:406` | `resolve_damage_with_rng(&defender, packet, rng)` → `(StatBlock, CombatResult)` | ❌ packet metadata **not copied** to result |
| 6 | `resolution.rs:411` | `results.push(result)` into the flat `Vec<CombatResult>` | ❌ gone |
| 7 | `resolve.rs:72` | obelisk-bevy sums all results | ❌ gone |

**The critical naming subtlety at step 4** (verified in source):
- `triggered_packet.skill_id` = the **triggered/child** skill ID (`"explosion"`) — set by
  `calculate_damage(... triggered_skill ...)` at line 592-599.
- `triggered_packet.triggered_by` = the **parent** skill ID (`Some("fireball")`) — line 601.
- `triggered_packet.is_triggered` = `true` — line 600.

So the secondary skill's own ID is recoverable from `packet.skill_id`; what's needed to *route* it
is simply the `is_triggered` flag. The `DamagePacket` already carries both fields
(`packet.rs:64-69`, defaulted false/None at `:115-116`).

**Where it dies:** `CombatResult` (`result.rs:8-91`) has **no** `is_triggered` / `triggered_by` /
`skill_id` field. `resolve_damage_with_rng` (`resolution.rs:31-35`) builds a fresh
`CombatResult::new()` and never copies the packet's trigger metadata onto it. After step 5 the
result is anonymous — obelisk-bevy cannot tell a triggered result from the primary one, nor which
secondary skill it came from.

Contrast: effect triggers survive because they are evaluated *at resolution time* and written onto
`CombatResult.triggered_effects` (`result.rs:79-82`) as structured `TriggeredEffect` objects
(`types.rs:267-281`), which obelisk-bevy reads at `resolve.rs:94-96`.

---

## 3. Recommended approach

### Decision: **Strategy B1** — carry the metadata through obelisk (small obelisk-side change).

> **An obelisk-crate change IS required.** This cannot be done cleanly in obelisk-bevy alone.

The two candidates considered:

- **Strategy A (obelisk-bevy only):** after `resolve_damage_with_triggers`, recover which
  `CombatResult` came from a triggered packet by index-matching results back to the original packet
  list (`packet.is_triggered`), then suppress that result's damage from the summed total and re-fire
  it. **Rejected — fragile and determinism-breaking:**
  - **Index coupling across the layer boundary.** `resolve_damage_with_triggers` **breaks early** if
    the defender dies mid-loop (`resolution.rs:403-405`), so `results.len()` is *not* guaranteed to
    equal `packets.len()`; a naive `packets[i] ↔ results[i]` zip is wrong whenever a kill short-
    circuits the loop. We *can* zip while the loop runs (results are pushed in packet order until
    the break), but it's brittle and silently mis-aligns on any future change to the loop.
  - **Double-count via subtraction.** obelisk has *already* resolved the triggered packet into a
    `CombatResult` and mutated the defender. To "re-fire" it as a secondary skill we would either
    (a) re-resolve it (defender double-damaged) or (b) suppress its damage from the primary total by
    post-hoc subtraction — arithmetic that introduces off-by-one / float-equality hazards and a
    CRITICAL double-count if suppression is ever wrong.
  - **Determinism break.** Re-computing the triggered packet in `on_hit_confirmed` (to recover its
    metadata) draws from the seeded RNG in a *different order* than the original
    `calculate_damage_with_triggers` run, which already consumed RNG for that packet. Determinism is
    sacred here (CLAUDE.md), so this is disqualifying.

- **Strategy B1 (obelisk carries metadata):** add `is_triggered: bool` + `triggered_by:
  Option<String>` to `CombatResult`; copy them from the packet inside `resolve_damage_with_rng`.
  Then obelisk-bevy can read `result.is_triggered` directly off each `CombatResult` with **no
  index-matching, no re-computation, no RNG re-draw, no subtraction.** RNG is consumed exactly once.
  **Boring and bulletproof.** This is the recommendation.

(Strategy B2 — returning unresolved "fire this skill" directives from
`resolve_damage_with_triggers` à la `TriggeredEffect` — is heavier: a new type + signature change +
obelisk-bevy must resolve the directive packets. B1 reuses the already-resolved results and is
strictly smaller. Rejected as over-engineered.)

### Double-count avoidance (central design constraint)

obelisk **already resolves** every triggered packet inline and mutates the defender. Therefore the
triggered damage is **already reflected** in `tr.results` and in `tr.defender`. We must **NOT** fire
the secondary skill as a *fresh* cast (that would damage the defender a second time and double-draw
RNG). Instead:

> **Re-classify, don't re-resolve.** The triggered `CombatResult` obelisk already produced *is* the
> secondary cast's result. We split it out of the summed primary total and **emit it as its own
> `TriggerFired` + `DamageResolved` pair**, reading the numbers straight off that existing result.
> The defender is mutated exactly once (by obelisk's inline loop); obelisk-bevy only re-buckets the
> already-computed output into separate trace lines.

This is the key difference from the effect-cascade worklist, which *does* re-resolve via
`resolve_damage_with_rng` (because effect triggers are **not** pre-resolved by obelisk — they arrive
as un-fired `TriggeredEffect`s). Skill-condition triggers arrive **already resolved**, so the
secondary cast is synthesized from the existing result, never re-fired.

---

## 4. Step-by-step implementation plan

### Step 1 — obelisk: carry trigger metadata onto `CombatResult`

File: `../obelisk/stat_core/src/combat/result.rs`

1a. Add two fields to the struct (near the existing flags, ~`:71-77`):

```rust
    // === Trigger provenance (skill-condition triggers) ===
    /// True if this result came from a skill-condition-triggered DamagePacket
    /// (DamagePacket.is_triggered). The primary hit's result has this false.
    pub is_triggered: bool,
    /// The PARENT skill ID that fired the trigger (DamagePacket.triggered_by),
    /// e.g. "fireball" for an "explosion" triggered by fireball's OnCrit condition.
    pub triggered_by: Option<String>,
```

1b. Initialize in `Default` (`:93-124`): `is_triggered: false, triggered_by: None,`.
    (`CombatResult::new()` delegates to `Default`, so no change there.)

1c. Copy from the packet in `resolve_damage_with_rng` (`resolution.rs:36-37`, right after
    `let mut result = CombatResult::new();`):

```rust
    result.is_triggered = packet.is_triggered;
    result.triggered_by = packet.triggered_by.clone();
```

That is the entire obelisk change: **2 struct fields + 2 default lines + 2 copy lines.** Both
serde-derive-safe (plain `bool` / `Option<String>`); no `#[serde(skip)]` needed (these are real
provenance, not transient — though `skip` is also fine since obelisk-bevy reads them in-process).
The secondary skill's own ID is already on `result`-adjacent data via the packet's `skill_id`
(`packet.skill_id` = the triggered skill, e.g. `"explosion"`) — see Step 3 for how obelisk-bevy
recovers it.

> **Note on `result.skill_id`:** `CombatResult` does **not** carry `skill_id` today. The triggered
> skill's ID lives on the *packet* (`packet.skill_id`), which obelisk-bevy still holds in
> `skill_result.packets`. We pair each triggered result with its packet by position **within the
> resolved slice** (safe — see Step 2) to recover the secondary skill ID. If the maintainer prefers,
> add a third field `pub skill_id: String` to `CombatResult` and set `result.skill_id =
> packet.skill_id.clone();` in 1c — this removes all pairing and is the cleanest variant. **Recommend
> adding `skill_id` too** (3 fields total) so obelisk-bevy needs zero index correlation.

### Step 2 — obelisk-bevy: split triggered results out in `resolve_one_hit`

File: `src/combat/resolve.rs`

2a. Add a field to `HitOutcome` (`:9-28`):

```rust
    /// Skill-condition-triggered hits that obelisk already resolved inline (NOT effect triggers).
    /// Each is a real CombatResult the defender already took; `on_hit_confirmed` re-buckets these
    /// into their own TriggerFired + DamageResolved (secondary cast), instead of summing them into
    /// the primary total. Carries the parent skill id (`triggered_by`) and the secondary skill id.
    pub triggered_skill_hits: Vec<TriggeredSkillHit>,
```

with a small projection struct (reads straight off the existing `CombatResult`):

```rust
#[derive(Debug, Clone)]
pub struct TriggeredSkillHit {
    pub secondary_skill_id: String,   // CombatResult.skill_id (or paired packet.skill_id)
    pub triggered_by: String,         // parent skill that fired the condition
    pub total_damage: f64,
    pub is_killing_blow: bool,
    pub is_critical: bool,
    pub damage_prevented: f64,
    pub life_gained: f64,
    pub mana_gained: f64,
}
```

2b. In `resolve_one_hit`, **partition** `tr.results` by the new `result.is_triggered` flag:

- **Primary results** (`!is_triggered`) feed the existing aggregates exactly as today
  (`total_damage`, `is_killing_blow`, `is_critical`, `damage_prevented`, leech). This is the
  critical double-count fix: triggered results are **excluded** from `total_damage`.
- **Triggered results** (`is_triggered`) each become a `TriggeredSkillHit` (numbers read off the
  result via the existing `combat_result_prevented` / `_life_gained` / `_mana_gained` helpers;
  `is_critical` from the paired packet's `is_critical`).

Pairing for the secondary skill id: if Step 1 added `result.skill_id`, use it directly. Otherwise,
iterate `tr.results` and `skill_result.packets` **together over the resolved prefix** — both are in
the same packet order and the resolved slice is `results.len()` long (the early-break only *omits*
trailing packets, never reorders), so `packets.iter().zip(tr.results.iter())` correctly pairs every
resolved result with its source packet. (Adding `result.skill_id` makes this moot — recommended.)

`*target = tr.defender;` (`:99`) stays unchanged — the defender already absorbed all packets,
including the triggered ones. **Do not** re-apply anything.

### Step 3 — obelisk-bevy: emit the secondary cast in `on_hit_confirmed`

File: `src/combat/system.rs`

After emitting the primary `DamageResolved` / effects / death (`:46-72`) and **before** the existing
effect-trigger worklist (`:85`), iterate `outcome.triggered_skill_hits` and emit, per hit:

```rust
for th in &outcome.triggered_skill_hits {
    commands.trigger(TriggerFired {
        source: ev.caster,
        target: ev.target,
        skill_id: th.secondary_skill_id.clone(),
        effect_id: String::new(),   // skill-condition trigger: no originating effect
    });
    commands.trigger(DamageResolved {
        caster: ev.caster,
        target: ev.target,
        skill_id: th.secondary_skill_id.clone(),
        total_damage: th.total_damage,
        is_killing_blow: th.is_killing_blow,
        life_after: target_attrs.0.current_life,   // defender already mutated by resolve_one_hit
        mana_spent: 0.0,
        is_critical: th.is_critical,
        damage_prevented: th.damage_prevented,
        life_gained: th.life_gained,
        mana_gained: th.mana_gained,
    });
    if th.is_killing_blow {
        commands.trigger(EntityDied { target: ev.target, killer: Some(ev.caster) });
    }
}
```

**Crucially, these are NOT added to the effect-trigger worklist** (`:86-134`) and **never call
`resolve_damage_with_rng`** — the damage was already applied by obelisk's inline loop in
`resolve_one_hit`. The effect-condition worklist is left exactly as-is and continues to handle only
`outcome.triggered` (the `TriggeredEffect`s). The two trigger kinds stay cleanly separated, matching
the existing architecture split (skill conditions = packet-level, pre-resolved; effect conditions =
`TriggeredEffect`, fired by the worklist).

> **Effect ordering note:** `effects_applied` from a triggered packet currently flows into the
> primary hit's `effects_applied` aggregate (`resolve.rs:74-78`) and is emitted as primary
> `EffectApplied`. If a maintainer wants effects attributed to the secondary skill, move the
> triggered results' `effects_applied` into `TriggeredSkillHit` too. **Out of scope** unless a
> fixture needs it — none do (§5). Flag for the maintainer (§6).

### Resulting trace

A `fireball --OnCrit--> explosion` hit now produces (mirroring `trigger_cascade`):

```
N  Damage        caster=player target=dummy skill=fireball   dmg=<primary>   crit=true ...
N  TriggerFired  source=player target=dummy skill=explosion  effect=
N  Damage        caster=player target=dummy skill=explosion  dmg=<secondary> ...
```

Primary `fireball` damage is **no longer inflated** by the explosion (double-count fixed); the
secondary `explosion` cast is observable (`TriggerFired`) and has its own `Damage` line. Determinism
preserved: RNG consumed exactly once, inside obelisk's single resolution pass.

---

## 5. Existing-golden impact — target ZERO change

**Expected: 0 of 27 goldens change.** (CLAUDE.md says "27 scenarios"; the count of golden files in
`tests/golden/` is 27. Treat "24 goldens" in the task framing as approximate — the verified live
count is 27.)

Why zero:

1. **No existing skill fixture uses a skill-condition damage trigger.** A grep over the fixtures
   (`firebolt`, `cleave`, `firebolt_cd`, `discharge_strike`, `static_discharge`, `quickjab`,
   `pummel`, `chancebolt`, …) shows **zero `trigger_skill` inside a `[skills.damage]`/`conditions`
   block.** Every existing trigger scenario (`trigger_cascade`, `on_apply_triggers_skill`,
   `on_expire_triggers_skill`, `on_max_stacks_triggers_and_consumes`) is an **effect-condition**
   trigger routed through `TriggeredEffect` — a code path this change does not touch.

2. **The new `HitOutcome.triggered_skill_hits` is empty for every current scenario**, so the new
   partition/emit code is a strict no-op: the primary aggregates are computed from a results set
   that contains *only* the primary result (no triggered results to exclude), identical to today.

3. **The obelisk change is purely additive.** Two new `CombatResult` fields, default false/None,
   read only by obelisk-bevy when `is_triggered` is true (never, for current fixtures). obelisk's
   own tests resolve packets with `is_triggered=false` → no behavioral change. No golden in obelisk
   serializes `CombatResult` in a way these fields perturb (add `#[serde(skip)]` if any snapshot
   test proves otherwise — low risk).

**Verification gate (run after implementing, per CLAUDE.md regression rule):**
```bash
# in ../obelisk
cargo test
# in obelisk-bevy
cargo test --features test-support --lib --tests
cargo test --features test-support --test golden        # MUST be clean — zero diffs
cargo clippy --features test-support --lib --tests -- -D warnings
```
If `golden` shows any diff, a current fixture unexpectedly exercises the path — stop and investigate
(do NOT blind-regenerate).

**New coverage (add, don't regenerate):** introduce a `skill_trigger_secondary_cast` scenario — a
skill with an `OnCrit` (or `EveryNthHit { n = 1 }` for guaranteed firing) `SkillCondition` pointing
at a second registered damage skill, seeded so the condition fires. Its golden should show the
three-line shape from §4 (`Damage` primary → `TriggerFired` secondary → `Damage` secondary), proving
parity with `trigger_cascade`. Add a `resolve.rs` unit test asserting the primary total **excludes**
the triggered damage and `triggered_skill_hits` carries it. Bump the scenario count in CLAUDE.md.

---

## 6. Open decisions for the maintainer

1. **Cross-repo obelisk change (required).** Strategy B1 needs `obelisk/stat_core` to gain
   `is_triggered` + `triggered_by` (+ recommended `skill_id`) on `CombatResult`. This is a small,
   additive, intent-clarifying change, but it is a **second-repo edit** and should ship as its own
   PR that obelisk-bevy's `Cargo.toml` path dep picks up. **Decision needed: approve the obelisk
   change?** (If rejected, the feature cannot be implemented cleanly — Strategy A's
   determinism/double-count hazards make an obelisk-bevy-only solution unsafe; we would have to defer
   the feature, not ship Strategy A.)

2. **Add `skill_id` to `CombatResult`?** Recommended (3 fields not 2) — it removes all
   result↔packet index pairing in obelisk-bevy and is the most robust. Costs one extra field +
   one assignment. Confirm.

3. **`additional = false` (replace) semantics.** When the condition replaces the primary
   (`additional=false`, `any_replace` at `calculation.rs:578-587`), obelisk produces **no primary
   packet** — `tr.results` contains only triggered results. Under this design the trace would then
   show *only* `TriggerFired` + a secondary `Damage` line and **no** primary `Damage` line. Confirm
   that's the desired observable shape for a replacing trigger (it is internally consistent, but
   worth an explicit sign-off since no fixture exercises it).

4. **Effect attribution.** Effects applied by a triggered packet currently roll into the **primary**
   hit's `EffectApplied` emission (§4 Step 3 note). Decide whether triggered-skill-applied effects
   should be attributed to the secondary skill (move `effects_applied` into `TriggeredSkillHit`) or
   stay folded into the primary. Out of scope for the damage-line fix; flagging for completeness.

5. **`triggered_by` is the parent skill, not the originating effect.** For skill-condition triggers
   there is no originating *effect*, so the synthesized `TriggerFired.effect_id` is empty (vs the
   effect cascade where it's e.g. `"charged"`). Confirm an empty `effect_id` is acceptable in the
   trace for skill-condition triggers (it distinguishes them from effect triggers, which is
   arguably useful).

6. **On-kill / defensive packets unchanged.** `resolve_damage_with_triggers`'s `on_kill_packets` /
   `defender_packets` (`resolution.rs:374-379`) remain a documented, separate limitation (splash /
   counter routing needs game-side target selection). This spec does **not** auto-route them; it
   only fixes the inline pre/post-calc triggered packets that obelisk already resolves against the
   primary target.

---

## Summary

- **Strategy: B1** (carry trigger metadata through obelisk), not A (obelisk-bevy-only
  index-matching), because A breaks determinism (RNG re-draw) and risks a CRITICAL double-count via
  result/packet index coupling and damage subtraction.
- **Obelisk change required: YES** — add `is_triggered` + `triggered_by` (+ recommended `skill_id`)
  to `CombatResult` and copy them from the packet in `resolve_damage_with_rng`. ~6 lines, additive.
  The feature **cannot** be done cleanly without it.
- **Double-count avoidance:** obelisk already resolves the triggered packet inline and mutates the
  defender once, so obelisk-bevy **re-buckets** that existing `CombatResult` into its own
  `TriggerFired` + `DamageResolved` (reading the numbers off it) and **excludes** it from the
  primary `total_damage` sum — it never re-fires or re-resolves the secondary skill.
- **Golden impact: ZERO** — no current fixture uses a skill-condition damage trigger; the new path
  is a strict no-op (`triggered_skill_hits` empty) for all 27 existing goldens.
- **Resulting trace:** `Damage(primary)` → `TriggerFired(skill=secondary)` → `Damage(secondary)`,
  matching the proven `trigger_cascade` effect-cascade shape.

**Implementation steps:**
1. obelisk `result.rs`: add `is_triggered`/`triggered_by`(/`skill_id`) fields + Default init.
2. obelisk `resolution.rs:36-37`: copy them from `packet` in `resolve_damage_with_rng`.
3. obelisk-bevy `resolve.rs`: add `TriggeredSkillHit` + `HitOutcome.triggered_skill_hits`; partition
   `tr.results` by `is_triggered` so triggered results are excluded from the primary aggregates.
4. obelisk-bevy `system.rs`: in `on_hit_confirmed`, emit `TriggerFired` + a separate `DamageResolved`
   per `triggered_skill_hit` (NOT via the effect worklist, NOT re-resolved).
5. Add a `skill_trigger_secondary_cast` golden + a resolve-layer unit test; bump the scenario count
   in CLAUDE.md.
6. Run `cargo test` (both repos) + the golden suite + clippy; confirm zero golden diffs before
   committing.
