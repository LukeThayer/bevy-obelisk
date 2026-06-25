# Obelisk-bevy → Bevy 0.18 / Avian 0.5 Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the `obelisk-bevy` crate from Bevy 0.17 / Avian 0.4 to Bevy 0.18 / Avian 0.5 with **zero behavior change** — all 39 golden traces byte-identical and all 84 tests green on the new versions — so it can compile in a workspace alongside `bevy_modal_editor` and `wisp`/lightyear (Phase 0 of the obelisk-arena design).

**Architecture:** This is a **preservation migration**, not a feature change. It adds **no new tests** and changes **no behavior**. The crate will not fully compile until most of the migration lands (Bevy's renames touch many modules), so the work is organized as **ordered compile-areas** with `cargo build` as the intermediate signal, and the **39-golden / 84-test suite is the final runtime gate** that proves behavior is unchanged. The surprise (verified in the migration map) is that almost the entire surface is *already* on the 0.18 idiom — the `Trigger→On` and `EventReader→MessageReader` renames were done ahead of time, and the SpatialQuery API is byte-identical 0.4→0.5. The genuinely load-bearing change is **one Avian edit** (`ColliderAabb::size()`→`half_size()`); everything else is verify-that-it-still-compiles plus the examples' render-API surface.

**Tech Stack:** Rust, Bevy 0.18, Avian3d 0.5, `ron`/`serde` data, ChaCha8 seeded RNG. Reference repos already on 0.18/0.5: `/Users/luke/src/wisp` and `/Users/luke/src/bevy_modal_editor`.

---

## Context every task needs (read before starting)

- **Migration map (the per-file survey this plan is built from):** `docs/superpowers/specs/2026-06-25-obelisk-018-migration-map.md`. Each task below references the relevant Area. When the compiler surfaces an error this plan didn't predict, the map's Area 1–5 "watch" lists are the first place to look, and `/Users/luke/src/wisp` + `/Users/luke/src/bevy_modal_editor` are the live 0.18/0.5 idiom references.
- **Design doc (umbrella):** `docs/superpowers/specs/2026-06-24-obelisk-arena-design.md` §6 defines this phase and its gate.
- **The gate = definition of done:** all 39 goldens byte-identical (no `UPDATE_GOLDEN`), all 84 tests green, `clippy -D warnings` + `fmt --check` clean, and lib + all three examples build on 0.18/0.5.
- **No worktree for this migration.** `obelisk-bevy/Cargo.toml` has **relative path deps** (`stat_core = { path = "../obelisk/stat_core" }`, etc.). A worktree at `obelisk-bevy/.worktrees/<branch>/` would resolve `../obelisk` to `obelisk-bevy/.worktrees/obelisk` (nonexistent) and fail to build. **Work in-place on a branch** in `/Users/luke/src/obelisk-bevy`.
- **The `feat/combat-result-trigger-metadata` carry.** That branch lives in the **sibling `../obelisk` repo** (not in obelisk-bevy). `../obelisk` is currently checked out on it (`feb0daa`), and obelisk-bevy's `src/combat/resolve.rs` reads the new `CombatResult` fields it adds. The carry rule: **keep `../obelisk` on `feat/combat-result-trigger-metadata` for the whole migration** (checking out `master` there would break obelisk-bevy's compile), and resolve its publish state in Task 8. The obelisk sibling crates are pure-Rust and Bevy-agnostic — they are **not** part of this migration.
- **Goldens are headless event traces, not images.** Gizmo/visual changes do **not** show up in any golden — the only detector for the one visually-subtle edit (Task 3) is the screenshot eyeball in Task 7.

## File Structure / migration surface

| Area | Files | Nature |
|---|---|---|
| Deps | `Cargo.toml` | bump `bevy 0.17→0.18`, `avian3d 0.4→0.5` |
| Observers `On<E>` | ~14 files (`testkit.rs`, `vfx.rs`, `loot.rs`, `present/mod.rs`, `present/debug_viz.rs`, `core/tick.rs`, `combat/system.rs`, `spatial/detect.rs`, `timeline/advance.rs`, `facade/combat.rs`, `core/cooldown.rs`, `verbs.rs`, `scenario/trace.rs`) | **verify-only** (already 0.18 idiom) |
| Required components | `core/components.rs` | **verify-only** |
| Messages | `net.rs`, `scenario/trace.rs`, `examples/headless_server.rs` | **verify-only** |
| App / schedule | `lib.rs`, example App builders | **verify-only** |
| Avian | `present/debug_viz.rs` (the 1 real edit), `spatial/{boxes,shapes,detect}.rs`, `scenario/mod.rs` (verify) | **1 real edit** + verify |
| Render / examples | `present/{mod,debug_viz}.rs`, `examples/{playground,screenshot,headless_server}.rs` | compile-break surface (esp. `screenshot.rs` `RenderTarget`) |

---

### Task 1: Pre-flight — branch, confirm green 0.17 baseline, pin the feat-branch carry

**Files:**
- No source changes. This task establishes the safety net: a branch to work on and a proven-green starting point.

- [ ] **Step 1: Confirm a clean obelisk-bevy tree on `main`**

Run: `cd /Users/luke/src/obelisk-bevy && git status -sb`
Expected: `## main...origin/main` with no modified/untracked files (clean).
If dirty: stop and report — do not migrate over uncommitted work.

- [ ] **Step 2: Confirm `../obelisk` is on the feat branch (the carry pin)**

Run: `git -C /Users/luke/src/obelisk branch --show-current`
Expected: `feat/combat-result-trigger-metadata`
If it shows `master` (or anything else): run `git -C /Users/luke/src/obelisk checkout feat/combat-result-trigger-metadata` and re-confirm. obelisk-bevy will not compile against `master` (missing `CombatResult` trigger fields). Leave it on the feat branch for the whole migration.

- [ ] **Step 3: Create the migration branch**

Run: `git checkout -b migrate/bevy-0.18-avian-0.5`
Expected: `Switched to a new branch 'migrate/bevy-0.18-avian-0.5'`

- [ ] **Step 4: Prove the 0.17 baseline is green BEFORE touching anything**

Run each and confirm green (this is the reference the migration must preserve):
```bash
cargo build
cargo test --features test-support --lib --tests
cargo test --features test-support --test golden
cargo clippy --features test-support --lib --tests -- -D warnings
cargo fmt --check
```
Expected: build OK; tests report **84 passed** (across lib + integration); golden test passes (39 traces match); clippy clean; fmt clean.
If any of these is red on 0.17: stop and report. The migration cannot start from a red baseline — you could not tell migration breakage from pre-existing breakage.

- [ ] **Step 5: Record the baseline golden checksum (so Task 6 can prove byte-identity)**

Run: `git rev-parse HEAD && shasum tests/golden/*.trace | shasum`
Expected: prints HEAD sha and a single aggregate checksum of all 39 traces. **Note both values in the task output** — Task 6 re-runs the aggregate checksum and it must be identical (the goldens are never edited by this migration).

No commit this task (branch creation + verification only).

---

### Task 2: Bump deps and compile the simulation (default + no-default-features)

**Files:**
- Modify: `Cargo.toml` (the two dependency lines)
- Possibly modify: any sim/observer/message/schedule file the 0.18 compiler flags (the map predicts **none** — these areas are already on the 0.18 idiom — but fix whatever errors surface)

- [ ] **Step 1: Bump the two dependency versions**

In `Cargo.toml`, under `[dependencies]`, change:
```toml
bevy = "0.17"
avian3d = "0.4"
```
to:
```toml
bevy = "0.18"
avian3d = "0.5"
```
Leave every other line unchanged. Do **not** touch the crate-local `[features]` table — `present`, `test-support`, `debug-gizmos` are obelisk-bevy's own feature names, unrelated to Bevy's 0.18 feature renames, and obelisk-bevy sets no explicit `features = [...]` on the `bevy` line (confirm with `grep 'features' Cargo.toml` — the `bevy`/`avian3d` lines have none).

- [ ] **Step 2: Pull the new crates and compile the default build**

Run: `cargo build`
Expected on first run: cargo fetches Bevy 0.18 + avian3d 0.5 (slow, one-time), then compiles. **If it compiles clean, the map's prediction held — skip to Step 4.**
If errors appear, they will be concentrated and shallow. Triage with the map:
  - Observer errors (`On<E>`, `.event()`, `add_observer`) → map Area 1. The only active 0.18 restriction is *exclusive systems can't be observers* — obelisk-bevy has none (its observers take `Commands`/`Res`/`Query`/`MessageWriter`, never `&mut World`). Reference spelling: `wisp/src/player/controller.rs:35`.
  - Required-component errors (`#[require(...)]` on `Combatant`) → map Area 2. Gameplay components are unaffected; reference: editor/wisp use `#[require]` throughout.
  - Message errors (`#[derive(Message)]`, `add_message::<NetEvent>()`, `MessageWriter`/`MessageReader`) → map Area 3. Reference: `bevy_modal_editor/src/ui/hierarchy.rs:135`, `wisp/src/spells/mod.rs`.
  - Schedule errors (`configure_sets`, `.chain()`, `.in_set()`) → map Area 4a. The crate uses no run-condition combinators and no `States`, so the 0.18 combinator/state silent changes don't apply.

- [ ] **Step 3: Fix each compile error using the map + reference repos, re-running `cargo build` after each fix**

For every error: apply the smallest change that matches the 0.18 idiom in `wisp`/`bevy_modal_editor`. Re-run `cargo build` until clean. Do not change behavior — these are spelling/signature fixes only.
Expected end state: `cargo build` exits 0.

- [ ] **Step 4: Compile the server build (presentation compiled out)**

Run: `cargo build --no-default-features`
Expected: exits 0. (This path excludes `present`; it exercises the sim + spatial + net modules.) Fix any errors the same way (map Areas 1–4a). Avian collider/RigidBody/SpatialQuery calls here are API-stable 0.4→0.5 (map Area 5a) and should not error.

- [ ] **Step 5: Run the canary tests early to catch a silent sim regression now**

Run: `cargo test --features test-support --lib spatial::detect`
Expected: passes (`spatial_query_finds_an_overlapping_hurtbox`, `hurtbox_tracks_a_moving_owner`, etc.). This is a cheap early read on whether Avian 0.5's broad-phase + static-body Transform tracking behave; a failure here predicts golden failures. If red, stop and treat as a behavior regression (see Task 5 escalation note) — do not proceed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock $(git diff --name-only)
git commit -m "migrate: bump to bevy 0.18 / avian 0.5, compile sim + server builds

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```
Expected: commit succeeds. If only `Cargo.toml`/`Cargo.lock` changed (no source edits needed), that confirms the map's verify-only prediction — commit them alone.

---

### Task 3: ~~The one real Avian edit — `ColliderAabb::size()` → `half_size()`~~ → VERIFIED NO-OP

> **✅ RESOLVED (2026-06-25): no edit needed.** Checked against the resolved avian3d **0.5.0**:
> `ColliderAabb::size()` still exists and is unchanged (full extent, same as 0.4); `half_size()` does
> not exist. `cargo build --features debug-gizmos` compiles **green as-is** with `size() * 0.5`, which
> is the correct radius math. avian 0.5 is a pure-compat release for this crate — `present/debug_viz.rs`
> needs zero Avian changes. The steps below are retained for the record but were **not executed** (the
> first step's "expected compile error" does not occur). No commit. Proceed to Task 4.

**Files:**
- ~~Modify: `src/present/debug_viz.rs:276-277`~~ — no change (verified no-op above)

**Why its own task:** this is the single change that can pass `cargo build` and `cargo test` while being *visually wrong*. In Avian 0.4 `ColliderAabb::size()` returns the **full** extent (the code multiplies by `0.5` to get a radius); in 0.5 `size()` is gone and `half_size()` returns the **half**-extent already. A mechanical rename that keeps the `* 0.5` halves the gizmo radius silently. The multiplier must be removed.

- [ ] **Step 1: Confirm the build currently fails on this call (`size()` removed in 0.5)**

Run: `cargo build --features debug-gizmos`
Expected: a compile error at `src/present/debug_viz.rs:277` — no method `size` on `ColliderAabb` (or similar). This is the expected, only Avian compile break.

- [ ] **Step 2: Apply the edit (switch to `half_size()` AND drop the `* 0.5`)**

In `src/present/debug_viz.rs`, change:
```rust
        let radius = aabb
            .map(|a| (a.size().max_element() * 0.5).max(0.05))
```
to:
```rust
        let radius = aabb
            .map(|a| a.half_size().max_element().max(0.05))
```
Leave the surrounding lines (the `aabb.map(|a| a.center())` fallback on the next line, the `gizmos.sphere(center, radius, ...)` call) unchanged — `center()` is identical in 0.5.

- [ ] **Step 3: Rebuild the debug-gizmos config**

Run: `cargo build --features debug-gizmos`
Expected: exits 0.

- [ ] **Step 4: Confirm the diff is exactly the documented one-line change**

Run: `git diff src/present/debug_viz.rs`
Expected: one line removed (`(a.size().max_element() * 0.5)`), one added (`a.half_size().max_element()`), nothing else. (The visual correctness — radius ≈ collider radius, not half — is verified in Task 7's screenshot; it cannot be checked by a test.)

- [ ] **Step 5: Commit**

```bash
git add src/present/debug_viz.rs
git commit -m "migrate(avian): ColliderAabb::size() -> half_size() (drop *0.5 to keep gizmo radius)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Build the examples (the render-API break surface)

**Files:**
- Possibly modify: `examples/screenshot.rs` (the `RenderTarget`/`Camera` wiring — most likely structural touch)
- Possibly modify: `examples/playground.rs`, `examples/headless_server.rs`, `src/present/{mod,debug_viz}.rs` (Text/HUD render APIs)

- [ ] **Step 1: Build the playground example**

Run: `cargo build --example playground --features debug-gizmos`
Expected: exits 0 (it shares the present/debug_viz code already built). Fix any `Text`/`TextFont`/`Node` errors per map Area 4b: 0.18 makes `LineHeight` a required component of `Text` (defaulted automatically when spawning `Text::new(...)`), and `TextFont` dropped its `line_height` field — obelisk-bevy spawns `TextFont { font_size, ..default() }` (no `line_height` set), so `..default()` should keep compiling. `Node`s set no `border_radius`, so the `BorderRadius`-removed-from-`Node` change does not apply.

- [ ] **Step 2: Build the screenshot example (the RenderTarget surface)**

Run: `cargo build --example screenshot --features debug-gizmos`
Expected: this is the **single most likely example to need a structural fix.** `examples/screenshot.rs` sets the off-screen target via `Camera { target: render_target, .. }` (line ~227) and `setup_render_target` returns `RenderTarget::Image(handle.into())` (line ~527), importing `camera::RenderTarget` (line ~39).
If it errors: 0.18 split `RenderTarget` off `Camera` into a standalone required component. Derive the exact new wiring from the compiler error plus the live references:
  - `wisp` / `bevy_modal_editor` camera spawns (grep `RenderTarget`, `Camera {` in both `src/` trees), and
  - Bevy 0.18's own `headless_renderer` example (the structure `screenshot.rs` was modeled on — check the installed Bevy 0.18 examples or bevyengine.org).
Apply the minimal change to make the off-screen camera target the image (e.g. spawning `RenderTarget` as its own component on the camera entity rather than as a `Camera` field, if that is the 0.18 shape). Keep the `ImageCopyPlugin`/`ImageCopier` render-graph path otherwise intact. Re-run until it exits 0.

- [ ] **Step 3: Build the headless_server example**

Run: `cargo build --example headless_server --no-default-features`
Expected: exits 0. It uses `MinimalPlugins` + `MeshPlugin`/`ScenePlugin` + `MessageReader<NetEvent>` drain (map Areas 3 + 4a) — fix any message/plugin-builder errors using the same references.

- [ ] **Step 4: Confirm the whole crate + all examples compile in every config**

Run:
```bash
cargo build
cargo build --no-default-features
cargo build --features debug-gizmos
cargo build --examples --features debug-gizmos
cargo build --example headless_server --no-default-features
```
Expected: all exit 0. After this step the crate is fully migrated at the compile level; the remaining tasks prove behavior.

- [ ] **Step 5: Commit (only if examples/present needed edits)**

```bash
git add examples/ src/present/
git commit -m "migrate: fix example render-API wiring for bevy 0.18 (RenderTarget/Text)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```
If no example needed changes (everything compiled clean), skip the commit and note that in the task output.

---

### Task 5: Spatial-determinism canary tests (the first real behavior gate)

**Files:**
- No source changes expected. This task **runs** the existing canary tests that prove the migration's top-3 silent-behavior risks did not fire.

- [ ] **Step 1: Run the SpatialQuery + static-body canaries**

Run: `cargo test --features test-support --lib spatial::detect`
Expected: all pass. These prove (a) Avian 0.5 `shape_intersections` returns the same candidate set (`spatial_query_finds_an_overlapping_hurtbox`) and (b) `RigidBody::Static` still re-reads Transform each step so hurtboxes follow moving owners (`hurtbox_tracks_a_moving_owner` — map risk #3).

- [ ] **Step 2: Run the facade ray/cone targeting tests**

Run: `cargo test --features test-support --test spatial_targeting`
Expected: all pass (confirms `facade/spatial.rs` `cast_ray` + cone target acquisition under 0.5).

- [ ] **Step 3: Run the determinism tests (RNG + dispatch order preserved)**

Run: `cargo test --features test-support --test determinism`
Expected: all pass (cross-seed divergence + same-seed idempotence — proves `CombatRng` ordering and observer/trigger dispatch order survived the 0.18 observer rearchitecture; map risk #5).

- [ ] **Step 4: If any canary is red, STOP and diagnose — do not regenerate anything**

A red canary means a real behavior change (the migration's true hazard). Localize it with the map's Area 8 risk list:
  - `spatial::detect` red → `shape_intersections` candidate-set changed (risk #1) or static-body tracking changed (risk #3).
  - `determinism` red → event/dispatch ordering changed (risk #5) or fixed-timestep rounding drifted (risk #6).
Report the specific failing test and the observed vs expected values. Do **not** edit goldens or canaries to make them pass — escalate; a behavior change here invalidates the migration's "zero behavior change" contract and needs a human decision.

No commit (no source change; this is a gate).

---

### Task 6: Full suite + 39 goldens byte-identical (the backbone gate)

**Files:**
- No source changes. The goldens must be **untouched** — proving them byte-identical is the whole point.

- [ ] **Step 1: Run the full 84-test suite**

Run: `cargo test --features test-support --lib --tests`
Expected: **84 passed, 0 failed** across lib + all integration tests (`facades.rs`, `netcode.rs`, `vertical_slice.rs`, `vfx_content.rs`, `spatial_targeting.rs`, `determinism.rs`, `golden.rs`).

- [ ] **Step 2: Run the 39-golden suite — must match WITHOUT regeneration**

Run: `cargo test --features test-support --test golden`
Expected: passes — all 39 traces match. The test reads `tests/golden/*.trace` and compares; it only rewrites them if `UPDATE_GOLDEN` is set, which it must **not** be here.

- [ ] **Step 3: Prove the golden files were never modified**

Run: `git status --porcelain tests/golden/ && shasum tests/golden/*.trace | shasum`
Expected: `git status` prints **nothing** (no golden file changed), and the aggregate checksum **equals the value recorded in Task 1 Step 5.** Byte-identity confirmed.

- [ ] **Step 4: If a golden diffs — investigate, never blind-regenerate**

A diff is a behavior change to justify, not to paper over. Run `cargo test --features test-support --test golden 2>&1 | head -60` to see the EXPECTED vs GOT lines (the `{:.3}`-precision trace makes a 1-tick or 1-damage shift visible). Map the diff to an Area 8 risk:
  - shifted `HitConfirmed`/`DamageResolved` → SpatialQuery candidate set (risk #1).
  - reordered net trace lines → `Messages<T>` drain order (risk #4).
  - off-by-one window timing → fixed-timestep rounding (risk #6) or intra-set ordering (risk #7).
Report the diff and the suspected cause. Only if a human confirms it is an *intended, unavoidable* numerical artifact of the engine bump may goldens be regenerated (`UPDATE_GOLDEN=1 cargo test … --test golden`, then `git diff tests/golden/` to review and record the rationale in the commit). For a pure plumbing migration the **expected diff count is zero.**

No commit if clean (nothing changed). If goldens were (human-approved) regenerated, commit them separately with the rationale.

---

### Task 7: Lint, format, and the screenshot eyeball (visual confirmation of the Avian edit)

**Files:**
- Possibly modify: any file clippy/fmt flags (formatting only — no behavior change)

- [ ] **Step 1: Clippy with warnings-as-errors**

Run: `cargo clippy --features test-support --lib --tests -- -D warnings`
Expected: clean. (Dead-code warnings originating in `stat_core` are the dependency's, not obelisk-bevy's — they are not gated by this command's `--lib --tests` on the obelisk-bevy crate; if clippy surfaces obelisk-bevy warnings, fix them minimally.)

- [ ] **Step 2: Format check**

Run: `cargo fmt --check`
Expected: clean. If it reports diffs, run `cargo fmt`, re-run `--check` to confirm clean, and include the formatting in the task's commit.

- [ ] **Step 3: Render the screenshot for the gizmo-size check (the only detector for the Task 3 multiplier trap)**

Run: `cargo run --example screenshot --features debug-gizmos -- --scenario firebolt_kill --tick 24`
Expected: writes `screenshots/firebolt_kill-24.png` (or the path the example prints). If the example's flag syntax differs, run `cargo run --example screenshot --features debug-gizmos -- --help` first and adapt.

- [ ] **Step 4: Read the screenshot and confirm hurtbox radius is correct**

Use the Read tool on the produced PNG. Confirm the hurtbox gizmo spheres render at approximately the collider radius (~0.5 world units), **not half that.** A visibly-small (half-radius) hurtbox means the `* 0.5` was not removed in Task 3 — go back and fix. (Goldens cannot catch this; this eyeball is the gate.)

- [ ] **Step 5: Commit any lint/fmt fixes**

```bash
git add -A
git commit -m "migrate: clippy/fmt clean on bevy 0.18

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```
If nothing needed fixing, skip the commit and note it.

---

### Task 8: Resolve the `feat/combat-result-trigger-metadata` publish state + finish

**Files:**
- No obelisk-bevy source changes. This task resolves the cross-repo dependency state and completes the branch.

- [ ] **Step 1: Re-run the full gate one last time to confirm done**

Run:
```bash
cargo build && cargo build --no-default-features && cargo build --features debug-gizmos
cargo test --features test-support --lib --tests
cargo test --features test-support --test golden
cargo clippy --features test-support --lib --tests -- -D warnings
cargo fmt --check
```
Expected: all green; goldens unchanged. This is the migration's definition-of-done.

- [ ] **Step 2: Surface the `../obelisk` feat-branch publish decision (do not auto-push)**

obelisk-bevy now depends, on this branch, on the `CombatResult` trigger metadata that lives only on `../obelisk`'s `feat/combat-result-trigger-metadata` branch (`feb0daa`, currently unmerged to `master`, unpushed). Present the options and let the human choose — pushing/merging another repo is outward-facing and was previously deferred:
  1. **Merge `feat/combat-result-trigger-metadata` → `master` locally in `../obelisk`** (stabilizes the path dep; no remote push).
  2. **Push the obelisk feat branch** (or the merged master) to its remote (only if the human wants obelisk-bevy buildable off a pushed obelisk).
  3. **Keep `../obelisk` on the feat branch as-is** (works for local dev; the carry rule — keep it checked out on that branch — continues).
Do not execute a merge/push without an explicit choice. Record the choice in the task output.

- [ ] **Step 3: Finish the migration branch**

Invoke the **superpowers:finishing-a-development-branch** skill. It will re-verify tests, then present the merge/PR/keep/discard options for `migrate/bevy-0.18-avian-0.5`. Default recommendation: merge to `main` once the gate is green, since Phase 1 (the obelisk-arena workspace) depends on a migrated obelisk-bevy on `main`.

---

## Self-Review

**1. Spec/map coverage** — every migration Area in the map maps to a task:
- Map Area 0 (Cargo bump) → Task 2 Step 1. ✓
- Map Areas 1–3 (observers/required-components/messages, verify-only) → Task 2 Steps 2–3 (triage list). ✓
- Map Area 4a (App/schedule) → Task 2 Steps 2–4. ✓
- Map Area 4b (present/UI/examples render) → Task 4. ✓
- Map Area 5b (the one Avian edit) → Task 3. ✓
- Map Area 5a/5c (collider/SpatialQuery verify) → Task 2 Step 5 + Task 5. ✓
- Map Area 6 (gate commands, ordered) → Tasks 5, 6, 7. ✓
- Map Area 8 risks #1/#3/#5 → Task 5; #4/#6/#7 → Task 6 Step 4; #2 (gizmo trap) → Task 3 + Task 7 Steps 3–4; #8 (screenshot RenderTarget) → Task 4 Step 2. ✓
- Design §6 feat-branch carry + publish → Task 1 Step 2 (pin) + Task 8 Step 2 (resolve). ✓
- Design §6 gate (39 goldens + 84 tests + clippy/fmt + 3 example builds) → Task 8 Step 1. ✓

**2. Placeholder scan** — no TBD/TODO/"handle edge cases". The one place exact new code is not pre-written is Task 4 Step 2 (`screenshot.rs` `RenderTarget`): this is deliberate and honest — the precise 0.18 form depends on the compiler error and is derived from named live references (wisp/editor + Bevy's `headless_renderer`), because the 0.18 `Camera`/`RenderTarget` split shape cannot be fabricated reliably. Every other code change (the Avian edit) has exact before/after.

**3. Type/command consistency** — gate commands are identical across Task 1 (baseline), Task 8 (final): same feature flags (`--features test-support`), same golden invocation (`--test golden`, no `UPDATE_GOLDEN`), same checksum method (Task 1 Step 5 ↔ Task 6 Step 3). The `debug-gizmos` build appears consistently for the gizmo path. The branch name `migrate/bevy-0.18-avian-0.5` is used identically in Task 1 Step 3 and Task 8 Step 3.

**4. Scope** — single crate, single plan, ends with a working migrated `obelisk-bevy`. Adds zero tests and zero behavior; preserves the existing 84-test/39-golden suite as the gate. Correct for a preservation migration (YAGNI).
