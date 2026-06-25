# Context + Feasibility Brief: Editor-Driven Skill Designer for a 1v1 Online Fighting Game

**Date:** 2026-06-24
**Status:** Context-gathering / feasibility (pre-spec)
**Goal:** A new template repo integrating `bevy-obelisk` (combat/skill rules) + `bevy_modal_editor` (in-Bevy editor) + `lightyear` (netcode, via `wisp`) into a 1v1 online fighting game. The signature tool is an **editor-driven obelisk SKILL DESIGNER**: an editor sequencer/particle pipeline acting as the *master authoring source* for skills, integrating player animation + particles + projectiles + hitboxes.

---

## 1. The Three Pieces — What Each Provides Today

### 1a. `bevy_modal_editor` — in-Bevy editor + effect/sequencer/VFX pipeline

**Game-integration API (mature, this is the strongest part).**
- Embed via `.add_plugins(EditorPlugin::default())` (or `EditorPlugin::new(config)`). One monolithic plugin bundling asset libraries, scene management, selection, gizmos, UI, prefabs, materials, VFX, physics. `EditorPluginConfig` toggles `add_egui`, `add_physics`, `pause_physics_on_startup`, `add_ambient_light`.
- **Game lifecycle state machine** in `EditorStatePlugin`: `GameState` = Editing / Playing / Paused. Events: `GameStartedEvent`, `GameResumedEvent`, `GamePausedEvent`, `GameResetEvent`; commands `PlayEvent`/`PauseEvent`/`ResetEvent`. `GameSnapshot` taken on Editing→Playing; reset restores it and despawns `GameEntity`-tagged entities. Gate gameplay systems with `.run_if(in_state(GameState::Playing))`.
- **Custom entity registration:** `.register_custom_entity::<T>(CustomEntityType { name, category, keywords, default_position, spawn, draw_inspector?, draw_gizmo?, regenerate? })` (`bevy_editor_game::lib.rs`, `RegisterCustomEntityExt`). Appears in command palette / Insert mode. `marble_demo` registers `SpawnPoint` + `GoalZone` with gizmos and validation.
- **Scene serialization:** `.register_scene_component::<T>()` adds a type to the allowlisted scene snapshot. Scenes are `.scn.ron` (+ `.meta.ron` sidecar for camera marks, material library, render settings). `regenerate_runtime_components()` rebuilds non-serialized runtime state (meshes, materials, colliders, lights, anim clips) on load — games hook via `CustomEntityType.regenerate`.
- **Prefabs:** `assets/prefabs/{name}/{name}.scn.ron`; spawn at runtime via `SpawnPrefabRequest(name, pos, rot)`.
- **Custom inspectors / materials / gizmos / GLTF asset libraries** are all extensible. `register_material_type::<T>()`, `register_gltf_library(path)` (indexes `MeshLibrary`/`AnimationLibrary`/`SceneLibrary` by name).
- **Reference template:** `crates/marble_demo/src/main.rs`.

**Effect / VFX / sequencer pipeline (the part relevant to "skill designer").** There are **TWO distinct, currently-uncoupled** sequencer-ish systems:
- **(1) `EffectMarker` sequencer** (`src/effects/data.rs:19-32`): a serializable component holding `Vec<EffectStep>`; each step = `name` + `EffectTrigger` + `Vec<EffectAction>`. Triggers (7): `AtTime`, `RepeatingInterval`, `OnCollision`, `OnEffectEvent`, `AfterRule`, `OnSpawn`, `AfterIdleTimeout`. Actions (21): `SpawnPrimitive`, `SpawnParticle`, `SetVelocity`, `ApplyImpulse`, `Despawn`, `EmitEvent`, `SetGravity`, `SpawnDecal`, `SpawnGltf`, `SpawnEffect`, `InsertComponent`, `RemoveComponent`, `TweenValue`, … Serialized as `.fx.ron` in `assets/effects/`. Runtime `EffectPlayback` (not serialized) ticks via `advance_effects` (`effects/mod.rs:244`), fires triggers, executes actions (spawns `EffectChild` entities). Editor panel `draw_effect_panel` (`effect_editor.rs:278`) gives Play/Pause/Stop + a **mini timeline strip** showing `AtTime` trigger positions.
- **(2) VFX / particle system** (`bevy_vfx`, `VfxSystem` → `EmitterDef`): Niagara-style GPU emitters (`SpawnModule`, `InitModule[]`, `UpdateModule[]` (18 variants: gravity, drag, noise, orbit, size/color-by-life…), `RenderModule` (Billboard/Ribbon/Mesh)). Serialized `.vfx.ron`. Edited in a separate `vfx_editor.rs` with curve editors. Particles age independently on GPU; `VfxSystem` loops on its own clock.

**Critical reality about the "sequencer":**
- `EffectMarker` is a **step/trigger list with implicit time values**, NOT a keyframe/track editor. No scrubber, no curve editor, no track lanes, no rewind (preview is live GPU sim — restart-and-watch only).
- The two systems model different concepts: `EffectMarker` = orchestrate child spawns + collision + cross-step events; `VfxSystem` = emit + age particles. **No unified timeline couples them.** `EffectMarker::SpawnParticle` references a `VfxSystem` preset by **string name** (no validation; dangling refs possible).
- Extension is via Rust source edits + `Reflect` (add enum variant → update `label`/`variant_index`/`from_variant_index` + `execute_action` match + register type + hand-code egui form in `draw_action_card`). **No published extension API** for custom triggers/actions beyond reflection + source edits. No version field on `EffectMarker` → serde migrations are manual.

### 1b. `wisp` — lightyear netcode + character/animation foundation

A **fully operational** multiplayer FPS wizard game *and* an intended foundation library.
- **Netcode:** `lightyear 0.26.4` (`features = ["netcode","udp","avian3d","input_bei","interpolation"]`) + `avian3d 0.5`. Client-server, UDP. Server owns `NetworkedPlayer` entities; clients identify their rig via replicated `NetworkOwner`. **Server-authoritative controller** runs in `FixedUpdate`, ships pose back via hand-rolled `NetworkedPosition` each tick. Client runs an identical controller locally for snappy feel but **does not reconcile** (no rollback yet — Stage Q partial). Interpolation buffers two most-recent samples. Baseline: 0 divergence, <5ms replication latency on the test harness.
- **Known blocker carried by wisp:** `bei` 0.25 (local) vs `lightyear_inputs_bei` wanting `bei` 0.22 → two incompatible types in the dep graph. Pragmatic resolution: **hand-rolled `PlayerInputMessage`** (per-tick WASD+yaw+jump+cast flag) instead of lightyear-native input replication. Do NOT attempt lightyear-native bei input until lightyear bumps to bei 0.25+.
- **Models + animation (directly reusable for a fighting game):** single unified `character.glb` with class outfits + variants; full Mixamo-retargeted skeleton. Named anim clips (`visuals.rs`): IDLE, WALK_F/B/L/R, FALLING + casting variants. `build_graph_when_loaded` builds an `AnimationGraph` at startup. **Animation blending pipeline** (`apply_locomotion_blend`, `step_airborne_blend`, `step_casting_blend`) cross-fades clips via `AnimationPlayer` weights. **Remote anim sync is derived from `NetworkedPosition` deltas** — no extra network messages; latency-tolerant. Aim pitch overrides spine rotation locally + remotely.
- **Data-driven content patterns to inherit:** spells (`.spell.ron` + `.body.ron` + Rust handlers, 14 shipped), weapons (`.weapon.ron`), replicated props/portals/projectiles, health/damage + respawn. Server binary registers catalog/handler/trigger plugins so new `.ron` content works without recompile.
- **API surface:** `build_shared()` + `add_avian_with_lightyear()` exported from `lib.rs`. The latter disables avian's `PhysicsTransformPlugin`/`PhysicsInterpolationPlugin`/`IslandPlugin` so lightyear owns Transform↔Position sync. **Watch out:** writing `Transform` directly on physics bodies gets clobbered next tick — use avian `Position`/`Rotation`.
- Net test harness: `tools/net-test/run_session.sh` (observer client; assert diverged=0, max_latency_ms<5).

### 1c. `obelisk-bevy` — the skill/combat rules engine

**Skill = a mandatory two-file format:**
- **(a) Stat TOML** (`skills/*.toml`, owned by obelisk rules): skill id, targeting, delivery, damage packets, effects, conditions, cooldown. (e.g. `pummel.toml`: id, physical [5–5], melee.)
- **(b) Cast-timeline RON** (`assets/skills/*.cast.ron`, owned by Bevy presentation): `CastTimeline` asset (`src/assets/mod.rs:5-15`) with `skill_id`, `phase_durations` (windup/active/recovery as f32), `collision_windows: Vec<CollisionWindow>`, `targeting` (SelfCast / SingleEntity{range} / Direction{range} / Cone{angle,range}), `delivery` (Melee / Instant / Projectile{speed}), and **`vfx_cues: HashMap<String,String>`**.

**`CollisionWindow`** (the hitbox model): `id`, `spawn_phase` (Windup/Active/Recovery), `spawn_offset` (f32 within phase), `active_duration`, `shape` (Sphere/Capsule/Cone), `motion` (Static / Linear{speed}), `hit_filter` (Caster/Allies/Enemies/All), `hit_mode` (OncePerTarget/FirstOnly/EveryTick), `rehit_interval`. (Note: shape is static per window — no per-frame shape animation; animated visuals go through cues, not the collision model.)

**`vfx_cues` is the editor seam (this is the key finding).** Keys are deterministic lifecycle events: `on_cast`, `on_hit`, `on_window_<windowid>`. Values are authored cue ids. At runtime `ObeliskCuePlugin` (`src/vfx.rs:30-39`) observes `CastBegan` / `HitWindowOpened` / `HitConfirmed` and emits `CueEvent { cue_id, source, position, kind: OnCast|OnWindow|OnHit }` (`src/events.rs:148-156`). The game binds effects via `app.observe_cue(cue_id, handler)`. Example: `firebolt.cast.ron` → `vfx_cues: {'on_cast':'firebolt_cast', 'on_hit':'firebolt_impact'}`.

**Schedule (deterministic, FixedUpdate):** Validate → Advance (phase transitions, windows spawn) → Projectiles → ResolveHits → TickEffects. Phase durations are **speed-scaled at cast start** (effective_rate = caster speed × skill modifier); authored values are base. Golden-trace test suite exists (`tests/golden/*.trace`).

---

## 2. The Headline Blocker — Version Alignment (HARD PREREQUISITE)

**`obelisk-bevy` is on Bevy 0.17 / Avian 0.4. The other three (`bevy_modal_editor`, `wisp`, `lightyear`) are on Bevy 0.18 / Avian 0.5.** These will **not compile together**. This is the single gating dependency for the whole project.

**Bevy 0.17 → 0.18 breaks in obelisk-bevy:**
- **Observers (`On<E>` params):** 70+ usages across `net.rs`, `vfx.rs`, `testkit.rs`, `combat/system.rs`. 0.18 moves to trigger-based routing (`Trigger<E>` / event-channel observation). Mechanical but pervasive; ordering semantics differ — **`ObeliskCuePlugin` must still fire AFTER ResolveHits**, verify the new ordering guarantees.
- **Required components (`#[require(...)]` on `Combatant`, `components.rs:31`):** semantics change (no longer auto-`Default`-constructs in all cases). Verify `make_combatant` still yields Attributes/Faction/SkillSlots/ObeliskId/Transform.
- **Message system:** `add_message` / `MessageWriter<T>` / `MessageReader<T>` (8 usages in `net.rs`, plus `scenario/trace.rs`) removed → migrate to event channels / `World::send_event`. Wire format (serde) survives; glue changes (~20-30 lines). Test end-to-end.
- **`EntityCommand::queue` closures** (~5 uses), `world_mut().entity_mut(e)` cascades.

**Avian 0.4 → 0.5 breaks (the bigger risk):**
- **`SpatialQuery` / `SpatialQueryFilter`** — Avian's largest-ever migration. `spatial/detect.rs` (~50-100 lines, the overlap detection that drives hit resolution) likely needs significant rewrite. **Test coverage mandatory.**
- `PhysicsPlugins::new(FixedUpdate)` signature/schedule binding; `RigidBody::Static` semantics (re-verify Static bodies still track `Transform` — the entire hurtbox model depends on it); `Collider::sphere`/`capsule` signatures; `ColliderAabb` read in `present/debug_viz.rs` (may not exist).

**Effort:** ~10 KLOC affected; ~11 files for each of the Bevy and Avian breaks. **Medium (Bevy) + High (Avian spatial) = HIGH overall, ~3–4 person-weeks with testing.** After migration, re-run golden traces with `UPDATE_GOLDEN=1` and **review every diff** — a silent schedule reordering can change hit timing/trigger order; any diff must be intentional and explained.

**Escape hatch worth noting:** an **external/web** editor only needs the `CastTimeline` RON *schema* (version-agnostic), avoiding the version lock entirely. An **in-process** editor (the more attractive UX) makes the 0.18/0.5 migration a non-negotiable prerequisite. This is one of the central forks (see §6).

---

## 3. The Skill-Designer Seam — Where the Editor Attaches

### Where it plugs in
The obelisk skill already has a **timeline of its own** (the `.cast.ron`: phase_durations + collision_windows + vfx_cues), and a clean lifecycle event bus (`CueEvent`). There are **two seams**:
1. **`vfx_cues` HashMap → `CueEvent`** — the *presentation* seam (particles/anim/impact effects). Keys `on_cast`/`on_hit`/`on_window_<id>` fire at phase boundaries; handlers spawn effects at the cue's position. **This is the primary editor hook.**
2. **`collision_windows`** — the *gameplay timing* seam (hitbox spawn frames, shapes, hit modes). The editor must let designers author these too, since hit-feel demands VFX align with active windows.

### Does the editor already have a sequencer, or does it need one?
**It has the *primitives* of a sequencer but NOT a timeline editor, and the two halves aren't unified.** `EffectMarker` is a trigger/step list (implicit times, no scrubber/keyframes/lanes); `VfxSystem` is a GPU emitter on its own clock. **Neither is plugged into obelisk's `CastTimeline`.** To honor "editor sequencer is the master source for skills," you must build a **binding/timeline layer**:

- **A real timeline UI** is needed: a playhead/scrubber, track lanes (animation / hitbox windows / VFX cues / projectiles), and keyframe-ish event markers tied to obelisk's phases (windup/active/recovery) and absolute base-times. Today's "mini timeline strip" is a read-only visual aid, not an editor.
- **A skill-binding layer** is needed: there is currently **no metadata linking effects to skills**, and **no single entity that represents "a skill"** (stat TOML, `.cast.ron`, and `EffectMarker`/`VfxSystem` are three independent things). Options for binding, pick ONE up front:
  - (a) `EffectMarker` ⇄ `CastTimeline` mapping (step `AtTime`s ↔ phase boundaries) with a converter,
  - (b) `EffectMarker` as an OPTIONAL "authored VFX sequence" layer that *decorates* a `.cast.ron` (referenced by skill id), or
  - (c) the editor authors directly into `CastTimeline` + `vfx_cues` and treats the existing `VfxSystem`/effect presets purely as the **cue targets** (cue_id → effect/sequence preset). **(c) is the lowest-friction path** because it leans on the seam obelisk already exposes.

### Recommended data flow (option (c) flavor)
```
EDITOR (in-Bevy skill designer panel)
  └─ author timeline: phases (windup/active/recovery base durations)
                      hitbox windows (shape/offset/duration/hit_mode)
                      animation track (named clip from character.glb)
                      VFX/particle cues (VfxSystem/effect preset per cue key)
                      projectile spawn events (delivery + socket)
  └─ SERIALIZE → two files (format is LOCKED, non-negotiable):
       skills/<id>.toml          (stat: damage/targeting/delivery/cooldown — obelisk-owned)
       assets/skills/<id>.cast.ron (CastTimeline: phases + windows + vfx_cues — Bevy-owned)
     plus the referenced effect/particle presets (.vfx.ron / .fx.ron) the cues point to
        ↓
GAME (built on wisp + lightyear + obelisk)
  └─ obelisk loads CastTimeline + Skill; runs Validate→Advance→Projectiles→ResolveHits→TickEffects
  └─ ObeliskCuePlugin emits CueEvent at CastBegan/HitWindowOpened/HitConfirmed
  └─ cue handlers drive: VFX (VfxSystem/EffectMarker), animation (wisp clip blend),
                          projectiles (replicated entity), hitboxes (collision_windows)
  └─ lightyear replicates server-authoritative results to the opponent
```

**Missing glue to budget for** (~2–3 weeks on top of migration, per the editor assessment): the timeline scrubbing/keyframe UI, the skill-binding metadata, a "play *skill*" preview (windup/active/recovery + hit window gizmos + damage resolve, not just particles), an **animation track** (effects can spawn GLTF but can't keyframe armature anim — obelisk/wisp own that), and a **socket/attachment** system for cue/projectile spawn points (`hand_r`, etc.).

---

## 4. Online Reference Patterns + Distilled Lesson

All mature engines converge on **timeline/sequencer-as-master-source**: an ability's full duration carries **phase-based, typed events**; at frame/time X an event fires and independent subscribers (animation, VFX, hitbox, projectile, damage) respond. Single source of truth → no sync drift.

- **Unreal GAS + Niagara + AnimNotify:** `Montage` (animation) carries `AnimNotify`s at frames → fire `GameplayEvents` → drive `GameplayCues` (Niagara) + `GameplayEffect` (damage/hitbox). Animation timing is the spine; combat/VFX decoupled. **Socket-based** spawn points.
- **Unity Timeline + VFX Graph:** a Timeline asset with **parallel tracks** (Animation / custom VFX / Hitbox / Projectile / Audio), frame callbacks; the ScriptableObject defines phase windows (startup 0–14, active 15–30, recovery 31–50 frames). GPU VFX with data bindings. Single source of truth.
- **ARPG/MOBA/Fighting (PoE, Dota2, SF6):** data-driven keyframed abilities. Dota2 `CastPoint` (startup frame where damage/projectile applies) + `CastBackswing` (recovery). Fighting-game **frame data** = `{ startFrame, endFrame, hitboxId, particleId, damage }`. Hitbox windows mapped to explicit frame ranges.

**Canonical data model they all share:**
`Ability { id, duration, animationClip, phases: [{ id, startTime, endTime, events: [{ type, params }] }] }`, event types ∈ {PlayAnimation, SpawnVFX, OpenHitbox/CloseHitbox, SpawnProjectile, DealDamage}.

**Distilled lesson for this project:** obelisk's `CastTimeline` is *already* this model (phases + windows + cue events). **Don't invent a parallel ability format on top of `EffectMarker`** — extend `CastTimeline` to be the master source, and make the editor a timeline-track UI over it. Key discipline carried from the references:
- Store **base/absolute times**, apply a single **playback-speed multiplier** at runtime (obelisk already speed-scales phases) so anim + VFX loop length + hitbox duration + projectile timing scale together.
- Keep a **small, reusable event/cue set** (resist one-off events per skill).
- **Bind projectile spawn to a recognizable timeline event** (CastPoint-style) so it feels responsive.
- **Sockets** for VFX/projectile spawn; validate socket names at load with a root fallback.
- Enforce **explicit hitbox close** (or auto-close at phase end) — obelisk's `active_duration` already handles this.
- **Prioritize the timeline-scrubber UI early** — without it, designers can't see phase/event breakdown and iteration stalls.

---

## 5. Scope Decomposition + First-Spec Focus

This effort spans three subsystems. **Recommended dependency order:**

**Phase 0 — Version alignment (HARD PREREQUISITE, ~3–4 wks).** Migrate `obelisk-bevy` to Bevy 0.18 / Avian 0.5. Heaviest item: Avian `SpatialQuery` in `spatial/detect.rs`. Exit criteria: golden traces pass with reviewed/explained diffs; obelisk compiles in a workspace alongside editor+wisp+lightyear. **Nothing else can start in a shared workspace until this lands.** (Decide monorepo-migrate-together vs split-editor-workspace first — see §6.)

**Phase 1 — Base online game (build on wisp).** Stand up a 1v1 client/server skeleton: wisp's `build_shared()` + `add_avian_with_lightyear()` + character.glb + animation-blend pipeline + server-authoritative controller. Swap wizard FPS framing for a fighting-game controller (ground-plane movement, facing, 1v1 match flow). Wire obelisk's `Combatant`/skills/health-damage into the replicated player. Validate with the net-test harness (diverged=0, latency<5ms). Carry forward wisp's `bei`-mismatch workaround (hand-rolled input message). Decide reconciliation later (server-auth is fine <150ms).

**Phase 2 — Editor integration (embed `bevy_modal_editor`).** Add `EditorPlugin`; register fighting-game custom entities (spawn points, stage geometry) + scene components; wire `GameState` Editing/Playing so the editor can play-test a match in-process. Establish the `assets/skills/<id>/` asset layout and load paths.

**Phase 3 — The skill designer (the signature feature).** Build the timeline-track UI over `CastTimeline`: phase lanes + hitbox-window lane + animation lane + VFX-cue lane + projectile-event lane, with a scrubber and a "play skill" preview (phase labels + hit-window gizmos + damage resolve). Emit both files (`<id>.toml` + `<id>.cast.ron`) plus referenced effect/particle presets. Add a particle-preset picker (with missing-ref warnings) and a socket picker.

### What the FIRST spec should cover
**Spec #1 = Phase 0: the obelisk-bevy → Bevy 0.18 / Avian 0.5 migration**, plus the **workspace topology decision** (monorepo vs split editor) it depends on. Rationale: it's the only hard blocker, it's well-scoped from the version-reconciliation findings (exact files/APIs enumerated), it's verifiable against the golden-trace suite, and **every downstream phase imports obelisk into a 0.18 workspace.** The spec should: enumerate the 0.17→0.18 and 0.4→0.5 changes file-by-file, define the golden-trace regression gate, decide the workspace layout, and explicitly call out the Avian `SpatialQuery` rewrite as the risk to front-load. (A thin "skill-designer seam architecture decision" — which of §3's binding options (a)/(b)/(c) — should ride alongside as a design note, since it shapes Phases 1–3, but it does not block Phase 0.)

---

## 6. Open Questions for the User (Genuine Design Forks)

1. **Upgrade obelisk now vs pin it.** Migrate `obelisk-bevy` to Bevy 0.18/Avian 0.5 up front (unlocks an *in-process* editor + shared workspace, ~3–4 wks), or keep obelisk pinned and drive an **external/web editor** off the version-agnostic `CastTimeline` RON schema (skips the migration but loses in-engine live preview and forces the editor to hand-serialize all the enum shapes)? This single choice cascades into workspace topology and editor architecture.

2. **Build-on-wisp vs new game from scratch.** Wisp gives a working lightyear transport, server-auth controller, character rig + animation-blend pipeline, and data-driven content patterns — but it's an FPS wizard game carrying a known `bei` mismatch and a not-yet-reconciled prediction model. Fork/build-on wisp (fast, inherits the workaround + FPS framing to strip out), or start a clean lightyear project taking only the patterns? For a **1v1 fighting game**, how much of wisp's locomotion/aim model survives the genre change?

3. **In-game editor mode vs separate editor binary.** Embed the editor in the game (Editing/Playing `GameState`, designers play-test skills live in-engine) vs a standalone editor binary/tool that emits the two files. In-process gives the best "author → preview the actual skill" loop but hard-requires the 0.18 migration and couples editor + game lifecycles; separate decouples versions but loses live gameplay preview (you'd see particles, not damage/hit-confirm/cooldown).

4. **How deep does the skill designer go for the first template?** A spectrum: (low) author `vfx_cues` + collision windows as form fields over the existing `.cast.ron`, reusing `EffectMarker`/`VfxSystem` editors as-is; (mid) add a real timeline scrubber + track lanes + "play skill" preview with hit-window gizmos; (high) full master-source sequencer with animation-track keyframing, socket system, projectile lane, and `EffectMarker`⇄`CastTimeline` binding. Which depth is the **template's** bar, and which of §3's binding options ((a) converter / (b) decorator layer / (c) author-into-CastTimeline) is the architecture?

---

## Appendix — Load-Bearing Facts to Remember

- **Skill format is LOCKED:** two files (`skills/<id>.toml` stats, `assets/skills/<id>.cast.ron` timeline). The editor must emit both; do not invent a combined format.
- **The seam is `vfx_cues` (HashMap<String,String>) → `CueEvent`**, keyed `on_cast`/`on_hit`/`on_window_<id>`, routed by `ObeliskCuePlugin`, observed via `app.observe_cue(id, handler)`.
- **`CastTimeline` ≈ the industry-standard ability model already.** Extend it; don't duplicate it on `EffectMarker`.
- **Editor has sequencer *primitives*, not a timeline editor.** Needs scrubber/track UI + skill-binding + "play skill" preview + animation track + sockets (~2–3 wks of glue).
- **Avian 0.5 `SpatialQuery` rewrite** (`spatial/detect.rs`) is the highest-risk migration item; golden-trace suite is the regression gate.
- **wisp owns Transform via lightyear** — write avian `Position`/`Rotation`, never `Transform`, on physics bodies.
- **`bei` version mismatch** is load-bearing in wisp; keep the hand-rolled `PlayerInputMessage` until lightyear bumps bei to 0.25+.
