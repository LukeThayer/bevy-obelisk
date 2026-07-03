# Obelisk Trigger Reform (Sub-projects 1+2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Obelisk rules own all cross-skill causality — triggered skills execute their own
timelines spatially at the trigger position — and `CastTimeline` v2 replaces the parallel
Chain/Retarget causality system with acquisition, anchors, templates, and emitters.

**Architecture:** Phase 1 adds `OnImpact`/`OnExpire` lifecycle vocabulary to loot_core via a
local `[patch]` of the vothuul/obelisk git deps. Phase 2 reforms obelisk-bevy in dependency
order: payload plumbing (position + depth on hit/end events) → resolve-seam widening (packet
+ per-result + pre-hit snapshots exposed) → the triggered-timeline executor (condition
stripping, free sub-casts, depth cap) → lifecycle evaluation in the end funnel → schema v2
(deletions + Template/anchor/strikes/acquisition/emitters) → chain-from-rules → goldens.
Every step is golden-trace-protected: existing traces stay byte-identical except where
`Chain`-using example content is deliberately migrated.

**Tech Stack:** Rust, Bevy 0.18 (FixedUpdate sim, observers), avian3d 0.5, stat_core/loot_core
(vothuul/obelisk), RON assets, TOML rules, golden-trace regression harness
(`UPDATE_GOLDEN=1`).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-02-skill-editor-reimplementation-design.md` — D1–D10 govern.
- Repo: work happens in `~/src/obelisk-bevy` (branch `trigger-reform`) and a NEW clone
  `~/src/vothuul-obelisk` (branch `lifecycle-conditions`). `~/src/obelisk` is a DIFFERENT
  repo (LukeThayer fork) — never touch it for this work.
- Determinism: no wall clock, no unseeded RNG; `CombatRng` draw order must not change for
  existing scenarios; emitter jitter uses the new separate `SpawnRng` stream only.
- Golden traces: `cargo test --test golden` must pass at every commit; regeneration
  (`UPDATE_GOLDEN=1`) only for the migration commit (Task 12) and new scenarios, with the
  diff explained in the commit message.
- Trigger depth cap: 8 (`MAX_TRIGGER_DEPTH`), drop + `warn!` at the cap.
- Free sub-casts: `depth > 0` hits resolve with `mana_cost` zeroed; no cooldown interaction.
- Timeline-target hit conditions must author `additional = true` (load-time validation).
- Full suite green at every commit: `cd ~/src/obelisk-bevy && cargo test && cargo clippy --all-targets -- -D warnings`.

---

### Task 1: vothuul clone + local patch + rev pin

**Files:**
- Create: `~/src/vothuul-obelisk` (clone)
- Modify: `~/src/obelisk-bevy/Cargo.toml` (git deps: add `rev`, add `[patch]`)
- Modify: `~/src/obelisk-bevy/.gitignore` (keep lock ignored for now — the pin is `rev =`, per spec D1 "commit Cargo.lock **or** pin rev")

**Interfaces:**
- Produces: a buildable obelisk-bevy against a local, editable vothuul checkout; later
  tasks import `loot_core::types::{TriggerCondition, ConditionPhase}` from it.

- [ ] **Step 1: Clone vothuul/obelisk and branch**

```bash
git clone ssh://git@github.com/vothuul/obelisk.git ~/src/vothuul-obelisk
cd ~/src/vothuul-obelisk && git checkout -b lifecycle-conditions
git log -1 --format=%H   # expect 54d0837... (master today)
```

- [ ] **Step 2: Pin + patch obelisk-bevy's deps**

In `~/src/obelisk-bevy/Cargo.toml`, add `rev = "54d0837"` to each of the four obelisk git
deps (stat_core, loot_core, skill_tree, tables_core — keep `branch = "master"` removed when
`rev` is set), then append:

```toml
[patch."ssh://git@github.com/vothuul/obelisk.git"]
stat_core = { path = "/home/luke/src/vothuul-obelisk/stat_core" }
loot_core = { path = "/home/luke/src/vothuul-obelisk/loot_core" }
skill_tree = { path = "/home/luke/src/vothuul-obelisk/skill_tree" }
tables_core = { path = "/home/luke/src/vothuul-obelisk/tables_core" }
```

- [ ] **Step 3: Verify the build sees the patch**

Run: `cd ~/src/obelisk-bevy && cargo build 2>&1 | tail -3 && cargo tree -p stat_core | head -2`
Expected: builds clean; `stat_core` resolves to the `/home/luke/src/vothuul-obelisk` path.

- [ ] **Step 4: Full suite still green**

Run: `cd ~/src/obelisk-bevy && cargo test 2>&1 | grep -cE "test result: ok"`
Expected: same suite count as `master` (no behavior change from pin+patch).

- [ ] **Step 5: Commit (obelisk-bevy only — the clone is not a repo change)**

```bash
cd ~/src/obelisk-bevy && git checkout -b trigger-reform
git add Cargo.toml && git commit -m "build: pin obelisk deps to rev 54d0837 + local vothuul patch for lifecycle work (spec D1/§3.1)"
```

---

### Task 2: Lifecycle trigger vocabulary in loot_core

**Files:**
- Modify: `~/src/vothuul-obelisk/loot_core/src/types.rs` (~1270: `TriggerCondition` enum; ~1403: `phase()`; the `Display` impl for the enum)
- Modify: `~/src/vothuul-obelisk/obelisk_editor/src/editors/global_conditional.rs:98-102` (exhaustive condition dropdown)
- Test: `~/src/vothuul-obelisk/loot_core/src/types.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `TriggerCondition::OnImpact`, `TriggerCondition::OnExpire`,
  `ConditionPhase::Lifecycle`; `cond.phase() == ConditionPhase::Lifecycle` for both; serde
  tags `on_impact` / `on_expire`. Consumed by obelisk-bevy Tasks 8–9.

- [ ] **Step 1: Write the failing tests** (append to the existing `types.rs` test module, or create one following the file's conventions)

```rust
#[test]
fn lifecycle_conditions_parse_and_classify() {
    let imp: TriggerCondition = toml::from_str(r#"type = "on_impact""#).unwrap();
    let exp: TriggerCondition = toml::from_str(r#"type = "on_expire""#).unwrap();
    assert_eq!(imp.phase(), ConditionPhase::Lifecycle);
    assert_eq!(exp.phase(), ConditionPhase::Lifecycle);
}

#[test]
fn lifecycle_phase_is_ignored_by_resolve_time_phases() {
    // The three resolve-time phase groups must NOT include Lifecycle.
    assert_ne!(ConditionPhase::Lifecycle, ConditionPhase::PreCalculation);
    assert_ne!(ConditionPhase::Lifecycle, ConditionPhase::PostCalculation);
    assert_ne!(ConditionPhase::Lifecycle, ConditionPhase::PostResolution);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd ~/src/vothuul-obelisk && cargo test -p loot_core lifecycle 2>&1 | tail -3`
Expected: FAIL — `no variant named OnImpact` (compile error counts as the failing state).

- [ ] **Step 3: Implement**

In the `TriggerCondition` enum (internally tagged `#[serde(tag = "type", rename_all = "snake_case")]`):

```rust
    /// Projectile/volume ended on world geometry (evaluated by the embedding
    /// spatial layer at its end events — never during damage resolution).
    OnImpact,
    /// Projectile/volume lifetime elapsed without a terminal hit (embedding
    /// layer, as above).
    OnExpire,
```

Add `Lifecycle` to `ConditionPhase` and the `phase()` match:

```rust
    TriggerCondition::OnImpact | TriggerCondition::OnExpire => ConditionPhase::Lifecycle,
```

Add `Display` arms (`"On World Impact"` / `"On Expire"`), and extend the obelisk_editor
dropdown match with the two variants (copy the adjacent arm pattern). If the enum derives
an `EnumVariants`-style ALL table, append both there too.

- [ ] **Step 4: Run tests + workspace build**

Run: `cd ~/src/vothuul-obelisk && cargo test -p loot_core lifecycle && cargo build --workspace 2>&1 | tail -2`
Expected: both tests PASS; workspace (incl. obelisk_editor's exhaustive matches) compiles.

- [ ] **Step 5: Verify stat_core resolve paths ignore Lifecycle** — grep the three
  evaluation fns; they match on specific variants / phase groups, so unknown-phase
  conditions fall through to "no match":

Run: `grep -n "Lifecycle" ~/src/vothuul-obelisk/stat_core/src/damage/triggers.rs | wc -l`
Expected: `0` (no resolve-time consumer — correct by construction). Add one stat_core test:

```rust
#[test]
fn lifecycle_conditions_never_fire_during_resolution() {
    let cond = SkillCondition {
        trigger_skill: "explosion".into(),
        additional: true,
        condition: TriggerCondition::OnImpact,
    };
    // evaluate_pre is the entry for non-packet phases; Lifecycle must be inert.
    assert!(!cond.condition.evaluate_pre(&StatBlock::default(), &StatBlock::default()));
}
```

Run: `cargo test -p stat_core lifecycle` → PASS.

- [ ] **Step 6: Commit + note the PR**

```bash
cd ~/src/vothuul-obelisk
git add -A && git commit -m "feat(triggers): OnImpact/OnExpire lifecycle conditions (ConditionPhase::Lifecycle)

Vocabulary for embedding spatial layers: evaluated at projectile/volume
end events, never during damage resolution. Editor dropdowns updated."
```

Then: open the PR to vothuul/obelisk (user action — flag it and continue; the local patch
keeps everything unblocked).

---

### Task 3: Depth + position payload plumbing (obelisk-bevy)

**Files:**
- Modify: `~/src/obelisk-bevy/src/spatial/boxes.rs` (Hitbox: add `depth: u8`)
- Modify: `~/src/obelisk-bevy/src/events.rs` (HitConfirmed: add `position: Vec3, depth: u8`; HitboxEnded: add `depth: u8` — it already carries `position`)
- Modify: `~/src/obelisk-bevy/src/timeline/advance.rs` (spawn_window_hitbox threads depth; ChainPayload gains `depth`)
- Modify: `~/src/obelisk-bevy/src/spatial/detect.rs` + `src/spatial/detect.rs::resolve_beam_hits` (fill position/depth on HitConfirmed)
- Test: `~/src/obelisk-bevy/tests/end_events.rs` (extend)

**Interfaces:**
- Consumes: existing `ChainPayload { beam_target, hop, visited }`.
- Produces: `Hitbox.depth: u8`; `HitConfirmed { …, position: Vec3, depth: u8 }`;
  `HitboxEnded { …, depth: u8 }`; `ChainPayload.depth: u8` (default 0). Tasks 6–9 consume.

- [ ] **Step 1: Write the failing test** (in `tests/end_events.rs`, using the existing
  `EventRecorder` harness):

```rust
#[test]
fn hit_and_end_events_carry_position_and_depth() {
    let mut app = harness_with_projectile_skill(); // existing helper pattern in this file
    run_until_hit(&mut app);
    let rec = app.world().resource::<EventRecorder>();
    let hit = rec.hits_confirmed.last().expect("a hit");
    assert!(hit.position.length() > 0.0, "hit carries the hitbox position");
    assert_eq!(hit.depth, 0, "a player cast is depth 0");
    let ended = rec.hitbox_ended.last().expect("an end");
    assert_eq!(ended.depth, 0);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test end_events hit_and_end_events` → compile FAIL (missing fields).

- [ ] **Step 3: Implement** — add the fields; `spawn_window_hitbox` copies
  `payload.depth` onto the `Hitbox`; `detect_overlaps` and `resolve_beam_hits` fill
  `position: hitbox_transform.translation()` and `depth: hitbox.depth` when triggering
  `HitConfirmed`; `end_hitboxes` copies `hb.depth` into `HitboxEnded`. Update every struct
  literal the compiler flags (testkit recorder included). `ChainPayload::default()` gives
  `depth: 0`.

- [ ] **Step 4: Suite + goldens green** — `cargo test 2>&1 | grep -E "FAILED|test result"` → all ok (new fields don't alter trace output).

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(events): position + trigger depth ride Hitbox/HitConfirmed/HitboxEnded (spec §3.2 payload plumbing)"`

---

### Task 4: Widen the resolve seam

**Files:**
- Modify: `~/src/obelisk-bevy/src/combat/resolve.rs` (HitOutcome + resolve_one_hit_charged)
- Test: `~/src/obelisk-bevy/src/combat/resolve.rs` (inline tests exist — extend)

**Interfaces:**
- Produces on `HitOutcome`: `primary_packet: stat_core::DamagePacket`,
  `attacker_before: StatBlock`, `defender_before: StatBlock` (pre-hit snapshots; the fn
  already snapshots the target — expose both). Task 5/6 consume these for obelisk-side
  `TriggerConditionEval` with correct phase semantics (pre-calc against pre-mutation state,
  `DamageOverThreshold` against the pre-mitigation packet).

- [ ] **Step 1: Failing test**

```rust
#[test]
fn hit_outcome_exposes_packet_and_pre_hit_snapshots() {
    let (mut atk, mut def) = two_blocks(); // existing test helper in this module
    let skill = fixture_skill_basic();
    let mut rng = ChaCha8Rng::seed_from_u64(0);
    let life_before = def.current_life;
    let out = resolve_one_hit_charged(&mut atk, &mut def, &skill, &registry(), &mut rng, None).unwrap();
    assert!(out.primary_packet.damages.iter().map(|d| d.amount).sum::<f64>() > 0.0);
    assert_eq!(out.defender_before.current_life, life_before, "snapshot is PRE-hit");
}
```

- [ ] **Step 2: Run** — `cargo test -p obelisk-bevy resolve::` → compile FAIL.

- [ ] **Step 3: Implement** — clone attacker+defender at fn entry into the outcome; keep
  the primary `DamagePacket` (it is currently consumed — retain a clone taken before
  `resolve_damage_with_triggers`). No behavioral change to any resolution math.

- [ ] **Step 4: Goldens byte-identical** — `cargo test --test golden` → PASS untouched (this is the spec's named highest-risk step; a golden diff here is a bug, not churn).

- [ ] **Step 5: Commit** — `git commit -am "feat(resolve): HitOutcome exposes primary packet + pre-hit snapshots (spec §3.2 seam widening)"`

---

### Task 5: Free sub-cast resolution (mana zeroing for depth > 0)

**Files:**
- Modify: `~/src/obelisk-bevy/src/combat/system.rs` (on_hit_confirmed)
- Test: `~/src/obelisk-bevy/tests/end_events.rs` or new `tests/triggered_exec.rs`

**Interfaces:**
- Consumes: `HitConfirmed.depth` (Task 3), `Hitbox.hop` (existing).
- Produces: the **billing rule** (spec §3.2): mana bills per-hit only for the cast's
  scheduled windows. `fn is_free_hit(ev: &HitConfirmed) -> bool { ev.depth > 0 || ev.hop > 0 }`
  (Task 11 extends it with `|| ev.emitted`; `HitConfirmed` gains `hop: u8` copied from the
  hitbox in this task) — free hits resolve against a `Skill` clone with `mana_cost = 0.0`
  (helper `fn free_clone(skill: &Skill) -> Skill`). No `Cooldowns::start` for sub-casts
  (cooldowns start at cast time in `advance.rs` — sub-casts never pass through there;
  assert it in the test).

- [ ] **Step 1: Failing test** (`tests/triggered_exec.rs`, new file using the end_events harness pattern):

```rust
#[test]
fn depth_gt_zero_hits_do_not_bill_or_fizzle_on_mana() {
    let mut app = harness();
    drain_caster_mana(&mut app); // set current_mana = 0 on the caster's Attributes
    // Manually trigger a HitConfirmed with depth: 1 against the dummy.
    fire_hit_confirmed(&mut app, depth: 1);
    let rec = app.world().resource::<EventRecorder>();
    assert!(!rec.damage_resolved.is_empty(), "zero mana must not fizzle a sub-cast hit");
    assert_eq!(caster_mana(&app), 0.0, "and nothing was billed");
}
```

- [ ] **Step 2: Run** → FAIL (today: `use_skill_against` errors on insufficient mana → early return, no damage).

- [ ] **Step 3: Implement** in `on_hit_confirmed`, before resolve:

```rust
let skill_for_resolve = if is_free_hit(&ev) { free_clone(skill) } else { skill.clone() };
```

with `fn free_clone(s: &Skill) -> Skill { let mut c = s.clone(); c.mana_cost = 0.0; c }`.
(The clone also becomes the site of Task 7's condition stripping — one clone, two edits.)

- [ ] **Step 4: Run + suite** → PASS; goldens untouched (no depth>0 hits exist in scenarios yet).

- [ ] **Step 5: Commit** — `git commit -am "feat(combat): depth>0 hits resolve mana-free (spec D4 free sub-casts)"`

---

### Task 6: Public timeline executor

**Files:**
- Create: `~/src/obelisk-bevy/src/timeline/triggered.rs`
- Modify: `~/src/obelisk-bevy/src/timeline/mod.rs` (module), `src/lib.rs` (system registration in `ObeliskSet::Advance`), `src/timeline/advance.rs` (make `spawn_window_hitbox` `pub(crate)`→callable from triggered.rs — it already is crate-visible; expose a pure `window_start_time(&PhaseDurations, &CollisionWindow) -> f32` helper)
- Test: `~/src/obelisk-bevy/tests/triggered_exec.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ExecPayload { pub position: Vec3, pub direction: Vec3,
      pub target: Option<Entity>, pub charge: Option<u8>, pub depth: u8 }
  /// Queue skill `skill_id`'s timeline to execute at the payload (free sub-cast).
  pub fn execute_skill_timeline(commands: &mut Commands, caster: Entity,
      skill_id: &str, payload: ExecPayload);
  pub const MAX_TRIGGER_DEPTH: u8 = 8;
  ```
  Internally spawns a `TriggeredExec` entity `{ caster, skill_id, payload, elapsed: f32,
  spawned: Vec<bool> }`; the new `advance_triggered_execs` system (in `ObeliskSet::Advance`)
  ticks `elapsed` by fixed delta and calls `spawn_window_hitbox` for each `Scheduled` window
  when `elapsed >= window_start_time(...)`, at `payload.position` (anchor resolution comes
  in Task 9's schema — until then windows spawn at the payload position directly), with
  `ChainPayload { depth: payload.depth, ..default() }`; despawns itself once all spawned.
  At `payload.depth >= MAX_TRIGGER_DEPTH`: `warn!` and do nothing (drop).
- Consumes: `CastTimelineHandles`/`Assets<CastTimeline>`, `spawn_window_hitbox`.

- [ ] **Step 1: Failing test**

```rust
#[test]
fn executor_spawns_windows_at_payload_honoring_offsets() {
    let mut app = harness_with_two_window_skill(); // window A offset 0.0, window B offset 0.5
    exec_timeline(&mut app, "test_skill", Vec3::new(3.0, 1.0, 0.0), depth: 1);
    app_step(&mut app, 1); // one fixed tick
    assert_eq!(hitbox_count(&app), 1, "window A immediate");
    let pos = hitbox_pos(&app, 0);
    assert!((pos - Vec3::new(3.0, 1.0, 0.0)).length() < 0.1, "spawned AT the payload");
    app_step_secs(&mut app, 0.6);
    assert_eq!(hitbox_count_total_seen(&app), 2, "window B after its offset");
}

#[test]
fn executor_drops_at_depth_cap_with_warning() {
    let mut app = harness_with_two_window_skill();
    exec_timeline(&mut app, "test_skill", Vec3::ZERO, depth: MAX_TRIGGER_DEPTH);
    app_step(&mut app, 5);
    assert_eq!(hitbox_count(&app), 0, "cap drops, never spawns");
}
```

- [ ] **Step 2: Run** → compile FAIL (`execute_skill_timeline` undefined).

- [ ] **Step 3: Implement** `triggered.rs` per the Produces block. `window_start_time` is
  extracted from the existing `advance_casts` spawn-time computation (pure fn + unit test
  asserting parity with the current inline math for all three phases).

- [ ] **Step 4: Run** → both PASS; suite green.

- [ ] **Step 5: Commit** — `git commit -am "feat(timeline): public triggered-timeline executor with depth cap (spec §3.2)"`

---

### Task 7: Hit-phase trigger integration (the fireball moment)

**Files:**
- Modify: `~/src/obelisk-bevy/src/combat/system.rs` (on_hit_confirmed: partition + strip + evaluate + execute)
- Modify: `~/src/obelisk-bevy/src/assets/mod.rs` (load-time validation: timeline-target hit conditions require `additional = true`)
- Test: `~/src/obelisk-bevy/tests/triggered_exec.rs`

**Interfaces:**
- Consumes: Task 4's widened `HitOutcome`, Task 5's `free_clone`, Task 6's executor.
- Produces: behavior — a hit by skill A whose condition names skill B: (a) B has a
  registered timeline → the condition is REMOVED from the clone stat_core resolves
  (no inline packet), evaluated obelisk-side (`TriggerConditionEval::evaluate_pre` against
  `attacker_before`/`defender_before`; `evaluate_post_calc` against `primary_packet`;
  `evaluate_post_resolution` against the outcome), and on match
  `execute_skill_timeline(B, ExecPayload { position: ev.position, depth: ev.depth + 1, … })`;
  (b) B has no timeline → untouched legacy path. `EveryNthHit` conditions targeting
  timeline skills are a load-time validation error (v1).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn hit_trigger_with_timeline_executes_spatially_not_as_packet() {
    let mut app = harness_with_fireball_pair(); // fireball: always->fireball_explosion additional=true; both timelines registered
    cast_and_run_until_hit(&mut app, "fireball");
    app_step_secs(&mut app, 0.5); // explosion window opens + resolves
    let rec = app.world().resource::<EventRecorder>();
    let ids: Vec<&str> = rec.damage_resolved.iter().map(|d| d.skill_id.as_str()).collect();
    assert!(ids.contains(&"fireball"), "bolt contact damage");
    assert!(ids.contains(&"fireball_explosion"), "explosion resolved via ITS OWN timeline");
    assert_eq!(ids.iter().filter(|i| **i == "fireball_explosion").count(), 1,
        "exactly once — no double-fire from the inline packet path");
}

#[test]
fn zero_damage_carrier_still_triggers() {
    let mut app = harness_with_fireball_pair_zero_damage_bolt();
    cast_and_run_until_hit(&mut app, "fireball");
    app_step_secs(&mut app, 0.5);
    let rec = app.world().resource::<EventRecorder>();
    assert!(rec.damage_resolved.iter().any(|d| d.skill_id == "fireball_explosion"));
}

#[test]
fn timeline_target_condition_requires_additional_true() {
    let toml = fireball_toml_with(additional = false);
    let err = load_and_validate(toml);
    assert!(err.to_string().contains("additional"));
}
```

- [ ] **Step 2: Run** → FAIL (explosion resolves as inline packet against the victim today, or double-fires).

- [ ] **Step 3: Implement** the partition in `on_hit_confirmed` (before resolve): split
  `skill.conditions` on `CastTimelineHandles` membership of `trigger_skill`; strip the
  timeline-target set from the clone; post-resolve, evaluate each by its `phase()` against
  the widened outputs; matches → executor. Validation lives beside
  `validate_skill_trigger_references` usage (obelisk-bevy side, at skill-registry load —
  `core/config.rs::add_obelisk_skills`).

- [ ] **Step 4: Run** → PASS ×3; goldens untouched (fixture trigger skills have no timelines).

- [ ] **Step 5: Commit** — `git commit -am "feat(combat): timeline-target hit triggers execute spatially, stripped from inline resolve (spec §3.2 executor)"`

---

### Task 8: Lifecycle evaluation in the end funnel

**Files:**
- Modify: `~/src/obelisk-bevy/src/timeline/advance.rs` (`end_hitboxes`)
- Modify: `~/src/obelisk-bevy/src/assets/mod.rs` (validation: Lifecycle-target must have a timeline)
- Test: `~/src/obelisk-bevy/tests/triggered_exec.rs`

**Interfaces:**
- Consumes: Task 6 executor; loot_core `ConditionPhase::Lifecycle` (Task 2).
- Produces: `end_hitboxes` maps `EndReason::HitWorld → OnImpact`, `EndReason::Fuse →
  OnExpire`, scans the ending skill's Lifecycle conditions from `SkillRegistry`, and
  executes matches at the end position with `depth + 1`. `HitEntity` endings do nothing
  here (the hit path already ran). Load validation: a Lifecycle condition whose
  `trigger_skill` lacks a timeline is an error.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn world_impact_triggers_explosion_at_the_impact_point() {
    let mut app = harness_with_fireball_pair(); // fireball TOML has on_impact + on_expire conditions too
    cast_at_ground(&mut app, "fireball"); // aim below the dummy line — bolt grounds
    run_until_world_hit(&mut app);
    app_step_secs(&mut app, 0.3);
    let rec = app.world().resource::<EventRecorder>();
    let exp = rec.damage_resolved.iter().find(|d| d.skill_id == "fireball_explosion");
    assert!(exp.is_some(), "ground impact exploded");
    let end_pos = rec.hitbox_ended.last().unwrap().position;
    let win = rec.hit_windows.iter().find(|w| w.skill_id == "fireball_explosion").unwrap();
    assert!((win.position - end_pos).length() < 0.2, "AT the impact point");
}

#[test]
fn fuse_expiry_triggers_explosion() {
    let mut app = harness_with_fireball_pair();
    cast_into_empty_air(&mut app, "fireball");
    app_step_secs(&mut app, 2.5); // past the bolt fuse
    assert!(recorded_skill_hit_window(&app, "fireball_explosion"));
}
```

- [ ] **Step 2: Run** → FAIL (end funnel has no lifecycle hook).

- [ ] **Step 3: Implement** per Produces (registry lookup by `hb.skill_id`; conditions
  filtered `phase() == Lifecycle` and variant-matched to the reason).

- [ ] **Step 4: Run** → PASS; suite green.

- [ ] **Step 5: Commit** — `git commit -am "feat(timeline): lifecycle conditions (OnImpact/OnExpire) execute triggered timelines from the end funnel (spec §3.2)"`

---

### Task 9: Schema v2 — deletions, spawn roles, anchors, strikes

**Files:**
- Modify: `~/src/obelisk-bevy/src/assets/mod.rs` (the schema), `src/timeline/advance.rs` (consumers), `src/spatial/detect.rs` (`strikes` gate), `src/vfx.rs` (cue slots unchanged in ids, `on_end_*` kept)
- Modify: `~/src/obelisk-bevy/assets/skills/*.cast.ron` + `tests/fixtures/**/*.cast.ron` (migrate in-repo content)
- Test: `~/src/obelisk-bevy/src/assets/mod.rs` inline (validation), `tests/end_events.rs` (rewrite chain tests as trigger tests — they became Task 7/8 coverage)

**Interfaces:**
- Produces (`CollisionWindow` v2):
  ```rust
  pub enum WindowSpawn { Scheduled { phase: WindowPhase, offset: f32 }, Template }
  pub enum WindowAnchor { Caster, CastPoint }   // + offset: Vec3 on the window
  // CollisionWindow: spawn: WindowSpawn, anchor: WindowAnchor, anchor_offset: Vec3,
  //   strikes: bool (default true), …existing shape/motion/hit fields…
  ```
  DELETED: `OnEnd`, `EndReaction`, `WindowPhase::Chained`, `CastDelivery`,
  `CastTargeting` (acquisition arrives in Task 10 — this task leaves aim resolution
  reading `CastAim` exactly as today). `validate_timeline` v2: `Template` windows must be
  emitter-referenced (emitters land in Task 11 — until then a `Template` window is an
  error, keeping validation honest at every commit); `strikes: false` windows are skipped
  by `detect_overlaps`.
- `HitboxEnded` + `on_end_{id}` cues survive untouched (D6: events and cues outlive
  reactions).

- [ ] **Step 1: Failing round-trip + validation tests** (inline, following `assets/mod.rs`'s existing round-trip test):

```rust
#[test]
fn v2_window_round_trips() {
    let tl = timeline_with(CollisionWindow {
        spawn: WindowSpawn::Scheduled { phase: WindowPhase::Active, offset: 0.0 },
        anchor: WindowAnchor::CastPoint, anchor_offset: Vec3::new(0.0, 8.0, 0.0),
        strikes: false, ..basic_window("storm")
    });
    let s = ron::ser::to_string_pretty(&tl, Default::default()).unwrap();
    let back: CastTimeline = ron::from_str(&s).unwrap();
    assert_eq!(back.collision_windows[0].anchor, WindowAnchor::CastPoint);
}

#[test]
fn non_striking_windows_never_hit() { /* zone with strikes:false over a dummy → no HitConfirmed after 1s */ }

#[test]
fn old_chain_schema_fails_loud() {
    assert!(ron::from_str::<CastTimeline>(OLD_FIREBOLT_V1_RON).is_err(),
        "v1 on_end/Chained content must not silently half-parse");
}
```

- [ ] **Step 2: Run** → compile FAIL.

- [ ] **Step 3: Implement** the schema; delete the `Chain`/`Retarget` execution arms from
  `end_hitboxes` (the funnel now: despawn + `HitboxEnded` + cue + Task 8's lifecycle hook);
  fix every struct literal the compiler flags.

- [ ] **Step 4: Migrate in-repo content** — every `assets/skills/*.cast.ron` and test
  fixture: `spawn_phase: Active` → `spawn: Scheduled(phase: Active, offset: X)` (offset
  from the old `spawn_offset` field), drop `on_end`/`targeting`/`delivery` blocks; the
  firebolt example becomes the fireball pair (bolt TOML gains the three trigger conditions;
  `blast` window moves into `fireball_explosion.cast.ron` with `anchor: CastPoint`);
  chain_lightning drops its `hop` window + retarget block (Task 12 restores hop behavior
  from rules).

- [ ] **Step 5: Goldens — deliberate regeneration** for scenarios exercising migrated
  content: `UPDATE_GOLDEN=1 cargo test --test golden`, then `git diff tests/golden/` and
  verify every changed trace is explained by the migration (chain blast damage now arrives
  via `fireball_explosion` skill id; counts unchanged). All other traces byte-identical.

- [ ] **Step 6: Commit** — `git commit -am "feat!(schema): CastTimeline v2 — spawn roles/anchors/strikes; Chain/Retarget/CastDelivery/CastTargeting deleted; content migrated (spec D6, §3.2)" `

---

### Task 10: Acquisition (authored aim resolution + point preservation)

**Files:**
- Modify: `~/src/obelisk-bevy/src/assets/mod.rs` (Acquisition schema), `src/timeline/cast.rs` + `src/timeline/advance.rs` (validate_casts consumes it; `CastAim::Point` preserved into `ActiveCast.cast_point: Option<Vec3>`; `WindowAnchor::CastPoint` resolves to it)
- Test: `tests/acquisition.rs` (new)

**Interfaces:**
- Produces:
  ```rust
  pub enum Acquisition {
      Aim,
      SelfPoint,
      HitscanEntity { range: f32, filter: HitFilter, fallback: AcqFallback },
      GroundPoint { range: f32, fallback: AcqFallback },
  }
  pub enum AcqFallback { Fizzle, Then(Box<Acquisition>) }
  ```
  `CastTimeline.acquisition: Acquisition` (default `Aim`). Contract: the HOST resolves raw
  input to a `CastAim` (unchanged verbs); `validate_casts` checks the resolved aim SATISFIES
  the authored acquisition (entity aim for `HitscanEntity`, point for `GroundPoint`) and
  applies fallbacks (`GroundPoint` unmet + `Then(SelfPoint)` → cast point = caster
  position; `Fizzle` → paid rejection, existing `CastRejected` path). `cast_point` =
  resolved point (`SelfPoint` → caster pos; `Aim`/`HitscanEntity` → target/impact pos not
  required in v1 — `CastPoint`-anchored windows in an `Aim` skill are a validation error).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn ground_point_is_preserved_to_cast_point_anchored_windows() {
    let mut app = harness_with_blizzard_stub(); // GroundPoint acq; storm window anchor CastPoint+(0,8,0), strikes:false
    cast_at_point(&mut app, "blizzard", Vec3::new(5.0, 0.0, 3.0));
    app_step(&mut app, 2);
    let w = first_hitbox_pos(&app);
    assert!((w - Vec3::new(5.0, 8.0, 3.0)).length() < 0.1, "storm ABOVE the point (not collapsed to a direction)");
}

#[test]
fn ground_point_falls_back_to_self_point() {
    let mut app = harness_with_blizzard_stub();
    cast_skill_dir(&mut app, "blizzard", Dir3::X); // no point provided
    app_step(&mut app, 2);
    let caster = caster_pos(&app);
    assert!((first_hitbox_pos(&app) - (caster + Vec3::Y * 8.0)).length() < 0.2, "storm above the CASTER");
}

#[test]
fn hitscan_fizzle_is_a_paid_rejection() { /* no entity aim + Fizzle → CastRejected recorded, mana spent per existing paid-fizzle semantics */ }

#[test]
fn cast_point_anchor_in_aim_skill_fails_validation() { /* validate_timeline error */ }
```

- [ ] **Step 2: Run** → compile FAIL.

- [ ] **Step 3: Implement** per Produces (this is where `advance.rs:118`'s point-collapse
  dies: `ActiveCast` carries `cast_point`, window spawning resolves `WindowAnchor`).

- [ ] **Step 4: Run + suite + goldens** (existing scenarios author no acquisition → default `Aim`, zero trace change).

- [ ] **Step 5: Commit** — `git commit -am "feat(timeline): authored acquisition with fallbacks; CastAim::Point preserved to CastPoint anchors (spec §3.2, blizzard blocker)"`

---

### Task 11: Emitters + SpawnRng + motion override

**Files:**
- Modify: `~/src/obelisk-bevy/src/assets/mod.rs` (`emitter: Option<Emitter>` on windows; `MotionDirection` override; validation: emitter target exists + is `Template`, `Template` is referenced)
- Create: `~/src/obelisk-bevy/src/core/spawn_rng.rs` (`SpawnRng(pub ChaCha8Rng)` resource, seeded by the existing `seed_combat_rng` ext alongside `CombatRng` from `seed ^ 0x5EED_5EED`)
- Modify: `~/src/obelisk-bevy/src/timeline/advance.rs` (emitter tick in the hitbox advance path)
- Modify: `~/src/obelisk-bevy/src/events.rs` + `src/vfx.rs` (cue slot `emit_{window_id}` → `"{skill}_emit_{id}"`, fired at each spawn)
- Test: `tests/emitters.rs` (new)

**Interfaces:**
- Produces:
  ```rust
  pub struct Emitter { pub rate: f32, pub jitter: f32, pub window: String }
  pub enum MotionDirection { Inherit, Down }   // on VolumeMotion-bearing windows
  ```
  A live hitbox whose window has an emitter spawns one instance of the named `Template`
  window every `1/rate` seconds at `hitbox_pos + jitter_offset` (xz disc sample from
  `SpawnRng`), depth inherited, `Hitbox.emitted = true` (extends Task 5's `is_free_hit`
  with `|| ev.emitted` — shard hits never bill mana). Emitted instances fire the
  `emit_{id}` cue ONLY (never `window_open`). Validation additions: emitter target exists
  and is `Template`; `Template` windows are emitter-referenced; **`Template` windows may
  not themselves carry emitters** (spec §3.2's Template→Template recursion guard — test:
  a self-emitting template fails `validate_timeline`).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn emitter_rains_template_windows_deterministically() {
    let mut app = harness_with_blizzard(); // storm rate 4.0, jitter 2.0, shard Template, Down motion
    cast_at_point(&mut app, "blizzard", Vec3::new(5.0, 0.0, 3.0));
    app_step_secs(&mut app, 1.05);
    assert_eq!(shard_spawn_count(&app), 4, "rate honored");
    let positions_a = shard_positions(&app);
    let positions_b = shard_positions(&rerun_same_seed());
    assert_eq!(positions_a, positions_b, "SpawnRng deterministic");
    assert!(positions_a.iter().all(|p| (p.xz() - Vec2::new(5.0, 3.0)).length() <= 2.01), "jitter bounded");
}

#[test]
fn spawn_rng_does_not_perturb_combat_rng() {
    // Golden guard in miniature: identical DamageResolved totals with/without an emitter skill also active.
}
```

- [ ] **Step 2: Run** → compile FAIL.

- [ ] **Step 3: Implement**; shards get `MotionDirection::Down` (velocity = `-Y * speed`).

- [ ] **Step 4: Run + full goldens byte-identical** (no existing scenario has an emitter; `SpawnRng` draws only inside emitter ticks).

- [ ] **Step 5: Commit** — `git commit -am "feat(timeline): emitters spawn Template windows on a dedicated SpawnRng stream; Down motion; emit cues (spec §3.2)"`

---

### Task 12: Chain-from-rules

**Files:**
- Modify: `~/src/obelisk-bevy/src/timeline/advance.rs` (re-key the retarget search: trigger = hit by a `can_chain` skill with hops remaining, not an authored reaction), `src/assets/mod.rs` (`chain_radius: f32` default 6.0 on `CastTimeline`)
- Modify: `~/src/obelisk-bevy/assets/skills/chain_lightning.cast.ron` + its rules fixture (add `can_chain = true, chain_count = 3`)
- Test: `tests/beam_retarget.rs` (rewrite assertions to the rules-driven shape — hop order, visited exclusion, determinism, charge inheritance tests all carry over)

**Interfaces:**
- Consumes: `Hitbox.hop`/`visited` (existing), stat_core `DamageConfig.can_chain`/`chain_count`.
- Produces: on `HitConfirmed` for a beam hitbox whose skill has `can_chain` and
  `hb.hop < chain_count`: nearest-unvisited search (existing deterministic tie-break)
  within `chain_radius` → `spawn_window_hitbox` re-strikes the same beam window at the
  found target with `hop + 1`. No authored retarget block anywhere.

- [ ] **Step 1: Port the failing tests** (beam_retarget.rs assertions against the new authoring: same expected hop sequence, now driven by `chain_count = 3`).

- [ ] **Step 2: Run** → FAIL (retarget arm was deleted in Task 9; hops don't happen).

- [ ] **Step 3: Implement** per Produces (the search fn `nearest_retarget_candidate` survives unchanged).

- [ ] **Step 4: Run + goldens** — the chain_lightning trace regenerates deliberately:
  hops (`hop > 0`) now resolve mana-free per the spec's billing rule (previously each hop
  billed per-hit), so `mana_spent` on hop DamageResolved lines drops to 0; damage totals
  must be identical. Explain both in the commit message.

- [ ] **Step 5: Commit** — `git commit -am "feat(spatial): chain hops driven by rules can_chain/chain_count within behavior chain_radius (spec D5)"`

---

### Task 13: New golden scenarios + presentation map schema + hygiene

**Files:**
- Modify: `~/src/obelisk-bevy/src/scenario/library.rs` (new scenarios: `triggered_hit_explosion`, `triggered_world_impact`, `triggered_fuse_expiry`, `trigger_depth_cap_terminates`, `zero_damage_carrier_triggers`, `chain_from_rules`, `blizzard_emitter`, `acquisition_fallbacks`, `charged_vs_uncharged_fireball`)
- Modify: `~/src/obelisk-bevy/src/assets/mod.rs` (the `cues:` presentation map — pure data: `HashMap<String, CueBinding>` with `CueBinding { effect: Option<String>, attach: CueAttach (World|Follow), anim: Option<String>, params: Vec<(String, ParamSource)> }`; `ParamSource::Charge` only in v1; round-trip test; sim never reads it), plus `chargeable: bool` + `max_hold: f32` fields
- Modify: `~/src/obelisk-bevy/src/lib.rs:1-3` + `CLAUDE.md` (0.18/0.5 doc fix), `src/timeline/advance.rs:289` (dead `registry` param)
- Test: `tests/golden/*.trace` (new traces, committed after review)

**Interfaces:**
- Produces: the frozen v2 file format phase 3 (the Skill mode) builds against; the golden
  suite that phase 3/4 regressions are measured by.

- [ ] **Step 1: Add scenarios + presentation schema with round-trip test** (schema is data-only — test is serde round-trip + "sim ignores unknown preset names without panicking": run a scenario whose cues name a nonexistent effect, assert completion + a single warn).

- [ ] **Step 2: Generate + review the new traces** — `UPDATE_GOLDEN=1 cargo test --test golden`, then read each new `.trace`: `triggered_hit_explosion` must show bolt `DamageResolved(fireball)` then `HitWindowOpened(fireball_explosion)` then `DamageResolved(fireball_explosion)`; `trigger_depth_cap_terminates` must END (bounded line count).

- [ ] **Step 3: Full suite + clippy** — `cargo test && cargo clippy --all-targets -- -D warnings` → green.

- [ ] **Step 4: Commit** — `git commit -am "test(golden): trigger-reform scenario coverage; cue-binding schema; doc hygiene (spec §3.2, §4)"`

- [ ] **Step 5: Wrap phase 2** — push `trigger-reform`; confirm `~/src/obelisk-arena` still
  builds against its OLD pinned obelisk-bevy rev (spec §5: arena stays on the pre-reform
  pin until phase 4): `cd ~/src/obelisk-arena && cargo build 2>&1 | tail -1` → unchanged.
  Report: phase 3 (bevy_modal_editor) is unblocked and gets its own plan.

---

## Self-Review (performed at write time)

- **Spec coverage:** D1 (Task 1), D3/§3.1 (Task 2), payload plumbing (Task 3), seam
  widening (Task 4), free sub-casts (Task 5), executor + depth cap (Task 6), hit
  integration + additional-validation + zero-damage carrier (Task 7), lifecycle (Task 8),
  schema v2 deletions/anchors/roles/strikes + migration (Task 9), acquisition + point
  preservation (Task 10), emitters/SpawnRng/Down (Task 11), chain-from-rules (Task 12),
  goldens + cue-binding schema + `chargeable`/`max_hold` + hygiene (Task 13). Not in this
  plan by design: everything §3.3/§3.4 (phases 3–4, separate plans); rules-side charge
  scaling (spec ratifies sim-side v1).
- **Placeholder scan:** the harness helpers named in tests (`harness_with_fireball_pair`,
  `cast_at_point`, `app_step_secs`, …) are the existing test-utility idiom in
  `tests/end_events.rs`/`beam_retarget.rs` — each new helper is defined in its task's test
  file as a thin wrapper over `ObeliskTestApp`; no TBDs remain.
- **Type consistency:** `ExecPayload`/`MAX_TRIGGER_DEPTH` (Task 6) are the names Tasks 7–8
  consume; `WindowSpawn`/`WindowAnchor`/`strikes` (Task 9) are what Tasks 10–11 extend;
  `depth` field name is uniform across Hitbox/HitConfirmed/HitboxEnded/ChainPayload/ExecPayload.
