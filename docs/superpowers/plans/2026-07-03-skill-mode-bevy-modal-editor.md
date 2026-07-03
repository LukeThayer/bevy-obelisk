# Skill Mode in bevy_modal_editor (Sub-project 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `bevy_modal_editor` gains a built-in, feature-gated **Skill mode** that authors the
obelisk skill triad (rules TOML + behavior RON + cue bindings) in the editor's native idioms,
with the effect runtime extracted into a reusable `bevy_effect` crate and the deterministic
sim-backed preview (persistent stage + synchronous scrub) ported in.

**Architecture:** Head of the series: the `CueEvent` payload gap closes in obelisk-bevy
(additive). Then bevy_modal_editor work in dependency order: characterization tests → the
`bevy_effect` extraction (type-path-preserving, cycle-guarded, format-owning) → the
`obelisk`-feature mode skeleton (the ~6 hard-coded mode sites) → `SkillLibrary` + content
roots + palette → the three panel regions (Rules / Behavior / Presentation) → validation +
explicit save (toml_edit) → the preview stage and scrub ports (adapted to schema v2) →
relationship chips + stage-proxy gizmos → screenshot-verified polish.

**Tech Stack:** Rust, Bevy 0.18, bevy_egui, avian3d 0.5, obelisk-bevy main @ `d72fe9f`+
(schema v2), stat_core @ `bf9f026`, `toml_edit`, RON.

## Global Constraints

- Spec: obelisk-bevy `docs/superpowers/specs/2026-07-02-skill-editor-reimplementation-design.md` — §3.3 is this plan's contract; D2, D7, D8, D9, D10 bind. Follow-ups: `docs/superpowers/plans/2026-07-03-trigger-reform-followups.md` (tickets 1, 3, 5 land here).
- Repos: obelisk-bevy work in `~/src/obelisk-bevy` (branch `cue-payload` off main); bevy_modal_editor work in a WORKTREE `~/src/bevy_modal_editor-skill` (branch `skill-mode` off main) — the user's `~/src/bevy_modal_editor` checkout has local mods; NEVER touch it. Commit the user's dirty files? NO — worktree from committed main (6cd041d or current); their uncommitted Cargo.toml/lock changes stay theirs.
- The editor MUST build and test with AND without `--features obelisk` at every commit (the obelisk-bevy dep is SSH-keyed; the gate is real). CI-equivalent: `cargo build && cargo build --features obelisk && cargo test && cargo test --features obelisk` in the editor workspace.
- Editor UI panels are windowed-only (no headless egui): compile gates + unit-tested pure helpers + scripted screenshot probes are the test strategy (the arena_editor precedent). NOTHING SHIPS UNSEEN: every panel task ends with a screenshot probe reviewed by the implementer.
- Scene-persisted reflect types keep their type paths (`#[type_path]`) across the extraction — breaking saved scenes is a task failure.
- obelisk-bevy changes: golden traces byte-identical unless a task explicitly sanctions a delta with per-scenario explanation.
- Preview-substrate ports carry their invariants verbatim: cosmetic aging on SIM time vs reaping on RENDER frames (bevy_vfx command-race grace ladder: remove `VfxSystem`, wait two render frames, despawn); synchronous scrub = exclusive system running `world.run_schedule(FixedUpdate)`; sim-freeze via run-condition gate on the obelisk sets (editor-only, additive set config); deterministic restarts (CombatRng reseed, cooldown clear, mana refill).
- Source material to port (do not rewrite from scratch): obelisk-arena pinned @ `f6472e4`, `crates/arena_editor/src/{preview_controller,scrub,preview_cosmetics,preview_rig,socket,derived,edits,timeline_geom}.rs` and their tests — ported files must adapt to schema v2 (no `WindowPhase::Chained`, `spawn:`/`anchor:`/`strikes:`/acquisition, rules-driven chains).
- Editor pattern anchors (from the 2026-07-02 survey): mode sites — `src/editor/state.rs:21-73`, `src/editor/input.rs:40-280`, `src/ui/panels.rs:97-122,313-437`, `src/ui/mod.rs:66-105`, `src/editor/plugin.rs:182-284`; panel template `src/ui/ai_editor.rs:19-177`; library precedent `src/vfx/mod.rs:37-123,347-366`; palette `src/ui/command_palette/particle_preset.rs:41-164`; effects runtime `src/effects/{data,mod,presets}.rs`.

---

### Task 1: CueEvent payload — charge + EndReason (obelisk-bevy)

**Files:**
- Modify: `~/src/obelisk-bevy/src/events.rs` (CueEvent + CastBegan), `src/vfx.rs` (all cue observers fill the new fields), `src/combat/system.rs` + `src/timeline/advance.rs` (warn-throttle, ticket 5)
- Test: `~/src/obelisk-bevy/tests/end_events.rs` (extend), `src/vfx.rs` inline

**Interfaces:**
- Produces: `CueEvent { …, charge: Option<u8>, end_reason: Option<EndReason> }` (serde-defaulted
  where serialized); `CastBegan.charge: Option<u8>`. Every cue slot carries the cast's charge;
  `on_end_{id}` cues carry the reason. Tasks 9/11 consume (charge→param baking; reason-aware
  presentation later).
- Ticket 5: the `additional = false` and unsupported-condition warns become once-per-
  (skill, condition) via a `WarnedOnce(HashSet<(String, String)>)` resource.

- [ ] **Step 1: Failing test** — extend the end-events harness: full-charge cast (`cast_skill_dir_charged(…, 255)`); assert the recorded `on_cast` cue has `charge == Some(255)` and the `on_end_bolt` cue has `end_reason == Some(EndReason::HitWorld)` for a grounded bolt (host-fired world hit, existing pattern).
- [ ] **Step 2: Run** → compile FAIL (missing fields).
- [ ] **Step 3: Implement** — thread from the sources (`ActiveCast`/`Hitbox.charge`; `HitboxEnded.reason`); update every `CueEvent{}` literal (compiler-driven); observers in `vfx.rs` fill per-slot. Warn-throttle: check-and-insert before each `warn!` at the two sites.
- [ ] **Step 4: Full suite + goldens byte-identical** (traces don't record cue events — verify) + clippy.
- [ ] **Step 5: Commit** `feat(vfx): cue payloads carry charge + end reason; once-per-condition warns (phase-3 prerequisite)` on branch `cue-payload`; merge to main + push (small, self-contained).

---

### Task 2: Characterization tests for the effect runtime (bevy_modal_editor)

**Files:**
- Create: `~/src/bevy_modal_editor-skill/tests/effect_runtime_characterization.rs` (workspace root tests dir — check where integration tests live; if none, `src/effects/mod.rs` inline `#[cfg(test)]`)

**Interfaces:**
- Produces: the behavioral contract the extraction must preserve: for each `EffectTrigger`
  variant, a minimal `EffectMarker` advanced N frames with a hand-rolled `Time` produces the
  expected `EffectPlayback` state + spawned children (by tag). Cover: `AtTime` fires once at
  t; `AfterRule{delay}` chains; `RepeatingInterval{max_count}` stops; `OnSpawn` fires
  immediately; `SpawnParticle` clones from `VfxLibrary` by name (unknown name → default,
  today's semantics); nested `SpawnEffect` one level.

- [ ] **Step 1: Read `src/effects/mod.rs` + `data.rs`** — write the tests against TODAY's code (they must pass BEFORE the move).
- [ ] **Step 2: Run** → PASS on unmodified main (this is characterization, not TDD RED).
- [ ] **Step 3: Commit** `test(effects): characterization suite ahead of bevy_effect extraction`.

---

### Task 3: Extract `crates/bevy_effect`

**Files:**
- Create: `~/src/bevy_modal_editor-skill/crates/bevy_effect/` (Cargo.toml, `src/{lib,data,runtime,loader}.rs`)
- Modify: workspace `Cargo.toml` (member), `src/effects/mod.rs` (becomes editor-side shell: re-exports + `.fx.ron` auto-save + presets), `src/ui/effect_editor.rs` (imports), `src/scene/mod.rs` (allowlist import path)
- Test: moved characterization suite + new cycle-guard + type-path tests

**Interfaces:**
- Produces: `bevy_effect::{EffectMarker, EffectStep, EffectTrigger, EffectAction, EffectPlayback, PlaybackState, EffectChild, SpawnLocation, TweenProperty, EasingType, EffectPlugin}` — runtime systems (`advance_effects`, `execute_action`, tweens, collision feed) in the crate; **the crate owns the `.fx.ron` serde format and a `load_effects_from_dir(&mut EffectLibrary, &Path)` helper** (editor keeps auto-SAVE; games use the loader). `EffectLibrary` moves into the crate.
- Editor-scene deps move too: `PrimitiveShape` and `GltfSource` + its materializing systems relocate into `bevy_effect` (re-exported from their old paths) — OR, if `GltfSource`'s systems are inseparable from editor scene code, `bevy_effect` gains a `SpawnProvider` resource of fn pointers the editor installs (decide by reading; report which).
- **`#[type_path = "bevy_modal_editor::effects::data"]`** (exact old paths) on every scene-persisted moved type (`EffectMarker`, `GltfSource` if moved) — saved scenes keep loading.
- `SpawnEffect` recursion: depth guard cap 8, drop + `warn!`, unit-tested with a self-spawning preset; EXCLUDED from the characterization contract (documented).

- [ ] **Step 1:** Move data + runtime; keep `src/effects/mod.rs` as shell (presets, VFX_DIR-style auto-save, re-exports so editor code compiles with minimal churn).
- [ ] **Step 2:** Type-path test: serialize a scene containing an `EffectMarker` on MAIN (before the move — capture the RON string as a fixture in Task 2 if not already), deserialize it against the extracted crate → loads.
- [ ] **Step 3:** Characterization suite green against the crate; cycle-guard test; `cargo build --workspace && cargo test --workspace`.
- [ ] **Step 4: Commit** `refactor!: extract bevy_effect runtime crate (format + loader owned; type paths preserved; SpawnEffect cycle-guarded)`.

---

### Task 4: `obelisk` feature + EditorMode::Skill skeleton

**Files:**
- Modify: `~/src/bevy_modal_editor-skill/Cargo.toml` (`obelisk = ["dep:obelisk-bevy"]`, git dep on LukeThayer/bevy-obelisk main rev-pinned to current), `src/editor/state.rs` (enum variant + `panel_side` → `Some(PanelSide::Right)`), `src/editor/input.rs` (K-key block, from-View-or-Shift rule), `src/ui/panels.rs` (status-bar name "SKILL" + color + hints arm), `src/ui/mod.rs` (panel plugin registration), `src/editor/plugin.rs` (SkillModePlugin composition)
- Create: `src/skill/mod.rs` (feature-gated module; empty panel drawing the `ai_editor.rs` template shape with a placeholder label)
- Test: `src/skill/mod.rs` inline (mode registration compile-gated both ways)

**Interfaces:**
- Produces: `EditorMode::Skill` variant — **NOT feature-gated on the enum** (a gated enum variant poisons every exhaustive match for non-obelisk builds); the variant always exists, the K-key/panel/systems are gated (`#[cfg(feature = "obelisk")]`), and without the feature the variant is simply unreachable (document on the variant). All 6 sites updated.
- CI-equivalent gate: both feature states build + test at this and every later commit.

- [ ] **Step 1:** Failing check: `cargo build --features obelisk` with the enum variant referenced by the new module → compile FAIL until all match sites covered.
- [ ] **Step 2:** Implement the 6 sites + skeleton panel (exclusive system, `EditorState.ui_enabled` + mode/pin gating, `panel_frame`, RIGHT_TOP anchor, pin button — copy `ai_editor.rs:19-60` shape).
- [ ] **Step 3:** Both-feature builds + suites green; screenshot probe: scripted `next_mode.set(EditorMode::Skill)` at frame 90 + screenshot → panel visible with placeholder (probe pattern: temp `--skill-probe` arg system, removed before commit… keep it as a permanent `#[cfg(feature = "obelisk")]` debug arg — it's reused by every later panel task; document it).
- [ ] **Step 4: Commit** `feat(editor): EditorMode::Skill skeleton behind the obelisk feature (K key, right panel)`.

---

### Task 5: SkillLibrary + content roots + palette

**Files:**
- Create: `src/skill/library.rs`, `src/skill/templates.rs`
- Modify: `src/skill/mod.rs` (plugin), `src/ui/command_palette/mod.rs` (F-key routing arm for Skill mode), create `src/ui/command_palette/skill_preset.rs` (mirror `particle_preset.rs`)
- Test: `src/skill/library.rs` + `templates.rs` inline (pure fns: scan, template construction, back-reference check)

**Interfaces:**
- Produces:
  ```rust
  pub struct SkillEntry { pub rules: stat_core::Skill, pub timeline: obelisk_bevy::assets::CastTimeline,
      pub rules_path: PathBuf, pub timeline_path: PathBuf, pub dirty_rules: bool, pub dirty_timeline: bool,
      pub disk_hash: (u64, u64) }   // stale-check anchors (Task 8)
  pub struct SkillLibrary { pub skills: BTreeMap<String, SkillEntry>, pub roots: Vec<PathBuf>, pub open: Option<String> }
  pub trait RegisterObeliskContentExt { fn register_obelisk_content(&mut self, root: impl Into<PathBuf>) -> &mut Self; }
  ```
  A content root = dir with `config/skills/`, `assets/skills/`, `assets/effects/`, `assets/vfx/`
  subtrees; registration ALSO feeds `bevy_effect::load_effects_from_dir` + the existing vfx
  preset loading (one call, spec §3.3). First root = default write target. Scan at Startup
  (after PreStartup library inits); rescan on demand (palette entry "Rescan content").
  Templates: `strike/projectile/zone/beam` — playable v2 timelines + minimal rules TOML;
  starter Effect presets shipped under the editor's own assets so fresh installs aren't
  dangling (spec: templates reference only presets that exist — use built-in Vfx preset names
  via simple starter Effects created in `templates.rs` and inserted into `EffectLibrary` at
  registration if absent).
  Lifecycle ops as pure fns + palette entries: `duplicate_skill`, `rename_skill`,
  `delete_skill` — each runs `skills_referencing(id) -> Vec<String>` (scan all rules'
  `conditions[].trigger_skill`) and the UI confirms ("3 skills trigger this — really delete?").
- Consumes: obelisk-bevy loaders (`load_skills_dir` semantics via `stat_core::config::parse_skills`; `CastTimeline` RON serde).

- [ ] **Step 1: Failing tests** — `scan_root_pairs_rules_with_timelines` (fixture root with 2 skills, 1 rules-only → entry with blank timeline flagged), `template_projectile_validates` (each template passes `validate_timeline` + rules parse), `back_reference_check_finds_triggering_skills`.
- [ ] **Step 2:** Implement; palette lists library keys + "New Skill (template…)" rows (follow `particle_preset.rs:41-164`).
- [ ] **Step 3:** Suites both-features; screenshot probe: palette open in Skill mode showing entries.
- [ ] **Step 4: Commit** `feat(skill): SkillLibrary, content roots, palette + archetype templates`.

---

### Task 6: Rules region (task-first tiers + trigger cards)

**Files:**
- Create: `src/skill/panel/rules.rs`, `src/skill/readouts.rs`
- Modify: `src/skill/mod.rs` (panel dispatches regions)
- Test: `src/skill/readouts.rs` inline (pure math); rules.rs is windowed (compile gate + probe)

**Interfaces:**
- Produces: `draw_rules_region(ui, &mut SkillEntry, &SkillLibrary, &ValidationReport)` — tier 1:
  mana/cooldown drags, damage lines (add/remove/type/min/max), crit, effect applications
  (effect-id picker fed from `stat_core::config::effect_registry()` ids); **trigger cards**:
  one card per `SkillCondition` reading "WHEN <condition> → CAST <trigger_skill>", condition
  dropdown from loot_core's `EnumVariants`-style tables (incl. `on_impact`/`on_expire`),
  target picked from `SkillLibrary` keys via the palette (no free text), `additional`
  checkbox with the D4 rule surfaced (timeline targets force it true, editable only for
  packet targets); Advanced drawer: conversions, use_conditions, global plumbing (read-only
  summary v1 + "edit TOML" hint). Live readouts panel: `readouts.rs` ports arena_editor's
  `derived.rs` math ADAPTED to v2 (max strikes = scheduled windows + `chain_count` when
  `can_chain` + 1 per triggered skill with a timeline, one level deep; label triggered
  contributions "≈" — document the approximation).
- [ ] **Step 1:** Port + adapt `derived.rs` tests to v2 fixtures (fireball pair: 1 bolt + 1 triggered explosion = 2 strikes; chain_bolt: 1 + 3 hops = 4) — failing first against the new math.
- [ ] **Step 2:** Implement readouts + region; wire into the panel.
- [ ] **Step 3:** Both-feature suites; screenshot probe: fireball open, trigger card visible, readouts showing the pair total.
- [ ] **Step 4: Commit** `feat(skill): rules region — tiers, trigger cards, live readouts`.

---

### Task 7: Behavior region (acquisition + window cards + emitter sub-cards)

**Files:**
- Create: `src/skill/panel/behavior.rs`, `src/skill/edits.rs` (pure timeline mutations, ported/adapted from arena_editor `edits.rs`)
- Test: `src/skill/edits.rs` inline

**Interfaces:**
- Produces: `draw_behavior_region(ui, &mut SkillEntry, &ValidationReport)` — phases + `chargeable`/`max_hold`; acquisition card (variant dropdown with plain-language labels: "Free aim" / "Hitscan target" / "Ground point" / "Self", range drag, fallback chain editor: fallible variants show a fallback row, `Then(…)` nests one level with an "add fallback" affordance); one card per window: id, `spawn` (Scheduled phase+offset | Template), shape + params, motion + direction override, `anchor` + offset drags, `strikes` toggle with the carrier explanation, hit filter/mode/rehit, fuse; emitter sub-card (rate/jitter/target-window picker limited to Template windows). `edits.rs` pure fns: `add_window_from_template`, `add_emitter`, `remove_window` (with emitter-reference check), all unit-tested; every mutation flips `dirty_timeline`.
- [ ] **Step 1:** Failing tests for the pure fns (template window validates; removing an emitter-referenced Template is refused with a message).
- [ ] **Step 2:** Implement region; live `validate_timeline` on every frame's edited copy → inline error text on the offending card.
- [ ] **Step 3:** Both-feature suites; screenshot probe: blizzard open — storm card with emitter sub-card + shard Template card visible.
- [ ] **Step 4: Commit** `feat(skill): behavior region — acquisition, window cards, emitters`.

---

### Task 8: Explicit save + ValidationRegistry

**Files:**
- Create: `src/skill/save.rs`, `src/skill/validation.rs`
- Modify: `src/skill/mod.rs` (header Save button + dirty badges), workspace `Cargo.toml` (+`toml_edit`)
- Test: both new files inline (save round-trip on tempdirs; every validation rule positive+negative)

**Interfaces:**
- Produces: `save_skill(&mut SkillEntry) -> Result<(), SaveError>`:
  - rules via **`toml_edit`** — load the existing document, patch only the fields the editor
    owns (comments/order preserved; test: fixture TOML with comments round-trips with one
    field changed and comments intact);
  - timeline via `ron::ser::to_string_pretty` (comment loss accepted per D8 — doc'd);
  - **stale-check**: hash the on-disk bytes vs `disk_hash` captured at scan; mismatch →
    `SaveError::StaleDisk` surfaced as a reload-or-overwrite prompt in the UI; both branches
    implemented.
  `validation.rs`: `validate_skill(entry, &SkillLibrary, &EffectLibrary, &VfxLibrary, &AnimationLibrary) -> ValidationReport` — rules: dangling `trigger_skill`; Lifecycle-target
  missing timeline (BLOCKING); hit-target missing timeline (warning); timeline-target
  `additional == false` (blocking); `EveryNthHit` on timeline target (blocking); unknown
  Effect/anim preset names in cue bindings (blocking); acquisition fallback dead ends;
  **Template windows authoring non-default anchor/offset (follow-ups ticket 3 — warning)**;
  trigger-cycle depth walk (>8 → blocking). Save gated on no-blocking. The same fn registers
  into the editor's `ValidationRegistry` (`register_validation`, bevy_editor_game
  `lib.rs:834-874`) so issues surface pre-Play; report re-runs on library scan AND on
  Effect/Vfx library mutation (change-detection system).
- [ ] **Steps:** TDD per rule (fixtures per case) → implement → both-feature suites → probe (validation text on a broken skill) → **Commit** `feat(skill): validation-gated explicit save (toml_edit, stale-check) + ValidationRegistry rules`.

---

### Task 9: Presentation region (cue rows + pickers + charge params)

**Files:**
- Create: `src/skill/panel/presentation.rs`, `src/skill/cue_slots.rs` (pure slot enumeration)
- Test: `cue_slots.rs` inline

**Interfaces:**
- Produces: `cue_slots(timeline) -> Vec<CueSlot>` — timeline-ordered: `on_cast`, per window
  `on_window_{id}`/`on_end_{id}`, per emitter `emit_{id}`, `on_hit` — with each slot's LEGAL
  options per the normative table (`CastTimeline::cues` docs): attach on `on_window_*`/`emit_*`
  only; anim on `on_cast` only. `draw_presentation_region` renders one row per slot: Effect
  preset picker (`EffectLibrary` keys), attach dropdown where legal, anim picker
  (`AnimationLibrary` clips) marked "editor-only" where legal, charge→param binding rows
  (`ParamSource::Charge` → param name picked from the chosen Effect's Vfx params where
  discoverable, else free text with a validation warning). Writes `CastTimeline.cues`;
  jump-to-Effect-mode button per row (set mode + select/spawn the preset — the pin system
  makes the round-trip; follow `PinnedWindows` semantics).
- [ ] **Steps:** TDD `cue_slots` ordering/legality → implement region → both-feature suites → probe (fireball: on_cast + on_window_bolt rows with Follow shown on the bolt row only) → **Commit** `feat(skill): presentation region — cue rows, effect/anim pickers, charge params`.

---

### Task 10: Preview stage port (schema-v2-adapted)

**Files:**
- Create: `src/skill/preview/{mod,stage,rig,sockets,cosmetics}.rs` (ported from arena_editor `preview_controller.rs`, `preview_rig.rs`, `socket.rs`, `preview_cosmetics.rs` @ obelisk-arena `f6472e4`)
- Modify: `src/skill/mod.rs` (plugin adds preview when `obelisk`)
- Test: `tests/skill_preview.rs` (headless, ported `preview_play.rs` harness adapted)

**Interfaces:**
- Produces: persistent stage (caster + auto-synced dummies, NOT `SceneEntity`/`GameEntity`;
  editor Reset heals via `GameResetEvent`); **stage-provided acquisition resolution**
  (scripted: `HitscanEntity` → first dummy; `GroundPoint` → the stage aim marker; `Aim` →
  lofted ballistic dir at the dummy — port `preview_aim`) and **stage flat-floor
  `HitboxWorldHit` reporter** (port `report_ground_hits` from arena_sim — OnImpact must scrub
  in the bare editor); dummy auto-sync rules: `chain_count + 1` dummies within
  `chain_radius` for chain skills, a dummy under the aim point for `GroundPoint` skills,
  else one. Cue-driven cosmetics: `on_preview_cue` re-targeted to `CueEvent`'s new
  charge payload + `CastTimeline.cues` bindings → spawn `bevy_effect` presets (by name from
  `EffectLibrary`) / bevy_vfx presets, bake `ParamSource::Charge` via the VfxParam seam;
  anim rows drive `AnimationLibrary` clips on the stage rig (first consumer). Grace-ladder
  reaping + sim-time aging invariants carried verbatim (Global Constraints).
- Adaptations from v1 source (explicit): casting uses acquisition-resolved `CastAim` (no
  `CastTargeting`); no `Chained` handling; triggered sub-casts and emitters run in-sim with
  no editor code (they're obelisk-bevy now) — the port DELETES the v1 chain/beam staging
  special-cases and keeps only presentation.
- [ ] **Steps:** port harness tests first (cast → damage on dummy; GroundPoint skill → storm above marker; fireball → explosion `DamageResolved` appears — the sub-cast composes in preview) → port/adapt code → both-feature suites → probe (stage visible in Skill mode; Play fires fireball with explosion visuals) → **Commit** `feat(skill): deterministic preview stage (v2-adapted port)`.

---

### Task 11: Synchronous scrub port + charge slider + markers

**Files:**
- Create: `src/skill/preview/scrub.rs` (port of arena_editor `scrub.rs`), `src/skill/panel/strip.rs` (port of the strip painting from `panel.rs` + `timeline_geom.rs`)
- Test: `tests/skill_scrub.rs` (port of `scrub_preview.rs` determinism suite)

**Interfaces:**
- Produces: `ScrubSim` exactly as v1 (exclusive `drive_scrub`, `sim_unfrozen` gate configured
  on the obelisk sets — editor-only additive config; synchronous seek; deterministic
  restarts) + **strip extent = base timeline span + trailing sub-cast region** (dynamic end:
  extend while any `Hitbox`/`TriggeredExec` lives, capped at +10 s) + **event markers** on
  the strip (window opens, hits, ends, trigger firings — from the recorder events) +
  charge slider (0–255 → cast charge byte; the scrub cast and Play both use it).
- [ ] **Steps:** port determinism tests (freeze-before-hit, seek-past-resolves, backward-identical, replay-to-end) + NEW: `seek_past_impact_shows_the_explosion` (strip extends past base span; explosion window spawned) → port/adapt → both-feature suites → probe (frozen mid-arc bolt + marker row + charge slider) → **Commit** `feat(skill): sim-backed synchronous scrub with sub-cast extent + charge`.

---

### Task 12: Relationship chips + stage proxies

**Files:**
- Create: `src/skill/panel/chips.rs`, `src/skill/proxies.rs`
- Test: pure helpers inline (chip derivation from rules conditions; proxy param mapping)

**Interfaces:**
- Produces: chips row above the dock — `fireball → fireball_explosion` (one chip per
  timeline-target condition; `↺ ×N` chip for `can_chain`); click switches `SkillLibrary.open`
  (dirty-check prompt if unsaved). Stage proxies: selecting a window card spawns an ephemeral
  gizmo entity (never `SceneEntity`) — sphere/cone radius + anchor-offset drag writes back to
  the card's fields (follow the editor's gizmo idioms, `src/gizmos/`; a drag sets
  `dirty_timeline`).
- [ ] **Steps:** TDD helpers → implement → both-feature suites → probe (chips visible; dragging a radius gizmo changes the card value) → **Commit** `feat(skill): causality chips + gizmo-editable window proxies`.

---

### Task 13: Polish, docs, wrap

**Files:**
- Modify: editor `README`/docs table of modes; `src/skill/mod.rs` module docs; CLAUDE.md-equivalent note if present
- Test: full both-feature matrix; all screenshot probes re-run

**Interfaces:** none new.

- [ ] **Step 1:** Empty states (no content roots registered → panel explains `register_obelisk_content`; empty library → points at the palette).
- [ ] **Step 2:** Both-feature matrix green; every probe screenshot reviewed; clippy both ways.
- [ ] **Step 3:** Push `skill-mode`; **do NOT merge to bevy_modal_editor main without the user** (their repo, active drift) — present the branch.
- [ ] **Step 4:** Record phase-4 handoff notes (what arena_editor deletes, the cue wire contract inputs) in obelisk-bevy `docs/superpowers/plans/` as a stub for the phase-4 plan.

---

## Self-Review (performed at write time)

- **Spec §3.3 coverage:** extraction w/ scoped snags (T3), dead seams' first consumers (T9-T11), mode sites + feature gate (T4), SkillLibrary/roots/palette/templates/starter presets/lifecycle ops (T5), three regions (T6/T7/T9), chips (T12), proxies (T12), preview + scrub + stage resolvers + dummy rules (T10-T11), validation incl. re-run triggers + runtime never-panic (T8 + T10 cosmetics use warn-once skip), explicit save w/ toml_edit + stale-check (T8). Follow-up tickets: #1 (T1), #3 (T8), #5 (T1). Deliberately out: cue wire contract + arena thinning (phase 4); baked thumbnails/undo (spec P6-era, later).
- **Placeholder scan:** none; where a decision is deferred to reading real code (GltfSource move vs SpawnProvider), the task names both options and requires the report to state the choice.
- **Type consistency:** `SkillEntry`/`SkillLibrary` (T5) consumed by T6-T12 under those names; `cue_slots` (T9) matches the normative slot vocabulary from `CastTimeline::cues` docs; `ValidationReport` produced (T8) consumed by T6/T7 region signatures.
