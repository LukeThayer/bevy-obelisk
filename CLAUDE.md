# CLAUDE.md

Guidance for Claude Code (and any agent) working in the **obelisk-bevy** repo.

`obelisk-bevy` is a Bevy 0.17 plugin that exposes the pure-Rust **obelisk** ARPG libraries
(`stat_core`, `loot_core`, `skill_tree`, `tables_core`) to Bevy games, adding the 3D + temporal
layer they lack: a spatiotemporal skill/cast model, hit/hurt boxes, casting primitives, VFX-cue
hooks, a server-authoritative netcode egress, and content integration (skill tree, loot).

---

## ⚠️ Working agreement (read first)

- **When confused, do NOT guess — ask the maintainer.** This repo wraps a real game-design domain
  with many genuine forks (gameplay policy, cascade routing, balance, transport choices). If a
  decision can't be resolved from the code or this file, stop and ask rather than inventing
  behavior. Past examples that were (correctly) escalated: the concurrent-cast policy (→ reject
  with `AlreadyCasting`), the trigger-cascade routing boundary. Pick the obvious default only when
  there genuinely is one; otherwise ask.
- **Determinism is sacred.** Never introduce non-deterministic behavior into the simulation
  (see Design Decisions). If a change could affect determinism, call it out.
- **The sim is authoritative; presentation is a consumer.** Never let render/VFX/audio concerns
  leak into the simulation. Keep `present` (and anything render-dependent) compile-outable.
- **TDD + verify.** Every behavior change gets a test; run `cargo test --features test-support`
  and report real output before claiming success. Don't weaken assertions to force a pass.
- obelisk lives at `../obelisk` (sibling). Path deps assume that layout. Cross-check every obelisk
  call against `../obelisk/<crate>/src` — the API has sharp edges (see "obelisk API footguns").

## Build & test

```bash
cargo build                                  # client build (default features: "present" on)
cargo build --no-default-features            # headless/server build (presentation compiled out)
cargo test --features test-support --lib --tests   # the suite (skips the examples; ~53 tests)
cargo clippy --features test-support --lib --tests -- -D warnings   # must be clean for obelisk-bevy
cargo fmt --check
cargo run --example playground --features debug-gizmos   # visual 3D demo (Space = cast)
cargo run --example headless_server --no-default-features # authoritative server, prints NetEvent stream
```
Upstream `stat_core` emits dead-code warnings — those are the dependency's, not ours; `-D warnings`
only gates obelisk-bevy's own crate.

---

## Validating changes

A scenario library (`src/scenario/`) drives three validation surfaces off one source of truth.
`feature_matrix()` (`src/scenario/library.rs`) is the canonical set of deterministic scenarios; the
golden traces are the agent-facing regression backbone, the screenshots are headless visual proof,
and the playground is the human-facing window.

**Golden-trace regression (the backbone — run this after any behavior change).**
```bash
cargo test --features test-support --test golden            # diff every scenario against its committed trace
UPDATE_GOLDEN=1 cargo test --features test-support --test golden   # regenerate the goldens
```
`run_scenario` (`src/scenario/run.rs`) plays each scenario headlessly through the **public**
integration path (`ObeliskSimPlugin` + the prelude verbs + the documented headless recipe) and
records every gameplay event into a stable-id, `{:.3}`-precision `Trace` (`src/scenario/trace.rs`),
diffed against `tests/golden/<name>.trace`. `feature_matrix()` is the canonical, always-current list
(**22 scenarios** as of this writing) spanning: combat core (`firebolt_kill`, `cone_cleave`,
`faction_filter`, `apply_effect`); cast rejection + interrupt (`out_of_range`, `line_of_sight`,
`already_casting`, `cooldown_gate`, `cast_rejected_insufficient_mana`, `cast_rejected_unknown_skill`,
`cast_rejected_no_target`, `interrupt_cast`); effect triggers from every condition (`trigger_cascade`
= OnConsume, `on_apply_triggers_skill`, `on_expire_triggers_skill`,
`on_max_stacks_triggers_and_consumes`); stat-driven effects via `ActorSpec::with_stat` /
`with_self_effect` + the `DamageResolved` breakdown fields (`self_buff_boosts_damage`, `crit_strike`,
`resistance_mitigates`, `cast_speed_scaling`); and loot/netcode (`loot_on_death`, `netcode_egress`).
`aoe_fan`/`stat_sources`/`vfx_cues` are intentionally NOT in the matrix (covered by direct unit tests
/ folded into `firebolt_kill`); see the `feature_matrix()` doc comment. When you add a scenario,
update this count.

> **Regression rule.** After **any** behavior change, run the golden suite. A failing golden is a
> behavior change you must justify. Do NOT blind-regenerate: if a trace change is intentional, run
> `UPDATE_GOLDEN=1`, **review the golden diff** (`git diff tests/golden/`) to confirm it matches the
> intended change, and note the rationale in the commit before committing. The goldens ARE the
> regression baseline; an unreviewed regenerate silently launders a regression.

**Headless screenshots (read a scene as an agent).**
```bash
cargo run --example screenshot --features debug-gizmos -- --scenario firebolt_kill --tick 24
```
Plays the named scenario to `--tick <n>` and renders the off-screen frame to
`screenshots/<name>-<tick>.png` (proven on Metal in this environment); `Read` the PNG to inspect the
scene. It composes the live sim meshes + the debug-viz gizmos + the projectile/hit/death material
reactions — the same visuals as the playground. **Limitation:** the `bevy_ui` HUD/event-log/floating
text does NOT compose into the off-screen target (no UI camera is wired into the image render); use
the windowed playground to see the HUD. Defaults: `--scenario firebolt_kill --tick 24`.

**Windowed playground (human visual check).**
```bash
cargo run --example playground --features debug-gizmos
```
Selection keys `1`-`9`/`0`/`-` jump to the first 11 scenarios, and `[` / `]` cycle prev/next through
ALL of `feature_matrix()` (so every scenario is reachable as the matrix grows). `Space` free-casts the
player's first skill at the nearest enemy (via `ObeliskSpatial`), `R` resets. The debug-viz layer
(`src/present/debug_viz.rs`) draws the gizmos (hurtbox/hitbox/cone/cast-ring, under `debug-gizmos`),
the projectile mesh + hit/death reactions, and the HUD (roster + event log + floating damage, under
`present`). The agent can't drive the window — use the screenshot renderer to corroborate.

---

## Architecture

**Single crate, modules** (the sim ↔ presentation boundary is module discipline + the `present`
feature, not a separate crate):

| Module | Responsibility |
|---|---|
| `assets` | `CastTimeline` asset (`.cast.ron`) + RON loader + authoring enums (shapes/phases/targeting/delivery/`vfx_cues`). |
| `core` | `Attributes`(StatBlock)/`Faction`/`SkillSlots`/`Combatant`; `ObeliskConfigExt` (guarded global init + `SkillRegistry` + `CombatRng`); `Cooldowns`; `tick_effects` driver; `ObeliskCorePlugin`. |
| `ids` | `ObeliskId` + `ObeliskEntityIndex` (Entity↔stable-string-id bimap, auto-synced). |
| `timeline` | the "skill" layer: `ActiveCast` state machine, `validate_casts` (range/LOS/mana/cooldown/already-casting gates), `advance_casts` (phases → spawn hit windows), `CastSkillExt` verbs, speed scaling. |
| `spatial` | the "hitbox" layer: `Hitbox`/`Hurtbox`, shape→`Collider`, cone math, `HitFilter`/`HitMode`, `SpatialQuery` overlap detection, projectile motion; `ObeliskSpatialPlugin` (adds Avian `PhysicsPlugins::new(FixedUpdate)`). |
| `combat` | the pure `resolve_one_hit` funnel + the `on_hit_confirmed` observer (resolves + emits + fires triggered cascades). |
| `facade` | consumer `SystemParam`s: `ObeliskRead` (HUD/AI), `ObeliskSpatial` (target acquisition), `ObeliskCombat` (programmatic resolve). |
| `verbs` | `ObeliskCommandsExt` EntityCommands verbs (spawn/grant/apply). |
| `vfx` | `ObeliskCuePlugin` (emits `CueEvent` from authored `vfx_cues`) + `observe_cue`. |
| `net` | `NetEvent` wire format + `ObeliskNetPlugin` (mirrors gameplay events → buffered egress). |
| `loot` | `DropTables`/`ItemGenerator`/`DropTableId` + roll-on-death → `LootDropped`. |
| `events` | all gameplay events. `present` (feature-gated): read-only observers → logging + debug gizmos. `testkit` (test-support feature): the headless `ObeliskTestApp` harness. `prelude`: the curated consumer surface. |

**Schedule** (FixedUpdate, chained): `ObeliskSet::{Validate → Advance → Projectiles → ResolveHits → TickEffects}`.
Presentation runs in `Update`. Combat resolution + cue/net mirrors are **observers** (`commands.trigger`/`On<E>`),
which fire when commands flush within the same `update()`.

**Plugins:** `ObeliskPlugins` (umbrella group: `ObeliskSimPlugin` + `ObeliskPresentPlugin` when `present`).
`ObeliskSimPlugin` = assets + spatial + core + combat + vfx + net + loot sub-plugins (all headless-capable).

---

## Design decisions (why things are the way they are)

- **Determinism / server-authority.** All combat RNG flows through a single seeded `CombatRng`
  (ChaCha8). The funnel is `resolve_one_hit` (`combat/resolve.rs`) → `use_skill_against` →
  `resolve_damage_with_triggers`, threading `&mut CombatRng`. **NEVER call obelisk's `receive_damage`
  / `resolve_damage` — they use `rand::thread_rng()` internally and break determinism.** AoE
  (`ObeliskCombat::resolve_aoe`) stable-sorts targets by `StatBlock.id` before drawing RNG so
  iteration order can't perturb results. Cooldowns/timeline use `Time<Fixed>`.
- **Two-file skill authoring (mandatory).** Each castable skill = obelisk rules (`skills/*.toml`,
  the `Skill`) + one `.cast.ron` (`CastTimeline`: phases/windows/shapes/targeting/delivery/`vfx_cues`,
  referencing the skill by `skill_id`). Every castable skill needs a `.cast.ron`; a geometry-free
  skill uses a minimal instant timeline. No single-file format, no Rust builder (YAGNI).
- **Authority split.** The `.cast.ron` owns *when* a hit fires (deterministic fixed-timestep
  phase/window timing); obelisk owns *what* it does (damage/effects/triggers). obelisk has no
  timing fields, so a windup on a rules-instant skill is legal and expected.
- **Speed scaling.** Authored phase durations are *base* values; the timeline divides them by the
  caster's effective cast/attack speed at cast start (`effective_rate`: `is_spell()`→cast_speed,
  `is_attack()`→attack_speed, ×`attack_speed_modifier`). Projectile world-motion speed is NOT
  scaled. Cooldown uses `Skill::effective_cooldown`, not the rate.
- **Overlap via `SpatialQuery`, not collision events.** Two kinematic sensors don't reliably emit
  Avian collision events; `detect_overlaps` queries `shape_intersections` per active hitbox each
  FixedUpdate. This also sidesteps same-tick physics ordering. Hitboxes are plain Transform+`Hitbox`
  entities (not physics bodies); only hurtboxes are colliders.
- **Hurtbox = collider on the owner entity.** `insert_hurtbox` puts `RigidBody::Static` + `Collider`
  on the combatant itself. **Avian 0.4 `RigidBody::Static` DOES track Transform changes** (static ≠
  frozen position — it re-reads the entity Transform each step), so hurtboxes follow moving owners.
  (Empirically verified in `spatial/detect.rs::hurtbox_tracks_a_moving_owner`.)
- **Team filtering at resolution time.** `HitFilter` (Caster/Allies/Enemies/All) is applied against
  `Faction` in `detect_overlaps`, not baked into static collision layers (which can't express N
  dynamic teams). `friendly_fire` is a runtime concern.
- **Concurrent casts → reject `AlreadyCasting`.** `validate_casts` rejects a new cast if the caster
  already has an `ActiveCast`; the in-flight cast continues. Caller can `interrupt_cast` first.
  (Decided with the maintainer.)
- **Trigger cascades are surfaced + on-hit triggered skills auto-fire.** `add_effect` /
  `use_skill_against` return `Vec<TriggeredEffect>`; these are NEVER silently dropped. Every trigger
  emits a `TriggerFired` event; on-hit triggered *skills* are auto-fired against the hit target via
  `to_damage_packet` → `resolve_damage_with_rng`, in a depth-guarded worklist (cap 8) drawing from
  the seeded RNG. **Boundary:** `resolve_damage_with_triggers`'s `on_kill_packets`/`defender_packets`
  (splash/counter routing) need game-level target selection and are NOT auto-resolved yet (a
  documented limitation — surface them to the game if you extend this).
- **Netcode dual-emit.** In-process observers (`On<DamageResolved>` …) drive VFX/UI; a parallel
  buffered `NetEvent` stream (serde, **stable String ids** not `Entity`) is the replication egress.
  `ObeliskNetPlugin` mirrors gameplay events via `Entity→ObeliskId` (`ObeliskEntityIndex`). If an
  entity has no `ObeliskId`, the mirror **skips + warns** (never writes an empty-string id).
  **Invariant: `ObeliskId` must equal the entity's `StatBlock.id`** — `make_combatant` enforces it.
- **Obelisk-native API names** (Skill/StatBlock-as-`Attributes`/Effect/SkillTag); GAS mapping is
  docs-only.

### Confirmed external facts (hard-won — don't re-derive)
- **Versions: Bevy `0.17` ↔ Avian `0.4`** (Avian 0.5–0.6 target Bevy 0.18). `bevy_common_assets` not
  used; the RON loader is hand-rolled (`ron` crate).
- **Headless app recipe** (manual `App`, not `run()`): `MinimalPlugins` + `AssetPlugin` (set
  `file_path:"."` for tests vs default `"assets"` for `DefaultPlugins`) + `bevy::mesh::MeshPlugin`
  + `bevy::scene::ScenePlugin` + `PhysicsPlugins::new(FixedUpdate)` + `TimeUpdateStrategy::
  ManualDuration(1/60s)` + `Time::<Fixed>::from_hz(60)`, then **`app.finish(); app.cleanup();`
  before the first `update()`** (loaders register in `finish`). A static collider is visible to
  `SpatialQuery` from the **2nd** `update()`.
- **Buffered events = `#[derive(Message)]` + `app.add_message::<T>()` + `MessageWriter::write` +
  `MessageReader::read`** (`add_event`/`EventWriter` are deprecated in 0.17). Observer events =
  `#[derive(Event)]` + `commands.trigger` + `On<E>`.
- **EntityCommand closures**: `self.queue(move |mut e: EntityWorldMut| { e.get_mut::<T>()… })`; fire
  an event from inside via `e.world_scope(|w| w.trigger(ev))`.
- `Entity::from_raw` was removed → use `Entity::from_raw_u32(n).unwrap()` in tests.
- `StatAccumulator` is at `stat_core::stat_block::StatAccumulator` (aggregator module is private);
  flat life is `stats.life_flat += x`.

### obelisk API footguns
- **Skill loaders** return `HashMap<String, Skill>` at `stat_core::config::{load_skills,
  load_skills_dir, parse_skills}` (the `skills` submodule is **private** — use the flat re-export).
  `parse_skills` wants a `[[skills]]` array; single-skill files use top-level `id`. **Do NOT use
  `default_skills()`/`load_skill_configs()`** — they return the legacy `DamagePacketGenerator`.
- **Globals are OnceLocks; `init_*` panic on a second call.** Use idempotent `ensure_constants_
  initialized()` (defaults) where possible; guard custom-path `init_constants(path)` /
  `init_effect_registry(dir)` with `*_initialized()` (no `ensure_*` variant loads from a path/dir;
  `ensure_effect_registry_initialized()` inits an EMPTY registry). Tests use the `Once`-guarded
  `testkit::init_test_obelisk()`.
- `Effect` has `total_duration` + `timers: StackTimers`, **no flat `duration`** (the *config* TOML
  uses `duration`, which maps to `total_duration`).
- `tick_effects(&self, delta) -> (StatBlock, TickResult)` is **immutable** — reassign the returned
  block. `TickResult` has only total `dot_damage` (no per-effect breakdown — so `DotTicked.effect_id`
  is currently empty; a real fix needs an obelisk-side `TickResult` change).
- Drop-table TOML: `[table] id` + `[[table.rolls]]` + `[[entries]]`; `DropTableRegistry::
  load_from_strings` requires **`.toml`-suffixed** path keys.

---

## Feature set

- **Spatiotemporal skills** — `.cast.ron` timelines: windup/active/recovery phases (speed-scaled),
  collision windows with shapes (sphere/capsule/**cone-sector**), projectile motion, targeting
  (SelfCast/SingleEntity/Direction/Cone) + delivery (Melee/Instant/Projectile).
- **Hit/hurt boxes** — Avian-backed hurtbox sensors + transient hitbox volumes; faction-aware
  `HitFilter`, `HitMode` (OncePerTarget/FirstOnly/EveryTick) + re-hit intervals; `SpatialQuery`
  detection; spatial target acquisition.
- **Casting primitives** — `cast_skill_at` / `cast_skill_at_point` / `cast_skill_dir`; authoritative
  cast state machine; validation (range, LOS, mana, cooldown, already-casting).
- **Deterministic combat** — seeded resolve funnel; statuses/DoTs tick on FixedUpdate; triggered-
  effect cascades (surfaced + on-hit auto-fire); cooldowns.
- **Consumer facades & verbs** — `ObeliskRead`/`ObeliskSpatial`/`ObeliskCombat` SystemParams;
  `make_combatant`/`apply_obelisk_effect`/`grant_skill`/`grant_barrier`/`grant_elude`/
  `apply_stat_sources`.
- **VFX-sequencing hooks** — `vfx_cues` → `CueEvent { cue_id, source, position, kind }`; bind with
  `app.observe_cue(id, handler)`.
- **Server-authoritative netcode** — serializable `NetEvent` egress (stable ids) + headless server
  example.
- **Content** — skill-tree stat sources (`apply_stat_sources`), loot drop-tables on death
  (`LootDropped`).

---

## API surface (the consumer touches via `obelisk_bevy::prelude::*`)

- **Plugins:** `ObeliskPlugins` (group), `ObeliskSimPlugin`, `ObeliskSet`.
- **Components:** `Combatant` (required-components root), `Attributes`(StatBlock), `Faction`,
  `SkillSlots`, `ObeliskId`, `Hitbox`/`Hurtbox` (+`insert_hurtbox`), `ActiveCast`, `DropTableId`.
- **Assets:** `CastTimeline` + `CastTimelineHandles` (`.cast.ron`).
- **Events (observe via `app.add_observer(|e: On<…>|)`):** `CastBegan`, `CastRejected`(`CastRejectReason`),
  `CastPhaseChanged`, `HitWindowOpened`, `HitConfirmed`, `DamageResolved`, `EffectApplied`,
  `EffectExpired`, `DotTicked`, `EntityDied`, `TriggerFired`, `CueEvent`, `CooldownStarted/Ready`,
  `LootDropped`.
- **Resources:** `SkillRegistry`, `CombatRng`, `Cooldowns`, `ObeliskEntityIndex`, `CastTimelineHandles`,
  `DropTables`, `ItemGenerator`.
- **App-builder (ObeliskConfigExt / ObeliskCueExt):** `add_obelisk_config_constants_default`,
  `add_obelisk_config_constants(path)`, `add_obelisk_effects(dir)`, `add_obelisk_skills(SkillSource)`,
  `seed_combat_rng(u64)`, `observe_cue(id, handler)`.
- **EntityCommands (ObeliskCommandsExt + CastSkillExt):** `make_combatant`, `apply_obelisk_effect`,
  `grant_skill`, `grant_barrier`, `grant_elude`, `apply_stat_sources`, `cast_skill_at[_point/_dir]`,
  `interrupt_cast`.
- **SystemParam facades:** `ObeliskRead` (`life_of`/`mana_of`/`can_cast`/`has_effect`/…),
  `ObeliskSpatial` (`nearest_enemy`/`enemies_in_range`/`cone_targets`/`raycast_target`/`los_clear`),
  `ObeliskCombat` (`resolve_skill_hit`/`resolve_aoe`).
- **Netcode:** `NetEvent` (drain with `MessageReader<NetEvent>`).

---

## Usage cases

**Client / single-player:**
```rust
App::new().add_plugins(DefaultPlugins).add_plugins(ObeliskPlugins)
    .insert_resource(Time::<Fixed>::from_hz(60.0));
app.add_obelisk_config_constants_default();
app.add_obelisk_skills(SkillSource::Dir("config/skills".into()));
app.seed_combat_rng(seed);
app.observe_cue("firebolt_impact", |cue, cmds| { /* spawn VFX at cue.position */ });
// spawn: commands.spawn_empty().make_combatant(StatBlock::with_id("player")).insert(Faction::Player).grant_skill("firebolt");
// cast:  commands.entity(player).cast_skill_at("firebolt", enemy);
```
Authoring a skill = a `skills/<id>.toml` (rules) + an `assets/skills/<id>.cast.ron` (timeline);
register the handle into `CastTimelineHandles` keyed by skill id.

**Headless / server:** `default-features = false` (presentation uncompilable), `MinimalPlugins +
AssetPlugin + ObeliskSimPlugin`; drain `MessageReader<NetEvent>` each frame and replicate. See
`examples/headless_server.rs`. The wire format is engine-neutral (stable string ids + serde).

**Deterministic testing:** `obelisk_bevy::testkit::ObeliskTestApp::new(seed)` + `advance_ticks(n)` +
`rec()` (an `EventRecorder`). Same seed + same scenario ⇒ identical event stream. See
`tests/*.rs` for patterns (firebolt slice, cone cleave, facades, netcode drain, cue firing).

---

## Known limitations / deferred (documented, not bugs)

- `on_kill`/splash/counter-attack packets from `resolve_damage_with_triggers` aren't auto-routed
  (need game-level target selection); `apply_obelisk_effect`'s triggers are surfaced via
  `TriggerFired` but not auto-fired from the command closure.
- `DotTicked.effect_id` is empty (the rollup) — `TickResult` has no per-effect breakdown; needs an
  obelisk-side change.
- In-process `EntityEvent` propagation to parent rigs is not implemented (events are global with an
  `Entity`/`source` field — observe globally + filter).
- Full bevy render-feature trim for a smaller server binary is not done (`--no-default-features`
  excludes `present`, but bevy keeps default features).
- Full `Item` generation pipeline on drop (rolling `Drop::Item` through `ItemGenerator` + currencies)
  is wired but not exercised; transport (sockets) is the game's choice (`bevy_replicon`/`lightyear`).

## Process artifacts
Specs + plans live in `docs/superpowers/`. The design spec
(`docs/superpowers/specs/2026-06-04-obelisk-bevy-plugin-design.md`) and the per-batch plans are the
authoritative record of intent + the verification lists. Remote: `github.com/LukeThayer/bevy-obelisk`.
