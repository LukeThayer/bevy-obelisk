# Phase 4 — Arena Migration (STUB)

**This is a STUB, not an implementation plan.** It exists so phase 4 doesn't start from a cold
read of spec §3.4 alone — it corrects/refines that section against the ACTUAL state of
`obelisk-arena` and `bevy_modal_editor` as of phase 3's close (2026-07-04, end of the skill-mode
series, Task 13). Before executing phase 4, write a real plan (`superpowers:writing-plans`) that
takes this stub, spec §3.4, and a fresh read of both repos as inputs — several items below are
flagged as open decisions the real plan must make, not settle.

**Sources:** spec `docs/superpowers/specs/2026-07-02-skill-editor-reimplementation-design.md`
§3.4 ("obelisk-arena"), §4 ("Testing" — acceptance bullets), §5.4 ("Arena" sub-project), §6
("What dies"); the phase-3 plan `docs/superpowers/plans/2026-07-03-skill-mode-bevy-modal-editor.md`;
`~/src/bevy_modal_editor-skill/.superpowers/sdd/progress.md` (TICKET lines); direct inspection of
`~/src/obelisk-arena` (workspace `master` @ `f6472e4`) at stub-writing time.

**Where phase 3 landed, in one line:** `bevy_modal_editor` (worktree `~/src/bevy_modal_editor-skill`,
branch `skill-mode`, NOT YET merged to the user's own `main`) now has a built-in, `--features
obelisk`-gated **Skill mode** (K key) authoring the rules/behavior/presentation triad against a
game-registered content root, with a deterministic sim-backed preview stage + synchronous scrub +
causality chips + gizmo proxies — see that worktree's `.superpowers/sdd/task-13-report.md` for the
full close-out. Phase 4 is what makes obelisk-arena actually USE it instead of its own bespoke v1
skill designer.

---

## (a) `arena_editor` → composition shell

Spec estimated "~15 files" to delete. Direct count at stub-writing time
(`~/src/obelisk-arena/crates/arena_editor/src/`, 28 files total) says **26**, not ~15 — every
`.rs` file except `lib.rs`/`main.rs`:

```
derived.rs edits.rs effect_model.rs effects_panel.rs enum_ui.rs fx_edits.rs gizmo.rs
inspector.rs io.rs model.rs panel.rs preview_controller.rs preview_cosmetics.rs
preview_rig.rs rules_edits.rs rules_model.rs rules_panel.rs scrub.rs selection.rs
sim_config.rs skill_designer.rs socket.rs stat_ui.rs timeline_geom.rs trigger_ui.rs
vfx_bind.rs
```

These are ports whose UPSTREAM replacements already exist in `bevy_modal_editor-skill`'s
`src/skill/` (each ported module's doc comment there names its `arena_editor @ f6472e4` source
file — cross-reference before deleting, in case any obelisk-arena-specific behavior didn't carry
over and needs a deliberate decision rather than silent loss).

`lib.rs`/`main.rs` don't survive AS-IS either — they get rewritten, not just left alone. Today
`main.rs` composes `EditorPlugin` + `GamePlugin` + `arena_editor::SkillDesignerPlugin` +
`arena_editor::sim_config::PreviewSimConfigPlugin` + `arena_sim::preview::ArenaSimPreviewPlugin`
+ a `character.glb` GLTF-library registration + a `PhysicsGizmos` gizmo-group init. The shell
keeps: `DefaultPlugins` + asset-root wiring + `EditorPlugin::new(EditorPluginConfig { add_physics:
false, .. })` (now built with `--features obelisk`) + `GamePlugin` + the `character.glb`
registration + the `PhysicsGizmos` init (still needed per its own comment: `add_physics:false`
skips Avian's debug plugin that would otherwise register that gizmo group) + one
`register_obelisk_content(root)` call pointed at obelisk-arena's own `config/`+`assets/` root.
Everything else in that list (`SkillDesignerPlugin`, `PreviewSimConfigPlugin`,
`ArenaSimPreviewPlugin`) is superseded by what `SkillModePlugin`/`SkillPreviewPlugin` already do
inside `bevy_modal_editor` itself.

**Also dies (not a file in the list above, may not even exist as a checked-in branch anymore —
confirm before assuming cleanup work here):** the OLD `[skill-designer]` bevy_modal_editor fork
branch and its generic `EditorMode::Custom(CustomModeId)` / `CustomModeRegistry` /
`register_editor_mode` extension seam (`src/editor/custom_mode.rs` in that old fork), which
`arena_editor::skill_designer::register_skill_mode`/`SkillDesignerPlugin` used to bolt the v1
designer on from outside. Confirmed at stub-writing time: this seam does **not** exist anywhere
in the current `bevy_modal_editor-skill` tree (`grep -rn "CustomModeRegistry\|EditorMode::Custom"
src/` — zero hits), so there's nothing to remove FROM THAT REPO; the only possible leftover is a
stale, already-superseded fork branch the user may still have lying around (see the old handoff
doc `docs/superpowers/2026-07-01-skill-designer-handoff.md`, line 75) — worth a "does this still
exist, archive it" check, not blocking code work.

**Workspace membership (open decision for the real plan):** `arena_editor` is today EXCLUDED
from the obelisk-arena workspace (`Cargo.toml`: `exclude = ["crates/arena_editor"]`, its own
`Cargo.lock`) — deliberately isolated so the editor's heavy Bevy features can't feature-unify
onto `arena_game` and destabilize the net-test build. Once it's a thin shell with no
skill-designer-specific heavy deps of its own, decide whether it still needs that isolation or
can rejoin the main workspace. Don't assume either way.

## (b) `arena_skills` lane model + `.skillfx.ron` die

`crates/arena_skills` (workspace member) is the `.skillfx.ron` cosmetic-binding layer: `SkillFx {
lanes: HashMap<String cue_id, LaneEvent> }` per skill file, flattened across every loaded file
into `SkillFxRegistry.by_cue: HashMap<String, Vec<LaneEvent>>`, looked up via
`resolve_cue(reg, &CueMessage) -> &[LaneEvent]` (`crates/arena_skills/src/lib.rs`). This is
**not** editor-authoring-only scaffolding — it's loaded at real runtime:
`crates/arena_game/src/client/scene.rs::load_skillfx_registry`, wired into the windowed client at
`crates/arena_game/src/client/app_windowed.rs:85`. Existing files: `assets/skills/{firebolt,
chain_lightning}.skillfx.ron`.

Phase 4 deletes `arena_skills`'s lane model + every `.skillfx.ron` file, replacing them with
`obelisk_bevy::assets::CueBinding`s living directly in each skill's `.cast.ron` (authored via the
Skill mode's Presentation region — see `bevy_modal_editor-skill`'s `src/skill/panel/
presentation.rs`), resolved through `bevy_effect` instead of `arena_skills::resolve_cue`. Whether
the `arena_skills` crate itself is deleted entirely or repurposed for something else lane-model-free
is a real-plan decision, not this stub's to make.

## (c) The cue wire contract

**Correction to spec §3.4's framing:** the spec describes this as if the relay needs building.
It doesn't — `crates/arena_game/src/skills.rs` ALREADY relays cues over lightyear today:
server-side `register_server_cue_egress` converts obelisk `CueEvent` → `CueMessage` → broadcasts
as `CueWireMessage` on a reliable `EventChannel`; client-side `register_client_cue_binding`
consumes `CueWireMessage` → `arena_skills::resolve_cue` → `client/cosmetics.rs`. So the plumbing
survives; two things inside it change:

1. **The wire payload shape.** Spec §6 ("What dies") is explicit that `CueWireMessage`'s LANE
   payload dies, replaced by the generic shape §3.4 describes: **slot id + payload**
   (positions / `from`+`to` for beam-style anchors / charge / end-reason) — no lane-specific
   fields, so triggered sub-casts, chain hops, and emitter cues all replicate through the same
   shape with zero special-casing. **Verify before assuming**: does `CueMessage`/`CueWireMessage`
   already carry the `charge`/`end_reason` fields obelisk-bevy's `CueEvent` gained in this
   series' Task 1 (merged to `main`), or does the wire struct still predate that and need
   extending? Check `crates/arena_game/src/skills.rs`'s `CueMessage` definition against
   `obelisk_bevy::events::CueEvent`'s current shape as the first step of the real plan.
2. **Client-side resolution.** Swap `arena_skills::resolve_cue` (`.skillfx.ron` lane lookup) for
   loading the skill's `.cast.ron` via the shared obelisk-bevy loader and resolving slot →
   Effect preset locally through `bevy_effect` — **EffectLibrary-first-then-VfxLibrary order**,
   the exact resolution order `bevy_modal_editor-skill`'s preview defined and documented in Task
   10 (`src/skill/preview/cosmetics.rs`; see progress.md's Task 10 TICKET line: "cue effect-name
   resolution order defined + documented — EffectLibrary first, then VfxLibrary; game client must
   mirror"). The existing local-cast prediction/de-dup story (predicted `cast` cue for the local
   caster, keyed on caster+skill+slot) carries over unchanged.

## (d) Acquisition resolver

**Correction to spec §3.4's framing:** the spec says the resolver "moves from `arena_game`'s cast
pipeline into a shared host helper" — implying pre-existing logic to relocate. There isn't any:
`crates/arena_sim/src` and `crates/arena_game/src/**` have zero references to
`obelisk_bevy::assets::Acquisition`. `crates/arena_game/src/server/cast_pipeline.rs` casts via
`cast_skill_dir` along the client's raw `aim_dir` — its own module doc says "free aim, no
auto-acquire" in so many words. So phase 4 doesn't relocate a resolver — it **newly adopts**
`Acquisition`/`AcqFallback` resolution in the live game, using
`bevy_modal_editor-skill`'s `src/skill/preview/stage.rs::resolve_stage_acquisition` as the
reference implementation (same "port, don't rewrite from scratch" discipline every other preview
module in that series followed from its `obelisk-arena @ f6472e4` source). Per spec, this lands
in a **shared host helper** (obelisk-bevy itself, or `arena_sim` — the real plan must pick one)
consumed by BOTH `arena_game`'s server (real casts) and the editor's own preview stage (which
currently has its own copy, `resolve_stage_acquisition`, living only in the editor) — the goal is
ONE implementation, not two hand-maintained copies that can drift. This is also where the design
question spec §3.4 doesn't resolve gets decided: does the live game's free-aim-only casting
become fully `Acquisition`-driven (matching what authored content now expects), or does authored
content need to stay compatible with free-aim as a first-class case? Reference content (item f)
can't be authored correctly in the Skill mode without knowing the answer.

## (e) Client `bevy_effect` rendering

Zero `Cargo.toml` in obelisk-arena references `bevy_effect` today (confirmed by grep across the
whole repo) — client effect rendering is 100% the bespoke `arena_skills`/`.skillfx.ron` path
(`client/cosmetics.rs`, `client/vfx_bind.rs`) via `bevy_vfx` directly. Phase 4 adds a
`bevy_effect` dependency to `arena_game`'s client and swaps the render path to consume
`CueBinding`s resolved per (c) above — same runtime crate the editor's own preview
(`bevy_modal_editor-skill`'s `src/skill/preview/cosmetics.rs`) already renders through, so the
client and the editor preview share the actual effect-playback code, not just the data format.

## (f) Reference content

Today: `firebolt` and `chain_lightning` exist as full OLD-schema (v1, pre-trigger-reform) triads
— `config/skills/{firebolt,chain_lightning}.toml` + `assets/skills/{firebolt,
chain_lightning}.cast.ron` + matching `.skillfx.ron` (+ `assets/skills/firebolt_trail.vfx.ron`).
No `fireball` or `blizzard` content exists anywhere yet. Per spec §3.4's migration bullet:

- `firebolt` → **`fireball` + `fireball_explosion`** (a trigger pair — matching the exact
  fireball-pair shape `bevy_modal_editor-skill`'s own tests already exercise end-to-end:
  `tests/skill_preview.rs::fireball_pair_composes_the_sub_cast_in_preview` /
  `::fireball_pair_is_deterministic`, and the probe's own `probe_fireball`/
  `probe_fireball_explosion` fixture in `src/skill/mod.rs::skill_probe` — author the real
  content the same shape, don't invent a new one).
- `chain_lightning` re-keyed onto rules-level `damage.can_chain`/`chain_count` (spec D5), dropping
  whatever v1 chain mechanism (`EndReaction::Chain`, already dead upstream since trigger-reform)
  its old `.cast.ron` used.
- `blizzard` authored fresh — no existing content to migrate, pure acceptance-content authoring
  (a zone/emitter-shaped skill exercising the Template/emitter machinery Tasks 7/11 built
  Behavior-region support for).

All three get authored/scrubbed/Play-verified IN the Skill mode (this is the whole point of
phase 3) BEFORE being wired into the live game — i.e. author against the `arena_editor`
composition shell from (a), using its own `register_obelisk_content` root, same workflow a
consuming game always uses.

## (g) Extended net-test

`crates/arena_game/tools/net-test/{run_session.sh,summarize.py}` exists today: spins
`arena-server` + two `arena-observer` clients, scripts a firebolt cast, asserts (via merged JSONL
traces) `CastBegan`/`DamageResolved` parity + matching damage across server and both observers.
Spec §4's acceptance bullets are the concrete extension targets:

- **fireball** — client renders the explosion cue at the server's impact position; damage shape
  = bolt + explosion as separate `DamageResolved` events with separate skill ids.
- **chain lightning** — N hop cues with correct `from`/`to` anchor pairs on an observer.
- **blizzard** — shard cue positions match the server-authoritative shard spawns.

Plus, given (c)/(e) above: an assertion that the resolution order (EffectLibrary-first-then-
VfxLibrary) actually holds on the CLIENT, not just in the editor preview — this is the one place
that invariant can silently drift once it's duplicated across two codebases.

## (h) Flip the `obelisk-bevy` pin

Mechanically closer to trivial than it sounds: both obelisk-arena's root `Cargo.toml` and
`crates/arena_editor/Cargo.toml` already declare `obelisk-bevy = { git = "...", branch = "main"
}` — the pin is a BRANCH NAME, not a fixed rev. What's actually stale is `Cargo.lock`, locked at
`68618a8c…`, confirmed (`git merge-base --is-ancestor`) to predate BOTH the trigger-reform merge
(`d72fe9f`) and the cue-payload merge (`ebbecc2`, current obelisk-bevy `main` HEAD) — this was
deliberate per spec §5 step 2 ("Arena pins its obelisk-bevy rev at the pre-reform commit for the
duration"), not an oversight. Reopening it is `cargo update -p obelisk-bevy` (and, once
`skill-mode` merges into the user's own `bevy_modal_editor` `main`, `cargo update -p
bevy_modal_editor` too, in `arena_editor/Cargo.toml`, which pins that dep the same branch-name
way). **The repin itself is not the work** — the work is everything upstream of it in this stub
((a)-(g)) that the repin's v2 schema exposes as needing to happen; don't schedule "flip the pin"
as a task that can land before the content migration, or every v1-schema file in `assets/skills/`
breaks loading immediately.

## (i) Open tickets this phase depends on

Carried verbatim from `bevy_modal_editor-skill`'s `.superpowers/sdd/progress.md` (grep `TICKET`);
relevance to phase 4 noted per ticket:

1. **(obelisk-bevy, from Task 11 review) — HIGH relevance.** `execute_skill_timeline` spatial
   triggers do NOT emit `TriggerFired` (only inline packet/effect paths do) — editors/observers
   can't see spatial sub-cast firings as trigger events. Phase 4's net-test parity assertions for
   `fireball`'s explosion sub-cast and `chain_lightning`'s hops need this: without `TriggerFired`
   (or equivalent) firing from spatial trigger paths, an observer client has no event to key a
   "did the sub-cast actually fire, and when" assertion off, the same gap the ticket already
   names for editor/observer tooling. Fix this in obelisk-bevy BEFORE writing the net-test
   extensions in (g), not after discovering the gap mid-task.
2. **(Task 10, resolved) — the resolution-order contract itself.** Cue effect-name resolution
   order is EffectLibrary-first-then-VfxLibrary, defined and documented in the preview
   (`src/skill/preview/cosmetics.rs`). This is not a remaining risk so much as the exact
   contract (c)/(e)/(g) above must mirror byte-for-byte on the client — restated here because
   it's the single most load-bearing invariant phase 4 inherits from phase 3.
3. **(pre-existing, amplified by Task 5) — LOW relevance, carry anyway.**
   `auto_save_effect_presets` rewrites EVERY effect preset to disk on the first frame of every
   process run (a `Local` cache resets per-process, `is_changed` fires on the startup `ResMut`
   insert) — surprising disk writes. Worth knowing before `bevy_effect` ships inside the live
   game client in (e); probably still out of phase-4 scope to fix, but don't let it surprise
   someone debugging unexpected git diffs in `assets/effects/`.
4. **(Task 11 follow-up, cosmetic) — LOW/unlikely relevance.** Transient ~20-40 frame viewport
   render lag scrubbing right after a Skill-mode exit/reentry cycle in the EDITOR (ECS state
   proven correct; likely a render-extraction cadence artifact from bursting
   `run_schedule(FixedUpdate)` on freshly-recreated entities). The live game client never
   scrubs, so this is unlikely to matter for phase 4 proper — carried per the brief's instruction
   to bring forward every open ticket, not because it's expected to bite.

---

## Non-goals of this stub

This document does NOT decide: the exact task breakdown/order for phase 4; whether the shared
acquisition-resolution helper (d) lives in obelisk-bevy or `arena_sim`; whether `arena_editor`
rejoins the main obelisk-arena workspace once thinned; whether `arena_skills` is deleted outright
or repurposed; the exact `CueMessage`/`CueWireMessage` wire schema (only that its lane payload
must die per spec §6, and that whether it already carries charge/end_reason needs a direct check).
Those are real-plan decisions — this stub's job is to make sure the real plan starts from
verified facts instead of the spec's original (now partially stale) assumptions.

## Pointers

- Spec: `docs/superpowers/specs/2026-07-02-skill-editor-reimplementation-design.md` (§3.4, §4, §5, §6)
- Phase-3 plan: `docs/superpowers/plans/2026-07-03-skill-mode-bevy-modal-editor.md`
- Phase-3 close-out: `~/src/bevy_modal_editor-skill/.superpowers/sdd/task-13-report.md` and
  `.superpowers/sdd/progress.md` (full task-by-task ledger, Tasks 1-13)
- Old v1 skill-designer handoff (historical, superseded by phase 3):
  `docs/superpowers/2026-07-01-skill-designer-handoff.md`
- obelisk-arena, as inspected for this stub: `master` @ `f6472e4`
