# Editor Integration Understanding — `bevy_modal_editor` → `obelisk-arena` skill designer

Repo roots (all paths below are relative to these):
- Editor: `/Users/luke/src/bevy_modal_editor`
- Game-API crate: `/Users/luke/src/bevy_modal_editor/crates/bevy_editor_game`
- Particle engine: `/Users/luke/src/bevy_modal_editor/crates/bevy_vfx`
- Game: `/Users/luke/src/obelisk-arena` (crate `arena_game`, binding crate `arena_skills`)
- Combat lib: `/Users/luke/src/obelisk-bevy`

Verification status: every API the design brief assumed was checked against current code and is accurate **except where flagged**. Two reader claims were wrong and are corrected here: `EffectAction` has **13** variants, not 21 (`src/effects/data.rs:142-209`, indices 0-12); the "sequencer" is a trigger→action rule list, **not** a keyframe/track timeline.

---

## 1. What the editor is + what it provides

`bevy_modal_editor` is a keyboard-first ("modal", vim-style) 3D level editor shipped as a single Bevy plugin, `EditorPlugin` (`src/editor/plugin.rs:112`), built on Avian3D + bevy_egui and designed to be embedded in a host game. `EditorPlugin::build` (`src/editor/plugin.rs:182-283`) conditionally adds third-party plugins only when absent (egui, Avian `PhysicsPlugins`+`PhysicsDebugPlugin` guarded by `Time<Physics>` presence at `:191`, Bevy Remote Protocol `RemotePlugin`+`RemoteHttpPlugin` at `:201-206`, `FrameTimeDiagnosticsPlugin`), then composes ~28 internal sub-plugins: `AssetLibraryPlugin`, `OutlinePlugin`, `GridMaterialPlugin`, gaussian-splatting sub-plugins, `WireframePlugin`, `MaterialsPlugin`, `EditorStatePlugin`, `EditorInputPlugin`, `EditorCameraPlugin`, `CameraMarksPlugin`, `InsertModePlugin`, `MeshModelPlugin`, `SplineEditPlugin`, `SceneLoadingPlugin`, `SplineFollowPlugin`, `ProceduralPlugin`, `SelectionPlugin`, `EditorGizmosPlugin`, `ScenePlugin`, `PrefabsPlugin`, `CommandsPlugin`, `VfxEditorPlugin`, `EffectPlugin`, `NavigationPlugin`, `UiPlugin` (`src/editor/plugin.rs:216-267`).

`EditorPluginConfig` (`src/editor/plugin.rs:55-64`) is the entire config surface: four bools — `add_egui`, `add_physics`, `pause_physics_on_startup`, `add_ambient_light` (defaults all-on except pause). Convenience ctors `without_egui()` / `without_physics()` (`:132,:143`) and free fn `recommended_image_plugin()` (`:167`). The guards mean a host that already owns egui/Avian (arena_game does) can coexist by setting those bools false (or letting the resource/plugin guards skip them).

Modal modes are a `States` enum `EditorMode` (`src/editor/state.rs:21-46`) with **11** variants: View, Edit, Insert, ObjectInspector, Hierarchy, Blockout, Material, Camera, Particle, AI, Effect. **Particle** drives the bevy_vfx particle engine; **Effect** drives the effect sequencer — these two are the existing skill-designer foundation. Mode keys are a hardcoded match in `handle_mode_input` (`src/editor/input.rs:40`), gated so a mode is only enterable from View unless Shift is held. There is **no data-driven keybinding/command registry** — adding a mode means editing the `EditorMode` enum + an input branch + `panel_side()` (`state.rs:57`) + a panel plugin.

Two "command" systems: undo/redo via full-scene RON **snapshots** (`SnapshotHistory`, `src/commands/history.rs:25`; snapshots skipped unless `GameState::Editing`), and the fuzzy command palette (`src/ui/command_palette/`) with a hardcoded `CommandAction` enum and 12 `PaletteMode`s (already includes `ParticlePreset`/`EffectPreset`). Palette commands are not externally registrable except via the custom-entity registry.

---

## 2. The game ↔ editor integration contract

The host depends on the lightweight `bevy_editor_game` crate (`crates/bevy_editor_game/src/lib.rs`) — **types/traits/events only, no systems** (its own doc, `lib.rs:1-12`). The editor crate re-exports it (`src/lib.rs:62,80-100`).

### Embedding + play-mode (the START/STOP hooks)
- Host adds `DefaultPlugins` → `EditorPlugin::new(EditorPluginConfig{ add_physics:false, add_egui:false, .. })` → **`GamePlugin` separately**. `GamePlugin` (`src/editor/game.rs:24`) is **NOT** added by `EditorPlugin::build`; it is only re-exported (`src/lib.rs:65`) and every demo adds it explicitly (`crates/marble_demo/src/main.rs:51`). Without it the `GameState` type and Play/Pause/Reset messages still exist (registered by `EditorStatePlugin`, `src/editor/state.rs:372-405`) but nothing transitions — no play-mode.
- `GameState` (`bevy_editor_game/src/lib.rs:103-112`) is a `States` enum: `Editing` (default), `Playing`, `Paused`.
- Two message families: **input** `PlayEvent`/`PauseEvent`/`ResetEvent` (`lib.rs:158-167`) drive transitions; **lifecycle** `GameStartedEvent`/`GameResumedEvent`/`GamePausedEvent`/`GameResetEvent` (`lib.rs:173-187`) are editor→game notifications. All are Bevy `Message`s (read with `MessageReader`).
- State machine in `src/editor/game.rs`: `handle_play_input` (`:38`) maps F4=Play/Resume, F6=Pause, F7=Reset, Escape-while-playing=Pause; the same events come from egui buttons in `draw_play_controls` (`:256`). Events are turned into exclusive-world `Command`s:
  - `PlayCommand` (`:149`) — from Editing it snapshots the `SceneEntity` set via `build_editor_scene` → serialized RON in `GameSnapshot.data` (`:19-22,:158-167`), sets `Time<Physics>` speed 1.0, flips `EditorState.editor_active/ui_enabled/gizmos_visible=false`, hides grid + physics gizmos, `NextState(Playing)`, fires `GameStartedEvent` (or `GameResumedEvent` from Paused, no re-snapshot).
  - `PauseCommand` (`:213`) — physics speed 0, re-enables editor UI, forces `EditorMode::View`, `NextState(Paused)`, fires `GamePausedEvent`.
  - `ResetCommand` (`:323`) — despawns every `GameEntity`-tagged entity, clears `Selected`, `restore_scene_from_data` from the snapshot, rebuilds the Avian spatial-query BVH, sets physics speed 1.0, `NextState(Editing)`, fires `GameResetEvent`.
- The editor does **not** launch a separate process — it enables physics + un-gates the host's own systems, which the host gates with `run_if(in_state(GameState::Playing))` (`crates/marble_demo/src/marble.rs:44`). Games spawn runtime entities tagged `GameEntity` (auto-despawned on reset) and cameras tagged `GameCamera`. Canonical example: `spawn_marble_on_game_start(mut events: MessageReader<GameStartedEvent>)` spawns the player tagged `GameEntity` (`marble.rs:50,82`).

Physics nuance: the `GameState` doc says "Editing = physics paused", but the real behavior is governed by `pause_physics_on_startup` (`plugin.rs:276-282`, deferred 3 frames) and `ResetCommand` actually sets speed **1.0** with the comment "normal editing state has physics running" (`game.rs:359-363`) so the spatial-query BVH stays fresh; `keep_spatial_query_updated` (`state.rs:606`) rebuilds the BVH from `Transform` whenever physics speed is 0. So "paused in Editing" is a soft default, not invariant.

### Custom entity / component registration (the "tags + data" model)
- `register_scene_component::<T: Component + GetTypeRegistration>()` (`bevy_editor_game/src/lib.rs:261`) — calls `register_type::<T>()` and pushes T into `SceneComponentRegistry` (`:215-236`); `SceneComponentRegistry::apply` folds each as `allow_component::<T>()` into `build_editor_scene` (`src/scene/mod.rs:112-114`). This is the no-fork path to "serialized + inspectable game data".
- `register_custom_entity::<T>(CustomEntityType)` (`bevy_editor_game/src/lib.rs:384`) — richest seam. `CustomEntityType` (`:300-320`): `name`, `category`, `keywords`, `default_position`, `spawn: fn(&mut Commands,Vec3,Quat)->Entity` (editor auto-adds `SceneEntity`/`Name`/`Selected`; game adds marker+Transform+Visibility+Collider), and three optional fn-pointers `draw_inspector: InspectorWidgetFn` (`fn(&mut World,Entity,&mut egui::Ui)->bool`, `:280`), `draw_gizmo: GizmoDrawFn` (`:284`), `regenerate: RegenerateFn` (`:290`). Registration internally calls `register_scene_component::<T>()` (`:390`) and stores a monomorphized `has_component::<T>` + `component_type_id` into `CustomEntityRegistry` (`:335`). Custom entities are surfaced as insertable palette commands (`SpawnCustomEntity(name)`) and into the inspector. Reference: `marble_demo` registers `SpawnPoint`/`GoalZone` (`main.rs:59-105`).
- Siblings: `register_validation(ValidationRule)` (`:867`), `register_gltf_library(path)` (`:48`) → indexes into `MeshLibrary`/`AnimationLibrary`/`SceneLibrary` (`:63,:69,:75`), `SpawnPrefabRequest` message (`:197`), `RegisterMaterialTypeExt` (re-exported `src/lib.rs:102`).

### Scene persistence
- `SceneEntity` (`src/scene/mod.rs:40`) is the master tag: selection raycasts only hit `SceneEntity` or `GameEntity` (`src/selection/selection.rs:48,262`) and require a physics `Collider`. `build_editor_scene` (`src/scene/mod.rs:51`) is the **single source of truth** for save/load + undo snapshots + play snapshots: `DynamicSceneBuilder::deny_all()` then an explicit allow-list. It already allow-lists `bevy_vfx::VfxSystem` (`:84`) and `crate::effects::EffectMarker` (`:86`); game components join via `SceneComponentRegistry::apply` (`:112`).
- Runtime components (Mesh3d/Collider/PointLight/material handles/particle playback) are **not** serialized — the model is "store a serializable MARKER, regenerate runtime state from it" via `regenerate_runtime_components` (`src/scene/mod.rs:121`) + per-type `RegenerateFn`. Save/load: `SaveSceneEvent`/`LoadSceneEvent` (`src/scene/serialization.rs`), RON scene + a `{path}.meta` sidecar holding library/resource data (`EditorMetadata`: camera marks, `MaterialLibrary`, `CameraRenderSettings`). `restore_scene_from_data` (`src/scene/mod.rs:669`) is shared by undo/redo and play-reset.

---

## 3. The entity tag / data / metadata model

A game component becomes editor-visible/editable through Bevy reflection: it must be in `AppTypeRegistry` (`register_type::<T>()`) **and** carry `ReflectComponent` (`#[reflect(Component)]`). The mandated derive set is `Component, Serialize, Deserialize, Clone, Reflect` + `#[reflect(Component)]` (editor skills `.claude/skills/add-scene-component.md`, `new-entity-type.md`). Three reflection-driven surfaces consume this: the Add-Component browser (`src/ui/component_browser.rs` — discovers every `ReflectComponent`, categorizes by crate path, reads `#[doc]`, gates "can instantiate" on `ReflectDefault`/`ReflectFromWorld`), the generic property editor (`src/ui/reflect_editor.rs` — renders structs/enums/primitives; **only switches Unit enum variants**, and does **not** edit `List`/`Map`/`Set` contents — renders length only; a real limitation for `Vec<Phase>`-shaped data), and the inspector dispatch (`src/ui/inspector.rs` — custom-entity widgets filtered by `has_component`, then a "Game Components" section from `SceneComponentRegistry`, then a Markers section).

Editor-owned tags the editor queries: `SceneEntity` (saved/selectable), `Selected` (`src/selection/selection.rs:19`), `GameEntity` (runtime, auto-despawned on reset, `bevy_editor_game/src/lib.rs:150`), `GameCamera` (`is_active` managed by editor, `:132`). So "annotations that improve editor handling" = `#[reflect(Component)]`+register → editable; `ReflectDefault` → addable; `#[doc]` → docs; `register_scene_component` → persisted + inspectable; `register_custom_entity` → palette entry + custom inspector/gizmo/regenerate; a `Collider` → raycast-selectable; `SceneEntity`/`GameEntity` → selectable/(saved).

The registries are the consistent extension idiom (`CustomEntityRegistry`, `SceneComponentRegistry`, `ValidationRegistry`, `CommandRegistry`) — all fn-pointer-in-a-Resource. What is **pluggable from game code**: scene components, custom entities, validation rules, GLTF libraries, material/VFX/effect presets. What is **hardcoded in the editor crate** (needs a fork/PR): keybindings, `EditorMode` variants, `PaletteMode`/`CommandAction` variants, and the core `build_editor_scene` allow-list entries.

---

## 4. Particle engine deep-dive (`crates/bevy_vfx`)

Data-first, GPU-driven, standalone (only deps: bevy + avian3d + serde + ron + fastrand). The authored asset is one component, `VfxSystem` (`crates/bevy_vfx/src/data.rs:18-29`): `emitters: Vec<EmitterDef>`, `params: Vec<VfxParam>`, `duration: f32` (0 = infinite), `looping: bool`. Each `EmitterDef` (`:57-77`) is a Niagara-style module stack: `spawn: SpawnModule`, `init: Vec<InitModule>`, `update: Vec<UpdateModule>`, `render: RenderModule`, `sim_space` (World/Local), `alpha_mode`.
- `SpawnModule` (`:222-244`): `Rate`, `Burst{count,interval,max_cycles,offset}`, `Once{count,offset}`, `Distance{spacing}`. The `offset` on Burst/Once is a **delay before firing** — directly usable for intra-phase sequencing.
- `InitModule` — **10** variants (`:421-447`), `ADD_OPTIONS` factory table at `:465`. `UpdateModule` — **18** variants (`:488-566`), `ADD_OPTIONS` at `:592`, including `SizeByLife(Curve)`, `ColorByLife(Gradient)`, `Scale3dByLife`, `OffsetByLife` — these are per-particle keyframe lanes.
- `RenderModule` (`:673`): exactly one of `Billboard`/`Ribbon`/`Mesh`; `BillboardConfig.texture: Option<String>` and `MeshParticleConfig.material_path: Option<String>` are asset-path/library references.
- `Curve<T>` / `Gradient` (`crates/bevy_vfx/src/curve.rs`): true keyframes at normalized life [0..1] with per-key easing; `.sample()` on CPU and `pack_for_gpu(8)` for GPU upload (`MAX_CURVE_KEYS = MAX_GRADIENT_KEYS = 8`). **This is the only real keyframe primitive in the codebase**, and it animates per-particle properties — NOT a multi-event skill timeline.

`VfxParam`/`VfxParamValue{Float,Vec3,Color,Curve}` (`data.rs:851-866`) are declared "future: bindable from game code / sequencer" and are **inert today**: the only references are `register_type` (`crates/bevy_vfx/src/lib.rs:128-129`); no extract/prepare/GPU/mesh/UI path reads `VfxSystem.params`. (The `params_buffer` in `gpu/prepare.rs:115-188` is the unrelated per-emitter `GpuEmitterParams` uniform — not `VfxParam`.) Wiring `VfxParam` through is the cleanest path to stat-driven skill VFX.

Runtime spawn (no editor needed): clone an effect from `VfxLibrary{effects: HashMap<String,VfxSystem>}` (`data.rs:873`) and insert `(VfxSystem, Transform, Visibility)`; `VfxPlugin` auto-inserts `VfxStartTime` and simulates it. `VfxRestart` marker resets. The crate has **no disk I/O** — that's the editor's job: `VfxEditorPlugin` (`src/vfx/mod.rs:19`) adds `bevy_vfx::VfxPlugin`, seeds `VfxLibrary` from `presets::default_presets()` (17 effects) at `PreStartup`, and RON-persists `assets/vfx/<name>.vfx.ron`. The authoring UI is `src/ui/vfx_editor.rs` (`EditorMode::Particle`, panel plugin `ui::vfx_editor::VfxEditorPlugin` added by `UiPlugin` at `src/ui/mod.rs:83` — note this is a **different** plugin from the same-named `src/vfx::VfxEditorPlugin` data/persistence plugin added in `plugin.rs:261`). The UI has interactive curve/gradient editing; preview is live because the entity simply has the `VfxSystem` component.

---

## 5. Sequencer deep-dive (`src/effects`) — what it actually is

The "sequencer" is the **Effect Sequencer**, a serialized trigger→action rule list — its own doc calls it "the effect sequencer system" (`src/effects/data.rs:1`). It is **NOT** a DAW track/keyframe timeline. The serialized unit is `EffectMarker{steps: Vec<EffectStep>}` (`data.rs:25`), a Reflect+Serde Component. Each `EffectStep{name, trigger, actions}` (`:41`) is one rule:
- `EffectTrigger` — **7** variants (`:56-71`): `AtTime(f32)`, `OnCollision{tag}`, `OnEffectEvent(String)`, `AfterRule{source_rule,delay}`, `RepeatingInterval{interval,max_count}`, `OnSpawn`, `AfterIdleTimeout{timeout}`. A hybrid of absolute-time + event/collision/chaining — closer to a reactive rule graph than a linear timeline.
- `EffectAction` — **13** variants (`:142-209`, indices 0-12): `SpawnPrimitive`, `SpawnParticle{tag,preset,at}`, `SetVelocity`, `ApplyImpulse`, `Despawn`, `EmitEvent(String)`, `SetGravity`, `SpawnDecal`, `SpawnGltf`, `SpawnEffect{preset,inherit_velocity}` (nested), `InsertComponent{target_tag,component_type,field_values}`, `RemoveComponent`, `TweenValue{property,from,to,duration,easing}`. Spawned entities are addressed by string `tag`; later steps reference the tag. `SpawnLocation` is `Offset(Vec3)` or `CollisionPoint` (`:366`).
- Each enum carries `variant_index`/`from_variant_index`/`VARIANT_LABELS` (`data.rs:92-133, 230-332`) that the inspector UI drives generically. **Variants are positional (indices hardcoded) — APPEND, never reorder, for RON back-compat.** There is no `version` field on `EffectMarker` → serde migration is manual.

Runtime: `EffectPlayback` (`data.rs:486`, non-serialized) is regenerated from the marker (`mod.rs:170`). `EffectPlugin` (`src/effects/mod.rs:26`) runs `detect_effect_collisions` → `advance_effects` → `advance_tweens` in **plain `Update`**, gated only on `any_with_component::<EffectPlayback>` and an internal `playback.state == Playing` check (`mod.rs:38-51, 255`) — it is **NOT** gated on `GameState`, and it advances on wall-clock `time.delta_secs()` (`mod.rs:259`). `execute_action` (`mod.rs:352`) dispatches; spawned children get an `EffectChild{effect_entity, tag}` marker (`data.rs:537`).

Important runtime gaps verified in code:
- `InsertComponent` is reflection-by-short-type-path but `field_values: HashMap<String,String>` is **dead** — explicitly `#[allow(dead_code)]` (`mod.rs:602-603`); `InsertComponentFromEffect` only inserts the `ReflectDefault` value (`mod.rs:616-653`). Driving typed obelisk params per-step requires implementing field application.
- `ApplyImpulse` just inserts `LinearVelocity` (identical to `SetVelocity`, `mod.rs:427-432`); `SpawnEffect.inherit_velocity` is a stubbed `LinearVelocity::ZERO` (`mod.rs:534-540`); `TweenProperty::Opacity`/`Custom` are runtime no-ops (`mod.rs:732-736`) — only `Scale` and `LightIntensity` tween.
- `SpawnParticle` hardcodes `Collider::sphere(physics::LIGHT_COLLIDER_RADIUS)` on spawned VFX (`mod.rs:414`) so `OnCollision` can fire.
- Effect-spawned children carry `EffectChild`, **not** `GameEntity` (`mod.rs:407-410`), so `ResetCommand` will **not** auto-clean them; only `cleanup_effect` (`mod.rs:744`) does. A skill that spawns hitboxes/projectiles during play would leak on Reset unless it tags them `GameEntity` or the sequence calls `cleanup_effect`.

Authoring UI: `EffectEditorPlugin` (`src/ui/effect_editor.rs:34`, added by `UiPlugin` at `src/ui/mod.rs:84`) draws a right-side panel in `EditorMode::Effect` with per-rule cards (trigger combo over 7, action combo over 13). Play/Pause/Stop buttons set `PlaybackState` directly (`effect_editor.rs:490-520`), **not** through `bevy_editor_game`'s lifecycle messages. `draw_mini_timeline` (`:1021`) plots only `AtTime` rules as dots on a per-second axis with a red playhead at `elapsed` (`:1110`) — but it is **display-only**: allocated `Sense::hover` (`:1031,:1093`), no drag/scrub/seek. You cannot scrub to a time or preview a paused frame.

Persistence: `EffectLibrary` (`mod.rs:18`) seeds from `presets::default_presets()` + loads `assets/effects/*.fx.ron` (`mod.rs:102`), auto-saves changes (`mod.rs:144`). `SpawnParticle`/`SpawnEffect` resolve presets by string name against `VfxLibrary`/`EffectLibrary` at runtime — unvalidated (dangling names default to empty `VfxSystem`, `mod.rs:399-403`).

**Two distinct "over time" axes — do not conflate:** (1) the effect sequencer (effect-level trigger→action rule list, elapsed in real seconds, `src/effects`); (2) bevy_vfx per-particle `Curve<T>`/`Gradient` over normalized [0..1] lifetime (`curve.rs`). A skill timeline maps onto (1); (2) is reusable as the keyframe primitive for any per-property lane.

---

## 6. Mapping to obelisk-arena + the skill-designer goal

The design's master-source model (umbrella spec) is "timeline-as-master-source": obelisk's `CastTimeline` is already the industry model, so the skill designer must author **into** it, not invent a parallel format on `EffectMarker`. The current obelisk types:
- `CastTimeline` (`obelisk-bevy/src/assets/mod.rs:5-15`): `skill_id`, `phase_durations{windup,active,recovery}`, `collision_windows: Vec<CollisionWindow>` (id, spawn_phase, spawn_offset, active_duration, shape, motion, hit_filter, hit_mode), `targeting`, `delivery`, `vfx_cues: HashMap<String,String>`. **CRITICAL: it derives `#[derive(Asset, TypePath, Debug, Clone, Deserialize)]` (`:5`) — it is a Bevy Asset, NOT a Component, and is `Deserialize`-only (no `Serialize`, no `Reflect`).** It is loaded from `.cast.ron` by a hand-rolled `CastTimelineLoader` (`:95`).
- `CueEvent{cue_id, source, position, kind}` (`obelisk-bevy/src/events.rs:151`) is an observer **`Event`** (fired via `commands.trigger`, observed via `app.observe_cue(id, handler)`, `obelisk-bevy/src/vfx.rs:7,66`), not a Bevy Message. Cues fire at `on_cast`/`on_window_<id>`/`on_hit` from authored `vfx_cues`. The sim is authoritative; cues/VFX/anim are pure client-side consumers.
- Obelisk runs `FixedUpdate` chained `Validate→Advance→Projectiles→ResolveHits→TickEffects`; overlap is detected via `SpatialQuery::shape_intersections` per tick, **not Avian collision events** (obelisk hitboxes are plain Transform+`Hitbox` entities, only hurtboxes are colliders); `ObeliskSpatialPlugin` adds Avian `PhysicsPlugins::new(FixedUpdate)`.
- The binding crate `arena_skills` **already exists and implements the seam**: `SkillFx` (`crates/arena_skills/src/lib.rs:18`), `SkillFxRegistry.by_cue: HashMap<String,Vec<LaneEvent>>` flattened from `*.skillfx.ron` (`:34,:44`), `LaneEvent` (`:71`), `CueMessage` wire type (`:145`), `cue_message_from_cue` egress (`:174`), `resolve_cue` consumer (`:184`). It is render-free and lightyear-free.

How the editor's primitives map onto this:
- The effect sequencer's `AtTime`/`AfterRule` steps ≈ phase boundaries; `SpawnParticle`/`SpawnEffect` ≈ per-phase visuals; `EmitEvent`/`OnEffectEvent` ≈ phase transitions; `InsertComponent`/`SpawnPrimitive`+collider ≈ a hitbox. So `EffectMarker` is structurally a skill timeline — but it is a **parallel** model the design explicitly rejects.
- bevy_vfx `VfxSystem` presets are exactly what a `LaneEvent::Particle{effect, socket}` references by name; `VfxLibrary` is the by-name handle store. Spawning by name at a position is `vfx_library.effects.get(id).cloned()` + insert (mirrors `EffectAction::SpawnParticle`, `mod.rs:396-419`).
- The editor's `register_gltf_library` → `AnimationLibrary` (`bevy_editor_game/src/lib.rs:69`, re-exported `src/lib.rs:98`) is the seam the Animation lane needs to pick wisp clips by name — though nothing in the editor currently drives an `AnimationPlayer` from effects.

---

## 7. Designed-intent vs reality reconciliation

What holds: every editor API the design brief assumed exists as described — `EditorPlugin`/`EditorPluginConfig` fields (`plugin.rs:55-64`), `GameState`+Play/Pause/Reset+lifecycle events (`bevy_editor_game/src/lib.rs:103-187`), `GameSnapshot`+`GameEntity`-driven reset (`game.rs:19,323`), `register_custom_entity`/`CustomEntityType` exact fields (`lib.rs:300-320`), `register_scene_component` (`:261`), `register_gltf_library`→Mesh/Animation/Scene libraries (`:48-77`), `SpawnPrefabRequest` (`:197`), `RegisterMaterialTypeExt`. Versions are aligned: editor, obelisk-bevy, and arena are **all Bevy 0.18 / Avian 0.5** (editor `Cargo.toml:10-11`, obelisk-bevy `Cargo.toml:15-16`, arena `Cargo.toml:11-15`) — Phase 0 migration is done; obelisk-bevy's CLAUDE.md "0.17/0.4" is **stale doc lag**, not the real version. avian3d is pinned at 0.5 by `lightyear_avian3d 0.26`.

What diverges (the load-bearing reconciliation):
1. **`CastTimeline` is an Asset, not a Component, and is `Deserialize`-only / not `Reflect`.** It therefore **cannot** ride `build_editor_scene` (which allow-lists Components) and **cannot** be inserted via `InsertComponentFromEffect` (needs `ReflectComponent`), and the editor's generic reflect inspector cannot edit it. Authoring "into `CastTimeline`" means the skill designer writes the `.cast.ron` **asset file** directly, and to round-trip it the editor would need obelisk-bevy to add `Serialize`(+`Reflect`) to `CastTimeline` and its enums — or the editor authors against a serializable mirror type. This is the single most important architectural gap to resolve with the user.
2. **The "sequencer" is a rule list, not a timeline UI.** The design already says "the editor today has particle primitives but no timeline editor — building that timeline UI is the bulk of Phase 3." Confirmed: `draw_mini_timeline` is `Sense::hover` display-only (`effect_editor.rs:1031,1093`); no scrub/seek/lanes/phases.
3. **Determinism/clocks differ.** `advance_effects` is plain `Update` + wall-clock `Time::delta` + `Commands`, ungated by `GameState` (`mod.rs:38-51,255`); obelisk is deterministic `FixedUpdate` + seeded RNG + observers. The editor sequencer is not rollback/lightyear-safe and is a different clock. A "Play the real skill" preview must run obelisk's own `FixedUpdate` pipeline, not the editor's effect executor, for the sim half.
4. **Hitbox/damage model mismatch.** The editor's `OnCollision` reads Avian contact events on `EffectChild` colliders; obelisk detects hits via `SpatialQuery::shape_intersections` with faction/HitMode filtering. The editor cannot author obelisk hits as physics contacts; it should author the `CastTimeline.collision_windows` data and let obelisk execute.
5. Minor: design brief's "EffectAction x21" is wrong (real: 13); `field_values` is dead; `VfxParam` is inert.

Net: the editor's **EditorPlugin embedding, GameState/lifecycle hooks, custom-entity/scene-component registration, particle engine, and the cue/lane binding (already in `arena_skills`) are real and usable today.** The skill designer's authoring target (`CastTimeline` + `.skillfx.ron`) is a game/asset format the editor must learn to read/write via a serializable bridge — not the editor's native `EffectMarker`/component-scene machinery — and the timeline UI + animation/hitbox lanes + deterministic preview are net-new.

---
**Most relevant files**: `/Users/luke/src/bevy_modal_editor/src/editor/plugin.rs`, `/Users/luke/src/bevy_modal_editor/src/editor/game.rs`, `/Users/luke/src/bevy_modal_editor/src/editor/state.rs`, `/Users/luke/src/bevy_modal_editor/crates/bevy_editor_game/src/lib.rs`, `/Users/luke/src/bevy_modal_editor/src/scene/mod.rs`, `/Users/luke/src/bevy_modal_editor/src/effects/data.rs`, `/Users/luke/src/bevy_modal_editor/src/effects/mod.rs`, `/Users/luke/src/bevy_modal_editor/src/ui/effect_editor.rs`, `/Users/luke/src/bevy_modal_editor/crates/bevy_vfx/src/data.rs`, `/Users/luke/src/bevy_modal_editor/crates/bevy_vfx/src/curve.rs`, `/Users/luke/src/bevy_modal_editor/src/vfx/mod.rs`, `/Users/luke/src/obelisk-bevy/src/assets/mod.rs`, `/Users/luke/src/obelisk-bevy/src/vfx.rs`, `/Users/luke/src/obelisk-arena/crates/arena_skills/src/lib.rs`.

---

## Integration contract (quick reference)

EMBED (host App build order): `DefaultPlugins` → `EditorPlugin::new(EditorPluginConfig{ add_egui:false, add_physics:false, pause_physics_on_startup:?, add_ambient_light:? })` (src/editor/plugin.rs:112,55) → `GamePlugin` SEPARATELY (src/editor/game.rs:24; NOT in EditorPlugin::build; re-exported src/lib.rs:65). For arena_game set add_physics:false (obelisk's ObeliskSpatialPlugin already adds Avian PhysicsPlugins) and add_egui to match what's present; the `Time<Physics>`/is_plugin_added guards (plugin.rs:186-211) also skip duplicates.

STATE: `GameState{Editing,Playing,Paused}` States enum (bevy_editor_game/src/lib.rs:103). Registered by EditorStatePlugin (src/editor/state.rs:372-405) so types exist even without GamePlugin.

LIFECYCLE MESSAGES (Bevy `Message`, MessageReader/Writer):
- IN (drive transitions): `PlayEvent`, `PauseEvent`, `ResetEvent` (lib.rs:158-167). Also hotkeys F4/F6/F7/Esc (game.rs:38).
- OUT (game reacts = START/STOP hooks): `GameStartedEvent`, `GameResumedEvent`, `GamePausedEvent`, `GameResetEvent` (lib.rs:173-187).
Pattern: gate sim systems `run_if(in_state(GameState::Playing))`; spawn runtime entities on `MessageReader<GameStartedEvent>` (marble_demo/src/marble.rs:44,50).

PLAY/RESET SEMANTICS: PlayCommand from Editing snapshots the `SceneEntity` set via build_editor_scene → RON in `GameSnapshot` resource (game.rs:149,19). ResetCommand despawns all `GameEntity`, restores snapshot, rebuilds spatial BVH, →Editing (game.rs:323).

TAGS the game adds to its own entities:
- `GameEntity` (lib.rs:150) → auto-despawned on Reset (use for projectiles/hitboxes/VFX spawned at play).
- `GameCamera` (lib.rs:132) → is_active managed by editor state.
- `SceneEntity` (src/scene/mod.rs:40) → selectable + saved/snapshotted (requires a `Collider` to be raycast-selectable).

REGISTRATION (App extension traits, build-time, no editor fork):
- `register_scene_component::<T: Component+GetTypeRegistration>()` (lib.rs:261) → reflect-register + add to build_editor_scene allow-list (SceneComponentRegistry, scene/mod.rs:112) + inspector "Game Components". Requires T: Component+Serialize+Deserialize+Clone+Reflect + #[reflect(Component)].
- `register_custom_entity::<T>(CustomEntityType{ name, category, keywords, default_position, spawn: fn(&mut Commands,Vec3,Quat)->Entity, draw_inspector: Option<fn(&mut World,Entity,&mut egui::Ui)->bool>, draw_gizmo: Option<fn(&mut Gizmos,&GlobalTransform)>, regenerate: Option<fn(&mut World,Entity)> })` (lib.rs:384,300) → palette/Insert entry + custom inspector/gizmo/regenerate; auto-calls register_scene_component.
- `register_validation(ValidationRule)` (lib.rs:867); `register_gltf_library(path)` → MeshLibrary/AnimationLibrary/SceneLibrary by name (lib.rs:48,63,69,75); `SpawnPrefabRequest{prefab_name,position,rotation}` message (lib.rs:197); `RegisterMaterialTypeExt` (src/lib.rs:102).

PERSISTENCE: `SaveSceneEvent`/`LoadSceneEvent` (src/scene/serialization.rs) → RON scene (allow-listed components only) + `{path}.meta` sidecar (libraries/resources). Marker→regenerate: store serializable marker, rebuild runtime via regenerate_runtime_components / RegenerateFn (scene/mod.rs:121). restore_scene_from_data shared by undo + reset (scene/mod.rs:669).

PARTICLE ENGINE (crate bevy_vfx, standalone — game can use without the editor): add `bevy_vfx::VfxPlugin`; populate `VfxLibrary{effects: HashMap<String,VfxSystem>}` (data.rs:873). Play an effect: `commands.spawn((vfx_library.effects.get(name).cloned().unwrap_or_default(), Transform, Visibility))`. Disk loaders live in the EDITOR (src/vfx/mod.rs:37 seeds default_presets + loads assets/vfx/*.vfx.ron) — a standalone game replicates that RON-load or depends on the editor crate.

SEQUENCER (src/effects): serialized `EffectMarker{steps: Vec<EffectStep{name, trigger: EffectTrigger(7 variants), actions: Vec<EffectAction(13 variants)>}>}` (data.rs:25,41,56,142). Runtime `EffectPlayback{state:Playing|Paused|Stopped,...}` regenerated from marker (data.rs:486; mod.rs:170). START = spawn/insert `(EffectMarker, EffectPlayback{state:Playing}, Transform)`; STOP = `cleanup_effect(&mut Commands,&mut EffectPlayback)` (mod.rs:744). NOTE: advance_effects runs in plain Update gated only on PlaybackState, NOT GameState (mod.rs:38-51).

OBELISK SEAM (already in arena_skills): obelisk emits observer `CueEvent{cue_id,source,position,kind}` (obelisk-bevy/src/events.rs:151) at on_cast/on_window_<id>/on_hit; bind via `app.observe_cue(id, handler)` (obelisk-bevy/src/vfx.rs:7). `arena_skills::SkillFxRegistry` maps cue_id→Vec<LaneEvent{Anim|Particle|Projectile}> loaded from `*.skillfx.ron` (crates/arena_skills/src/lib.rs:34,71,184). Master skill data = obelisk `CastTimeline` Asset from `.cast.ron` (obelisk-bevy/src/assets/mod.rs:5) — Deserialize-only, NOT a Component/Reflect.

---

## Skill-designer extension sketch

GOAL: an in-editor Skill Designer whose AUTHORING TARGET is obelisk's `CastTimeline` (.cast.ron) + the existing `.skillfx.ron` (arena_skills), with a track timeline UI, that previews via the REAL obelisk pipeline. The editor's particle engine + effect-sequencer are the reusable primitives, but NOT the serialization root.

EXTENSION SEAMS (grounded in code):
1. New mode `EditorMode::Skill`: extend the enum (src/editor/state.rs:21), add a key branch in handle_mode_input (src/editor/input.rs:40), a `panel_side()` entry (state.rs:57), and a panel plugin added by UiPlugin (src/ui/mod.rs:84) — exactly how Particle/Effect modes were added. This is an editor-crate edit (no data-driven keybind registry exists).
2. The timeline UI is net-new: grow `draw_mini_timeline` (src/ui/effect_editor.rs:1021) from a `Sense::hover` display strip into an interactive multi-lane scrubber. Lanes = Animation / Particles / Projectile / Hit-window / Cues, time-anchored to windup/active/recovery bands derived from `CastTimeline.phase_durations`. Reuse bevy_vfx `Curve<T>`/`Gradient` (crates/bevy_vfx/src/curve.rs) + the existing interactive curve widgets in src/ui/vfx_editor.rs for any per-property lane.
3. Author INTO obelisk data, not EffectMarker (the design's "no parallel model"). Because `CastTimeline` is a Deserialize-only Asset (obelisk-bevy/src/assets/mod.rs:5), the designer needs a serializable BRIDGE: either (a) add `Serialize`(+`Reflect`) to obelisk-bevy's CastTimeline/enums so the editor can round-trip `.cast.ron`, or (b) define an editor-side serializable mirror `SkillDef`/`CastTimelineEdit` (Component+Serialize+Deserialize+Clone+Reflect+#[reflect(Component)]) registered via `register_scene_component` (bevy_editor_game/src/lib.rs:261) that the designer edits and bakes to `.cast.ron`. Mirror precedent already in-tree: `BaseMaterialProps` mirrors StandardMaterial (bevy_editor_game/src/lib.rs:501). The cue lane edits `CastTimeline.vfx_cues` (HashMap<String,String>) + the `.skillfx.ron` `lanes` map (arena_skills/src/lib.rs:23); cue-key naming on_cast/on_window_<id>/on_hit + the CueMessage wire shape are LOCKED.

NEW TIMELINE DATA the lanes need (beyond what EffectMarker has):
- A first-class PHASE concept (windup/active/recovery with durations + labels) — absent from EffectMarker; comes free from `CastTimeline.phase_durations`. If extending EffectMarker instead, add `EffectTrigger::OnPhase{...}` (append-only; indices are positional, data.rs:92) — but prefer authoring obelisk phases directly.
- Hit-window events → author `CastTimeline.collision_windows` (id, spawn_phase, spawn_offset, active_duration, shape, hit_filter, hit_mode) — obelisk executes these via SpatialQuery; the editor must NOT emit hits as Avian collisions (its `OnCollision`/`EffectChild` model, mod.rs:185, doesn't match obelisk's shape_intersections).
- Per-lane events: Particle{effect_id, socket} → bevy_vfx VfxSystem by name; Projectile{effect_id, socket, speed} (obelisk owns the authoritative projectile via CastDelivery::Projectile, editor authors the cosmetic one); Anim{clip, layer} → resolve clip by name via `register_gltf_library`→AnimationLibrary (bevy_editor_game/src/lib.rs:69) on wisp character.glb. A SOCKET picker (named bones queried by Name, root fallback) is net-new.
- If gameplay actions ARE wanted in the editor sequencer, append `EffectAction::SpawnProjectile/EmitHitbox/DealDamage/PlayAnimation` (data.rs:142 + execute_action arm mod.rs:352 + draw_action_editor arm + VARIANT_LABELS) — but per the design these belong in obelisk; prefer the generic `InsertComponent` path. CAVEAT: `InsertComponent.field_values` is DEAD (mod.rs:602) — only ReflectDefault is inserted; per-step typed params need real reflection field-application work first.

SERIALIZE + REPLAY in arena_game:
- Round-trip = author → write skills/<id>.toml (rules, untouched) + assets/skills/<id>.cast.ron (timeline) + assets/skills/<id>.skillfx.ron (lanes) + referenced assets/vfx/*.vfx.ron presets. arena_skills::SkillFxRegistry::load_dir already flattens .skillfx.ron → by_cue (arena_skills/src/lib.rs:44). A SkillLibrary mirroring EffectLibrary/VfxLibrary auto-save (src/effects/mod.rs:144, src/vfx/mod.rs) could persist the editor mirror.
- "Play the real skill" preview: press Play (GameState→Playing, GameStartedEvent) → spawn a test caster + dummy tagged `GameEntity` (auto-cleaned on Reset, bevy_editor_game/src/lib.rs:150) → call obelisk `cast_skill_at` → obelisk's FixedUpdate Validate→Advance→Projectiles→ResolveHits→TickEffects runs the REAL phases/hitbox/damage, fires CueEvents → arena_skills binding resolves LaneEvents → editor bevy_vfx spawns the particles. Do NOT use the editor's advance_effects to execute the sim half — it's wall-clock Update, ungated by GameState, non-deterministic (mod.rs:38-51,255). The editor sequencer/EffectMarker can still be used for editor-only standalone VFX preview.
- Cleanup caveat: effect-spawned children carry EffectChild not GameEntity (mod.rs:407), so they survive Reset; any preview-spawned skill entities must be tagged GameEntity or torn down via cleanup_effect.

---

## Gaps (what the editor does NOT yet provide)

- No timeline/track UI: the only timeline is `draw_mini_timeline` (src/ui/effect_editor.rs:1021), display-only `Sense::hover` (1031,1093), plotting only `AtTime` dots + a read-only playhead — no scrub/seek/lanes/phase bands/loop/pause-preview.
- No first-class skill PHASE concept (windup/active/recovery) anywhere in the editor; EffectMarker has no phase model (src/effects/data.rs). Phases exist only in obelisk's CastTimeline.phase_durations.
- No projectile / hitbox-window / deal-damage / play-animation EffectAction variants — only the 13 generic ones (data.rs:142). Hitbox-as-collision doesn't match obelisk's SpatialQuery model.
- `EffectAction::InsertComponent.field_values` is DEAD CODE (#[allow(dead_code)], mod.rs:602-603): only the ReflectDefault value is inserted (mod.rs:616-653) — cannot drive typed obelisk params per step today.
- `VfxSystem.params` (VfxParam) is declared 'future' but entirely INERT — only register_type touches it (bevy_vfx/lib.rs:128-129); no extract/GPU/UI path reads it. Stat-driven skill VFX needs this wired first.
- No animation lane / AnimationPlayer driving: AnimationLibrary exists (bevy_editor_game/src/lib.rs:69) but effects never reference clips; no socket/bone-attachment system.
- Sequencer playback is NOT bound to GameState and runs on wall-clock `Time::delta` in plain Update (mod.rs:38-51,255); not deterministic / not rollback-safe for lightyear; uses Commands + HashMap iteration.
- Effects anchor to their own GlobalTransform (mod.rs:232), with no caster-origin / aim-direction / target context binding a skill needs.
- Effect-spawned children carry EffectChild not GameEntity (mod.rs:407), so ResetCommand won't clean them up — leak risk; only cleanup_effect (mod.rs:744) tears them down.
- Preset name references (SpawnParticle/SpawnEffect) are unvalidated — dangling names silently default to empty VfxSystem (mod.rs:399-403). No validation that referenced .vfx.ron/.fx.ron exist.
- EffectMarker has NO version field and positional enum indices (data.rs:92,230) — serde migration is manual; appending-only is required.
- The editor cannot natively author obelisk's CastTimeline: it's an Asset, Deserialize-only, not Reflect/Component (obelisk-bevy/src/assets/mod.rs:5) — it can't ride build_editor_scene or InsertComponent; a Serialize derive or a serializable mirror is required.
- No published extension API for new EffectMarker triggers/actions, EditorMode variants, keybindings, or palette commands — all hardcoded in the editor crate; a Skill mode + skill actions require editing/forking bevy_modal_editor (not just the bevy_editor_game registration surface).
- Generic reflect editor can't edit List/Map/Set contents or non-unit enum variants (src/ui/reflect_editor.rs) — a Vec<Phase>/Vec<LaneEvent> skill struct needs a custom draw_inspector.
- GPU curve fidelity capped at MAX_CURVE_KEYS = MAX_GRADIENT_KEYS = 8 (bevy_vfx/src/buffers.rs); curves resample beyond that.
- Disk loaders for VfxLibrary/EffectLibrary live in the editor crate (src/vfx/mod.rs, src/effects/mod.rs), not in bevy_vfx — a game wanting presets without the editor must replicate the RON-loading or depend on the editor crate (arena_game currently has the bevy_vfx dep DEFERRED, arena Cargo.toml:34).

---

## Open questions for the user

- CastTimeline authoring path: add `Serialize`+`Reflect` to obelisk-bevy's CastTimeline (so the editor round-trips .cast.ron directly), OR author against an editor-side serializable mirror (`SkillDef`) that bakes to .cast.ron? The first edits obelisk-bevy; the second keeps obelisk untouched but adds a mirror+bake step (precedent: BaseMaterialProps mirrors StandardMaterial).
- Is extending the editor crate (fork/PR: new EditorMode::Skill, timeline UI, optional skill EffectAction variants) acceptable, or must the skill designer stay strictly within the bevy_editor_game registration surface? There is no data-driven mode/keybind/action registration API today — a Skill mode requires editing the editor crate.
- Division of execution: should the editor only AUTHOR data consumed by obelisk's own runtime (recommended — sim authoritative, deterministic), with the editor's EffectPlugin used only for editor-local standalone VFX preview? Or is the editor expected to execute any gameplay (hitbox/damage)? This decides whether DealDamage/EnableHitbox become real executor actions vs. authored CastTimeline.collision_windows.
- Physics coexistence in one binary: editor `add_physics` vs obelisk's ObeliskSpatialPlugin (Avian PhysicsPlugins::new(FixedUpdate)) vs wisp/lightyear's avian-with-lightyear config (which disables some avian sub-plugins so lightyear owns Transform and replicates Position/Rotation). The is_plugin_added/Time<Physics> guards (plugin.rs:186-211) prevent double-registration, but the SCHEDULE (editor default vs obelisk FixedUpdate) and the Transform-vs-Position authority must be reconciled — set add_physics:false and confirm BVH/spatial behavior.
- Preview determinism: the editor's GameState::Playing toggles local Time<Physics> + in_state gating; obelisk needs FixedUpdate + seeded CombatRng + Time<Fixed>. Does 'Play the real skill' run obelisk's FixedUpdate pipeline cleanly under GameState::Playing, and does GameSnapshot/reset correctly restore obelisk combat state (ActiveCast, Cooldowns, CombatRng)? Skill preview is offline/local only, distinct from a live networked lightyear session?
- Animation + sockets are net-new: confirm wisp's character.glb named clips (IDLE/WALK/cast variants) surface through register_gltf_library→AnimationLibrary, and that a named-bone socket picker (wand_tip/hit_point, root fallback) must be built in arena_editor — neither exists in the editor today.
- Skill-designer v1 depth: timeline + 'Play the real skill' preview first (recommended), or a fuller sequencer? And how much of the lane vocabulary (LaneEvent variants beyond Anim/Particle/Projectile, timing offsets) to support in v1 — the .skillfx.ron format in arena_skills is intentionally extensible but its current LaneEvent set is minimal.
- Should authored skill effects be standalone assets (assets/skills/<id>.{cast.ron,skillfx.ron} + referenced .vfx.ron, like prefabs/MaterialLibrary) rather than embedded in the level scene RON? (Skills are reused across many entities, so standalone is the natural fit — confirm.)