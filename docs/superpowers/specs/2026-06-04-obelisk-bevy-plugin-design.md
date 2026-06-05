# obelisk-bevy — Design Spec

**Date:** 2026-06-04
**Status:** Approved design, pre-implementation
**Topic:** A Bevy plugin exposing the `obelisk` ARPG libraries (loot/stat/skill-tree/tables) to Bevy game developers, extended with a 3D + temporal skill model, hit/hurt boxes, skill-usage primitives, and VFX-sequencing hooks.

---

## 1. Goal & context

`obelisk` is a set of pure-Rust ARPG libraries (no ECS, no engine, no async): `loot_core`, `stat_core`, `skill_tree`, `tables_core`. `stat_core` already provides the hard parts of an ARPG:

- **Skills** — `Skill` with `Targeting` (SelfCast / SingleEnemy / None), `Delivery` (Melee / Projectile / Instant), `DamageConfig`, `EffectApplication`, cooldown/mana, `use_conditions`.
- **Triggered effects** — `TriggerCondition` evaluated across four phases (Pre-Calculation, Post-Calculation, Post-Resolution, Defensive-Resolution); `GlobalConditional` / `SkillCondition`.
- **Statuses / ailments** — a unified `Effect` (buffs/debuffs/DoTs) with `StackTimers` (Shared / PerStack), charges, `EffectTrigger` (OnApply / OnMaxStacks / OnExpire / OnConsume), driven by `tick_effects(delta)`.
- **Combat** — `DamagePacket` → `resolve_damage_with_rng` / `resolve_damage_with_triggers` → `CombatResult`.
- **StatBlock** — the entity stat container, mutated by `equip`, `add_effect`, `use_skill_against`, `tick_effects`, etc.

What obelisk **does not** have, and this plugin must add: any spatial/3D concept, range, area, cast timing, hit geometry, projectile motion, target acquisition, an automatic tick loop, an entity model, or events. The plugin grafts a **spatiotemporal + ECS + eventing** layer onto the pure-logic core and drives obelisk's existing pipelines from Bevy schedules.

### Decided constraints

| Decision | Choice |
|---|---|
| Bevy version | **0.17 / newest** (exact minor + Avian-compat version verified during implementation) |
| Spatial backend | **Avian3d** (sensors for hit/hurt detection, `SpatialQuery` for target acquisition) |
| Execution model | **Server-authoritative-ready**: a headless, deterministic, seeded-RNG simulation is the authority; presentation is a pure event consumer |
| First implementation scope | **Vertical slice first** (§11) — spec covers all features; first plan builds one end-to-end path |
| VFX consumption | **Events / observers only** — no built-in declarative timeline; the sim emits a rich event set |
| Cast timing authority | **Obelisk owns the authoritative timeline** — fixed-timestep phase timers open/close hit windows and emit timing events that animation/VFX sync *to* |
| Skill spatial/temporal data | **A new Bevy asset** (`CastTimelineAsset`, `.cast.ron`) that *references* an existing `stat_core::Skill` by id; forks nothing |
| Crate layout | **Single `obelisk-bevy` crate, modules only**; presentation behind a default-on `present` Cargo feature |
| Public API vocabulary | **Obelisk-native names** (Skill, StatBlock/Attributes, Effect, SkillTag); GAS mapping lives in docs only |

---

## 2. Crate & module layout

One crate, `obelisk-bevy`, organized into modules. The authoritative-sim ↔ presentation boundary is enforced by **module discipline + the `present` Cargo feature** (not a separate crate). A headless/server build compiles with `default-features = false`, which excludes the `present` module entirely so render/VFX code cannot link.

```
obelisk-bevy/
  Cargo.toml          # features: ["present"] default-on; "client" = present + bevy defaults; "server" = headless
  src/
    lib.rs            # ObeliskPlugins group, prelude, re-exports
    assets/           # CastTimelineAsset + loader + authoring enums + engine-neutral event payload structs
    core/             # StatBlock<->ECS bridge: Attributes, ObeliskId, entity index, globals init, registries,
                      #   RNG, FixedUpdate tick/regen/death, Effects application
    timeline/         # the "skill" module: ActiveCast state machine, phase timers, hit windows, cast verbs
    spatial/          # the "hitbox" module: Hitbox/Hurtbox, shape->Collider, teams/layers, overlap->hit,
                      #   projectiles, ObeliskSpatial target-query facade
    combat/           # the deterministic resolve funnel (ObeliskCombat), event emission
    present/          # (feature = "present") read-only observers -> VFX/audio/anim/floating text/icons/gizmos
    prelude.rs
  assets/             # example .cast.ron skills (shipped for examples/tests)
  examples/
    playground.rs     # visual 3D test binary
  tests/
    *.rs              # headless integration tests
```

**Engine-neutral payloads.** The event payload structs and the `CastTimelineAsset` authoring data live in `assets/` and must not reference Avian types. The `CollisionShape → avian3d::Collider` conversion and the physics-layer enum live in `spatial/`. This keeps the payloads serializable as a future netcode wire format without dragging the physics engine in.

**Sub-plugins.** `ObeliskSimPlugin` is composed of `ObeliskCorePlugin`, `ObeliskTimelinePlugin`, `ObeliskSpatialPlugin`, `ObeliskCombatPlugin` so a consumer can opt into subsets (e.g. stats-only, no spatial). `ObeliskPresentPlugin` is `#[cfg(feature = "present")]`.

---

## 3. Feature list (everything the plugin must support)

### 3.1 Core / stat bridge (`core/`)
1. `Attributes` component — newtype over `stat_core::StatBlock`; only sim systems hold `&mut`.
2. `Entity ↔ StatBlock.id` bidirectional index (`ObeliskEntityIndex`), auto-synced on spawn and on `RemovedComponents<ObeliskId>` (no leak on despawn). obelisk references actors by `String` id throughout (`StatBlock.id`, `DamagePacket.source_id`, `Effect.source_id`, `TriggeredEffect.skill_id`).
3. Idempotent global init: `ensure_constants_initialized` / `ensure_effect_registry_initialized` (NOT `init_*`, which panic on second call). Markers `ConstantsReady` / `EffectRegistryReady`.
4. `SkillRegistry` resource = `HashMap<String, stat_core::Skill>`, built via re-exported `load_skills` / `load_skills_dir` / `parse_skills` (which return `HashMap<String, Skill>`) or the `From<DamagePacketGenerator> for Skill` bridge over `default_skills()`. **Never** populate it from `default_skills()` / `load_skill_configs()` directly — those return `HashMap<String, DamagePacketGenerator>` (legacy) and will not type-check against `use_skill_against`.
5. Seeded `CombatRng` (ChaCha8) resource — the *only* RNG threaded into combat. No facade exposes a path to a global RNG.
6. FixedUpdate **tick** of effects/DoTs via `tick_effects(delta) -> (StatBlock, TickResult)`; apply `dot_damage`, emit `DotTicked` / `EffectExpired` / `EntityDied` / re-enter `triggered_effects`.
7. Resource regen / recovery (life/mana/barrier; elude regen timers) on FixedUpdate.
8. Effect application path that **captures `add_effect`'s returned `Vec<TriggeredEffect>`** (OnApply / OnMaxStacks cascades), re-entering the trigger funnel.
9. Stat rebuild scheduling when sources change (equipment, skill tree, expired stat-mod effects) via `rebuild_from_sources`.
10. Loot + skill-tree + drop-table integration: `ItemGenerator`, `DropTables` resources; `skill_tree::SkillTree::to_stat_source` → `apply_stat_sources`.
11. Cooldowns: a single `Cooldowns` source of truth keyed by `(Entity, skill_id)`, computed from `Skill::effective_cooldown(reduction)`. Emits `CooldownStarted` / `CooldownReady`.

### 3.2 Spatiotemporal skill / casting (`timeline/`)
12. `CastTimelineAsset` (§7) — phases, collision/hurtbox windows, shapes, motion, targeting, range, delivery, vfx-cue ids. References a `Skill` id.
13. `ActiveCast` component — authoritative cast state machine: handle, elapsed, `SkillPhase`, resolved targets, `fired_windows`. Driven by fixed-timestep timers.
14. Phase timeline: crossing a phase boundary emits `CastPhaseChanged`; opening/closing a collision window spawns/despawns a `Hitbox` and emits `HitWindowOpened` / `HitWindowClosed`.
15. "Use a skill" primitives — `cast_skill` / `cast_skill_at(Vec3)` / `cast_skill_dir(Dir3)` / `interrupt_cast` EntityCommands verbs → `CastRequested` → validation → `ActiveCast` or `CastRejected`.
16. Cast validation: range, line-of-sight, cooldown, mana + `use_conditions` (via `can_use_skill`), asset-loaded. Structured `CastRejectReason`.
17. Projectile motion for `Delivery::Projectile` (obelisk's projectile resolves instantly, so *flight* is new): `Projectile` component over an Avian Kinematic body; lifetime, pierce/chain counts, on-hit despawn rules.
18. Targeting types: SelfCast, SingleEntity, Direction/aim, GroundTargeted, Cone — each parameterized by range + delivery.
19. Channeled / repeating skills, multi-hit (`hits_per_attack`), movement/dash skills (feature-list; not all in the vertical slice).
20. Interrupt/cancel policy (resources are consumed up front by `use_skill_against` → decide refund policy: none / pre-consumption-only).

### 3.3 Hit / hurt boxes (`spatial/`)
21. `Hurtbox` component — defensive Avian sensor bound to an `Attributes` entity, with team/layer membership. Auto-added by `Combatant`.
22. `Hitbox` component — transient offensive sensor spawned per active window; carries `window_id`, owner, `skill_id`, `HitMode`, already-hit set.
23. Shape primitives: `Sphere`, `Capsule`, `Box`, `Cone`, `Projectile{radius}` → `to_collider()` (in `spatial/`).
24. `VolumeMotion`: `Static`, `Linear{speed}`, `Attached{to caster/bone}`, `Curve{…}` — integrated on FixedUpdate, giving the volume a 3D position that evolves over skill-local time.
25. Teams / factions (`Faction`) and friendly-fire: static `CollisionLayers` encode Hitbox/Hurtbox/Terrain membership; team filtering (`HitFilter` Caster/Allies/Enemies/All) is applied at **resolution time** against `Faction` (static layers can't express N dynamic teams or a friendly-fire toggle). `friendly_fire` is an `ObeliskSimConfig` flag.
26. Overlap → hit: `On<CollisionStart>` filters Hitbox↔Hurtbox + `Faction` + `HitMode` → `HitConfirmed`.
27. Hit filtering: `HitMode` OncePerTarget / FirstOnly / EveryTick; pierce/chain counts; ignore list; optional re-hit interval.
28. Hurtbox windows: parry / i-frame / block windows (`DamageMitigation`) active during specific phases.
29. Spatial target acquisition: `ObeliskSpatial` facade over `SpatialQuery` — `nearest_enemy`, `cone_targets`, `raycast_target`, `targets_in_range`, `los_clear`.

### 3.4 Combat resolution (`combat/`)
30. The deterministic resolve funnel `ObeliskCombat::resolve_skill_hit(caster, target, skill_id, window)`: `use_skill_against(&mut caster_sb, Some(&target_sb), skill, &registry, source_id, &mut rng)` → take `SkillResult.packets` → `resolve_damage_with_triggers(attacker, defender, packets, skill, &registry, &mut rng)` → **write the returned `StatBlock` back** → emit events. Cloning the target `StatBlock` (or `get_many_mut`) handles the `&mut caster + &target` borrow.
31. `resolve_aoe(caster, targets: &[Entity], skill_id, window)`: fan one cast over N targets with a **stable target sort** (by `StatBlock.id` / spawn index) before drawing from `CombatRng`, so collision-pair ordering can't perturb the seed sequence. Decide once-per-cast resource consumption vs per-target (recommended: consume + roll skill-level effects once at Active-window open; resolve per-target damage from the produced packets).
32. Never call `receive_damage` / `resolve_damage` (both use `thread_rng`).
33. Trigger cascade: re-enter the funnel for `TriggeredEffect`s (threading `CombatRng`) with a **depth/iteration guard** to bound re-entrant chains within one tick.
34. Status application channel: `PendingStatusEffect` → `add_effect`, capturing returned triggers.

### 3.5 VFX / presentation hooks (`present/`, feature-gated) — §6
35. A complete, well-typed event set with owned payloads carrying enough timing/identity/spatial context to sequence VFX/audio/animation against the authoritative timeline.
36. `CueEvent` layer + `observe_cue(cue_id, closure)` ergonomic registration.
37. Default presentation niceties: floating combat text, status-effect icons, debug gizmos for hit/hurtboxes (wrapping Avian's debug render). All read-only.

### 3.6 Test binary + automated-testing hooks — §8
38. `examples/playground.rs`: a visual 3D Bevy app (player, dummy enemies, cast skills, gizmo-rendered hit/hurtboxes, egui/overlay event log).
39. Headless integration-test harness: build a `MinimalPlugins + AssetPlugin + ObeliskPlugins::server()` app, step `FixedUpdate` deterministically, assert on emitted events / `StatBlock` state.
40. Automated-testing hooks: seeded RNG resource, an event-recorder resource, a scripted "scenario" format (intents queued at given ticks), `advance_ticks(n)` helpers, deterministic replay.
41. CI-runnable with no GPU (`cargo test`).

---

## 4. Public API surface (obelisk-native names)

### Plugins
- `ObeliskPlugins` — PluginGroup; `::client()` (sim + present) / `::server()` (sim only). `.set()/.disable()` per-plugin.
- `ObeliskSimPlugin` = `ObeliskCorePlugin` + `ObeliskTimelinePlugin` + `ObeliskSpatialPlugin` + `ObeliskCombatPlugin`.
- `ObeliskPresentPlugin` (`#[cfg(feature = "present")]`), `ObeliskDebugGizmosPlugin`.
- Re-export `avian3d::PhysicsPlugins` so consumers can tune `length_unit` / gravity.

### Components
- **Consumer inserts:** `Combatant` (`#[require(Attributes, SkillSlots, ObeliskId, Faction, Transform, Hurtbox)]`), `Attributes`, `Faction`, `SkillSlots`.
- **Auto / internal:** `ObeliskId`, `Hurtbox`, `ActiveCast`, `Hitbox`, `Projectile`.
- **Present-only:** `VfxCueBinding`, `FloatingText`, `StatusIcon`, `CueAnchor`.
- *Spawn note:* `Combatant` alone yields a valid-but-empty `StatBlock::default()`; real stats come from `make_combatant(stat_block)` at spawn. The "sim is the only mutator" rule is about *runtime* mutation; spawn-time construction is the documented single build path.

### Assets
- `CastTimelineAsset` (`.cast.ron`) + `CastTimelineLoader`. Authoring enums: `SkillPhase`, `CollisionShape`, `VolumeMotion`, `VolumeAttachment`, `HitFilter`, `HitMode`, `CastTargeting`, `CastDelivery`, `DamageMitigation`.
- Load-time validator: on `AssetEvent::Added`, confirm `skill_id` resolves in `SkillRegistry` and the timeline is well-formed; emit `CastTimelineValidationFailed` rather than failing silently at runtime.

### Resources
`SkillRegistry`, `CombatRng`, `ObeliskEntityIndex`, `ObeliskSimConfig`, `CastTimelineHandles`, `Cooldowns`, `ConstantsReady`/`EffectRegistryReady`, `ItemGenerator`, `DropTables`, and (present) `VfxCueRegistry`.

### Verbs (EntityCommands / App-builder)
- EntityCommands: `cast_skill` / `cast_skill_at` / `cast_skill_dir` / `interrupt_cast`, `make_combatant(StatBlock)`, `apply_stat_sources(Vec<Box<dyn StatSource>>)`, `grant_skill(id)`, `apply_obelisk_effect(id)`, `grant_barrier(amt)`, `grant_elude(n)`.
- App-builder: `add_obelisk_config(constants_path, effects_dir, skill_source)`, `seed_combat_rng(u64)`.

### SystemParam facades
- `ObeliskCombat` — authoritative `resolve_skill_hit` / `resolve_aoe`, `lookup_entity`, `stat_of`. Holds `SkillRegistry + &mut CombatRng + ObeliskEntityIndex + Query<&mut Attributes> + Assets<CastTimelineAsset>`.
- `ObeliskRead` — read-only HUD/UI: `life_of` / `max_life_of` / `mana_of` / `computed_stat`, `effect_config(id)` (icon/banner from `effect_registry().get`), `can_cast(entity, skill) -> Result<(), CastRejectReason>`.
- `ObeliskSpatial` — target queries (above).
- `CastDirector` — advances `ActiveCast` against the asset.
- `CueWriter` (present) — emits `CueEvent` with positions resolved from `CueAnchor`/`Transform`/`HitConfirmed.contact_point`.

### SystemSets (FixedUpdate) & run conditions
`ObeliskSet::Input → Validate → Advance → SpawnVolumes → ResolveHits → TickEffects → EmitEvents`; ordered around Avian's narrowphase (SpawnVolumes before, ResolveHits after — verify Avian schedule names). Presentation runs in `Update` after `Events`/`Messages` update. Run conditions: `constants_ready`, `effect_registry_ready`, `assets_ready`, `sim_authority_enabled`, `resource_exists::<SkillRegistry>`, `resource_exists::<CombatRng>` (each attached per-schedule).

---

## 5. Events & observers (the VFX/gameplay hook set)

**Dual-emit policy.** Each event is emitted as both (a) an `EntityEvent`/`Event` for synchronous in-process observers (`On<…>`) and (b) a buffered `Message` mirror for a server to drain/serialize/replicate (`MessageReader`). This resolves the "observers fire inside FixedUpdate, can't be drained in Update" mismatch and gives the netcode egress a real tool. All payloads are owned/cloned data (Entity ids + `Vec3` + numbers + cloned obelisk result structs).

| Event | Target / key payload |
|---|---|
| `CastRequested` (Message) | caster, slot_or_skill_id, target?/point?/dir? |
| `CastValidated` / `CastRejected` | caster, skill_id, `CastRejectReason` (obelisk `SkillUseError` vs plugin range/cooldown/LOS/not-loaded) |
| `CastBegan` (caster) | skill_id, profile handle, total_duration |
| `CastPhaseChanged` (caster) | skill_id, from/to `SkillPhase`, phase_duration, elapsed — **the VFX sequencing spine** |
| `CastCompleted` / `CastInterrupted` (caster) | skill_id, elapsed |
| `HitWindowOpened` / `HitWindowClosed` (caster) | window_id, skill_id, phase, shape, hitbox entity |
| `HitConfirmed` (target) | caster, target, window_id, skill_id, contact_point, normal (pre-resolution; contact geometry computed for VFX only — see §10) |
| `DamageResolved` (target + Message) | caster, target, skill_id, total_damage, per-type `damage_taken`, is_critical, is_killing_blow, oneshot_protection, barrier before/after, life before/after, life/mana on hit, effects_applied |
| `EffectApplied` (target) | target, effect_id, magnitude, total_duration, timers, stacks_after, source_id, source_skill, is_from_crit |
| `EffectExpired` (target) | target, effect_id, `EffectExpireReason`, stacks_removed |
| `DotTicked` (target) | target, effect_id, dot_damage, life_remaining |
| `TriggerFired` (+ Message) | source, target, skill_id, effect_id, stored_damage, stacks, magnitude, damage_type |
| `EntityDied` (target) | target, killer? (from in-flight `DamagePacket.source_id` via index — obelisk has no killer field), culled |
| `BarrierGranted` / `ManaSpent` / `EludeConsumed` / `CooldownStarted` / `CooldownReady` (Message) | lightweight UI cues |
| `CueEvent` (present) | cue_id, anchor, position, magnitude, is_crit, `CueKind`, `CuePayload` |

**Observers.** Sim-internal: `On<CollisionStart>` → filter → `HitConfirmed` → resolve funnel; trigger-cascade observer with depth guard. Consumer: `On<DamageResolved>`, `On<CastPhaseChanged>`, `On<HitWindowOpened>`, `On<EffectApplied/Expired>`, `On<DotTicked>`, `On<EntityDied>`, `On<CueEvent>`. Entity-targeted variants use the correct `#[event_target]` field annotation (target is `caster`/`target`, not a field literally named `entity`); optional `#[entity_event(propagate)]` requires hitboxes to be `ChildOf` the caster rig.

---

## 6. VFX sequencing model

VFX consume **events/observers only**; there is no built-in declarative timeline. Sequencing precision comes from the events carrying skill-local timing + identity + spatial context:

- **Phase-driven cues:** `CastPhaseChanged` (with `phase_duration` + `elapsed`) is the spine — windup glow, active flash, recovery fade key off it.
- **Window-driven cues:** `HitWindowOpened/Closed` mark exact spawn/despawn ticks of each geometric volume.
- **Impact cues:** `HitConfirmed` (contact point/normal) and `DamageResolved` (crit flag, damage breakdown, killing blow) drive sparks, floating numbers, screen shake.
- **Status cues:** `EffectApplied/Expired/DotTicked` drive buff auras / status icons / tick numbers.
- **Cue id layer:** `CastTimelineAsset.vfx_cues` carries cue-id *strings* bound to phases/windows; the sim re-emits them as `CueEvent`s. Consumers register handlers via `observe_cue("firebolt_impact", |cue, cmds| {…})` and resolve assets through `VfxCueRegistry`. No render data lives in the authoritative asset.

This is *two-tier*: advanced consumers observe raw authoritative events; casual consumers bind cue ids to particle/audio handles.

---

## 7. Spatiotemporal skill model (`CastTimelineAsset`)

```ron
(
  skill_id: "firebolt",                 // MUST match a Skill.id in skills.toml (validated at load)
  phase_durations: (windup: 0.3, active: 0.1, recovery: 0.2),   // channel optional
  collision_windows: [
    ( id: "bolt", spawn_phase: Active, spawn_offset: 0.0, active_duration: 2.0,
      shape: Projectile(radius: 0.5), motion: Linear(speed: 20.0),
      hit_filter: Enemies, hit_mode: FirstOnly ),
  ],
  hurtbox_windows: [],                  // parry / i-frame / block windows: { phase, duration, mitigation }
  targeting: SingleEntity(range: 15.0), // SelfCast | SingleEntity | Direction | GroundTargeted | Cone
  delivery: Projectile(speed: 20.0, gravity: 0.0),   // Melee | Instant | Projectile
  vfx_cues: { on_cast: "firebolt_cast", on_window_bolt: "firebolt_launch", on_hit: "firebolt_impact" },
)
```

- **Division of authority:** the asset owns *when* a hit fires (deterministic fixed-timestep phase/window timing); obelisk owns *what* the hit does (damage/effects/triggers). obelisk has no timing fields, so the asset's `phase_durations` are the sole timing authority — a 0.3s windup on a rules-instant skill is legal and expected. Document this.
- **Drive loop:** `ObeliskSet::Advance` adds `FixedTime` delta to `ActiveCast.elapsed`; phase crossings emit `CastPhaseChanged`. `SpawnVolumes` spawns an Avian Kinematic Sensor `Hitbox` when `elapsed` enters a window, with the converted collider + `CollisionLayers` filtered by `Faction`/`HitFilter`, emits `HitWindowOpened`, despawns after `active_duration`. `VolumeMotion` integrates the volume's transform each fixed step.

---

## 8. Test binary & automated testing

### `examples/playground.rs` (visual)
Full `DefaultPlugins` 3D app: a controllable player, dummy enemies with `Combatant`, hotkeys to cast example skills, Avian debug gizmos for hit/hurtboxes, an on-screen event log. For manual/visual validation.

### Headless harness (`tests/` + a `testkit` module behind a `test-support` feature)
- `ObeliskTestApp::new()` → `MinimalPlugins + AssetPlugin + ObeliskPlugins::server()` + fixed `Time::<Fixed>` + seeded `CombatRng` + an `EventRecorder` resource that captures all sim events.
- Helpers: `spawn_combatant(id, stat_block, faction, pos)`, `queue_cast(entity, skill, target)`, `advance_ticks(n)`, `events::<E>()`, `stat_of(entity)`.
- **Scenario format:** a list of `(tick, Intent)` the harness feeds deterministically, so integration tests (and automated agent-written tests) are declarative and reproducible.
- Determinism: same seed + same scenario ⇒ identical event stream and final `StatBlock`s. Snapshot/assert on the recorded event stream.
- No GPU; runs under `cargo test` in CI.

---

## 9. How obelisk maps in (verified entry points)

| obelisk system | Real entry point (verified) | Bevy surface |
|---|---|---|
| Skill use | `StatBlock::use_skill_against(&mut self, Option<&StatBlock>, &Skill, &HashMap<String,Skill>, String, &mut impl Rng) -> Result<SkillResult, SkillUseError>` (mod.rs:1467); consumes resources + applies self-effects internally | `ObeliskCombat::resolve_skill_hit` |
| Damage | `resolve_damage_with_rng(&StatBlock, &DamagePacket, &mut impl Rng)` (resolution.rs:31); `resolve_damage_with_triggers(attacker, defender, &[DamagePacket], &Skill, &registry, &mut rng) -> TriggerResolutionResult` (resolution.rs:390) | the resolve funnel; **never** `receive_damage`/`resolve_damage` (thread_rng) |
| Effects/DoTs | `add_effect(&mut self, Effect) -> Vec<TriggeredEffect>` (mod.rs:1692); `tick_effects(&self, f64) -> (StatBlock, TickResult)` (mod.rs:1843) | `apply_obelisk_effect` (captures returned triggers); `ObeliskSet::TickEffects` |
| Validation | `can_use_skill(&self, &Skill) -> bool` (mod.rs:1199) | `ObeliskRead::can_cast` (+ plugin cooldown/range/LOS) |
| Skill loading | `load_skills`/`load_skills_dir`/`parse_skills` → `HashMap<String,Skill>`; `From<DamagePacketGenerator> for Skill` | `add_obelisk_config` → `SkillRegistry` |
| Globals | `ensure_constants_initialized` / `ensure_effect_registry_initialized` (idempotent); `effect_registry()` is a `&'static OnceLock` | `add_obelisk_config`; `EffectRegistryReady` marker, not a per-App resource |
| Effect shape | `Effect { total_duration, timers: StackTimers (Shared/PerStack), magnitude, stacks, … }` — **no flat `duration`** | `EffectApplied` payload uses `total_duration` + `timers` |
| Identity | `StatBlock.id` / `source_id` / `Effect.source_id` are `String` | `ObeliskEntityIndex` bimap |
| Stat sources | `rebuild_from_sources`, `equip`, `skill_tree::SkillTree::to_stat_source` | `make_combatant` / `apply_stat_sources` |
| Loot / tables | `loot_core::Generator`/`Config`; `tables_core::DropTableRegistry` | `ItemGenerator` / `DropTables`, fired off `EntityDied` by the consumer |

---

## 10. Correctness constraints (non-negotiable)

1. **Determinism:** all randomness flows through the seeded `CombatRng`. `receive_damage`/`resolve_damage` are forbidden in the sim. AoE fan-out sorts targets by a stable key before drawing RNG.
2. **Idempotent globals:** `ensure_*_initialized` only; tolerate two Apps (client+server) in one process sharing the global `OnceLock`s — documented hazard.
3. **Registry type:** `SkillRegistry` is `HashMap<String, Skill>`, never `DamagePacketGenerator`.
4. **Trigger cascades:** capture and re-enter `Vec<TriggeredEffect>` from `add_effect` and `TickResult`/`CombatResult`; bound recursion with a depth guard.
5. **Authoritative/presentation boundary:** `present` is read-only and feature-gated; sim has no dependency on it; payload structs carry no Avian/render types.
6. **Borrow reality:** `use_skill_against` needs `&mut caster + &target`; resolve via clone or `get_many_mut`.

---

## 11. Vertical slice (first implementation plan target)

One end-to-end deterministic path, headless-test-covered and visually demoable:

> Spawn player (`Faction::Player`) + dummy (`Faction::Enemy`), each a `Combatant` with a real `StatBlock`. Load `firebolt.cast.ron`. `cast_skill_at(player, "firebolt", dummy_pos)` → validate (range/cooldown/mana) → `CastBegan` → windup→active (`CastPhaseChanged`) → spawn projectile `Hitbox` (`HitWindowOpened`) → Avian overlap with dummy `Hurtbox` (`HitConfirmed`) → `ObeliskCombat::resolve_skill_hit` (`use_skill_against` → `resolve_damage_with_triggers` → write-back) → `DamageResolved` (+ `EffectApplied` if firebolt applies burn) → `tick_effects` ticks burn (`DotTicked`) → dummy dies (`EntityDied`) → a `present` observer logs/draws each event.

Deliverables: the crate skeleton + `ObeliskCorePlugin` (init, registry, RNG, index, tick) + minimal `ObeliskTimelinePlugin` (windup/active/recovery + one window) + `ObeliskSpatialPlugin` (sphere/projectile hitbox, hurtbox, overlap) + `ObeliskCombatPlugin` (resolve funnel + events) + headless test asserting the event stream + a minimal `playground` example.

**Spike first (before committing the pipeline):** confirm Avian Kinematic-Sensor-vs-Sensor `CollisionStart` actually fires (§12 item 1). If not, adopt the fallback before building on it.

---

## 12. Open questions / verification list

**Highest risk:**
1. **Avian sensor-vs-sensor overlap.** Do two Kinematic Sensors (hitbox vs hurtbox) emit `CollisionStart`? Many engines suppress non-dynamic contacts. Fallbacks: one side `RigidBody::Dynamic` (gravity off) or resolve via `SpatialQuery` shape-intersection in `ResolveHits`. **Spike this first.**
2. **Contact geometry.** `CollisionStart` gives colliders/bodies, not a contact point/normal for sensors. `HitConfirmed.contact_point/normal` (VFX-only) must be computed separately or dropped from the authoritative payload.
3. **Same-tick spawn→narrowphase→resolve ordering** within one `FixedUpdate` step (Avian schedule/set names; whether a hitbox spawned this step is eligible this step or lands one tick late).

**Bevy 0.17 API:**
4. Required-component **default expressions** (e.g. forcing `RigidBody::Kinematic` on `Hitbox` insert) — exact syntax, else fall back to a Bundle.
5. `EntityEvent` `#[event_target]` field rules + `#[entity_event(propagate)]` and whether hitboxes are parented (`ChildOf`) for propagation.
6. `Event`/`EntityEvent` (observer) vs `Message` (buffered) dual-emit pattern; `Messages::update` timing across many FixedUpdate steps per render frame.
7. Headless `MinimalPlugins` + `Time::<Fixed>` + `ScheduleRunnerPlugin` config so `FixedUpdate` advances at a fixed cadence.

**Avian 0.x:**
8. Schedule/set names for §10 ordering; `LinearVelocity` vs manual `Transform` on Kinematic sensors for projectiles; `CollisionLayers::new` API; `PhysicsDebugPlugin` render-dep containment.

**obelisk source (confirm against pinned rev):**
9. Re-export paths/visibility of `load_skills`/`parse_skills` and `resolve_damage_with_rng`/`resolve_damage_with_triggers` (not in `stat_core::lib.rs` today). `calculate_damage_with_triggers` is `pub(crate)` — unusable.
10. `From<DamagePacketGenerator> for Skill` produces a fully-usable `Skill`, or require file-loading as the primary path.
11. `SkillResult` contents (does it carry the `packets` the funnel consumes?) and the exact split of responsibilities between `use_skill_against` and `resolve_damage_with_triggers` (avoid double-resolution).
12. Interrupt/refund: `use_skill_against` consumes resources up front and has no refund API — decide plugin-side policy.
13. `Effect` field confirmation (`total_duration` + `StackTimers`, no `duration`); buff-bar UI reconstructs remaining time from timers.
