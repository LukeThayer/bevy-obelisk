# Skill Designer — Handoff (2026-07-01)

Handoff for the next session (Fable) to pick up the obelisk-arena in-editor **skill designer**. Everything through M1–M3 + the live-game cosmetic wiring is **done, verified, and merged to mainline**. **M4 (rules authoring)** is researched and awaiting the user's design decisions — that's where you pick up.

---

## TL;DR — where things stand

| Phase | Status | Where |
|---|---|---|
| **M1** Foundation (CastTimeline serialize, `arena_sim` extraction, editor `EditorMode::Custom` seam, `arena_editor` scaffold) | ✅ done+merged | obelisk-bevy `main`, obelisk-arena `master`, bevy_modal_editor `main` |
| **M2** Timeline UI + "Play the real skill" preview | ✅ done+merged | obelisk-arena `master` (arena_editor) |
| **M3** Cosmetic lanes (particle/projectile/anim + sockets + VfxParam) | ✅ done+merged | obelisk-arena `master` (arena_editor + arena_skills) |
| **Live-game cosmetic wiring** (authored `.skillfx.ron` effects render in the actual game via bevy_vfx) | ✅ done+merged | obelisk-arena `master` (arena_game) |
| **M4** Rules authoring (obelisk `Skill`/`Effect`/`Trigger` `.toml`) | 🔬 researched, **awaiting user decisions** | doc written; not started |

**Mainline heads (all local/unpushed, per the user's convention — never push without asking):**
- obelisk-bevy `main` @ `74ac384`
- obelisk-arena `master` @ `eb0e83e`
- bevy_modal_editor `main` @ `2f1d417`

**Stale feat branches** (fully merged into master; can be deleted or reused): obelisk-arena `feat/skill-designer`, `feat/live-cosmetics`; obelisk-bevy/bevy_modal_editor `feat/skill-designer`. Currently checked out: obelisk-arena `feat/live-cosmetics` (== master).

---

## What the skill designer IS (built)

A new **`arena_editor`** binary (obelisk-arena `crates/arena_editor`, built on `bevy_modal_editor`) where you author obelisk skills on a bottom-dock timeline and **Play the real skill** through the actual deterministic obelisk sim. A castable skill is 3 files keyed by the same id:
- `config/skills/<id>.toml` — obelisk **rules** (damage/effects/triggers/mana/cooldown/tags). **Hand-edited today — this is M4.**
- `assets/skills/<id>.cast.ron` — the **CastTimeline** (phases/windows/shapes/targeting/delivery). ✅ editor-authored (M2).
- `assets/skills/<id>.skillfx.ron` — **cosmetics** (particle/projectile/anim lanes, sockets, VfxParam bindings). ✅ editor-authored (M3), and now ✅ rendered in the live game.

Run it: `cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo run --bin arena-editor` (windowed; boots on Metal, press `K` for Skill mode). Core design spec: `docs/superpowers/specs/2026-06-30-skill-designer-design.md`. Implementation plan: `docs/superpowers/plans/2026-06-30-skill-designer-m1-m3-plan.md`.

---

## ⚠️ CRITICAL build-environment gotchas (hard-won — read before touching code)

These cost hours to discover. They are NOT in the plan (they were environment surprises):

1. **nix is UNUSABLE on this mac.** `bevy_modal_editor`'s nix devShell fails building `graphviz` on aarch64-darwin. Its CLAUDE.md says "run everything in `nix develop`" — **ignore that; use plain `cargo`** (there's a rustup toolchain, `cargo 1.93.1`).
2. **`arena_editor` is its OWN standalone workspace** (`crates/arena_editor/Cargo.toml` has an empty `[workspace]`; it's in obelisk-arena's `[workspace] exclude`). This isolates the editor's heavy bevy features (`dynamic_linking`, `file_watcher`, `bevy_remote`, …) so they can't feature-unify onto arena_game / break the net-test. **Build/test it via `cd crates/arena_editor && cargo build/test`, NEVER `-p arena_editor` from the obelisk-arena root.**
3. **`arena_editor` pins a `Cargo.lock`** (bevy 0.18.0 / bevy_egui 0.39 @ rev `81904da`). Reason: `bevy_egui`'s git `main` moved on to **Bevy 0.19** (needs rustc 1.95). A fresh resolve breaks; the committed lock keeps it at 0.18. If you ever `cargo update` there, re-pin.
4. **The full editor app CANNOT advance a frame headlessly** (egui/render/winit). `arena_editor::build_editor_app()` (in `src/lib.rs`) uses `DefaultPlugins` + **no window** + **`WinitPlugin` disabled** + a **real Metal backend**, and only **constructs** (assert build-time resources, no `.update()`). **Test skill-designer LOGIC on minimal apps or the `arena_sim` headless harness (see `crates/arena_sim/tests/preview_smoke.rs`), NEVER `build_editor_app().update()`.** The windowed binary (`main.rs`) uses full `DefaultPlugins` incl. winit + registers `PhysicsGizmos` (see `a40c476` — needed because `add_physics:false` skips Avian's debug plugin that registers that gizmo group).
5. **Cold editor/arena_editor builds (~3–4 min) exceed the subagent 600s stream watchdog** and silently kill agents. Pattern that works: **pre-warm the cold build in the main loop** (no watchdog), then run task agents against the warm target (incremental = fast).
6. **The net-test is FLAKY** (firebolt hit/miss on wall-clock; ~50% raw pass). Always **retry up to 3×** and treat one `session PASS` as green. Also `pkill -f arena-server; pkill -f arena-client` first (stray servers hold port). macOS ControlCenter holds *TCP* :5000; arena uses *UDP* :5000, so that's fine.
7. **fp_demo** in bevy_modal_editor hardcoded `/home/zach/src/bevy_locomotion` → it's `exclude`d from the editor workspace (commit `6b5fdd5`) so plain cargo resolves.

---

## HARD GATES (must stay green — the whole build respected these)

- **obelisk-bevy 39-golden suite byte-identical**: `cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --test golden` — must pass WITHOUT `UPDATE_GOLDEN`. (M4 shouldn't touch obelisk-bevy sim, but if it does, this gates it.)
- **arena_game net-test PASS**: `cd /Users/luke/src/obelisk-arena && pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh` (retry ≤3×). Proves the game still composes. M4 is arena_editor-only so it *shouldn't* affect this, but run it after any workspace/dep change.
- **Builds/tests clean**: plain cargo; the 12 pre-existing `stat_core` dependency dead-code warnings are ALLOWED. `arena_editor` has ~28 tests (all green).
- **Determinism sacred** (obelisk): seeded `CombatRng`; write avian `Position` not `Transform`; the editor/client never resolves combat.

---

## Code layout (what's where)

**`crates/arena_editor`** (standalone workspace) — the designer. Key modules:
- `lib.rs` (`build_editor_app` headless) · `main.rs` (windowed binary; adds EditorPlugin{add_physics:false}+GamePlugin+SkillDesignerPlugin+PreviewSimConfigPlugin+ArenaSimPreviewPlugin+PhysicsGizmos group).
- `skill_designer.rs` (`SkillDesignerPlugin`, `register_skill_mode` → `EditorMode::Custom("skill")`, key `K`; inits `EditedSkill`/`EditedSkillFx`/preview).
- `model.rs` (`EditedSkill`, `EditedSkillFx`, `blank_cast_timeline`, `derive_vfx_cues`, `blank_skillfx`) · `io.rs` (save/load `.cast.ron` + `.skillfx.ron`) · `sim_config.rs` (`PreviewSimConfigPlugin` loads obelisk config+skills+seeds RNG).
- `panel.rs` (`draw_skill_panel` — the bottom-dock egui timeline: bands + targeting/delivery ComboBoxes + windows lane + playhead + Save + cosmetic-lane rows) · `timeline_geom.rs`/`edits.rs`/`enum_ui.rs`/`fx_edits.rs` (pure, unit-tested helpers).
- `preview_controller.rs` (`PreviewControllerPlugin` — F4/GameStarted → spawn caster+dummy duel + cast via `cast_skill_dir` + register the edited timeline; determinism-tested) · `gizmo.rs` (hit-window viewport gizmo) · `preview_rig.rs` (character.glb rig + anim graph) · `socket.rs` (`RigSockets`/`resolve_socket`) · `vfx_bind.rs` (CPU-bake `VfxParamBinding` → bevy_vfx `EmitterDef`) · `preview_cosmetics.rs` (`on_preview_cue` observer → anim + vfx-at-socket).

**`crates/arena_sim`** (transport-agnostic shared sim, in obelisk-arena workspace) — obelisk composition + shared controller + spawn recipe + `ArenaSimPreviewPlugin`/`spawn_preview_world`. Used by BOTH arena_game and arena_editor. Tests in `tests/preview_smoke.rs` = the headless obelisk harness pattern.

**`crates/arena_skills`** — `SkillFx`/`LaneEvent` (extended additively: `ParticleSpec`/`ProjectileCosmetic`/`AnimLayer` sockets + `VfxParamBinding`/`VfxBindSource`; `Serialize` on `SkillFx`) + pure `normalize`/`modulate`/`resolve_binding`. **Stays bevy_vfx-free.**

**`crates/arena_game`** — the game. Now depends on `bevy_vfx` (windowed-only at runtime). `client/vfx_bind.rs` + `client/cosmetics.rs` (`spawn_cue_cosmetics` renders authored `.skillfx.ron` effects via bevy_vfx, billboard fallback) + `client/app_windowed.rs` (adds VfxPlugin + seeds VfxLibrary). Architecture: `crates/arena_game/CLAUDE.md`.

**`bevy_modal_editor`** — gained ONLY a generic seam (no obelisk/arena names): `EditorMode::Custom(CustomModeId)` + `CustomModeRegistry` + `register_editor_mode` + `PanelSide` export (`src/editor/custom_mode.rs`). PR-ready upstream.

---

## 🎯 NEXT: M4 rules authoring — status + what to do

**Research is DONE:** `docs/superpowers/research/2026-07-01-m4-rules-authoring-understanding.md` (empirically validated — a throwaway crate round-tripped 5 real skills + 6 effects). Headline findings:
- **Zero obelisk-core changes needed for serialization.** obelisk `Skill`/`Effect`/enums already derive `Serialize`+`Deserialize`. Read/write `config/skills/<id>.toml` (and `config/effects/<id>.toml`) directly via `toml` 0.8. toml 0.8 auto-reorders scalars before tables (the feared serialize error doesn't occur).
- **No Reflect** (stat_core is pure-Rust). Build a hand-crafted serde-driven egui form like M2's `draw_skill_panel`.
- **UI plan:** add a "Rules" surface to the EXISTING Skill mode (side panel or a Timeline|Rules|Cosmetics tab), mirroring the M2 triad — add `EditedRules { skill: stat_core::Skill, path, dirty }` resource + `io` save/load (toml) + `draw_rules_panel`. One Save writes all 3 files. Re-load `SkillRegistry` after Save so "Play the real skill" uses new rules.
- **Cargo:** `arena_editor` must add direct `stat_core = { path = "../../../obelisk/stat_core" }` + `toml = "0.8"` deps (today it only has `ron`, reaches stat_core transitively).
- **The hard parts:** (a) the **34-variant `TriggerCondition` picker** (for skill-level trigger cascades — build a reusable `trigger_condition_ui` grouped by 4 phases); (b) the **~80-variant `StatType` picker** (for effect stat-mods — recommend a searchable text combo, not an enumerated dropdown); (c) **effect hot-reload is blocked by a process-global `OnceLock`** (`effects.rs:19`) — edited effects can't re-init in-process.

**⛔ BLOCKED ON USER DECISIONS.** I asked the user two forks; they answered ONE and want to clarify before finalizing:
- **v1 SCOPE** — user leaned **"+ full effect authoring"** (the largest scope: Skill rules + skill trigger cascades + full effect-body authoring). This is ambitious (both hard pickers + the effect-reload problem). **Confirm with the user + resolve the sub-questions below before building.**
- **TOML write strategy** — UNANSWERED. Options: full-rewrite via `toml::to_string` (simple, but ~4× verbose — every default field written, e.g. firebolt 16→63 lines) vs `toml_edit` merge (preserves hand formatting/comments, more code). Recommend full-rewrite for v1.
- **Open sub-questions raised (the user was mid-clarifying when we broke):**
  1. **Effect hot-reload:** with full effect authoring, edited effects won't re-preview without an editor restart *unless* obelisk-core gains a small **registry-swap API** (an obelisk change). Ask: add the swap API, or accept restart-to-preview-effects for v1?
  2. **Sequencing:** build all of "full effect authoring" in one M4 pass, or in **stages** (skill rules → skill triggers → effect bodies) so each lands verified? (Recommend stages given the two big pickers + the reload caveat.)
  3. Minor defaults I proposed (fine unless user objects): don't add Reflect · one unified Save for all 3 files · "new skill" offers both an Attack seed and a Spell seed (note `Skill::default()` is a weapon-scaling *attack* — a spell needs `weapon_effectiveness=0.0` + flat `base_damages`).

**Recommended first action for the next session:** resume the clarification with the user (the 3 sub-questions above), lock the M4 scope, then write an M4 implementation plan (superpowers:writing-plans) and build it — using the same orchestration that worked for M1–M3 (pre-warm the arena_editor target; serial verify-or-revert task workflow; test logic on minimal/arena_sim harnesses; gate on net-test + golden). The `authoring_surface`/`ui_sketch`/`design_forks` fields of the research (in the workflow output, and summarized in the understanding doc) are the plan's raw material.

---

## Deferred refinements (nice-to-haves, not blocking)

- **Live-cosmetics polish:** effects spawn at the muzzle *world* position (~head height), NOT bone-parented to `wand_tip`/hand (looks slightly off — visible in the capture); `LocalCue` doesn't carry the cast's charge so `VfxBindSource::Charge` bakes at 1.0 (real per-cast charge threading deferred); the authored anim-lane clip isn't driven in-game (the rig already animates from `cast_phase`); the built-in `fire`/`explosion` presets aren't gorgeous (prettier `firebolt.vfx.ron` authoring). All noted in code + memory.
- **Upstream:** the `bevy_modal_editor` custom-mode seam is PR-ready (generic, no arena names).

## How to verify you're in a good state (sanity commands)

```bash
# golden byte-identical
cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --test golden
# net-test (retry ≤3)
cd /Users/luke/src/obelisk-arena && pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh
# arena_editor suite (standalone workspace!)
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test
# windowed editor boots
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo run --bin arena-editor   # press K for Skill mode
```

Memory (auto-loaded): `obelisk-arena-project.md` has the phase status + the build-env facts + the skill-designer decisions.
