# Obelisk-Arena In-Editor Skill Designer — Design Spec

**Goal:** An in-editor skill designer — a new `arena_editor` built on `bevy_modal_editor` — where a phase-based timeline authors obelisk skills (timing/geometry/cosmetics, and later rules), with a "Play the real skill" preview that runs obelisk's real deterministic simulation, so what you author is exactly what the game plays.

**Architecture:** A new transport-agnostic `arena_sim` crate (extracted from `arena_game`) is the shared simulation both the game and the editor's preview run. `arena_editor` embeds `bevy_modal_editor::EditorPlugin` and adds a custom **Skill** mode (a bottom-dock timeline) that reads/writes obelisk's `CastTimeline` (`.cast.ron`) + `arena_skills`' `.skillfx.ron`, previews via `arena_sim` + obelisk's `FixedUpdate` pipeline, and reuses the editor's `bevy_vfx` particle engine for cosmetics.

**Tech stack:** Bevy 0.18.1, Avian3d 0.5, `bevy_modal_editor` (egui), `bevy_vfx`, obelisk-bevy, `arena_skills`. All crates are already on Bevy 0.18 / Avian 0.5 (Phase 0 migration complete).

**Background:** Editor capabilities, integration contract, and the gap analysis are in [`../research/2026-06-30-editor-integration-understanding.md`](../research/2026-06-30-editor-integration-understanding.md). Read it before implementing — it cites the exact editor APIs (`EditorPlugin`/`GameState`/lifecycle events, `register_*` seams, `bevy_vfx` data model, the `EffectMarker` "sequencer", and the obelisk `CastTimeline`/`CueEvent`/`arena_skills` seam).

---

## Decisions (locked)

These were chosen during brainstorming and are fixed for this spec:

1. **Make obelisk `CastTimeline` serializable.** Add `Serialize` + `Reflect` (+ keep `Clone`) to obelisk-bevy's `CastTimeline` and its authoring enums so the editor round-trips `.cast.ron` directly. Authoring derives only — no behavior change, gated by the 39-golden suite. (Single source of truth; no parallel format.)
2. **New downstream `arena_editor` crate** depending on `bevy_modal_editor`, with a minimal generic mode/panel registration **PR'd upstream** so the editor stays generic. Thin vendored fork as a fallback until the PR merges.
3. **v1 = full vertical slice**, but **decomposed into sequenced milestones**: M1 Foundation → M2 Timeline+preview → M3 Cosmetic lanes (incl. animation, bone sockets, `VfxParam` stat-binding). **M4 Rules authoring** is a sequenced follow-on with its own obelisk-core deep-dive + spec.
4. **Preview = shared net-free `arena_sim` core** extracted from `arena_game`, so the preview behaves exactly like the live game (not a second sim copy).
5. **Authoring target = obelisk data**, never the editor's native `EffectMarker`: `CastTimeline` (timing/geometry) + `.skillfx.ron` (cosmetics), with M4 adding the obelisk `Skill`/`Effect`/`Trigger` rules `.toml`.

---

## Crate architecture

### `arena_sim` (NEW) — the shared, transport-agnostic simulation

Extracted from `arena_game` (which was recently modularized, so this is a clean lift). Owns everything the simulation needs that is **independent of transport (lightyear) and the windowed client**:

- The obelisk composition: `add_obelisk_sim(app, resolve_hits: true)` (the unified builder), the `ObeliskSet` ordering, and the spatial-pipeline refresh systems (`refresh_spatial_pipeline` / `_pre_detect`) required under an Avian + obelisk setup.
- The shared force controller (`shared_controller`: `apply_arena_movement` / `apply_arena_yaw`).
- The combatant spawn recipe: capsule dims (`PLAYER_CAPSULE_RADIUS/LENGTH`), the child `Hurtbox` sensor, `Faction`, `make_combatant` + `grant_skill`, `faction_for_slot`.
- The arena tuning constants (movement/jump/gravity/ground/capsule), the static floor spawn, and `Gravity`.
- **Physics setup is parameterized**: `arena_sim` exposes the obelisk/controller/spawn pieces; the *host* installs Avian. `arena_game` installs lightyear-Avian (`add_avian_with_lightyear`, `AvianReplicationMode::Position`); `arena_editor` installs **plain** `PhysicsPlugins::new(FixedUpdate)` + `Gravity` (no lightyear, no replication).

`arena_game` then = `arena_sim` + lightyear netcode (`net/*`, the predicted/interpolated materialization) + the windowed client (rendering/HUD/input). The split boundary is "does this run on a headless authoritative server with no transport?" → `arena_sim`; "is this about replication or the windowed client?" → `arena_game`.

**Extraction guardrail:** `arena_game`'s net-test harness MUST stay green at every step — it is the proof the extracted core still composes the game identically.

### `arena_editor` (NEW) — the skill designer binary

```
App
  ::add_plugins(DefaultPlugins)                       // editor owns the window + egui
  ::add_plugins(EditorPlugin::new(EditorPluginConfig {
        add_physics: false,                            // arena_sim/editor own physics
        add_egui: true, ..default() }))
  ::add_plugins(GamePlugin)                            // GameState + Play/Pause/Reset lifecycle
  ::register_editor_mode(skill_mode_def())            // the upstream custom-mode seam
  ::add_plugins(SkillDesignerPlugin)                  // all skill-designer logic lives here
  ::add_plugins(arena_sim::ArenaSimPreviewPlugin)     // the preview mini-world (paused in Editing)
  // bevy_vfx::VfxPlugin comes via EditorPlugin; arena_skills loaded for cue→lane binding
```

`SkillDesignerPlugin` (in `arena_editor`) owns: the timeline panel UI, the in-memory skill being edited, load/save of `.cast.ron` + `.skillfx.ron`, the lane editors, the socket picker, the `VfxParam` binding UI, and the preview controller. All game-specific knowledge stays here; `bevy_modal_editor` gains only the generic mode seam.

### `obelisk-bevy` — `CastTimeline` serialization (M1)

Add `Serialize` + `Reflect` to `CastTimeline` and **every type it transitively contains** — the phase-durations struct, the collision-window struct, and the shape / motion / hit-filter / hit-mode / targeting / delivery enums (enumerate the exact set by reading `obelisk-bevy/src/assets/`, not from this list). Keep `Deserialize` + `Clone`. The hand-rolled `CastTimelineLoader` is unchanged (it still reads `.cast.ron`); the editor writes `.cast.ron` via `ron::ser`. No simulation code changes; the 39-golden suite proves it.

### Upstream seam — generic custom-mode registration (PR into `bevy_modal_editor`)

The editor's modes are a hardcoded `EditorMode` `States` enum + an input match + a `panel_side()` match + a panel plugin. Minimal generic seam:

- `EditorMode::Custom(CustomModeId)` — a new data-carrying variant (`CustomModeId` = a small interned id).
- `CustomModeRegistry` resource: `Vec<CustomModeDef { id, name, activation_key: KeyCode, panel_side: Side, panel: SystemId /* egui draw */, on_enter: Option<SystemId>, on_exit: Option<SystemId> }>`.
- `App::register_editor_mode(CustomModeDef)` extension (mirrors the existing `register_custom_entity` idiom).
- The editor's `handle_mode_input` consults registered `activation_key`s; `panel_side()` and the panel dispatch consult the registry for `EditorMode::Custom(id)`.

This is small, generic, and reusable by any game. If upstream review lags, vendor a thin fork of `bevy_modal_editor` carrying just this change until the PR merges.

---

## M1 — Foundation

**Deliverable:** the game still works; `arena_editor` boots into an (empty) Skill mode; `CastTimeline` round-trips through `.cast.ron`.

Tasks:
1. Add `Serialize`+`Reflect` to obelisk-bevy `CastTimeline` + enums. Round-trip unit test (parse a `.cast.ron` → serialize → re-parse → assert structural equality). Run the 39-golden suite (must stay byte-identical).
2. Create `arena_sim`; move the transport-agnostic pieces out of `arena_game` (obelisk composition, shared controller, combatant recipe, tuning, floor, gravity), parameterizing physics install. `arena_game` depends on `arena_sim`. **Net-test harness stays green.** Add an `arena_sim` headless smoke test (spawn caster+dummy, cast `firebolt`, advance `FixedUpdate`, assert a `DamageResolved` on the dummy, seeded RNG).
3. Implement the upstream custom-mode seam (in `bevy_modal_editor` or a vendored fork) + a test that a registered mode is enterable and its panel draws.
4. Scaffold `arena_editor`: window + `EditorPlugin{add_physics:false}` + `GamePlugin` + the Skill mode registered (empty panel) + `arena_sim` preview plugin (mini-world paused in `Editing`). It launches and you can enter Skill mode.

## M2 — Timeline authoring + "Play the real skill" preview

**Deliverable:** author a `CastTimeline` on the bottom-dock timeline and Play it through the real obelisk sim.

**Layout:** bottom-dock timeline (chosen), 3D preview viewport above. Lanes read left→right over a seconds ruler.

**Authoring (writes `assets/skills/<id>.cast.ron`):**
- **Phase bands** — windup/active/recovery; drag boundaries or type to set `phase_durations`.
- **Targeting + Delivery** — dropdowns over the obelisk enums (`SelfCast/SingleEntity/Direction/Cone` × `Melee/Instant/Projectile`).
- **Hit-windows lane** — add `CollisionWindow`s; drag each bar within its phase to set `spawn_phase` + `spawn_offset` + `active_duration`; pick `shape` (sphere/capsule/cone-sector) + size, `hit_filter`, `hit_mode`, `motion`. A viewport gizmo shows the shape live.
- **`vfx_cues` keys** auto-derive: `on_cast`, `on_window_<id>` per window, `on_hit` (M3 binds visuals to these).
- **Load/Save** via `ron` against the now-serializable `CastTimeline`.

**Preview controller (`arena_sim` + obelisk):**
- The editor holds a live `arena_sim` mini-world (one caster + one dummy on the floor, built from the shared combatant recipe), paused in `GameState::Editing` and owning its own `Time<Fixed>` + seeded `CombatRng`.
- **▶ Play** (`PlayEvent` → `GameState::Playing`, `GameStartedEvent`): reset/spawn caster (current skill granted) + dummy, trigger `cast_skill_dir`; obelisk's real `FixedUpdate` (`Validate→Advance→Projectiles→ResolveHits→TickEffects`) runs the authored timeline. The playhead syncs to `ActiveCast.phase` + elapsed; hitbox gizmos appear when windows open; the projectile flies; `DamageResolved` lands on the dummy.
- **⟲ Reset** (`ResetEvent`): despawn `GameEntity`-tagged entities (caster/dummy/projectiles/cosmetics) → back to `Editing`.
- **Determinism:** seeded RNG + `Time<Fixed>`; same skill + seed = identical preview. The editor's wall-clock `EffectMarker` executor is **not** used for the sim.
- **Cast-speed:** preview defaults to a cast-speed-1 caster (authored durations = displayed). "Preview at cast-speed N" deferred.

## M3 — Cosmetic lanes (animation, particles, projectile, VfxParam)

**Deliverable:** full WYSIWYG — author cosmetics that bind to the timeline's cues and render in preview exactly as in-game.

Cosmetics author into `arena_skills`' `.skillfx.ron` (existing format, extended). Each lane event is a `LaneEvent` bound to a cue key (`on_cast`/`on_window_<id>`/`on_hit`) + an offset.

- **Animation lane** — pick a clip by name (from `character.glb` via the editor's `register_gltf_library` → `AnimationLibrary`) + layer/weight, bound to a cue. In preview drives an `AnimationPlayer` on the rig (net-new; reuses arena_game's anim graph + the shared `character.glb`).
- **Particle lane** — pick a `bevy_vfx` effect by name (`VfxLibrary`) + a socket + offset; spawns at the socket when the cue fires.
- **Projectile lane** — the *cosmetic* projectile (obelisk owns the authoritative one): a `bevy_vfx` trail + socket origin + speed.
- **Bone-socket picker** — the rig's named bones are queried by `Name` and listed (`wand_tip`, `hit_point`, … with `root` fallback); a lane's socket = a bone name, and the spawned effect attaches as that bone's child. Net-new.
- **`VfxParam` stat-binding** — wire the currently-inert `VfxParam` (extract → GPU/CPU): a `VfxSystem` param (tint / scale / emission-rate) binds to a runtime **source** — for v1, **charge level (0..1)** or **one caster stat (a chosen `StatType`)** — sampled **at spawn** over a min→max range. Continuous per-frame stat animation deferred. *(Riskiest net-new bit; see Risks.)*

**Flow-through:** `LaneEvent` gains the variants (`Anim{clip,layer,weight}`, `Particle{effect,socket,offset,param_bindings}`, `Projectile{effect,socket,speed}`); `arena_skills` already loads `.skillfx.ron` → `by_cue`, and arena_game's runtime cue→`LaneEvent` binding consumes it unchanged — so authored cosmetics drop straight into the live game. In preview, real obelisk `CueEvent`s fire the same binding, sampling `VfxParam` sources from the live caster's charge/stats.

## M4 — Rules authoring (sequenced follow-on; own spec)

Authors obelisk `Skill`/`Effect`/`Trigger` `.toml` (damage packets, effects with conditions/stacking/duration, trigger cascades, mana, cooldown, tags). Slots into the same Skill mode as a "Rules" tab beside the timeline.

**Prerequisite (do before designing M4):** a focused obelisk-core deep-dive — the `Skill`/`Effect`/`Trigger` schema in `../obelisk` (`stat_core`), the `skills/*.toml` format, and whether those types can gain `Serialize`+`Reflect` or need an editor-side rules mirror that bakes TOML. That decision is M4's. M1–M3 do not depend on it; until M4, the rules `.toml` is shown read-only as context (so the designer sees what the skill does).

---

## Data formats

- **`assets/skills/<id>.cast.ron`** — obelisk `CastTimeline` (now `Serialize`). Authored by M2. The single source of timing/geometry/targeting/delivery + the `vfx_cues` key map.
- **`assets/skills/<id>.skillfx.ron`** — `arena_skills::SkillFx` (extended `LaneEvent` set). Authored by M3. Cue-keyed cosmetic lanes.
- **`skills/<id>.toml`** — obelisk `Skill` rules. Hand-edited until M4.
- **`assets/vfx/<name>.vfx.ron`** — `bevy_vfx::VfxSystem` presets, referenced by name from particle/projectile lanes. Authored in the editor's existing Particle mode (reused).

The editor writes into arena_game's actual asset directories, so the author→run-the-game loop is direct.

---

## Testing / verification

- **M1:** `CastTimeline` round-trip test; 39-golden suite green; arena_game net-test green; `arena_sim` headless smoke (seeded cast→damage); custom-mode-seam test.
- **M2:** author→`.cast.ron`→reload round-trip; headless author→preview integration (build a skill, run the preview sim, assert phases/window/damage fire); determinism (same-seed-same-output) test.
- **M3:** `.skillfx.ron` round-trip; `arena_skills` binding test for the new `LaneEvent` variants; `VfxParam` unit test (charge/stat → expected param value); **windowed screenshot acceptance** (author a skill, Play, screenshot the cosmetics) using the session's existing windowed-capture harness.
- **Manual acceptance:** author a complete skill end-to-end in the windowed editor, Play it, then load it in `arena_game` and confirm identical behavior.

## Risks + mitigations

1. **`VfxParam` wiring** (inert today; GPU-extract net-new — the riskiest) — scope to bind-at-spawn + a small param/source set; CPU-modulation fallback if GPU extract is hard; it's M3, so M1–M2 ship without it.
2. **Upstream editor churn** — thin vendored fork until the custom-mode PR merges; the seam is small and localized.
3. **`arena_sim` extraction breaking the game** — the net-test harness is the gate; extract incrementally, keep it green.
4. **Physics coexistence** (editor `add_physics:false` + plain-Avian sim vs the game's lightyear-Avian) — `arena_sim` parameterizes physics; the editor uses plain Avian; verify the obelisk spatial-pipeline refresh + BVH behave in the editor context early in M1.
5. **Preview determinism under play-mode** — the preview owns its own `Time<Fixed>` + seed; verify obelisk's `FixedUpdate` runs cleanly under `GameState::Playing` in M1/M2 (the understanding doc flagged this).
6. **`CastTimeline` is an Asset, not a Component** — the editor edits the asset file via the custom timeline UI (not the reflect-component inspector); `Serialize` is the load-bearing derive, `Reflect` is a bonus for a fallback inspector.

## Out of scope (v1) / deferred

- M4 rules authoring (own spec, after the obelisk-core deep-dive).
- Continuous per-frame `VfxParam` animation (v1 samples at spawn).
- "Preview at cast-speed N" (v1 previews at cast-speed 1).
- Networked/multi-entity preview (preview is local, single caster + single dummy).
- Authoring obelisk effects/triggers (that's M4).
- Editing `.vfx.ron` presets themselves — reuse the editor's existing Particle mode.

## Milestone delivery seams (for the plan)

- **M1** ships: game still works (net-test green), editor boots a Skill mode, `CastTimeline` round-trips.
- **M2** ships: author a `CastTimeline` + Play-the-real-skill preview (sim only).
- **M3** ships: cosmetic lanes + animation + `VfxParam` → full WYSIWYG skill.
- **M4** (later, own spec): rules authoring.
