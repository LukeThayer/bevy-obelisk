# Validation Environment & Integrated Regression Harness — Design Spec

**Date:** 2026-06-16
**Status:** Approved design, pre-implementation
**Topic:** Turn the inert `playground` into (a) an agent-usable, integration-level regression harness with golden traces, (b) a real visual debug layer, and (c) a windowed demo + headless screenshots — all driven from one shared scenario library that exercises obelisk-bevy the way a game integrates it.

---

## 1. Goal & context

The current `examples/playground.rs` runs combat (logged via `present`'s `info!`) but renders **nothing** — the projectile `Hitbox` is a logic-only entity (no mesh), targets never react to hits or death, and `PhysicsDebugPlugin` only draws physics colliders (the hurtbox), not hitboxes/cone-arcs/cast-phases/combat state. Pressing Space "does nothing" visibly, and there is **no surface an agent can use to validate behavior** beyond ad-hoc log scraping.

This spec builds a validation environment whose **primary purpose is to let the agent (and humans) validate changes and behavior in an integrated setting**, with a **full regression suite** that exercises the crate's **public integration path** (not test-support internals). Two validation modalities:
1. **Deterministic golden traces** (the backbone) — the reliable, diffable regression net the agent reads.
2. **Headless screenshots + a windowed playground** — actual visual confirmation, sharing the same scenarios + a real debug-viz layer.

### Decided constraints

| Decision | Choice |
|---|---|
| Agent validation modality | **Trace-assert backbone + headless screenshots** |
| Regression model | **Golden-trace snapshots** (committed `*.trace`; `cargo test` diffs; `UPDATE_GOLDEN=1` regenerates) |
| Visual layer | **Full gameplay debug-viz** (gizmos for hit/hurt/cone, cast-phase rings, projectile mesh, hit flash, death viz, floating damage text, HUD bars, event-log panel) |
| Coverage | **Full feature matrix** (one scenario per major behavior) |
| Decomposition | **In-crate (Approach A)** — one shared scenario library across golden tests, screenshots, and the windowed playground; debug-viz is a real `present`-layer feature |
| Integration path | Trace runs the **headless sim stack** (authoritative gameplay: sim + cues + net + cooldowns + loot); visuals run the **client/render stack** (sim + debug-viz). Both assemble via the public plugin/prelude API + skills authored as `.cast.ron` assets. |

---

## 2. Architecture

One scenario library feeds three surfaces:

```
            ┌─────────────────────── scenario library (src/scenario/library.rs) ───────────────────────┐
            │  feature_matrix() -> Vec<Scenario>   (actors + timed Actions + duration, public-API only) │
            └───────────┬───────────────────────────────┬──────────────────────────────┬───────────────┘
                        │ headless sim                   │ headless render               │ windowed
                        ▼                                ▼                               ▼
   tests/golden.rs: run_scenario → Trace      examples/screenshot.rs:           examples/playground.rs:
   diff vs tests/golden/<name>.trace          render scenario@tick → PNG        DefaultPlugins + picker
   (UPDATE_GOLDEN=1 regenerates)              (off-screen image readback)       + free-cast + debug-viz
                        ▲                                ▲                               ▲
                        └──── src/scenario/{mod,run,trace}.rs ────┘   └──── src/present/debug_viz.rs (shared viz) ────┘
```

- **`src/scenario/`** (`test-support` feature) — `Scenario`/`ActorSpec`/`Action` model + builder, the integration `run_scenario` runner, the `Trace`/`TraceRecorder`, and `library::feature_matrix()`.
- **`src/present/debug_viz.rs`** (`present` feature; gizmo/mesh parts under `debug-gizmos`) — the gameplay visualizers, shared by the playground + screenshot renderer.
- **`tests/golden.rs` + `tests/golden/*.trace`** — the regression suite.
- **`examples/playground.rs`** (rewritten) + **`examples/screenshot.rs`** (new).

---

## 3. Components

### 3.1 Scenario model (`src/scenario/mod.rs`)
```text
Scenario { name: &str, seed: u64, ticks: usize, actors: Vec<ActorSpec>, script: Vec<ScriptStep> }
ActorSpec { id, faction, life, mana, pos, skills: Vec<String>, drop_table: Option<String>, hurtbox_radius: f32 }
ScriptStep { at_tick: usize, action: Action }
Action = Cast { caster: String, skill: String, aim: Aim } | CastPoint{..} | CastDir{..}
       | ApplyEffect { target, effect } | SetMana { id, mana } | Move { id, to: Vec3 } | Despawn { id }
Aim = Entity(String) | Point(Vec3) | Dir(Vec3)
```
A fluent builder (`Scenario::new(name, seed).actor(..).at(tick).cast(..)…`). Actors/aims reference **string ids** (resolved to entities at run time via `ObeliskEntityIndex`), so scenarios are declarative + match the wire model.

### 3.2 Integration runner (`src/scenario/run.rs`)
`run_scenario(&Scenario) -> Trace` builds the app **the documented headless way**: `MinimalPlugins + AssetPlugin{file_path:"."} + MeshPlugin + ScenePlugin + ObeliskSimPlugin` + `TimeUpdateStrategy::ManualDuration(1/60s)` + `Time::<Fixed>::from_hz(60)`; `add_obelisk_config_constants_default()`, `add_obelisk_effects(<fixtures>)`, `add_obelisk_skills(SkillSource::Dir(<fixtures>))`, `seed_combat_rng(scenario.seed)`; loads each referenced `.cast.ron`; `finish()/cleanup()`; spawns actors via `make_combatant` (so `ObeliskId == StatBlock.id`); installs the `TraceRecorder`; then advances tick-by-tick, applying each `ScriptStep` at its tick (via the public verbs: `cast_skill_at`/`_point`/`_dir`, `apply_obelisk_effect`, etc.), and returns the recorded `Trace`. This is the **public integration boundary** — no `ObeliskTestApp` internals beyond the shared headless recipe.

### 3.3 Trace + golden (`src/scenario/trace.rs`, `tests/golden.rs`)
- `Trace` = ordered `Vec<TraceLine>`; `TraceLine { tick: usize, kind: &str, detail: String }`. A `TraceRecorder` (a Resource + observers for every gameplay event: `CastBegan/CastRejected/CastPhaseChanged/HitWindowOpened/HitConfirmed/DamageResolved/EffectApplied/EffectExpired/DotTicked/EntityDied/TriggerFired/CueEvent/CooldownStarted/CooldownReady/LootDropped`) appends a line per event, in fire order, tagged by kind, with **stable string ids** (Entity→id via the index) and floats formatted to fixed precision (`{:.3}`) to avoid float-noise diffs.
- **`NetEvent` recording is opt-in per scenario** (`Scenario.record_net: bool`, default false) — otherwise every golden would be cluttered with wire-mirror duplicates of the gameplay events. Only `netcode_egress` enables it, so that scenario's golden regression-covers the serializable egress while the others stay clean.
- Serialization: `Trace::to_text()` → deterministic, line-oriented (`<tick>\t<KIND>\t<detail>`), human-readable + diffable.
- `tests/golden.rs`: for each scenario in `feature_matrix()`, `run_scenario` → `to_text()` → compare to `tests/golden/<name>.trace`. Mismatch ⇒ test fails printing the diff. `UPDATE_GOLDEN=1 cargo test` (re)writes the goldens. The diff **is** the agent's read of what a change did.
- **Determinism contract:** seeded `CombatRng` + `Time<Fixed>` + fixed schedule + stable-id formatting + fixed float precision ⇒ same scenario → identical trace. (Cross-platform float identity is not guaranteed; goldens are authoritative on the dev/CI platform — noted.)

### 3.4 Debug-viz (`src/present/debug_viz.rs`)
A `present`-layer plugin (`ObeliskDebugVizPlugin`) that turns gameplay state into visuals (real feature, usable by consumers). Built on `bevy_ui` + `Gizmos` (no new dependency):
- **Gizmos** (under `debug-gizmos`): draw each active `Hitbox`'s shape at its transform (sphere/capsule wireframe; **cone → an arc/sector**), draw `Hurtbox` spheres, and a **cast-phase ring** on casters colored by `SkillPhase`.
- **Projectile mesh:** give spawned projectile hitboxes a small emissive sphere mesh so the bolt is visible in flight.
- **Reactions:** on `HitConfirmed`/`DamageResolved`, flash the target's material; on `EntityDied`, grey + scale-down (and optionally despawn after a beat).
- **Floating damage text:** spawn a short-lived `Text`/world-anchored number from `DamageResolved`.
- **HUD:** per-combatant life/mana bars + active-cooldown readouts (bevy_ui), and a **scrolling event-log panel** (last N events).
- Read-only w.r.t. `Attributes` (consistent with the present-layer rule).

### 3.5 Windowed playground (`examples/playground.rs`, rewritten)
`DefaultPlugins + ObeliskPlugins + ObeliskDebugVizPlugin` + the shared scenario library. Keys: number keys select/replay a scenario from `feature_matrix()`; Space free-casts the player's first skill at the nearest enemy (via `ObeliskSpatial`); `R` resets. The HUD + log + gizmos make combat visible. This is the human demo and the manual fallback if headless screenshots don't work.

### 3.6 Headless screenshot renderer (`examples/screenshot.rs`)
Runs a named scenario through a **render-capable headless app** (RenderPlugins + a `Camera3d` targeting an off-screen `RenderTarget::Image`) + `ObeliskDebugVizPlugin`, advances to a `--tick N` (or several), copies the render image back to CPU, and writes `screenshots/<name>-<tick>.png`. The agent runs it and `Read`s the PNG for visual validation. Shares the scenario library + debug-viz. **Highest-risk piece** — see §5.

---

## 4. Scenario matrix (the full regression)

Each is a `Scenario` with a committed golden trace; ★ also gets a screenshot.

1. ★ `firebolt_kill` — projectile hits, 20 dmg + burn DoT, target dies (no post-death ticks).
2. ★ `cone_cleave` — cone hits two enemies in-arc, not the one behind, not an ally.
3. `faction_filter` — Enemies filter spares same-faction.
4. `out_of_range` — `CastRejected{OutOfRange}`, no damage.
5. `line_of_sight` — obstacle ⇒ `CastRejected{NoLineOfSight}`; cleared ⇒ cast begins.
6. `cooldown_gate` — second cast within cooldown ⇒ `CastRejected{OnCooldown}` + `CooldownStarted/Ready`.
7. `already_casting` — second cast mid-windup ⇒ `CastRejected{AlreadyCasting}`.
8. `trigger_cascade` — an OnConsume/OnApply trigger ⇒ `TriggerFired` + the triggered skill resolves.
9. `aoe_fan` — `ObeliskCombat::resolve_aoe` over N targets, stable order.
10. `netcode_egress` — the `NetEvent` stream for a full cast→death (stable ids).
11. `vfx_cues` — `on_cast`/`on_hit` cues fire as `CueEvent`s.
12. `loot_on_death` — enemy with a `DropTableId` drops loot (`LootDropped`).
13. `apply_effect` — `apply_obelisk_effect` adds a status (+ `TriggerFired` if any).
14. `stat_sources` — `apply_stat_sources` raises a computed stat (asserted in-trace via a follow-up read line).

---

## 5. Risks / verification list

1. **Headless render-to-PNG (HIGH).** Off-screen `RenderTarget::Image` + GPU readback in this environment (Metal adapter confirmed available). Spike `examples/screenshot.rs` to a single PNG before wiring the matrix. Fallback: screenshots are produced only via the windowed playground (manual); the trace backbone + golden suite stand regardless.
2. **Trace determinism / float noise.** Fixed `{:.3}` formatting + seeded RNG + fixed timestep. Verify a scenario produces an identical trace across two runs (a meta-test). Cross-platform float identity not guaranteed — goldens authoritative per platform.
3. **Observer fire-order stability.** Trace lines are appended in observer/system fire order within a tick; confirm this is deterministic given the fixed schedule (it should be — Bevy observer order is registration-deterministic). If not, sort within a tick by a stable key.
4. **bevy_ui vs egui for the HUD/log.** Spec uses `bevy_ui` (no new dep). If text/world-anchoring is too fiddly, a `bevy_egui` dep is the fallback (note the version pin).
5. **`RenderTarget::Image` + `MeshPlugin`/`ScenePlugin` interplay** under a manually-built render app (vs `DefaultPlugins`). Confirm the minimal render plugin set.

---

## 6. How the agent uses this

- **Regression after any change:** `cargo test --features test-support golden` → a failing scenario prints the trace diff (exactly what changed). Intentional change ⇒ `UPDATE_GOLDEN=1 cargo test …` + review the golden diff in the commit.
- **Visual validation:** `cargo run --example screenshot --features "test-support debug-gizmos" -- --scenario firebolt_kill --tick 30` → `Read screenshots/firebolt_kill-30.png`.
- **Manual/human:** `cargo run --example playground --features debug-gizmos`.

## 7. File structure

```
src/scenario/{mod.rs (model+builder), run.rs (runner), trace.rs (Trace+recorder), library.rs (feature_matrix)}
src/present/debug_viz.rs            # ObeliskDebugVizPlugin (gizmos/mesh/reactions/HUD/log)
tests/golden.rs                     # golden-diff harness over feature_matrix()
tests/golden/<scenario>.trace       # committed goldens
examples/playground.rs              # rewritten windowed demo (scenario picker + free-cast + viz)
examples/screenshot.rs              # headless render scenario@tick -> PNG
Cargo.toml                          # features: scenario lib under test-support; debug-gizmos gates gizmo viz
```

## 8. Out of scope (future)
Cross-platform golden normalization; input-recording/replay from the live window; perf/soak scenarios; multi-frame screenshot "filmstrips"; a data-driven (RON) scenario format (code-defined is enough now).
