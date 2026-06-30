# Obelisk-Arena Skill Designer (M1–M3) Implementation Plan

**Agentic-workers note.** Written for subagent-driven execution (`superpowers:subagent-driven-development` / `superpowers:executing-plans`): each `### Task N` is a self-contained TDD unit one worker takes start-to-finish — write the failing test (real code), run it and paste the real FAIL, write the minimal real implementation, run it and paste the real PASS, then commit with the given message. Run every gate command verbatim and paste real output; never weaken an assertion or skip a commit to force a pass. Tasks are ordered; later tasks consume the exact `Produces:` signatures of earlier ones. A task whose step 1 is "read <file> to confirm signature X" reconciles its test to the real signature before proceeding. Editor/`arena_editor`/`bevy_modal_editor` commands run inside `nix develop` (the editor pulls a git `bevy_egui`); `obelisk-bevy`/`arena_sim`/`arena_game`/`arena_skills` build with plain `cargo`.

**Goal.** Build an in-editor Skill Designer (`arena_editor` on `bevy_modal_editor`) whose phase-based bottom-dock timeline authors obelisk skills — timing/geometry (`.cast.ron`) + cosmetics (`.skillfx.ron`) — with a "Play the real skill" preview running obelisk's real deterministic simulation, so what you author is exactly what the game plays.

**Architecture.** A new transport-agnostic `arena_sim` crate, lifted out of `arena_game`, is the shared simulation both the live game (lightyear host) and the editor preview (plain-Avian host) run, parameterizing physics install so the preview behaves identically to the game. `arena_editor` embeds `EditorPlugin{add_physics:false}` + `GamePlugin`, registers a generic custom **Skill** mode (a small upstream `EditorMode::Custom` seam) whose timeline reads/writes obelisk's now-`Serialize` `CastTimeline` + `arena_skills`' extended `.skillfx.ron`, previews via `arena_sim` + obelisk's `FixedUpdate`, and reuses the editor's `bevy_vfx` engine for cosmetics. All game-specific authoring lives in `arena_editor`/`arena_sim`/`arena_skills`; `bevy_modal_editor` gains only the generic mode seam.

**Tech stack.** Bevy 0.18.1, Avian3d 0.5, `bevy_modal_editor` (egui), `bevy_vfx`, `bevy_editor_game`, obelisk-bevy, `arena_skills`, `ron` 0.8, lightyear 0.26.4 (game only). obelisk-bevy's CLAUDE.md still says Bevy 0.17/Avian 0.4 — that is stale doc-lag; the Phase-0 migration to 0.18.1/0.5 is complete across all repos (research §7).

## Global Constraints
- **Versions:** Bevy 0.18.1 / Avian3d 0.5 everywhere. avian `0.5` is pinned by `lightyear_avian3d 0.26`.
- **Obelisk determinism is sacred:** all combat RNG flows through one seeded `CombatRng`; spawn/reset write avian `Position`, **never `Transform`** (lightyear-avian `Position` mode owns the sync); the client/editor **never resolves combat** — obelisk's server-only `ObeliskSet::ResolveHits` does (the preview owns a local seeded `CombatRng` + `Time::<Fixed>`, but the editor itself never draws RNG or spawns a `Hitbox`).
- **Green gates, every commit that touches their repo:** obelisk-bevy's **39-golden suite** (`cargo test --features test-support --test golden`, byte-identical, never `UPDATE_GOLDEN`) + `--lib --tests`; the **arena_game net-test harness** (`bash crates/arena_game/tools/net-test/run_session.sh`) — the proof the extracted `arena_sim` still composes the game identically; arena_sim / arena_skills / arena_editor in-crate suites.
- **Follow `arena_game/CLAUDE.md` invariants:** `add_avian_with_lightyear` stays the sole `PhysicsPlugins` registrant in the game; both obelisk composers omit `ObeliskSpatialPlugin`; hurtbox on a CHILD sensor; `Collider::capsule(0.35,0.48)` single-sourced; `faction_for_slot` sorted-id slots; force-refresh the `SpatialQueryPipeline`; trace kinds / env hooks unchanged.
- **M4 rules authoring (obelisk `Skill`/`Effect`/`Trigger` `.toml`) is OUT OF SCOPE here** — its own spec after an obelisk-core deep-dive; until then the rules `.toml` is read-only context.

## File Structure
**obelisk-bevy** — `src/assets/mod.rs` (M): `Serialize`+`Reflect` on `CastTimeline` + 9 transitive types, `register_type` in `ObeliskAssetsPlugin`, round-trip + reflect tests.
**obelisk-arena (workspace)** — `Cargo.toml` (M): members + `[workspace.dependencies]` (`arena_sim`,`arena_editor`,`bevy_modal_editor`,`bevy_editor_game`,`bevy_vfx`).
**arena_sim (NEW)** — `Cargo.toml`; `src/lib.rs` (root+`ARENA_SIM_TICK_HZ`); `src/tuning.rs` (geometry/tuning consts); `src/input.rs` (`ArenaInput`+`MapEntities`); `src/shared_controller.rs` (force controller); `src/spawn.rs` (`spawn_arena_floor`/`make_arena_combatant`/`SPAWN_MARKERS`/`faction_for_slot`); `src/obelisk.rs` (`add_obelisk_sim`(+headless/client)+spatial refresh); `src/preview.rs` (`ArenaSimPreviewPlugin`/`spawn_preview_world`/`PreviewCaster`/`PreviewDummy`); `tests/builds.rs`,`tests/recipe.rs`,`tests/preview_smoke.rs`.
**arena_game (M)** — `Cargo.toml` (+`arena_sim`); `src/lib.rs`,`src/net/mod.rs`,`src/net/input.rs`,`src/shared_controller.rs`,`src/server/spawn.rs`,`src/server/rounds.rs`,`src/client/net.rs` (all → re-export from `arena_sim`; `add_avian_with_lightyear` stays).
**bevy_modal_editor (M)** — `src/editor/custom_mode.rs` (NEW: `CustomModeId`/`CustomModeDef`/`CustomModeRegistry`/`RegisterEditorModeExt`/`dispatch_custom_panel`); `src/editor/mod.rs`,`src/editor/state.rs`,`src/editor/input.rs`,`src/ui/panels.rs`,`src/ui/mod.rs`,`src/lib.rs`.
**arena_editor (NEW)** — `Cargo.toml`; `src/main.rs`; `src/lib.rs`; `src/skill_designer.rs` (`SkillDesignerPlugin`/`register_skill_mode`/`SKILL_MODE_ID`); `src/io.rs`; `src/sim_config.rs`; `src/model.rs`; `src/timeline_geom.rs`; `src/edits.rs`; `src/enum_ui.rs`; `src/preview_controller.rs`; `src/gizmo.rs`; `src/panel.rs`; `src/fx_edits.rs`; `src/socket.rs`; `src/preview_rig.rs`; `src/vfx_bind.rs`; `src/preview_cosmetics.rs`; `tests/{boots,sim_config,cast_io,preview_play,skillfx_io,preview_rig,preview_cosmetics,screenshot_acceptance}.rs`.
**arena_skills (M)** — `src/lib.rs` (extend `ParticleSpec`/`ProjectileCosmetic`/`AnimLayer`; add `VfxParamBinding`/`VfxBindSource`; `Serialize SkillFx`; `normalize`/`modulate`/`resolve_binding`); `tests/skillfx_roundtrip.rs`.

---

# Milestone M1 — Foundation
Ships: game still works (net-test green), editor boots a Skill mode, `CastTimeline` round-trips `.cast.ron`.

### Task 1: Serialize+Reflect on `CastTimeline` + 9 transitive types; RON round-trip + reflect-registration; golden green
**Files:** Modify `/Users/luke/src/obelisk-bevy/src/assets/mod.rs` (import L3; derives L5,17,24,40,47,54,63,71,78,86; `ObeliskAssetsPlugin::build` 134-139; tests mod 142-180).
**Interfaces:** Produces — `CastTimeline` + `PhaseDurations`,`CollisionWindow`,`WindowPhase`,`CollisionShape`,`VolumeMotion`,`HitFilter`,`HitMode`,`CastTargeting`,`CastDelivery` now `Reflect + FromReflect + Serialize` (+ existing `Clone + Deserialize`); all 10 `register_type`'d. `CollisionShape`/`CastTargeting`/`CastDelivery` keep **no** `Eq`/`PartialEq` (f32) → compare via `format!("{:?}")`. Consumed by Tasks 14-33 (`ron::ser::to_string`).
- [ ] Append failing tests to the `tests` mod in `assets/mod.rs`:
```rust
    #[test]
    fn cast_timeline_round_trips_through_ron() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"),"/assets/skills/firebolt.cast.ron")).expect("read firebolt.cast.ron");
        let parsed: CastTimeline = ron::de::from_str(&src).expect("parse original");
        let serialized = ron::ser::to_string(&parsed).expect("serialize");
        let reparsed: CastTimeline = ron::de::from_str(&serialized).expect("re-parse");
        assert_eq!(parsed.skill_id, reparsed.skill_id);
        assert_eq!(parsed.phase_durations.windup, reparsed.phase_durations.windup);
        assert_eq!(parsed.phase_durations.active, reparsed.phase_durations.active);
        assert_eq!(parsed.phase_durations.recovery, reparsed.phase_durations.recovery);
        assert_eq!(parsed.collision_windows.len(), reparsed.collision_windows.len());
        assert_eq!(format!("{:?}", parsed.collision_windows[0].shape), format!("{:?}", reparsed.collision_windows[0].shape));
        assert_eq!(format!("{:?}", parsed.targeting), format!("{:?}", reparsed.targeting));
        assert_eq!(format!("{:?}", parsed.delivery), format!("{:?}", reparsed.delivery));
        assert_eq!(parsed.vfx_cues, reparsed.vfx_cues);
    }
    #[test]
    fn cast_timeline_type_is_registered_for_reflection() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(AssetPlugin { file_path: ".".into(), ..default() }).add_plugins(ObeliskAssetsPlugin);
        app.finish(); app.cleanup();
        let registry = app.world().resource::<AppTypeRegistry>().read();
        assert!(registry.get(std::any::TypeId::of::<CastTimeline>()).is_some(), "CastTimeline must be registered");
    }
```
- [ ] Run FAIL: `cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --lib assets:: 2>&1 | tail -20`
- [ ] Impl: change L3 import to `use serde::{Deserialize, Serialize};`
- [ ] Impl: on `CastTimeline` (L5) **drop explicit `TypePath`** (Reflect supplies it; both = conflicting impl) and add `Reflect, Serialize`: `#[derive(Asset, Reflect, Debug, Clone, Serialize, Deserialize)]`.
- [ ] Impl: add `Reflect, Serialize` to the other 9 derives (keep each type's existing `Copy`/`PartialEq`/`Eq`/`Default`), e.g. `#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]`.
- [ ] Impl: in `ObeliskAssetsPlugin::build` after the `init_resource` call: `app.register_type::<CastTimeline>().register_type::<PhaseDurations>().register_type::<CollisionWindow>().register_type::<WindowPhase>().register_type::<CollisionShape>().register_type::<VolumeMotion>().register_type::<HitFilter>().register_type::<HitMode>().register_type::<CastTargeting>().register_type::<CastDelivery>();`
- [ ] Run PASS: `cargo test --features test-support --lib assets:: 2>&1 | tail -20`
- [ ] Gates: `cargo test --features test-support --lib --tests 2>&1 | tail -15 && cargo clippy --features test-support --lib --tests -- -D warnings 2>&1 | tail -5 && cargo fmt --check` then golden byte-identical: `cargo test --features test-support --test golden 2>&1 | tail -15`
- [ ] Commit: `cd /Users/luke/src/obelisk-bevy && git checkout -b m1-cast-timeline-serialize && git add -A && git commit -m "feat(assets): derive Serialize+Reflect on CastTimeline + transitive types; RON round-trip + reflect tests; golden unchanged"`

### Task 2: Scaffold `arena_sim` crate + workspace wiring + build smoke
**Files:** Create `crates/arena_sim/Cargo.toml`, `crates/arena_sim/src/lib.rs`, `crates/arena_sim/tests/builds.rs`; Modify `/Users/luke/src/obelisk-arena/Cargo.toml` (members+deps), `crates/arena_game/Cargo.toml`.
**Interfaces:** Produces — crate `arena_sim` with `pub const ARENA_SIM_TICK_HZ: u32 = 60;`.
- [ ] Failing test `crates/arena_sim/tests/builds.rs`: `#[test] fn crate_links_and_exposes_tick_hz(){ assert_eq!(arena_sim::ARENA_SIM_TICK_HZ, 60); }`
- [ ] Run FAIL: `cd /Users/luke/src/obelisk-arena && cargo test -p arena_sim 2>&1 | tail -10`
- [ ] Create `Cargo.toml`:
```toml
[package]
name = "arena_sim"
version = "0.1.0"
edition = "2021"
[dependencies]
bevy = { workspace = true }
avian3d = { workspace = true }
obelisk-bevy = { workspace = true }
stat_core = { workspace = true }
serde = { workspace = true }
[dev-dependencies]
obelisk-bevy = { workspace = true, features = ["test-support"] }
stat_core = { workspace = true }
```
- [ ] Create `src/lib.rs`: `//! Transport-agnostic arena simulation shared by arena_game + arena_editor.` + `pub const ARENA_SIM_TICK_HZ: u32 = 60;`
- [ ] Edit workspace `Cargo.toml`: add `"crates/arena_sim"` to members; under `[workspace.dependencies]` add `arena_sim = { path = "crates/arena_sim" }`.
- [ ] Edit `crates/arena_game/Cargo.toml`: add `arena_sim = { workspace = true }`.
- [ ] Run PASS + game still builds: `cargo test -p arena_sim 2>&1 | tail -5 && cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -5`
- [ ] Commit: `git checkout -b m1-arena-sim && git add -A && git commit -m "feat(arena_sim): scaffold transport-agnostic sim crate + workspace wiring"`

### Task 3: Move tuning consts + `ArenaInput` into `arena_sim`; re-export from `arena_game`
**Files:** Create `crates/arena_sim/src/tuning.rs`,`src/input.rs`; Modify `arena_sim/src/lib.rs`, `arena_game/src/net/mod.rs` (consts L40,45,52-53,69), `arena_game/src/net/input.rs`.
**Interfaces:** Produces — `arena_sim::tuning::{GROUND_Y=0.59, GRAVITY=20.0, ARENA_EYE_HEIGHT=0.5, PLAYER_CAPSULE_RADIUS=0.35, PLAYER_CAPSULE_LENGTH=0.48}`; `arena_sim::input::ArenaInput { movement: Vec2, yaw: f32, jump: bool, charging: bool }` deriving `Serialize,Deserialize,Clone,Copy,PartialEq,Debug,Default,Reflect` + `impl MapEntities`.
- [ ] Step 1: read `arena_game/src/net/mod.rs` L30-110 to confirm the const defs + the L32 `pub use crate::shared_controller::{...}` re-export.
- [ ] Failing test (append `builds.rs`): `#[test] fn tuning_and_input_are_exposed(){ assert_eq!(arena_sim::tuning::GROUND_Y,0.59); assert_eq!(arena_sim::tuning::GRAVITY,20.0); assert_eq!(arena_sim::tuning::PLAYER_CAPSULE_RADIUS,0.35); assert_eq!(arena_sim::tuning::PLAYER_CAPSULE_LENGTH,0.48); let i=arena_sim::input::ArenaInput::default(); assert!(!i.jump && !i.charging); }`
- [ ] Run FAIL: `cargo test -p arena_sim tuning_and_input 2>&1 | tail -10`
- [ ] Create `tuning.rs` with the 5 consts (`pub const GROUND_Y: f32 = 0.59;` … values above) — copy the values + doc lines verbatim from net/mod.rs.
- [ ] Create `input.rs` — move the struct verbatim from `arena_game/src/net/input.rs` keeping the bevy `MapEntities` impl (no lightyear): `use bevy::ecs::entity::{EntityMapper, MapEntities}; use bevy::prelude::*; use serde::{Deserialize, Serialize};` + `#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default, Reflect)] pub struct ArenaInput { pub movement: Vec2, pub yaw: f32, pub jump: bool, pub charging: bool }` + `impl MapEntities for ArenaInput { fn map_entities<M: EntityMapper>(&mut self,_:&mut M){} }`
- [ ] Add to `lib.rs`: `pub mod input; pub mod tuning;`
- [ ] Edit `arena_game/src/net/input.rs`: replace body with `pub use arena_sim::input::ArenaInput;` (keep module doc).
- [ ] Edit `arena_game/src/net/mod.rs`: delete the 5 local const defs; add `pub use arena_sim::tuning::{ARENA_EYE_HEIGHT, GRAVITY, GROUND_Y, PLAYER_CAPSULE_LENGTH, PLAYER_CAPSULE_RADIUS};` (keep `TICK_HZ`, `session_seed`, netcode/round consts).
- [ ] Run PASS + build: `cargo test -p arena_sim 2>&1 | tail -5 && cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -5`
- [ ] Net-test gate: `bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -15`
- [ ] Commit: `git add -A && git commit -m "refactor(arena_sim): lift tuning consts + ArenaInput out of arena_game; re-export; net-test green"`

### Task 4: Move `shared_controller` into `arena_sim`; re-export; keep unit test
**Files:** Create `crates/arena_sim/src/shared_controller.rs` (moved); Modify `arena_sim/src/lib.rs`, `arena_game/src/shared_controller.rs` (→re-export), `arena_game/src/net/mod.rs` (L32 source).
**Interfaces:** Produces — `arena_sim::shared_controller::{MAX_SPEED=4.0, MAX_ACCELERATION=30.0, JUMP_SPEED=7.0, AIR_CONTROL=1.0, apply_arena_movement(&ComputedMass,f32,&ArenaInput,ForcesItem), apply_arena_yaw(&ArenaInput,&mut Rotation)}`. Consumes — `arena_sim::input::ArenaInput`, `arena_sim::tuning::GROUND_Y`.
- [ ] Move full contents of `arena_game/src/shared_controller.rs` into `arena_sim/src/shared_controller.rs`, changing only `use crate::net::input::ArenaInput;`→`use crate::input::ArenaInput;` and `crate::net::GROUND_Y`→`crate::tuning::GROUND_Y` (the `#[cfg(test)] desired_velocity_frame` test moves verbatim).
- [ ] Add `pub mod shared_controller;` to `lib.rs`.
- [ ] Run moved test PASS: `cargo test -p arena_sim desired_velocity_frame 2>&1 | tail -8`
- [ ] Replace `arena_game/src/shared_controller.rs` body with `pub use arena_sim::shared_controller::*;` (keep module doc). Confirm `arena_game/src/net/mod.rs` L32 still resolves (`crate::shared_controller` forwards; or set `pub use arena_sim::shared_controller::{JUMP_SPEED, MAX_ACCELERATION, MAX_SPEED};`).
- [ ] Build + in-crate tests PASS: `cargo test -p arena_game --lib 2>&1 | tail -10`
- [ ] Net-test gate: `bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -15`
- [ ] Commit: `git add -A && git commit -m "refactor(arena_sim): move shared force controller out of arena_game; re-export; net-test green"`

### Task 5: Extract floor + `make_arena_combatant` + `faction_for_slot` + `SPAWN_MARKERS`
**Files:** Create `crates/arena_sim/src/spawn.rs`, `crates/arena_sim/tests/recipe.rs`; Modify `arena_sim/src/lib.rs`, `arena_game/src/lib.rs` (floor L78-88→re-export), `arena_game/src/server/spawn.rs` (factor body+hurtbox; `SPAWN_MARKERS` L59), `arena_game/src/server/rounds.rs` (`faction_for_slot` L46→re-export), `arena_game/src/client/net.rs` (capsule L~231→consts).
**Interfaces:** Produces — `arena_sim::spawn::{spawn_arena_floor(&mut Commands), make_arena_combatant(&mut Commands,&str,Faction,Vec3)->Entity, SPAWN_MARKERS:[Vec3;2], faction_for_slot(usize)->Faction}` (combatant = obelisk+avian Dynamic body + child `Hurtbox` sensor; NO networked/replicate, NO grant_skill). Consumes — `arena_sim::tuning::*`, `obelisk_bevy::prelude::{make_combatant?,Faction,Hurtbox}`, `stat_core::StatBlock`.
- [ ] Step 1: read `arena_game/src/server/spawn.rs` L92-219 to confirm the body tuple (172-190) + child hurtbox (211-219) + `make_combatant`/`Faction`/`Collider::capsule`/`LockedAxes`/`Friction`/`Sensor`/`ChildOf` idioms.
- [ ] Failing test `crates/arena_sim/tests/recipe.rs`:
```rust
use arena_sim::spawn::{faction_for_slot, make_arena_combatant, SPAWN_MARKERS};
use arena_sim::tuning::GROUND_Y;
use avian3d::prelude::*; use bevy::prelude::*; use obelisk_bevy::prelude::*;
#[test] fn faction_for_slot_assigns_opposing_factions(){ assert_eq!(faction_for_slot(0),Faction::Player); assert_eq!(faction_for_slot(1),Faction::Enemy); }
#[test] fn spawn_markers_are_two_opposed_points(){ assert_eq!(SPAWN_MARKERS.len(),2); assert_eq!(SPAWN_MARKERS[0],Vec3::new(-4.0,GROUND_Y,0.0)); assert_eq!(SPAWN_MARKERS[1],Vec3::new(4.0,GROUND_Y,0.0)); }
#[test] fn make_arena_combatant_builds_dynamic_capsule_with_child_hurtbox(){
    let mut app=App::new(); app.add_plugins(MinimalPlugins); obelisk_bevy::testkit::init_test_obelisk();
    let player={ let mut c=app.world_mut().commands(); make_arena_combatant(&mut c,"player_0",Faction::Player,SPAWN_MARKERS[0]) };
    app.world_mut().flush();
    assert!(app.world().get::<RigidBody>(player).is_some());
    assert!(app.world().get::<Faction>(player).is_some());
    assert_eq!(app.world().get::<Position>(player).map(|p|p.0),Some(SPAWN_MARKERS[0]));
    let children=app.world().get::<Children>(player).expect("child hurtbox"); assert_eq!(children.len(),1);
    assert!(app.world().get::<Hurtbox>(children[0]).is_some());
}
```
- [ ] Run FAIL: `cargo test -p arena_sim --test recipe 2>&1 | tail -12`
- [ ] Create `spawn.rs`: move `spawn_arena_floor` verbatim from `arena_game/lib.rs:78-88`; lift `SPAWN_MARKERS`+`faction_for_slot`; factor the body+hurtbox recipe out of `spawn_player_on_connect`:
```rust
use avian3d::prelude::*; use bevy::prelude::*; use obelisk_bevy::prelude::*; use stat_core::StatBlock;
use crate::tuning::{GROUND_Y, PLAYER_CAPSULE_LENGTH, PLAYER_CAPSULE_RADIUS};
pub const SPAWN_MARKERS: [Vec3; 2] = [Vec3::new(-4.0, GROUND_Y, 0.0), Vec3::new(4.0, GROUND_Y, 0.0)];
pub fn faction_for_slot(slot: usize) -> Faction { if slot == 0 { Faction::Player } else { Faction::Enemy } }
pub fn spawn_arena_floor(commands: &mut Commands) {
    const FLOOR_SIZE: f32 = 40.0; const FLOOR_THICKNESS: f32 = 1.0;
    commands.spawn((Name::new("ArenaFloor"), RigidBody::Static, Collider::cuboid(FLOOR_SIZE, FLOOR_THICKNESS, FLOOR_SIZE), Position(Vec3::new(0.0, -FLOOR_THICKNESS/2.0, 0.0)), Rotation::default()));
}
pub fn make_arena_combatant(commands: &mut Commands, obelisk_id: &str, faction: Faction, spawn: Vec3) -> Entity {
    let player = commands.spawn_empty().make_combatant(StatBlock::with_id(obelisk_id)).insert((
        faction, Position(spawn), Rotation::default(), LinearVelocity::default(), AngularVelocity::default(),
        RigidBody::Dynamic, Collider::capsule(PLAYER_CAPSULE_RADIUS, PLAYER_CAPSULE_LENGTH),
        LockedAxes::default().lock_rotation_x().lock_rotation_y().lock_rotation_z(),
        Friction::new(0.0).with_combine_rule(CoefficientCombine::Min),
    )).id();
    commands.spawn((Hurtbox { owner: player }, Collider::capsule(PLAYER_CAPSULE_RADIUS, PLAYER_CAPSULE_LENGTH), Sensor, Transform::default(), ChildOf(player)));
    player
}
```
(adjust `make_combatant`/`Hurtbox`/`ChildOf` to the exact idioms confirmed in Step 1.)
- [ ] Add `pub mod spawn;` to `lib.rs`. Run PASS: `cargo test -p arena_sim --test recipe 2>&1 | tail -10`
- [ ] Rewire arena_game: `lib.rs` → `pub use arena_sim::spawn::spawn_arena_floor;`; in `server/spawn.rs` replace the inlined body+hurtbox in `spawn_player_on_connect` with `let player = arena_sim::spawn::make_arena_combatant(&mut commands, &obelisk_id, faction, spawn);` then keep the existing networked/`Replicate`/`grant_skill("firebolt")` inserts on `player`, and `pub use arena_sim::spawn::SPAWN_MARKERS;`; `server/rounds.rs` → `pub(crate) use arena_sim::spawn::faction_for_slot;` (keep its `#[cfg(test)]` test); `client/net.rs` → `Collider::capsule(arena_sim::tuning::PLAYER_CAPSULE_RADIUS, arena_sim::tuning::PLAYER_CAPSULE_LENGTH)` (invariant #10, single source).
- [ ] Build + lib tests: `cargo test -p arena_game --lib 2>&1 | tail -10 && cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -5`
- [ ] Net-test gate: `bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -15`
- [ ] Commit: `git add -A && git commit -m "refactor(arena_sim): extract floor + make_arena_combatant + faction_for_slot + SPAWN_MARKERS; dedupe capsule consts; net-test green"`

### Task 6: Move `add_obelisk_sim` (+ refresh systems + headless/client wrappers) into `arena_sim`
**Files:** Create `crates/arena_sim/src/obelisk.rs`; Modify `arena_sim/src/lib.rs`, `arena_game/src/lib.rs` (move `add_obelisk_sim` 116-212, refresh fns 248-257, wrappers 219-243 → re-export; keep `add_avian_with_lightyear` 55-71).
**Interfaces:** Produces — `arena_sim::obelisk::{add_obelisk_sim(&mut App, resolve_hits: bool), add_obelisk_sim_headless(&mut App), add_obelisk_sim_client(&mut App)}` (adds obelisk sub-plugins omitting `ObeliskSpatialPlugin`, the `ObeliskSet` chain, timeline/projectile/(server)detect systems + spatial refresh; does NOT install `PhysicsPlugins` — the host does). `add_avian_with_lightyear` STAYS in arena_game.
- [ ] Failing test (append `builds.rs`):
```rust
#[test]
fn add_obelisk_sim_composes_under_plain_avian_without_panicking() {
    use avian3d::prelude::*; use bevy::prelude::*;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(bevy::asset::AssetPlugin { file_path: ".".into(), ..default() })
        .add_plugins(bevy::mesh::MeshPlugin).add_plugins(bevy::scene::ScenePlugin)
        .add_plugins(PhysicsPlugins::new(FixedUpdate))
        .insert_resource(Gravity(Vec3::new(0.0, -arena_sim::tuning::GRAVITY, 0.0)))
        .insert_resource(Time::<Fixed>::from_hz(60.0));
    arena_sim::obelisk::add_obelisk_sim(&mut app, true);
    app.finish(); app.cleanup(); app.update();
}
```
- [ ] Run FAIL: `cargo test -p arena_sim add_obelisk_sim_composes 2>&1 | tail -12`
- [ ] Create `obelisk.rs` by moving `add_obelisk_sim` (lib.rs:116-212, make it `pub`), the two `refresh_spatial_pipeline*` fns (248-257, keep module-private), and `add_obelisk_sim_headless`/`add_obelisk_sim_client` (219-243) verbatim. Body references only `obelisk_bevy::{assets,combat,core,loot,net,spatial,timeline,vfx,ObeliskSet}` + `avian3d::prelude::PhysicsSystems` (gravity stays the host's job). Add `pub mod obelisk;` to `lib.rs`.
- [ ] Run PASS: `cargo test -p arena_sim add_obelisk_sim_composes 2>&1 | tail -8`
- [ ] Rewire `arena_game/src/lib.rs`: delete the moved fns, add `pub use arena_sim::obelisk::{add_obelisk_sim, add_obelisk_sim_client, add_obelisk_sim_headless};` (keep `add_avian_with_lightyear` + `spawn_arena_floor` re-export; bins call the wrappers unchanged).
- [ ] Build + net-test: `cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -5 && bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -15`
- [ ] Commit: `git add -A && git commit -m "refactor(arena_sim): move add_obelisk_sim + spatial refresh into arena_sim (physics parameterized to host); net-test green"`

### Task 7: `ArenaSimPreviewPlugin` + headless seeded cast→`DamageResolved` smoke + determinism
**Files:** Create `crates/arena_sim/src/preview.rs`, `crates/arena_sim/tests/preview_smoke.rs`; Modify `arena_sim/src/lib.rs`.
**Interfaces:** Produces — `arena_sim::preview::{ArenaSimPreviewPlugin, spawn_preview_world(&mut Commands), PreviewCaster, PreviewDummy}` (plugin installs plain `PhysicsPlugins::new(FixedUpdate)` if absent + `Gravity` + `add_obelisk_sim(app,true)`; `spawn_preview_world` spawns floor + Player `PreviewCaster` + Enemy `PreviewDummy`). Consumed by Tasks 13/21/29/30/32.
- [ ] Failing smoke `crates/arena_sim/tests/preview_smoke.rs` (plain-Avian headless via `add_obelisk_sim`, re-root onto obelisk-bevy fixtures, seeded, cast firebolt → assert `DamageResolved`; confirm `EventRecorder.{cast_began,damage_resolved}` + `.total_damage` field names here — they propagate to Tasks 21/32):
```rust
use arena_sim::obelisk::add_obelisk_sim;
use arena_sim::spawn::{make_arena_combatant, spawn_arena_floor};
use arena_sim::tuning::GRAVITY;
use avian3d::prelude::*; use bevy::prelude::*; use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::{EventRecorder, EventRecorderPlugin, init_test_obelisk};
use std::time::Duration;
fn enter_obelisk_root() {
    let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let obelisk = d.ancestors().nth(2).expect("obelisk-arena").parent().expect("parent").join("obelisk-bevy");
    assert!(obelisk.join("assets/skills/firebolt.cast.ron").exists());
    std::env::set_var("BEVY_ASSET_ROOT", &obelisk);
    std::env::set_current_dir(&obelisk).expect("re-root");
}
fn run(seed: u64, ticks: usize) -> App {
    init_test_obelisk();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(bevy::asset::AssetPlugin { file_path: ".".into(), ..default() })
        .add_plugins(bevy::mesh::MeshPlugin).add_plugins(bevy::scene::ScenePlugin).add_plugins(PhysicsPlugins::new(FixedUpdate))
        .insert_resource(Gravity(Vec3::new(0.0, -GRAVITY, 0.0))).add_plugins(EventRecorderPlugin)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0/60.0)))
        .insert_resource(Time::<Fixed>::from_hz(60.0));
    add_obelisk_sim(&mut app, true);
    use obelisk_bevy::core::config::SkillSource; use obelisk_bevy::prelude::ObeliskConfigExt;
    app.add_obelisk_skills(SkillSource::Dir(std::path::PathBuf::from("tests/fixtures/skills")));
    app.seed_combat_rng(seed); app.finish(); app.cleanup();
    let handle: Handle<CastTimeline> = app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 { app.update(); if app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() { break; } }
    app.world_mut().resource_mut::<CastTimelineHandles>().0.insert("firebolt".into(), handle);
    spawn_arena_floor(&mut app.world_mut().commands());
    let caster = make_arena_combatant(&mut app.world_mut().commands(), "caster", Faction::Player, Vec3::new(0.0,0.59,0.0));
    let target = make_arena_combatant(&mut app.world_mut().commands(), "target", Faction::Enemy, Vec3::new(0.0,0.59,2.0));
    app.world_mut().flush(); app.update();
    app.world_mut().commands().entity(caster).cast_skill_at("firebolt", target);
    for _ in 0..ticks { app.update(); }
    app
}
#[test] fn firebolt_resolves_damage_on_the_dummy() {
    enter_obelisk_root(); let app = run(0xC0FFEE, 60); let rec = app.world().resource::<EventRecorder>();
    assert!(!rec.cast_began.is_empty()); assert!(!rec.damage_resolved.is_empty());
    assert!(rec.damage_resolved.iter().map(|d| d.total_damage).sum::<f64>() > 0.0);
}
#[test] fn firebolt_damage_is_deterministic() {
    enter_obelisk_root();
    let total = |s: u64| run(s, 60).world().resource::<EventRecorder>().damage_resolved.iter().map(|d| d.total_damage).sum::<f64>();
    let (a, b) = (total(0xABCDEF), total(0xABCDEF)); assert!(a > 0.0); assert_eq!(a, b);
}
```
- [ ] Run FAIL: `cargo test -p arena_sim --test preview_smoke 2>&1 | tail -20`
- [ ] Create `preview.rs`:
```rust
use avian3d::prelude::*; use bevy::prelude::*; use obelisk_bevy::prelude::Faction;
use crate::obelisk::add_obelisk_sim;
use crate::spawn::{make_arena_combatant, spawn_arena_floor, SPAWN_MARKERS};
use crate::tuning::GRAVITY;
#[derive(Component)] pub struct PreviewCaster;
#[derive(Component)] pub struct PreviewDummy;
pub struct ArenaSimPreviewPlugin;
impl Plugin for ArenaSimPreviewPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<PhysicsPlugins>() { app.add_plugins(PhysicsPlugins::new(FixedUpdate)); }
        app.insert_resource(Gravity(Vec3::new(0.0, -GRAVITY, 0.0)));
        add_obelisk_sim(app, true);
    }
}
pub fn spawn_preview_world(commands: &mut Commands) {
    spawn_arena_floor(commands);
    let caster = make_arena_combatant(commands, "preview_caster", Faction::Player, SPAWN_MARKERS[0]);
    let dummy = make_arena_combatant(commands, "preview_dummy", Faction::Enemy, SPAWN_MARKERS[1]);
    commands.entity(caster).insert(PreviewCaster);
    commands.entity(dummy).insert(PreviewDummy);
}
```
(confirm the `is_plugin_added::<PhysicsPlugins>()` guard idiom; if `PhysicsPlugins` isn't a single `Plugin`, guard on a marker resource. The smoke test installs physics itself so it is independent of the guard.) Add `pub mod preview;` to `lib.rs`.
- [ ] Run PASS: `cargo test -p arena_sim --test preview_smoke 2>&1 | tail -12`
- [ ] Suite + clippy: `cargo test -p arena_sim 2>&1 | tail -8 && cargo clippy -p arena_sim --tests -- -D warnings 2>&1 | tail -5`
- [ ] Commit: `git add -A && git commit -m "feat(arena_sim): ArenaSimPreviewPlugin + spawn_preview_world; headless seeded firebolt->DamageResolved smoke + determinism"`

### Task 8: `EditorMode::Custom(CustomModeId)` + cover the 5 exhaustive match sites
**Files:** Create `/Users/luke/src/bevy_modal_editor/src/editor/custom_mode.rs`; Modify `src/editor/mod.rs`, `src/editor/state.rs` (variant 20-46; `panel_side` 58), `src/editor/input.rs` (E-key 227-240), `src/ui/panels.rs` (mode_text 97-109; mode_color 110-122; hints 313), `src/lib.rs`.
**Interfaces:** Produces — `CustomModeId(pub &'static str)` (`Clone,Copy,PartialEq,Eq,Hash,Debug`); `EditorMode::Custom(CustomModeId)`; `EditorMode::Custom(_).panel_side()==None`.
- [ ] Step 1: read `src/editor/mod.rs` to confirm module declarations (most submodules are private `mod X;` + `pub use`).
- [ ] Failing test in `state.rs` `#[cfg(test)]`:
```rust
#[cfg(test)] mod custom_mode_variant_tests {
    use super::*; use crate::editor::custom_mode::CustomModeId;
    #[test] fn custom_mode_is_states_compatible_and_side_less() {
        let m = EditorMode::Custom(CustomModeId("skill"));
        assert_eq!(m, EditorMode::Custom(CustomModeId("skill")));
        assert_ne!(m, EditorMode::Custom(CustomModeId("other")));
        assert_eq!(m.panel_side(), None);
        let mut set = std::collections::HashSet::new(); set.insert(m);
        assert!(set.contains(&EditorMode::Custom(CustomModeId("skill"))));
    }
}
```
- [ ] Run FAIL: `cd /Users/luke/src/bevy_modal_editor && nix develop --command cargo test -p bevy_modal_editor custom_mode_is_states 2>&1 | tail -12`
- [ ] Create `custom_mode.rs`: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub struct CustomModeId(pub &'static str);`
- [ ] `mod.rs`: add `pub mod custom_mode;` (per Step 1 form). `state.rs`: add `use super::custom_mode;`, variant `Custom(custom_mode::CustomModeId),` after `Effect`, and `panel_side` arm `Self::Custom(_) => None,`.
- [ ] `input.rs` E-key match (227-240): extend the final no-op arm with `| EditorMode::Custom(_) => {}`.
- [ ] `panels.rs`: add `EditorMode::Custom(_) => "CUSTOM",` (mode_text), `EditorMode::Custom(_) => colors::ACCENT_BLUE,` (mode_color), `EditorMode::Custom(_) => Vec::new(),` (get_hints_for_mode).
- [ ] `lib.rs`: `pub use editor::custom_mode::CustomModeId;`
- [ ] Run PASS + workspace check: `nix develop --command cargo test -p bevy_modal_editor custom_mode_is_states 2>&1 | tail -8 && nix develop --command cargo check --workspace 2>&1 | tail -5`
- [ ] Commit: `git checkout -b m1-custom-mode-seam && git add -A && git commit -m "feat(editor): add EditorMode::Custom(CustomModeId) + cover the 5 exhaustive match sites"`

### Task 9: `CustomModeRegistry` + `CustomModeDef` + `register_editor_mode` App ext
**Files:** Modify `src/editor/custom_mode.rs`, `src/editor/state.rs` (`EditorStatePlugin::build` ~395), `src/lib.rs`.
**Interfaces:** Produces — `CustomModeDef { id: &'static str, name: &'static str, activation_key: KeyCode, panel_side: PanelSide, panel: SystemId }`; `CustomModeRegistry { defs: Vec<CustomModeDef> }` with `lookup(CustomModeId)->Option<&CustomModeDef>` + `custom_mode_for_key(KeyCode)->Option<CustomModeId>`; `trait RegisterEditorModeExt { fn register_editor_mode(&mut self, def: CustomModeDef)->&mut Self; }` for `App`; registry inited in `EditorStatePlugin`.
- [ ] Failing test in `custom_mode.rs` `#[cfg(test)]`:
```rust
#[cfg(test)] mod tests {
    use super::*; use crate::editor::state::PanelSide; use bevy::prelude::*;
    fn noop_panel() {}
    #[test] fn register_editor_mode_inserts_and_lookups() {
        let mut app = App::new(); let panel = app.register_system(noop_panel);
        app.register_editor_mode(CustomModeDef { id:"skill", name:"SKILL", activation_key:KeyCode::KeyK, panel_side:PanelSide::Right, panel });
        let reg = app.world().resource::<CustomModeRegistry>();
        assert_eq!(reg.defs.len(), 1);
        assert_eq!(reg.lookup(CustomModeId("skill")).map(|d| d.name), Some("SKILL"));
        assert_eq!(reg.custom_mode_for_key(KeyCode::KeyK), Some(CustomModeId("skill")));
        assert_eq!(reg.custom_mode_for_key(KeyCode::KeyZ), None);
    }
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p bevy_modal_editor register_editor_mode_inserts 2>&1 | tail -12`
- [ ] Append to `custom_mode.rs`:
```rust
use bevy::ecs::system::SystemId; use bevy::prelude::*; use crate::editor::state::PanelSide;
#[derive(Clone)] pub struct CustomModeDef { pub id: &'static str, pub name: &'static str, pub activation_key: KeyCode, pub panel_side: PanelSide, pub panel: SystemId }
#[derive(Resource, Default)] pub struct CustomModeRegistry { pub defs: Vec<CustomModeDef> }
impl CustomModeRegistry {
    pub fn lookup(&self, id: CustomModeId) -> Option<&CustomModeDef> { self.defs.iter().find(|d| d.id == id.0) }
    pub fn custom_mode_for_key(&self, key: KeyCode) -> Option<CustomModeId> { self.defs.iter().find(|d| d.activation_key == key).map(|d| CustomModeId(d.id)) }
}
pub trait RegisterEditorModeExt { fn register_editor_mode(&mut self, def: CustomModeDef) -> &mut Self; }
impl RegisterEditorModeExt for App {
    fn register_editor_mode(&mut self, def: CustomModeDef) -> &mut Self {
        self.world_mut().get_resource_or_insert_with(CustomModeRegistry::default).defs.push(def); self
    }
}
```
- [ ] `EditorStatePlugin::build` (~395): add `.init_resource::<crate::editor::custom_mode::CustomModeRegistry>()`. `lib.rs`: `pub use editor::custom_mode::{CustomModeDef, CustomModeRegistry, RegisterEditorModeExt};`
- [ ] Run PASS + check: `nix develop --command cargo test -p bevy_modal_editor register_editor_mode_inserts 2>&1 | tail -8 && nix develop --command cargo check --workspace 2>&1 | tail -5`
- [ ] Commit: `git add -A && git commit -m "feat(editor): CustomModeRegistry + CustomModeDef + register_editor_mode App ext; inited in EditorStatePlugin"`

### Task 10: Custom-mode key entry in `handle_mode_input` + egui panel dispatch
**Files:** Modify `src/editor/input.rs` (`handle_mode_input` 40-73), `src/editor/custom_mode.rs` (add `dispatch_custom_panel`), `src/ui/mod.rs` (register in `EguiPrimaryContextPass`).
**Interfaces:** Produces — `dispatch_custom_panel(Res<State<EditorMode>>, Res<CustomModeRegistry>, Commands)` runs the active custom mode's `panel` SystemId; `handle_mode_input` toggles registered modes under `can_change_mode` + `GameState::Editing` gates.
- [ ] Failing test in `custom_mode.rs` `#[cfg(test)]`:
```rust
    #[derive(Resource, Default)] struct PanelDrawCount(u32);
    fn counting_panel(mut c: ResMut<PanelDrawCount>) { c.0 += 1; }
    #[test] fn dispatch_runs_the_registered_panel_when_in_that_custom_mode() {
        let mut app = App::new(); app.init_resource::<PanelDrawCount>(); app.init_state::<crate::editor::state::EditorMode>();
        let panel = app.register_system(counting_panel);
        app.register_editor_mode(CustomModeDef { id:"skill", name:"SKILL", activation_key:KeyCode::KeyK, panel_side:PanelSide::Right, panel });
        app.add_systems(Update, dispatch_custom_panel);
        app.update(); assert_eq!(app.world().resource::<PanelDrawCount>().0, 0);
        app.world_mut().resource_mut::<NextState<crate::editor::state::EditorMode>>().set(crate::editor::state::EditorMode::Custom(CustomModeId("skill")));
        app.update(); app.update();
        assert!(app.world().resource::<PanelDrawCount>().0 >= 1);
    }
```
- [ ] Run FAIL: `nix develop --command cargo test -p bevy_modal_editor dispatch_runs_the_registered 2>&1 | tail -12`
- [ ] Add to `custom_mode.rs`:
```rust
use crate::editor::state::EditorMode;
pub fn dispatch_custom_panel(mode: Res<State<EditorMode>>, registry: Res<CustomModeRegistry>, mut commands: Commands) {
    if let EditorMode::Custom(id) = mode.get() { if let Some(def) = registry.lookup(*id) { commands.run_system(def.panel); } }
}
```
- [ ] Run dispatch test PASS: `nix develop --command cargo test -p bevy_modal_editor dispatch_runs_the_registered 2>&1 | tail -8`
- [ ] Production dispatch: `src/ui/mod.rs` `UiPlugin::build` add `.add_systems(EguiPrimaryContextPass, crate::editor::custom_mode::dispatch_custom_panel)`.
- [ ] Entry: extend `handle_mode_input` (input.rs:40) — add param `custom_registry: Res<crate::editor::custom_mode::CustomModeRegistry>,` and before the E-key match insert:
```rust
    for def in custom_registry.defs.iter() {
        if keyboard.just_pressed(def.activation_key) {
            let target = EditorMode::Custom(crate::editor::custom_mode::CustomModeId(def.id));
            if *current_mode.get() == target { next_mode.set(EditorMode::View); }
            else if can_change_mode { next_mode.set(target); *transform_op = TransformOperation::None; *axis_constraint = AxisConstraint::None; }
            return;
        }
    }
```
(inherits the system's `should_process_input` + `GameState::Editing` guards; Escape→View already exits.)
- [ ] Run editor tests + check: `nix develop --command cargo test -p bevy_modal_editor 2>&1 | tail -10 && nix develop --command cargo check --workspace 2>&1 | tail -5`
- [ ] Commit: `git add -A && git commit -m "feat(editor): custom-mode key entry in handle_mode_input + egui panel dispatch"`

### Task 11: `arena_editor` binary booting `EditorPlugin{add_physics:false}` + `GamePlugin`
**Files:** Create `crates/arena_editor/Cargo.toml`, `src/main.rs`, `src/lib.rs`, `tests/boots.rs`; Modify workspace `Cargo.toml`.
**Interfaces:** Produces — `arena_editor::build_editor_app() -> App` (headless, `add_egui:false`); windowed `main()`. Consumes — `bevy_modal_editor::{EditorPlugin, EditorPluginConfig, GamePlugin, recommended_image_plugin, RegisterEditorModeExt, CustomModeDef, CustomModeId, EditorMode}`.
- [ ] Failing boot test `tests/boots.rs`: `use bevy::prelude::*; use bevy_modal_editor::EditorMode; #[test] fn editor_app_registers_the_editor_mode_state(){ let app=arena_editor::build_editor_app(); assert!(app.world().contains_resource::<State<EditorMode>>()); }`
- [ ] Run FAIL: `cd /Users/luke/src/obelisk-arena && nix develop --command cargo test -p arena_editor 2>&1 | tail -10`
- [ ] Create `Cargo.toml`:
```toml
[package]
name = "arena_editor"
version = "0.1.0"
edition = "2021"
default-run = "arena-editor"
[[bin]]
name = "arena-editor"
path = "src/main.rs"
[dependencies]
bevy = { workspace = true }
avian3d = { workspace = true }
obelisk-bevy = { workspace = true }
arena_sim = { workspace = true }
arena_skills = { workspace = true }
bevy_modal_editor = { workspace = true }
bevy_editor_game = { workspace = true }
[dev-dependencies]
obelisk-bevy = { workspace = true, features = ["test-support"] }
```
(the dev-dep enables `obelisk_bevy::testkit` for Tasks 13/21/32 — testkit is `#[cfg(feature="test-support")]`.)
- [ ] Workspace `Cargo.toml`: add `"crates/arena_editor"` to members; `[workspace.dependencies]` add `bevy_modal_editor = { path = "../bevy_modal_editor" }`, `bevy_editor_game = { path = "../bevy_modal_editor/crates/bevy_editor_game" }`.
- [ ] Create `src/lib.rs`:
```rust
use bevy::prelude::*;
use bevy_modal_editor::{EditorPlugin, EditorPluginConfig, GamePlugin};
pub fn build_editor_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(AssetPlugin::default())
        .add_plugins(EditorPlugin::new(EditorPluginConfig { add_egui: false, add_physics: false, ..default() }))
        .add_plugins(GamePlugin);
    app
}
```
- [ ] Create `src/main.rs`:
```rust
use bevy::prelude::*;
use bevy_modal_editor::{recommended_image_plugin, EditorPlugin, EditorPluginConfig, GamePlugin};
fn main() {
    App::new().add_plugins(DefaultPlugins.set(recommended_image_plugin()))
        .add_plugins(EditorPlugin::new(EditorPluginConfig { add_physics: false, add_egui: true, ..default() }))
        .add_plugins(GamePlugin).run();
}
```
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): scaffold editor binary on EditorPlugin{add_physics:false}+GamePlugin; headless boot test"`

### Task 12: `SkillDesignerPlugin` + `register_skill_mode` registering the empty Skill mode
**Files:** Create `crates/arena_editor/src/skill_designer.rs`; Modify `src/lib.rs`, `src/main.rs`, `tests/boots.rs`.
**Interfaces:** Produces — `arena_editor::SKILL_MODE_ID="skill"`; `skill_designer::SkillDesignerPlugin`; `register_skill_mode(&mut App)`. Registered mode = `EditorMode::Custom(CustomModeId("skill"))`, key `KeyCode::KeyK`, panel side `Right`, panel = `draw_skill_panel` stub.
- [ ] Failing test (append `boots.rs`):
```rust
#[test] fn skill_mode_is_registered_and_enterable() {
    use bevy_modal_editor::{CustomModeId, CustomModeRegistry, EditorMode};
    let mut app = arena_editor::build_editor_app(); arena_editor::register_skill_mode(&mut app);
    let reg = app.world().resource::<CustomModeRegistry>();
    assert!(reg.lookup(CustomModeId(arena_editor::SKILL_MODE_ID)).is_some());
    app.world_mut().resource_mut::<NextState<EditorMode>>().set(EditorMode::Custom(CustomModeId(arena_editor::SKILL_MODE_ID)));
    app.update();
    assert_eq!(*app.world().resource::<State<EditorMode>>().get(), EditorMode::Custom(CustomModeId(arena_editor::SKILL_MODE_ID)));
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor skill_mode_is_registered 2>&1 | tail -12`
- [ ] Create `skill_designer.rs`:
```rust
use bevy::prelude::*;
use bevy_modal_editor::{CustomModeDef, PanelSide, RegisterEditorModeExt};
pub const SKILL_MODE_ID: &str = "skill";
fn draw_skill_panel() {}
pub fn register_skill_mode(app: &mut App) {
    let panel = app.register_system(draw_skill_panel);
    app.register_editor_mode(CustomModeDef { id: SKILL_MODE_ID, name: "SKILL", activation_key: KeyCode::KeyK, panel_side: PanelSide::Right, panel });
}
pub struct SkillDesignerPlugin;
impl Plugin for SkillDesignerPlugin { fn build(&self, app: &mut App) { register_skill_mode(app); } }
```
- [ ] `lib.rs`: `pub mod skill_designer; pub use skill_designer::{register_skill_mode, SkillDesignerPlugin, SKILL_MODE_ID};`. `main.rs`: add `.add_plugins(arena_editor::SkillDesignerPlugin)` after `GamePlugin`.
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor skill_mode_is_registered 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): SkillDesignerPlugin registers the empty Skill mode (key K)"`

### Task 13: Embed the `arena_sim` preview mini-world (caster+dummy, idle in Editing)
**Files:** Modify `src/skill_designer.rs`, `src/main.rs`, `tests/boots.rs`.
**Interfaces:** Produces — `arena_editor::spawn_preview_on_startup` system; editor adds `ArenaSimPreviewPlugin` + spawns the mini-world idle in `GameState::Editing`. (Reconciliation: Task 21 retires this Startup spawn in favor of the spec's Play/Reset lifecycle; `spawn_preview_world` stays a tested arena_sim helper.)
- [ ] Failing test (append `boots.rs`; confirm `RunSystemOnce` import = `bevy::ecs::system::RunSystemOnce`):
```rust
#[test] fn preview_world_spawns_a_caster_and_a_dummy() {
    use arena_sim::preview::{PreviewCaster, PreviewDummy}; use avian3d::prelude::PhysicsPlugins; use bevy::prelude::FixedUpdate;
    let mut app = arena_editor::build_editor_app();
    app.add_plugins(PhysicsPlugins::new(FixedUpdate)).add_plugins(arena_sim::preview::ArenaSimPreviewPlugin);
    obelisk_bevy::testkit::init_test_obelisk();
    app.world_mut().run_system_once(arena_editor::spawn_preview_on_startup).ok();
    app.world_mut().flush();
    assert_eq!(app.world_mut().query::<&PreviewCaster>().iter(app.world()).count(), 1);
    assert_eq!(app.world_mut().query::<&PreviewDummy>().iter(app.world()).count(), 1);
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor preview_world_spawns 2>&1 | tail -14`
- [ ] Add to `skill_designer.rs`: `pub fn spawn_preview_on_startup(mut commands: Commands) { arena_sim::preview::spawn_preview_world(&mut commands); }` and re-export from `lib.rs`.
- [ ] `main.rs`: add `.add_plugins(arena_sim::preview::ArenaSimPreviewPlugin)` and `.add_systems(Startup, arena_editor::spawn_preview_on_startup)`.
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor preview_world_spawns 2>&1 | tail -8`
- [ ] Final M1 gate: `nix develop --command cargo test -p arena_editor 2>&1 | tail -6 && cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -3 && bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): embed arena_sim preview mini-world (caster+dummy) at startup, idle in Editing"`

---

# Milestone M2 — Timeline authoring + "Play the real skill" preview
Ships: author a `CastTimeline` on the bottom-dock timeline and Play it through the real obelisk sim. Lives in `arena_editor` (all commands `nix develop`). Net-test is a sanity gate at the end (M2 touches no game runtime).

### Task 14: `PreviewSimConfigPlugin` — load obelisk constants/effects/skills + seed RNG
**Files:** Create `crates/arena_editor/src/io.rs` (`editor_root` only), `src/sim_config.rs`, `tests/sim_config.rs`; Modify `src/lib.rs`, `src/main.rs`.
**Interfaces:** Produces — `io::editor_root()->PathBuf` (workspace root = manifest `ancestors().nth(2)`); `sim_config::PreviewSimConfigPlugin` (default constants + `config/effects` + `config/skills` + `seed_combat_rng(1)`; mirrors arena_game server.rs:47-50).
- [ ] Failing test `tests/sim_config.rs`:
```rust
use obelisk_bevy::prelude::{CombatRng, SkillRegistry};
#[test] fn preview_sim_config_loads_firebolt_and_seeds_rng() {
    let mut app = arena_editor::build_editor_app();
    app.add_plugins(arena_editor::sim_config::PreviewSimConfigPlugin);
    assert!(app.world().resource::<SkillRegistry>().0.contains_key("firebolt"));
    assert!(app.world().get_resource::<CombatRng>().is_some());
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test sim_config 2>&1 | tail -12`
- [ ] Create `io.rs`:
```rust
use std::path::PathBuf;
pub fn editor_root() -> PathBuf {
    match std::env::var_os("CARGO_MANIFEST_DIR") {
        Some(dir) => PathBuf::from(dir).ancestors().nth(2).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}
```
- [ ] Create `sim_config.rs`:
```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::{ObeliskConfigExt, SkillSource};
use crate::io::editor_root;
pub struct PreviewSimConfigPlugin;
impl Plugin for PreviewSimConfigPlugin {
    fn build(&self, app: &mut App) {
        let root = editor_root();
        app.add_obelisk_config_constants_default();
        app.add_obelisk_effects(&root.join("config/effects"));
        app.add_obelisk_skills(SkillSource::Dir(root.join("config/skills")));
        app.seed_combat_rng(1);
    }
}
```
- [ ] `lib.rs`: `pub mod io; pub mod sim_config; pub use sim_config::PreviewSimConfigPlugin;`. `main.rs`: add `.add_plugins(arena_editor::sim_config::PreviewSimConfigPlugin)` after `SkillDesignerPlugin`.
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor --test sim_config 2>&1 | tail -8`
- [ ] Commit: `git checkout -b m2-skill-timeline && git add -A && git commit -m "feat(arena_editor): PreviewSimConfigPlugin loads obelisk constants/effects/skills + seeds RNG"`

### Task 15: `EditedSkill` resource + `blank_cast_timeline` + `derive_vfx_cues`
**Files:** Create `crates/arena_editor/src/model.rs`; Modify `src/lib.rs`, `src/skill_designer.rs`, `tests/boots.rs`.
**Interfaces:** Produces — `model::EditedSkill { timeline: CastTimeline, path: PathBuf, dirty: bool, selected_window: Option<usize> }` (`#[derive(Resource)]`) + `EditedSkill::from_timeline(CastTimeline, PathBuf)`; `blank_cast_timeline(impl Into<String>)->CastTimeline` (0.3/0.1/0.2, SingleEntity/Instant); `derive_vfx_cues(&CastTimeline)->HashMap<String,String>` (`on_cast→"{id}_cast"`, `on_window_{wid}→"{id}_window_{wid}"`, `on_hit→"{id}_impact"` — the LOCKED cue map; M3 binds lanes to these VALUES).
- [ ] Failing test (append `boots.rs`):
```rust
#[test] fn skill_designer_inits_an_edited_skill_resource() {
    use arena_editor::model::EditedSkill;
    let mut app = arena_editor::build_editor_app(); app.add_plugins(arena_editor::SkillDesignerPlugin);
    let edited = app.world().resource::<EditedSkill>();
    assert!(!edited.timeline.skill_id.is_empty());
    assert!(edited.timeline.phase_durations.windup > 0.0);
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test boots skill_designer_inits 2>&1 | tail -12`
- [ ] Create `model.rs`:
```rust
use bevy::prelude::*;
use obelisk_bevy::assets::{CastDelivery, CastTargeting, CastTimeline, CollisionWindow, PhaseDurations};
use std::collections::HashMap; use std::path::PathBuf;
#[derive(Resource)]
pub struct EditedSkill { pub timeline: CastTimeline, pub path: PathBuf, pub dirty: bool, pub selected_window: Option<usize> }
impl EditedSkill { pub fn from_timeline(timeline: CastTimeline, path: PathBuf) -> Self { Self { timeline, path, dirty: false, selected_window: None } } }
pub fn blank_cast_timeline(skill_id: impl Into<String>) -> CastTimeline {
    CastTimeline { skill_id: skill_id.into(), phase_durations: PhaseDurations { windup: 0.3, active: 0.1, recovery: 0.2 },
        collision_windows: Vec::<CollisionWindow>::new(), targeting: CastTargeting::SingleEntity { range: 15.0 }, delivery: CastDelivery::Instant, vfx_cues: HashMap::new() }
}
pub fn derive_vfx_cues(tl: &CastTimeline) -> HashMap<String, String> {
    let id = &tl.skill_id; let mut cues = HashMap::new();
    cues.insert("on_cast".to_string(), format!("{id}_cast"));
    for w in &tl.collision_windows { cues.insert(format!("on_window_{}", w.id), format!("{id}_window_{}", w.id)); }
    cues.insert("on_hit".to_string(), format!("{id}_impact"));
    cues
}
```
- [ ] Add unit test in `model.rs` `#[cfg(test)]` proving cues for a 1-window firebolt = `{on_cast:firebolt_cast, on_window_bolt:firebolt_window_bolt, on_hit:firebolt_impact}` (len 3).
- [ ] `lib.rs`: `pub mod model; pub use model::{blank_cast_timeline, derive_vfx_cues, EditedSkill};`. `skill_designer.rs` `SkillDesignerPlugin::build`: `app.insert_resource(EditedSkill::from_timeline(blank_cast_timeline("firebolt"), crate::io::editor_root().join("assets/skills/firebolt.cast.ron")));` (Task 16 swaps to load-or-blank).
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor 2>&1 | tail -10`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): EditedSkill + blank_cast_timeline + derive_vfx_cues (locked cue keys)"`

### Task 16: `save_cast_timeline` / `load_cast_timeline` — `.cast.ron` round-trip; load-or-blank
**Files:** Modify `src/io.rs`, `src/skill_designer.rs`, `Cargo.toml` (+`ron`); Create `tests/cast_io.rs`.
**Interfaces:** Produces — `io::{default_cast_path(&str)->PathBuf, save_cast_timeline(&CastTimeline,&Path)->std::io::Result<()>, load_cast_timeline(&Path)->Result<CastTimeline,String>}`.
- [ ] Failing test `tests/cast_io.rs`:
```rust
use arena_editor::io::{load_cast_timeline, save_cast_timeline};
use arena_editor::model::blank_cast_timeline;
use obelisk_bevy::assets::{CastDelivery, CastTargeting, CollisionShape, CollisionWindow, HitFilter, HitMode, VolumeMotion, WindowPhase};
#[test] fn author_save_reload_round_trips() {
    let mut tl = blank_cast_timeline("zap");
    tl.collision_windows.push(CollisionWindow { id:"burst".into(), spawn_phase:WindowPhase::Active, spawn_offset:0.0, active_duration:0.2,
        shape:CollisionShape::Cone{angle:90.0,range:5.0}, motion:VolumeMotion::Linear{speed:8.0}, hit_filter:HitFilter::Enemies, hit_mode:HitMode::OncePerTarget, rehit_interval:None });
    tl.targeting = CastTargeting::Cone{angle:90.0,range:5.0}; tl.delivery = CastDelivery::Projectile{speed:12.0};
    tl.vfx_cues.insert("on_cast".into(), "zap_cast".into());
    let path = std::env::temp_dir().join("arena_editor_rt_zap.cast.ron");
    save_cast_timeline(&tl, &path).expect("save"); let back = load_cast_timeline(&path).expect("reload");
    assert_eq!(tl.skill_id, back.skill_id);
    assert_eq!(format!("{:?}", tl.phase_durations), format!("{:?}", back.phase_durations));
    assert_eq!(format!("{:?}", tl.collision_windows[0]), format!("{:?}", back.collision_windows[0]));
    assert_eq!(format!("{:?}", tl.targeting), format!("{:?}", back.targeting));
    assert_eq!(format!("{:?}", tl.delivery), format!("{:?}", back.delivery));
    assert_eq!(tl.vfx_cues, back.vfx_cues);
}
#[test] fn loads_the_real_firebolt_asset() {
    let path = arena_editor::io::editor_root().join("assets/skills/firebolt.cast.ron");
    let tl = load_cast_timeline(&path).expect("parses"); assert_eq!(tl.skill_id, "firebolt"); assert_eq!(tl.collision_windows.len(), 1);
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test cast_io 2>&1 | tail -14`
- [ ] `Cargo.toml`: add `ron = { workspace = true }` to `[dependencies]`.
- [ ] Append to `io.rs`:
```rust
use obelisk_bevy::assets::CastTimeline; use std::path::Path;
pub fn default_cast_path(skill_id: &str) -> PathBuf { editor_root().join(format!("assets/skills/{skill_id}.cast.ron")) }
pub fn save_cast_timeline(tl: &CastTimeline, path: &Path) -> std::io::Result<()> {
    let s = ron::ser::to_string_pretty(tl, ron::ser::PrettyConfig::new()).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(path, s)
}
pub fn load_cast_timeline(path: &Path) -> Result<CastTimeline, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    ron::de::from_str::<CastTimeline>(&s).map_err(|e| e.to_string())
}
```
- [ ] `skill_designer.rs`: swap the `EditedSkill` init to load-or-blank: `let path = crate::io::default_cast_path("firebolt"); let timeline = crate::io::load_cast_timeline(&path).unwrap_or_else(|_| blank_cast_timeline("firebolt")); app.insert_resource(EditedSkill::from_timeline(timeline, path));`
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor --test cast_io 2>&1 | tail -10`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): save/load .cast.ron via ron; round-trip; load-or-blank at startup"`

### Task 17: `timeline_geom` — phase-span + window-span + `time_to_x` (pure)
**Files:** Create `crates/arena_editor/src/timeline_geom.rs`; Modify `src/lib.rs`.
**Interfaces:** Produces — `timeline_geom::{total_duration(&PhaseDurations)->f32, phase_spans(&PhaseDurations)->[(f32,f32);3], time_to_x(t,span,left,width)->f32, window_span(&CollisionWindow,&PhaseDurations)->(f32,f32)}`.
- [ ] Failing test (`timeline_geom.rs` `#[cfg(test)]`): assert `phase_spans({0.3,0.1,0.2})==[(0,0.3),(0.3,0.4),(0.4,0.6)]`, `total_duration==0.6`; `time_to_x(0.0,0.6,10,100)==10`, `time_to_x(0.6,0.6,10,100)==110`, `time_to_x(0.3,0.6,0,100)==50`, `time_to_x(5,0,7,100)==7`; `window_span(Active@0.0,0.1)==(0.3,0.4)`, `window_span(Recovery@0.05,0.1).0==0.45`.
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --lib timeline_geom 2>&1 | tail -12`
- [ ] Impl:
```rust
use obelisk_bevy::assets::{CollisionWindow, PhaseDurations, WindowPhase};
pub fn total_duration(d: &PhaseDurations) -> f32 { d.windup.max(0.0) + d.active.max(0.0) + d.recovery.max(0.0) }
pub fn phase_spans(d: &PhaseDurations) -> [(f32, f32); 3] {
    let w = d.windup.max(0.0); let a = d.active.max(0.0); let r = d.recovery.max(0.0);
    [(0.0, w), (w, w + a), (w + a, w + a + r)]
}
pub fn time_to_x(t: f32, span: f32, left: f32, width: f32) -> f32 { if span <= 0.0 { left } else { left + (t / span).clamp(0.0, 1.0) * width } }
pub fn window_span(w: &CollisionWindow, d: &PhaseDurations) -> (f32, f32) {
    let phase_start = match w.spawn_phase { WindowPhase::Windup => 0.0, WindowPhase::Active => d.windup.max(0.0), WindowPhase::Recovery => d.windup.max(0.0) + d.active.max(0.0) };
    let start = phase_start + w.spawn_offset.max(0.0); (start, start + w.active_duration.max(0.0))
}
```
- [ ] `lib.rs`: `pub mod timeline_geom;`. Run PASS: `nix develop --command cargo test -p arena_editor --lib timeline_geom 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): timeline_geom phase/window-span + time_to_x (pure)"`

### Task 18: Edit ops — phase-boundary drag, window add, window-start drag (pure)
**Files:** Create `crates/arena_editor/src/edits.rs`; Modify `src/lib.rs`.
**Interfaces:** Produces — `edits::{set_phase_boundary(&mut PhaseDurations, boundary: usize, t: f32), default_window(impl Into<String>)->CollisionWindow, add_collision_window(&mut CastTimeline) (auto-id window_<n>), set_window_start(&mut CollisionWindow,&PhaseDurations,new_start_abs: f32)}`.
- [ ] Failing test (`edits.rs` `#[cfg(test)]`): `set_phase_boundary({0.3,0.1,0.2},0,0.35)`→`{0.35,0.05,0.2}`; clamp at `cap`→`{0.4,0.0,0.2}`; `add_collision_window` appends `window_0` (Sphere) then `window_1`; `set_window_start(default("a"),d,0.32)`→offset 0.02, `0.1`→clamp 0.
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --lib edits 2>&1 | tail -12`
- [ ] Impl:
```rust
use obelisk_bevy::assets::{CastTimeline, CollisionShape, CollisionWindow, HitFilter, HitMode, PhaseDurations, VolumeMotion, WindowPhase};
pub fn set_phase_boundary(d: &mut PhaseDurations, boundary: usize, t: f32) {
    match boundary {
        0 => { let cap = d.windup + d.active; let nb = t.clamp(0.0, cap); let delta = nb - d.windup; d.windup = nb; d.active = (d.active - delta).max(0.0); }
        1 => { let lo = d.windup; let cap = lo + d.active + d.recovery; let nb = t.clamp(lo, cap); let na = nb - lo; let delta = na - d.active; d.active = na; d.recovery = (d.recovery - delta).max(0.0); }
        _ => {}
    }
}
pub fn default_window(id: impl Into<String>) -> CollisionWindow {
    CollisionWindow { id: id.into(), spawn_phase: WindowPhase::Active, spawn_offset: 0.0, active_duration: 0.1,
        shape: CollisionShape::Sphere { radius: 0.5 }, motion: VolumeMotion::Static, hit_filter: HitFilter::Enemies, hit_mode: HitMode::OncePerTarget, rehit_interval: None }
}
pub fn add_collision_window(tl: &mut CastTimeline) { let id = format!("window_{}", tl.collision_windows.len()); tl.collision_windows.push(default_window(id)); }
pub fn set_window_start(w: &mut CollisionWindow, d: &PhaseDurations, new_start_abs: f32) {
    let phase_start = match w.spawn_phase { WindowPhase::Windup => 0.0, WindowPhase::Active => d.windup.max(0.0), WindowPhase::Recovery => d.windup.max(0.0) + d.active.max(0.0) };
    w.spawn_offset = (new_start_abs - phase_start).max(0.0);
}
```
- [ ] `lib.rs`: `pub mod edits;`. Run PASS: `nix develop --command cargo test -p arena_editor --lib edits 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): edit ops — set_phase_boundary, add/default window, set_window_start (pure)"`

### Task 19: Enum option helpers — targeting/delivery/shape/motion index↔ctor (pure)
**Files:** Create `crates/arena_editor/src/enum_ui.rs`; Modify `src/lib.rs`.
**Interfaces:** Produces — `enum_ui::{TARGETING_LABELS:[&str;4], targeting_index/targeting_variant, DELIVERY_LABELS:[&str;3], delivery_index/delivery_variant, SHAPE_LABELS:[&str;3], shape_index/shape_variant, MOTION_LABELS:[&str;2], motion_index/motion_variant}` for the no-`PartialEq` obelisk enums (HitFilter/HitMode/WindowPhase derive Eq → panel uses `selectable_value` directly).
- [ ] Failing test (`enum_ui.rs` `#[cfg(test)]`): `targeting_index(SelfCast)==0`, `Cone==3`, `targeting_variant(1)` is SingleEntity, `targeting_index(targeting_variant(2))==2`; `delivery_index(Projectile)==2`, `delivery_variant(0)` Melee; `shape_index(Capsule)==1`, `Cone==2`, `shape_variant(0)` Sphere; `motion_index(Linear)==1`, `motion_variant(0)` Static.
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --lib enum_ui 2>&1 | tail -12`
- [ ] Impl:
```rust
use obelisk_bevy::assets::{CastDelivery, CastTargeting, CollisionShape, VolumeMotion};
pub const TARGETING_LABELS: [&str; 4] = ["SelfCast", "SingleEntity", "Direction", "Cone"];
pub fn targeting_index(t: &CastTargeting) -> usize { match t { CastTargeting::SelfCast=>0, CastTargeting::SingleEntity{..}=>1, CastTargeting::Direction{..}=>2, CastTargeting::Cone{..}=>3 } }
pub fn targeting_variant(i: usize) -> CastTargeting { match i { 0=>CastTargeting::SelfCast, 2=>CastTargeting::Direction{range:15.0}, 3=>CastTargeting::Cone{angle:90.0,range:5.0}, _=>CastTargeting::SingleEntity{range:15.0} } }
pub const DELIVERY_LABELS: [&str; 3] = ["Melee", "Instant", "Projectile"];
pub fn delivery_index(d: &CastDelivery) -> usize { match d { CastDelivery::Melee=>0, CastDelivery::Instant=>1, CastDelivery::Projectile{..}=>2 } }
pub fn delivery_variant(i: usize) -> CastDelivery { match i { 0=>CastDelivery::Melee, 2=>CastDelivery::Projectile{speed:20.0}, _=>CastDelivery::Instant } }
pub const SHAPE_LABELS: [&str; 3] = ["Sphere", "Capsule", "Cone"];
pub fn shape_index(s: &CollisionShape) -> usize { match s { CollisionShape::Sphere{..}=>0, CollisionShape::Capsule{..}=>1, CollisionShape::Cone{..}=>2 } }
pub fn shape_variant(i: usize) -> CollisionShape { match i { 1=>CollisionShape::Capsule{radius:0.4,height:1.0}, 2=>CollisionShape::Cone{angle:90.0,range:3.0}, _=>CollisionShape::Sphere{radius:0.5} } }
pub const MOTION_LABELS: [&str; 2] = ["Static", "Linear"];
pub fn motion_index(m: &VolumeMotion) -> usize { match m { VolumeMotion::Static=>0, VolumeMotion::Linear{..}=>1 } }
pub fn motion_variant(i: usize) -> VolumeMotion { match i { 1=>VolumeMotion::Linear{speed:20.0}, _=>VolumeMotion::Static } }
```
- [ ] `lib.rs`: `pub mod enum_ui;`. Run PASS: `nix develop --command cargo test -p arena_editor --lib enum_ui 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): enum_ui index<->ctor helpers for the no-PartialEq obelisk enums"`

### Task 20: `PreviewControllerPlugin` — Play spawns duel + registers edited timeline + casts; headless damage + determinism
**Files:** Create `crates/arena_editor/src/preview_controller.rs`, `tests/preview_play.rs`; Modify `src/lib.rs`, `src/skill_designer.rs`, `src/main.rs`.
**Interfaces:** Produces — `preview_controller::PreviewControllerPlugin`; `spawn_preview_floor(Commands)` (Startup, persistent); `start_preview(MessageReader<GameStartedEvent>, Res<EditedSkill>, ResMut<CastTimelineHandles>, ResMut<Assets<CastTimeline>>, Commands)` (registers `edited.timeline.clone()` with `derive_vfx_cues` into `CastTimelineHandles`, spawns Player `PreviewCaster` + Enemy `PreviewDummy` both `GameEntity`-tagged, `grant_skill`, `cast_skill_at`). **Reconciliation:** this Task retires Task 13's `spawn_preview_on_startup` Startup wiring — floor → controller Startup, combatants → on Play.
- [ ] Step 1: read `main.rs` + `skill_designer.rs` to find the Task 13 `Startup, spawn_preview_on_startup` registration; confirm `ArenaSimPreviewPlugin` installs physics+gravity+`add_obelisk_sim` and does NOT spawn the world. Confirm Bevy-0.18 `World::write_message` name (else `app.world_mut().resource_mut::<Messages<GameStartedEvent>>().write(...)`).
- [ ] Failing test `tests/preview_play.rs`:
```rust
use arena_editor::io::{editor_root, load_cast_timeline};
use arena_editor::model::EditedSkill;
use arena_editor::preview_controller::PreviewControllerPlugin;
use arena_sim::preview::{ArenaSimPreviewPlugin, PreviewCaster};
use bevy::prelude::*; use bevy_editor_game::{GameEntity, GameStartedEvent};
use obelisk_bevy::prelude::{ObeliskConfigExt, SkillSource};
use obelisk_bevy::testkit::{init_test_obelisk, EventRecorder, EventRecorderPlugin};
use std::time::Duration;
fn run(seed: u64, ticks: usize) -> App {
    init_test_obelisk(); let root = editor_root();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(bevy::asset::AssetPlugin { file_path: ".".into(), ..default() })
        .add_plugins(bevy::mesh::MeshPlugin).add_plugins(bevy::scene::ScenePlugin).add_plugins(EventRecorderPlugin)
        .init_state::<bevy_editor_game::GameState>().add_message::<GameStartedEvent>().add_message::<bevy_editor_game::GameResetEvent>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0/60.0)))
        .insert_resource(Time::<Fixed>::from_hz(60.0)).add_plugins(ArenaSimPreviewPlugin).add_plugins(PreviewControllerPlugin);
    app.add_obelisk_skills(SkillSource::Dir(root.join("config/skills"))); app.seed_combat_rng(seed);
    let tl = load_cast_timeline(&root.join("assets/skills/firebolt.cast.ron")).expect("firebolt");
    app.insert_resource(EditedSkill::from_timeline(tl, root.join("assets/skills/firebolt.cast.ron")));
    app.finish(); app.cleanup();
    app.world_mut().write_message(GameStartedEvent);
    for _ in 0..ticks { app.update(); }
    app
}
#[test] fn play_resolves_damage_on_the_dummy() {
    let app = run(0xC0FFEE, 90); let rec = app.world().resource::<EventRecorder>();
    assert!(!rec.cast_began.is_empty()); assert!(!rec.damage_resolved.is_empty());
    assert!(rec.damage_resolved.iter().map(|d| d.total_damage).sum::<f64>() > 0.0);
}
#[test] fn play_spawns_game_entity_tagged_caster() {
    let mut app = run(1, 3);
    let n = app.world_mut().query_filtered::<Entity, (With<PreviewCaster>, With<GameEntity>)>().iter(app.world()).count();
    assert_eq!(n, 1);
}
#[test] fn preview_is_deterministic() {
    let total = |s: u64| run(s, 90).world().resource::<EventRecorder>().damage_resolved.iter().map(|d| d.total_damage).sum::<f64>();
    let (a, b) = (total(0xABCDEF), total(0xABCDEF)); assert!(a > 0.0); assert_eq!(a, b);
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test preview_play 2>&1 | tail -20`
- [ ] Create `preview_controller.rs`:
```rust
use bevy::prelude::*;
use bevy_editor_game::{GameEntity, GameStartedEvent};
use obelisk_bevy::prelude::{CastSkillExt, CastTimeline, CastTimelineHandles, Faction, ObeliskCommandsExt};
use arena_sim::preview::{PreviewCaster, PreviewDummy};
use arena_sim::spawn::{make_arena_combatant, spawn_arena_floor, SPAWN_MARKERS};
use crate::model::{derive_vfx_cues, EditedSkill};
pub struct PreviewControllerPlugin;
impl Plugin for PreviewControllerPlugin {
    fn build(&self, app: &mut App) { app.add_systems(Startup, spawn_preview_floor).add_systems(Update, start_preview); }
}
pub fn spawn_preview_floor(mut commands: Commands) { spawn_arena_floor(&mut commands); }
pub fn start_preview(mut started: MessageReader<GameStartedEvent>, edited: Res<EditedSkill>, mut handles: ResMut<CastTimelineHandles>, mut timelines: ResMut<Assets<CastTimeline>>, mut commands: Commands) {
    if started.read().next().is_none() { return; }
    let mut tl = edited.timeline.clone(); tl.vfx_cues = derive_vfx_cues(&tl);
    let skill_id = tl.skill_id.clone(); let handle = timelines.add(tl); handles.0.insert(skill_id.clone(), handle);
    let caster = make_arena_combatant(&mut commands, "preview_caster", Faction::Player, SPAWN_MARKERS[0]);
    commands.entity(caster).insert((PreviewCaster, GameEntity)).grant_skill(skill_id.clone());
    let dummy = make_arena_combatant(&mut commands, "preview_dummy", Faction::Enemy, SPAWN_MARKERS[1]);
    commands.entity(dummy).insert((PreviewDummy, GameEntity));
    commands.entity(caster).cast_skill_at(skill_id, dummy);
}
```
- [ ] `lib.rs`: `pub mod preview_controller; pub use preview_controller::PreviewControllerPlugin;`. `skill_designer.rs`: add `app.add_plugins(crate::preview_controller::PreviewControllerPlugin);` and REMOVE the Task 13 `spawn_preview_on_startup` Startup registration. `main.rs`: remove `.add_systems(Startup, arena_editor::spawn_preview_on_startup)` (keep `ArenaSimPreviewPlugin`).
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor --test preview_play 2>&1 | tail -16`
- [ ] Re-point Task 13 boot test: update `preview_world_spawns` to call `arena_sim::preview::spawn_preview_world` directly via `RunSystemOnce`/a closure system (it now tests the arena_sim helper, decoupled from the removed startup wiring): `nix develop --command cargo test -p arena_editor --test boots 2>&1 | tail -12`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): PreviewControllerPlugin — Play spawns duel + registers edited timeline + casts; retire idle startup spawn"`

### Task 21: `Playhead` synced from `ActiveCast`; cleared on Reset
**Files:** Modify `src/preview_controller.rs`, `tests/preview_play.rs`.
**Interfaces:** Produces — `preview_controller::Playhead { active: bool, phase: Option<SkillPhase>, elapsed: f32, total: f32 }` (`#[derive(Resource, Default)]`); `sync_playhead` (reads `PreviewCaster`'s `ActiveCast`) + `clear_playhead_on_reset` (on `GameResetEvent`). Consumes — `obelisk_bevy::prelude::{ActiveCast, SkillPhase}`.
- [ ] Failing test (append `preview_play.rs`): early run (12t) → `ph.active`, `phase==Some(Windup)`, `total>0`; reset (despawn `GameEntity`s + `write_message(GameResetEvent)` + update) → `!ph.active` and 0 `PreviewCaster`. (Use `arena_editor::preview_controller::Playhead`, `obelisk_bevy::prelude::SkillPhase`.)
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test preview_play playhead 2>&1 | tail -16`
- [ ] Append to `preview_controller.rs`:
```rust
use bevy_editor_game::GameResetEvent;
use obelisk_bevy::prelude::{ActiveCast, SkillPhase};
#[derive(Resource, Default)] pub struct Playhead { pub active: bool, pub phase: Option<SkillPhase>, pub elapsed: f32, pub total: f32 }
pub fn sync_playhead(mut ph: ResMut<Playhead>, q: Query<&ActiveCast, With<PreviewCaster>>) {
    if let Ok(ac) = q.single() { ph.active = true; ph.phase = Some(ac.phase); ph.elapsed = ac.elapsed; ph.total = ac.total_duration(); }
    else { ph.active = false; ph.phase = None; }
}
pub fn clear_playhead_on_reset(mut ph: ResMut<Playhead>, mut ev: MessageReader<GameResetEvent>) { if ev.read().next().is_some() { *ph = Playhead::default(); } }
```
- [ ] `PreviewControllerPlugin::build`: `.init_resource::<Playhead>()` + `.add_systems(Update, (start_preview, sync_playhead, clear_playhead_on_reset))`.
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor --test preview_play 2>&1 | tail -12`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): Playhead synced from ActiveCast.phase/elapsed; cleared on GameResetEvent"`

### Task 22: `gizmo_shape` + Skill-mode viewport gizmo for the selected hit-window
**Files:** Create `crates/arena_editor/src/gizmo.rs`; Modify `src/lib.rs`, `src/skill_designer.rs`.
**Interfaces:** Produces — `gizmo::{GizmoShape{Sphere{radius},Capsule{radius,height},Cone{half_angle_rad,range}}, gizmo_shape(&CollisionShape)->GizmoShape}` (cone degrees→half-radians); `draw_window_gizmo(Gizmos, Res<EditedSkill>, Query<&Transform, With<PreviewCaster>>, Res<State<EditorMode>>)` (Skill-mode gated).
- [ ] Failing test (`gizmo.rs` `#[cfg(test)]`, pure): `gizmo_shape(Cone{90,5})` → `half_angle_rad≈FRAC_PI_4`, range 5; Sphere/Capsule dims passthrough.
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --lib gizmo 2>&1 | tail -12`
- [ ] Step: read `skill_designer.rs` to confirm `SKILL_MODE_ID` + `EditorMode::Custom(CustomModeId(SKILL_MODE_ID))`; confirm `bevy_modal_editor` re-exports `EditorMode`+`CustomModeId`.
- [ ] Impl:
```rust
use bevy::prelude::*;
use bevy_modal_editor::{CustomModeId, EditorMode};
use obelisk_bevy::assets::CollisionShape;
use arena_sim::preview::PreviewCaster;
use crate::model::EditedSkill;
use crate::skill_designer::SKILL_MODE_ID;
#[derive(Debug, Clone, Copy)] pub enum GizmoShape { Sphere { radius: f32 }, Capsule { radius: f32, height: f32 }, Cone { half_angle_rad: f32, range: f32 } }
pub fn gizmo_shape(shape: &CollisionShape) -> GizmoShape {
    match *shape { CollisionShape::Sphere { radius } => GizmoShape::Sphere { radius },
        CollisionShape::Capsule { radius, height } => GizmoShape::Capsule { radius, height },
        CollisionShape::Cone { angle, range } => GizmoShape::Cone { half_angle_rad: angle.to_radians() * 0.5, range } }
}
pub fn draw_window_gizmo(mut gizmos: Gizmos, edited: Res<EditedSkill>, caster: Query<&Transform, With<PreviewCaster>>, mode: Res<State<EditorMode>>) {
    if *mode.get() != EditorMode::Custom(CustomModeId(SKILL_MODE_ID)) { return; }
    let Some(idx) = edited.selected_window else { return };
    let Some(window) = edited.timeline.collision_windows.get(idx) else { return };
    let origin = caster.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let c = Color::srgb(1.0, 0.4, 0.1);
    match gizmo_shape(&window.shape) {
        GizmoShape::Sphere { radius } => { gizmos.sphere(Isometry3d::from_translation(origin), radius, c); }
        GizmoShape::Capsule { radius, height } => { let half = height * 0.5 + radius; gizmos.line(origin - Vec3::Y * half, origin + Vec3::Y * half, c); gizmos.sphere(Isometry3d::from_translation(origin), radius, c); }
        GizmoShape::Cone { half_angle_rad, range } => { let e1 = Quat::from_rotation_y(half_angle_rad) * (Vec3::Z * range); let e2 = Quat::from_rotation_y(-half_angle_rad) * (Vec3::Z * range); gizmos.line(origin, origin + e1, c); gizmos.line(origin, origin + e2, c); }
    }
}
```
(confirm `Gizmos::sphere`/`line` + `Isometry3d` against the editor's Bevy-0.18 usage if the build flags a mismatch; only `gizmo_shape` is unit-tested, the draw system is a `cargo build` gate.)
- [ ] `lib.rs`: `pub mod gizmo;`. `skill_designer.rs`: `app.add_systems(Update, crate::gizmo::draw_window_gizmo);`.
- [ ] Run PASS + build: `nix develop --command cargo test -p arena_editor --lib gizmo 2>&1 | tail -8 && nix develop --command cargo build -p arena_editor 2>&1 | tail -5`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): gizmo_shape + Skill-mode viewport gizmo for the selected hit-window"`

### Task 23: The bottom-dock phase-timeline egui panel; M2 gate
**Files:** Create `crates/arena_editor/src/panel.rs`; Modify `src/skill_designer.rs` (point Skill-mode panel at `panel::draw_skill_panel`), `src/lib.rs`, `Cargo.toml` (+`bevy_egui`), `tests/boots.rs`.
**Interfaces:** Produces — `panel::draw_skill_panel(contexts: EguiContexts, edited: ResMut<EditedSkill>, playhead: Res<Playhead>)` — bottom-dock `TopBottomPanel`: id + targeting/delivery ComboBoxes; windup/active/recovery `DragValue`s; painted phase-band + window-bar strip + live playhead (`time_to_x`); hit-windows list (shape/motion/filter/mode/phase/offset/duration + select); Add-Window; Save (`derive_vfx_cues`+`save_cast_timeline`); all edits set `dirty`. (Signature extends in Tasks 27/28 — final: `+ ResMut<EditedSkillFx>, Res<RigSockets>`.) Dispatched by Task 10 `dispatch_custom_panel`.
- [ ] Step 1: read `skill_designer.rs` (`register_skill_mode`/stub) + `bevy_modal_editor/Cargo.toml` for the exact `bevy_egui` git/branch spec to copy verbatim; confirm `EguiContexts::ctx_mut()` returns `Result` (panels.rs:45 uses `?`).
- [ ] Failing test (append `boots.rs`):
```rust
#[test] fn skill_mode_panel_and_playhead_wired() {
    use bevy_modal_editor::{CustomModeId, CustomModeRegistry};
    let mut app = arena_editor::build_editor_app(); app.add_plugins(arena_editor::SkillDesignerPlugin);
    let reg = app.world().resource::<CustomModeRegistry>();
    assert!(reg.lookup(CustomModeId(arena_editor::SKILL_MODE_ID)).is_some());
    assert!(app.world().get_resource::<arena_editor::model::EditedSkill>().is_some());
    assert!(app.world().get_resource::<arena_editor::preview_controller::Playhead>().is_some());
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test boots skill_mode_panel_and_playhead 2>&1 | tail -14`
- [ ] `Cargo.toml`: add `bevy_egui = { ... }` copying the EXACT spec (git+branch/rev) from `bevy_modal_editor/Cargo.toml`.
- [ ] Create `panel.rs` (egui shell over the pure helpers; compiles but runs only windowed — `cargo build` is the gate, the wiring test asserts resources/registration):
```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use obelisk_bevy::assets::{HitFilter, HitMode, WindowPhase};
use crate::edits::add_collision_window;
use crate::enum_ui::{delivery_index, delivery_variant, motion_index, motion_variant, shape_index, shape_variant, targeting_index, targeting_variant, DELIVERY_LABELS, MOTION_LABELS, SHAPE_LABELS, TARGETING_LABELS};
use crate::io::save_cast_timeline;
use crate::model::{derive_vfx_cues, EditedSkill};
use crate::preview_controller::Playhead;
use crate::timeline_geom::{phase_spans, time_to_x, total_duration, window_span};
const STRIP_H: f32 = 40.0;
const PHASE_COLORS: [egui::Color32; 3] = [egui::Color32::from_rgb(60,80,130), egui::Color32::from_rgb(130,70,60), egui::Color32::from_rgb(60,110,80)];
pub fn draw_skill_panel(mut contexts: EguiContexts, mut edited: ResMut<EditedSkill>, playhead: Res<Playhead>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let mut changed = false; let mut save_clicked = false;
    egui::TopBottomPanel::bottom("skill_timeline").resizable(true).min_height(180.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&edited.timeline.skill_id).strong());
            let mut ti = targeting_index(&edited.timeline.targeting);
            egui::ComboBox::from_id_salt("targeting").selected_text(TARGETING_LABELS[ti]).show_ui(ui, |ui| {
                for (i, l) in TARGETING_LABELS.iter().enumerate() { if ui.selectable_value(&mut ti, i, *l).clicked() { edited.timeline.targeting = targeting_variant(i); changed = true; } }
            });
            let mut di = delivery_index(&edited.timeline.delivery);
            egui::ComboBox::from_id_salt("delivery").selected_text(DELIVERY_LABELS[di]).show_ui(ui, |ui| {
                for (i, l) in DELIVERY_LABELS.iter().enumerate() { if ui.selectable_value(&mut di, i, *l).clicked() { edited.timeline.delivery = delivery_variant(i); changed = true; } }
            });
            if ui.button("Save").clicked() { save_clicked = true; }
        });
        ui.horizontal(|ui| {
            let d = &mut edited.timeline.phase_durations;
            for (lab, val) in [("windup", &mut d.windup), ("active", &mut d.active), ("recovery", &mut d.recovery)] {
                ui.label(lab);
                if ui.add(egui::DragValue::new(val).speed(0.01).range(0.0..=10.0).suffix(" s")).changed() { changed = true; }
            }
        });
        let span = total_duration(&edited.timeline.phase_durations).max(0.0001);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), STRIP_H), egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, egui::Color32::from_rgb(24,24,28));
        for (i, (s, e)) in phase_spans(&edited.timeline.phase_durations).iter().enumerate() {
            let x0 = time_to_x(*s, span, rect.left(), rect.width()); let x1 = time_to_x(*e, span, rect.left(), rect.width());
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.center().y)), 0.0, PHASE_COLORS[i]);
        }
        for w in &edited.timeline.collision_windows {
            let (ws, we) = window_span(w, &edited.timeline.phase_durations);
            let x0 = time_to_x(ws, span, rect.left(), rect.width()); let x1 = time_to_x(we, span, rect.left(), rect.width());
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.center().y + 2.0), egui::pos2(x1.max(x0 + 2.0), rect.bottom())), 2.0, egui::Color32::from_rgb(220,180,60));
        }
        if playhead.active && playhead.total > 0.0 {
            let x = time_to_x(playhead.elapsed, span, rect.left(), rect.width());
            p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(2.0, egui::Color32::from_rgb(230,70,70)));
        }
        ui.horizontal(|ui| { ui.label("Hit Windows"); if ui.button("+ Add").clicked() { add_collision_window(&mut edited.timeline); changed = true; } });
        let len = edited.timeline.collision_windows.len();
        for idx in 0..len {
            let selected = edited.selected_window == Some(idx);
            ui.push_id(idx, |ui| { ui.horizontal(|ui| {
                if ui.selectable_label(selected, &edited.timeline.collision_windows[idx].id).clicked() { edited.selected_window = Some(idx); }
                let mut si = shape_index(&edited.timeline.collision_windows[idx].shape);
                egui::ComboBox::from_id_salt("shape").selected_text(SHAPE_LABELS[si]).show_ui(ui, |ui| { for (i, l) in SHAPE_LABELS.iter().enumerate() { if ui.selectable_value(&mut si, i, *l).clicked() { edited.timeline.collision_windows[idx].shape = shape_variant(i); changed = true; } } });
                let mut mi = motion_index(&edited.timeline.collision_windows[idx].motion);
                egui::ComboBox::from_id_salt("motion").selected_text(MOTION_LABELS[mi]).show_ui(ui, |ui| { for (i, l) in MOTION_LABELS.iter().enumerate() { if ui.selectable_value(&mut mi, i, *l).clicked() { edited.timeline.collision_windows[idx].motion = motion_variant(i); changed = true; } } });
                let f = &mut edited.timeline.collision_windows[idx].hit_filter;
                egui::ComboBox::from_id_salt("filter").selected_text(format!("{f:?}")).show_ui(ui, |ui| { for o in [HitFilter::Caster, HitFilter::Allies, HitFilter::Enemies, HitFilter::All] { if ui.selectable_value(f, o, format!("{o:?}")).clicked() { changed = true; } } });
                let m = &mut edited.timeline.collision_windows[idx].hit_mode;
                egui::ComboBox::from_id_salt("mode").selected_text(format!("{m:?}")).show_ui(ui, |ui| { for o in [HitMode::OncePerTarget, HitMode::FirstOnly, HitMode::EveryTick] { if ui.selectable_value(m, o, format!("{o:?}")).clicked() { changed = true; } } });
                let ph = &mut edited.timeline.collision_windows[idx].spawn_phase;
                egui::ComboBox::from_id_salt("phase").selected_text(format!("{ph:?}")).show_ui(ui, |ui| { for o in [WindowPhase::Windup, WindowPhase::Active, WindowPhase::Recovery] { if ui.selectable_value(ph, o, format!("{o:?}")).clicked() { changed = true; } } });
                let w = &mut edited.timeline.collision_windows[idx];
                if ui.add(egui::DragValue::new(&mut w.spawn_offset).speed(0.01).range(0.0..=10.0).prefix("off ")).changed() { changed = true; }
                if ui.add(egui::DragValue::new(&mut w.active_duration).speed(0.01).range(0.0..=10.0).prefix("dur ")).changed() { changed = true; }
            }); });
        }
    });
    if changed { edited.dirty = true; }
    if save_clicked { edited.timeline.vfx_cues = derive_vfx_cues(&edited.timeline); let path = edited.path.clone(); if save_cast_timeline(&edited.timeline, &path).is_ok() { edited.dirty = false; } }
}
```
- [ ] `skill_designer.rs`: in `register_skill_mode` replace the stub with `let panel = app.register_system(crate::panel::draw_skill_panel);` and delete `fn draw_skill_panel(){}`. `lib.rs`: `pub mod panel;`.
- [ ] Run wiring test + build: `nix develop --command cargo test -p arena_editor --test boots skill_mode_panel_and_playhead 2>&1 | tail -8 && nix develop --command cargo build -p arena_editor 2>&1 | tail -5`
- [ ] Suite + clippy: `nix develop --command cargo test -p arena_editor 2>&1 | tail -12 && nix develop --command cargo clippy -p arena_editor --tests -- -D warnings 2>&1 | tail -5`
- [ ] M2 sanity gate: `cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -3 && bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): bottom-dock phase-timeline panel (bands+dropdowns+windows lane+playhead+Save); M2 complete"`

---

# Milestone M3 — Cosmetic lanes (animation / particle / projectile + bone sockets + VfxParam stat-binding)
Ships: full WYSIWYG — cosmetics bind to the timeline's cues + render in preview. **Locked: struct-extension, NOT enum-refactor** — `arena_skills::LaneEvent` (lib.rs:70-85) is a STRUCT with three independent `Option` fields (firebolt_cast carries all three at once; `arena_game/cosmetics.rs:153,180` + the shipped asset depend on it); an enum refactor breaks the asset + consumer + net-test. M3 extends the three nested specs under `#[serde(default)]`. arena_skills tests use plain `cargo`; arena_editor inside `nix develop`.

### Task 24: Extend `ParticleSpec`/`ProjectileCosmetic`/`AnimLayer` + `VfxParamBinding`/`VfxBindSource`; `Serialize SkillFx`; round-trip + legacy-parse + registry tests
**Files:** Modify `/Users/luke/src/obelisk-arena/crates/arena_skills/src/lib.rs` (L17 SkillFx derive; ParticleSpec 97-107; ProjectileCosmetic 115-124; AnimLayer 129-133; append types after 133); Create `crates/arena_skills/tests/skillfx_roundtrip.rs`.
**Interfaces:** Produces — `SkillFx` += `Serialize`. `ParticleSpec` += `effect: Option<String>, socket: Option<String>, offset: Vec3, param_bindings: Vec<VfxParamBinding>` (all `#[serde(default)]`). `ProjectileCosmetic` += `effect`/`socket`. `AnimLayer` += `clip: Option<String>, layer: u32, weight: f32(=1.0)`; `state` now `#[serde(default)]`. `VfxParamBinding { param: String, source: VfxBindSource, min: f32, max: f32 }`, `VfxBindSource { Charge, Stat { stat, stat_min, stat_max } }` (`Serialize,Deserialize,PartialEq`).
- [ ] Failing test `tests/skillfx_roundtrip.rs`:
```rust
use arena_skills::{AnimLayer, LaneEvent, ParticleSpec, ProjectileCosmetic, SkillFx, SkillFxRegistry, VfxBindSource, VfxParamBinding};
use bevy::math::Vec3; use std::collections::HashMap;
fn extended_lane() -> LaneEvent {
    LaneEvent { lane_id: "x_muzzle".into(), kind: arena_skills::CueKind::OnCast,
        particle: Some(ParticleSpec { count:12, lifetime:0.4, color:[1.0,0.5,0.1], speed:4.0, effect:Some("fire_burst".into()), socket:Some("wand_tip".into()), offset:Vec3::new(0.0,0.1,0.2), param_bindings:vec![VfxParamBinding{param:"scale".into(), source:VfxBindSource::Charge, min:0.2, max:1.0}] }),
        projectile: Some(ProjectileCosmetic { speed:20.0, color:[1.0,0.4,0.05], radius:0.2, effect:Some("fire_trail".into()), socket:Some("wand_tip".into()) }),
        anim: Some(AnimLayer { state:String::new(), clip:Some("casting_idle".into()), layer:1, weight:0.8 }) }
}
#[test] fn extended_lane_round_trips_through_ron() {
    let fx = SkillFx { skill_id:"x".into(), lanes: HashMap::from([("x_cast".to_string(), extended_lane())]) };
    let s = ron::ser::to_string(&fx).expect("ser"); let back: SkillFx = ron::de::from_str(&s).expect("de");
    let l = back.lanes.get("x_cast").unwrap(); let p = l.particle.as_ref().unwrap();
    assert_eq!(p.effect.as_deref(), Some("fire_burst")); assert_eq!(p.socket.as_deref(), Some("wand_tip"));
    assert_eq!(p.offset, Vec3::new(0.0,0.1,0.2)); assert_eq!(p.param_bindings.len(), 1); assert_eq!(p.param_bindings[0].source, VfxBindSource::Charge);
    assert_eq!(l.projectile.as_ref().unwrap().effect.as_deref(), Some("fire_trail"));
    let a = l.anim.as_ref().unwrap(); assert_eq!(a.clip.as_deref(), Some("casting_idle")); assert_eq!(a.layer, 1); assert!((a.weight-0.8).abs()<1e-6);
}
#[test] fn existing_firebolt_asset_still_parses_with_defaults() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf();
    let s = std::fs::read_to_string(root.join("assets/skills/firebolt.skillfx.ron")).expect("read");
    let fx: SkillFx = ron::de::from_str(&s).expect("legacy parses"); let p = fx.lanes.get("firebolt_cast").unwrap().particle.as_ref().unwrap();
    assert_eq!(p.count, 12); assert!(p.effect.is_none()); assert!(p.param_bindings.is_empty()); assert_eq!(p.offset, Vec3::ZERO);
}
#[test] fn registry_resolves_lanes_with_new_fields() {
    let dir = std::env::temp_dir().join("arena_m3_1_skillfx"); std::fs::create_dir_all(&dir).unwrap();
    let fx = SkillFx { skill_id:"x".into(), lanes: HashMap::from([("x_cast".to_string(), extended_lane())]) };
    std::fs::write(dir.join("x.skillfx.ron"), ron::ser::to_string(&fx).unwrap()).unwrap();
    let reg = SkillFxRegistry::load_dir(&dir); let lanes = reg.lanes("x_cast").expect("bound");
    assert_eq!(lanes.len(), 1); assert_eq!(lanes[0].particle.as_ref().unwrap().effect.as_deref(), Some("fire_burst"));
}
```
- [ ] Run FAIL: `cd /Users/luke/src/obelisk-arena && cargo test -p arena_skills --test skillfx_roundtrip 2>&1 | tail -20`
- [ ] L17: `#[derive(Asset, TypePath, Debug, Clone, Serialize, Deserialize)]` on `SkillFx`.
- [ ] Replace `ParticleSpec` (97-107) to add fields: `effect: Option<String>` (`#[serde(default)]`), `socket: Option<String>` (`#[serde(default)]`), `offset: Vec3` (`#[serde(default)]`), `param_bindings: Vec<VfxParamBinding>` (`#[serde(default)]`) — keep `count`, `lifetime` (`default_lifetime`), `color` (`#[serde(default)]`), `speed` (`default_speed`).
- [ ] Replace `ProjectileCosmetic` (115-124) to add `effect: Option<String>`, `socket: Option<String>` (both `#[serde(default)]`) — keep `speed`, `color`, `radius` (`default_proj_radius`).
- [ ] Replace `AnimLayer` (129-133): `state` → `#[serde(default)]`; add `clip: Option<String>` (`#[serde(default)]`), `layer: u32` (`#[serde(default)]`), `weight: f32` (`#[serde(default = "default_weight")]`); add `fn default_weight() -> f32 { 1.0 }`.
- [ ] Append after AnimLayer:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VfxParamBinding { pub param: String, pub source: VfxBindSource, pub min: f32, pub max: f32 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VfxBindSource { Charge, Stat { stat: String, stat_min: f32, stat_max: f32 } }
```
- [ ] Run round-trip PASS: `cargo test -p arena_skills --test skillfx_roundtrip 2>&1 | tail -14`
- [ ] Game compiles + net-test green (additive serde-defaulted extension): `cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -5 && bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -10`
- [ ] Commit: `git checkout -b m3-cosmetic-lanes && git add -A && git commit -m "feat(arena_skills): extend cosmetic specs + VfxParamBinding/VfxBindSource; Serialize SkillFx; legacy asset parses; net-test green"`

### Task 25: Pure `VfxParam` modulation — `normalize`/`modulate`/`resolve_binding`
**Files:** Modify `crates/arena_skills/src/lib.rs` (append after the binding types).
**Interfaces:** Produces — `normalize(raw,lo,hi)->f32` (clamped 0..1, 0 when `hi<=lo`); `modulate(out_min,out_max,t)->f32`; `resolve_binding(&VfxParamBinding, source_value: f32)->f32` (Charge ⇒ `t=clamp01(value)`; Stat ⇒ `t=normalize(value, stat_min, stat_max)`; returns `modulate(min,max,t)`).
- [ ] Failing test (`lib.rs` `#[cfg(test)] mod vfxparam_tests`): `normalize(50,0,100)≈0.5`, `-10→0`, `999→1`, `(5,3,3)→0`; `modulate(1,3,0.5)≈2`, clamps; `resolve_binding(Charge{0.2,1.0},0.5)≈0.6`, `0.0→0.2`; `resolve_binding(Stat{0,100;min0,max10},50)≈5`, `200→10`.
- [ ] Run FAIL: `cargo test -p arena_skills --lib vfxparam_tests 2>&1 | tail -16`
- [ ] Append:
```rust
pub fn normalize(raw: f32, lo: f32, hi: f32) -> f32 { if hi <= lo { 0.0 } else { ((raw - lo) / (hi - lo)).clamp(0.0, 1.0) } }
pub fn modulate(out_min: f32, out_max: f32, t: f32) -> f32 { out_min + (out_max - out_min) * t.clamp(0.0, 1.0) }
pub fn resolve_binding(b: &VfxParamBinding, source_value: f32) -> f32 {
    let t = match &b.source { VfxBindSource::Charge => source_value.clamp(0.0, 1.0), VfxBindSource::Stat { stat_min, stat_max, .. } => normalize(source_value, *stat_min, *stat_max) };
    modulate(b.min, b.max, t)
}
```
- [ ] Run PASS: `cargo test -p arena_skills --lib vfxparam_tests 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_skills): pure VfxParam normalize/modulate/resolve_binding + unit tests"`

### Task 26: `EditedSkillFx` + `blank_skillfx` + `save_skillfx`/`load_skillfx`; load-or-blank; Save writes both files
**Files:** Modify `crates/arena_editor/src/io.rs`, `src/model.rs`, `src/lib.rs`, `src/skill_designer.rs`, `src/panel.rs`; Create `tests/skillfx_io.rs`.
**Interfaces:** Produces — `model::EditedSkillFx { fx: SkillFx, path: PathBuf, dirty: bool }` (`#[derive(Resource)]`) + `from_fx`; `blank_skillfx(impl Into<String>)->SkillFx`; `io::{default_skillfx_path(&str)->PathBuf, save_skillfx(&SkillFx,&Path)->std::io::Result<()>, load_skillfx(&Path)->Result<SkillFx,String>}`.
- [ ] Failing test `tests/skillfx_io.rs`: author a "zap" SkillFx with a particle (effect "spark", socket "wand_tip", offset Y*0.1) + anim (clip "casting_idle"), `save_skillfx`→`load_skillfx` round-trips; `load_skillfx(default_skillfx_path("firebolt"))` → skill_id "firebolt" + lane "firebolt_cast"; `SkillDesignerPlugin` inits `EditedSkillFx` resource.
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test skillfx_io 2>&1 | tail -14`
- [ ] Append to `io.rs`:
```rust
use arena_skills::SkillFx;
pub fn default_skillfx_path(skill_id: &str) -> PathBuf { editor_root().join(format!("assets/skills/{skill_id}.skillfx.ron")) }
pub fn save_skillfx(fx: &SkillFx, path: &Path) -> std::io::Result<()> {
    let s = ron::ser::to_string_pretty(fx, ron::ser::PrettyConfig::new()).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; } std::fs::write(path, s)
}
pub fn load_skillfx(path: &Path) -> Result<SkillFx, String> { let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?; ron::de::from_str::<SkillFx>(&s).map_err(|e| e.to_string()) }
```
- [ ] Append to `model.rs`:
```rust
use arena_skills::SkillFx;
#[derive(Resource)] pub struct EditedSkillFx { pub fx: SkillFx, pub path: PathBuf, pub dirty: bool }
impl EditedSkillFx { pub fn from_fx(fx: SkillFx, path: PathBuf) -> Self { Self { fx, path, dirty: false } } }
pub fn blank_skillfx(skill_id: impl Into<String>) -> SkillFx { SkillFx { skill_id: skill_id.into(), lanes: std::collections::HashMap::new() } }
```
- [ ] `lib.rs`: extend re-export with `EditedSkillFx, blank_skillfx`. `skill_designer.rs` `build`: after `EditedSkill`, `let fx_path = crate::io::default_skillfx_path("firebolt"); let fx = crate::io::load_skillfx(&fx_path).unwrap_or_else(|_| blank_skillfx("firebolt")); app.insert_resource(EditedSkillFx::from_fx(fx, fx_path));`. `panel.rs`: add `mut edited_fx: ResMut<EditedSkillFx>` param + in the `save_clicked` block also `if save_skillfx(&edited_fx.fx, &edited_fx.path).is_ok() { edited_fx.dirty = false; }` (add `use crate::io::save_skillfx; use crate::model::EditedSkillFx;`).
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor --test skillfx_io 2>&1 | tail -10`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): EditedSkillFx + save/load .skillfx.ron; load-or-blank; Save writes both files"`

### Task 27: Pure cosmetic-lane edit helpers + the lane-editor rows in the panel
**Files:** Create `crates/arena_editor/src/fx_edits.rs`; Modify `src/lib.rs`, `src/panel.rs`.
**Interfaces:** Produces — `fx_edits::{ensure_lane(&mut SkillFx,&str,CueKind)->&mut LaneEvent, set_anim_clip(&mut LaneEvent,Option<String>,u32,f32), set_particle_effect(&mut LaneEvent,Option<String>), set_particle_socket(&mut LaneEvent,Option<String>), add_param_binding(&mut LaneEvent,VfxParamBinding), cue_keys_for(&CastTimeline)->Vec<String>}` (the sorted `derive_vfx_cues` VALUES).
- [ ] Failing test (`fx_edits.rs` `#[cfg(test)]`): `ensure_lane` idempotent (len stays 1); `set_anim_clip`/`set_particle_effect`/`set_particle_socket`/`add_param_binding` mutate as expected; `cue_keys_for(firebolt + add_collision_window)` contains `firebolt_cast`, `firebolt_window_window_0`, `firebolt_impact`.
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --lib fx_edits 2>&1 | tail -14`
- [ ] Impl:
```rust
use arena_skills::{AnimLayer, CueKind, LaneEvent, ParticleSpec, SkillFx, VfxParamBinding};
use obelisk_bevy::assets::CastTimeline;
use crate::model::derive_vfx_cues;
pub fn ensure_lane<'a>(fx: &'a mut SkillFx, cue_id: &str, kind: CueKind) -> &'a mut LaneEvent {
    fx.lanes.entry(cue_id.to_string()).or_insert_with(|| LaneEvent { lane_id: cue_id.to_string(), kind, particle: None, projectile: None, anim: None })
}
pub fn set_anim_clip(lane: &mut LaneEvent, clip: Option<String>, layer: u32, weight: f32) {
    match clip { None => lane.anim = None, Some(c) => lane.anim = Some(AnimLayer { state: String::new(), clip: Some(c), layer, weight }) }
}
fn ensure_particle(lane: &mut LaneEvent) -> &mut ParticleSpec {
    lane.particle.get_or_insert_with(|| ParticleSpec { count:12, lifetime:0.4, color:[1.0,0.5,0.1], speed:4.0, effect:None, socket:None, offset:bevy::math::Vec3::ZERO, param_bindings:Vec::new() })
}
pub fn set_particle_effect(lane: &mut LaneEvent, effect: Option<String>) { ensure_particle(lane).effect = effect; }
pub fn set_particle_socket(lane: &mut LaneEvent, socket: Option<String>) { ensure_particle(lane).socket = socket; }
pub fn add_param_binding(lane: &mut LaneEvent, b: VfxParamBinding) { ensure_particle(lane).param_bindings.push(b); }
pub fn cue_keys_for(tl: &CastTimeline) -> Vec<String> { let mut v: Vec<String> = derive_vfx_cues(tl).into_values().collect(); v.sort(); v }
```
- [ ] `lib.rs`: `pub mod fx_edits;`. Run helper PASS: `nix develop --command cargo test -p arena_editor --lib fx_edits 2>&1 | tail -8`
- [ ] Panel UI (compile-gated): in `panel.rs` after the hit-windows loop, add a "Cosmetic Lanes" section listing `cue_keys_for(&edited.timeline)`; per cue key a row with an anim-clip ComboBox (over `preview_rig::PREVIEW_CLIPS` + "(none)"), a particle-effect text field, a socket ComboBox (`Res<crate::socket::RigSockets>`, "(root)" default), and a "+ charge→scale" param-binding button — each calling `fx_edits::*` on `ensure_lane(&mut edited_fx.fx, &cue, kind_for(&cue))` and setting `edited_fx.dirty = true`. Derive `kind` from the suffix (`_cast`→OnCast, `_impact`→OnHit, else OnWindow). (egui shell — `cargo build` is the gate; the helpers above are the tested surface.)
- [ ] Build: `nix develop --command cargo build -p arena_editor 2>&1 | tail -5`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): pure fx_edits lane helpers + cosmetic-lane editor rows in the panel"`

### Task 28: `RigSockets` index + pure `resolve_socket` with root fallback
**Files:** Create `crates/arena_editor/src/socket.rs`; Modify `src/lib.rs`, `src/skill_designer.rs`.
**Interfaces:** Produces — `socket::RigSockets { names: Vec<String>, by_name: HashMap<String, Entity> }` (`#[derive(Resource, Default)]`); `index_rig_sockets(Query<(Entity,&Name),Added<Name>>, Query<&ChildOf>, Query<Entity,With<PreviewCaster>>, ResMut<RigSockets>)` (records `Name`d descendants of a `PreviewCaster`); `resolve_socket(&RigSockets, Option<&str>, root: Entity)->Entity`.
- [ ] Failing test (`socket.rs` `#[cfg(test)]`): `resolve_socket(Some("chest_joint"),root)==bone`, `Some("missing")==root`, `None==root`; `index_rig_sockets` records a `Name("chest_joint")` under a `PreviewCaster` but ignores an unrelated `Name("ui_label")`. (Use `Entity::from_raw_u32(n).unwrap()`.)
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --lib socket 2>&1 | tail -14`
- [ ] Impl:
```rust
use bevy::prelude::*; use std::collections::HashMap;
use arena_sim::preview::PreviewCaster;
#[derive(Resource, Default)] pub struct RigSockets { pub names: Vec<String>, pub by_name: HashMap<String, Entity> }
pub fn index_rig_sockets(q: Query<(Entity, &Name), Added<Name>>, parents: Query<&ChildOf>, caster: Query<Entity, With<PreviewCaster>>, mut sockets: ResMut<RigSockets>) {
    for (entity, name) in &q {
        let mut cur = entity; let mut under = caster.get(cur).is_ok();
        while let Ok(p) = parents.get(cur) { cur = p.0; if caster.get(cur).is_ok() { under = true; break; } }
        if !under { continue; }
        let n = name.as_str().to_string();
        if sockets.by_name.insert(n.clone(), entity).is_none() { sockets.names.push(n); }
    }
}
pub fn resolve_socket(s: &RigSockets, name: Option<&str>, root: Entity) -> Entity { name.and_then(|n| s.by_name.get(n)).copied().unwrap_or(root) }
```
- [ ] `lib.rs`: `pub mod socket;`. `skill_designer.rs` `build`: `app.init_resource::<crate::socket::RigSockets>().add_systems(Update, crate::socket::index_rig_sockets);`.
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor --lib socket 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): RigSockets index (scan Name under preview caster) + resolve_socket root fallback"`

### Task 29: Preview rig (`character.glb` via `register_gltf_library`) + `PreviewAnimGraph` + `clip_node_for` + anim driver
**Files:** Create `crates/arena_editor/src/preview_rig.rs`, `tests/preview_rig.rs`; Modify `src/lib.rs`, `src/skill_designer.rs`, `src/main.rs`.
**Interfaces:** Produces — `preview_rig::{PREVIEW_CLIPS:[&str;11], clip_node_for(&AnimationLibrary,&str)->Option<Handle<AnimationClip>>, PreviewAnimGraph{graph:Option<Handle<AnimationGraph>>, nodes:HashMap<String,AnimationNodeIndex>}, drive_anim_clip(&mut AnimationPlayer,AnimationNodeIndex,f32), PreviewRigPlugin}` + systems `build_preview_anim_graph`/`attach_preview_anim_graph`/`spawn_preview_rig`. Consumes — `bevy_editor_game::{AnimationLibrary, RegisterGltfLibraryExt, GameStartedEvent}`.
- [ ] Step 1: read `arena_game/src/client/scene.rs` (`load_rig`) for the exact `character.glb` asset path + `#Scene0` label, and `bevy_editor_game/src/asset_libraries/mod.rs:220-235` for the `"{gltf_name}::{anim_name}"` key shape; confirm `register_gltf_library` (lib.rs:44) + `AnimationLibrary.clips` (lib.rs:69-70).
- [ ] Failing test `tests/preview_rig.rs`:
```rust
use arena_editor::preview_rig::clip_node_for; use bevy::prelude::*; use bevy_editor_game::AnimationLibrary;
#[test] fn clip_node_for_matches_short_name_suffix() {
    let mut lib = AnimationLibrary::default();
    lib.clips.insert("character::casting_idle".into(), Handle::default());
    lib.clips.insert("character::idle".into(), Handle::default());
    assert!(clip_node_for(&lib, "casting_idle").is_some());
    assert!(clip_node_for(&lib, "idle").is_some());
    assert!(clip_node_for(&lib, "no_such_clip").is_none());
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test preview_rig 2>&1 | tail -12`
- [ ] Create `preview_rig.rs` (adapt `arena_game/src/client/rig.rs:90-189` sourcing clips from `AnimationLibrary`):
```rust
use bevy::prelude::*; use bevy_editor_game::{AnimationLibrary, GameStartedEvent}; use std::collections::HashMap;
use arena_sim::preview::PreviewCaster;
pub const PREVIEW_CLIPS: [&str; 11] = ["idle","walk_forward","walk_backward","walk_left","walk_right","falling","casting_idle","casting_walk_forward","casting_walk_backward","casting_walk_left","casting_walk_right"];
pub fn clip_node_for(lib: &AnimationLibrary, short_name: &str) -> Option<Handle<AnimationClip>> {
    let suffix = format!("::{short_name}");
    lib.clips.iter().find(|(k, _)| k.ends_with(&suffix) || k.as_str() == short_name).map(|(_, h)| h.clone())
}
#[derive(Resource, Default)] pub struct PreviewAnimGraph { pub graph: Option<Handle<AnimationGraph>>, pub nodes: HashMap<String, AnimationNodeIndex> }
impl PreviewAnimGraph { pub fn ready(&self) -> bool { self.graph.is_some() } }
pub fn build_preview_anim_graph(lib: Res<AnimationLibrary>, mut graphs: ResMut<Assets<AnimationGraph>>, mut state: ResMut<PreviewAnimGraph>) {
    if state.ready() || lib.clips.is_empty() { return; }
    let mut graph = AnimationGraph::new(); let root = graph.root;
    for name in PREVIEW_CLIPS { if let Some(clip) = clip_node_for(&lib, name) { let node = graph.add_clip(clip, 1.0, root); state.nodes.insert(name.to_string(), node); } }
    if state.nodes.is_empty() { return; } state.graph = Some(graphs.add(graph));
}
pub fn attach_preview_anim_graph(mut commands: Commands, state: Res<PreviewAnimGraph>, pending: Query<Entity, (With<AnimationPlayer>, Without<AnimationGraphHandle>)>, mut players: Query<&mut AnimationPlayer>) {
    let Some(graph) = state.graph.clone() else { return };
    for entity in &pending { let Ok(mut player) = players.get_mut(entity) else { continue }; for node in state.nodes.values() { player.play(*node).repeat().set_weight(0.0); } commands.entity(entity).insert(AnimationGraphHandle(graph.clone())); }
}
pub fn drive_anim_clip(player: &mut AnimationPlayer, node: AnimationNodeIndex, weight: f32) { player.play(node).repeat().set_weight(weight); }
pub fn spawn_preview_rig(mut started: MessageReader<GameStartedEvent>, caster: Query<Entity, With<PreviewCaster>>, asset_server: Res<AssetServer>, mut commands: Commands) {
    if started.read().next().is_none() { return; }
    let Ok(caster) = caster.single() else { return };
    let scene: Handle<Scene> = asset_server.load("character.glb#Scene0");
    commands.entity(caster).with_children(|c| { c.spawn((SceneRoot(scene), Transform::default())); });
}
pub struct PreviewRigPlugin;
impl Plugin for PreviewRigPlugin {
    fn build(&self, app: &mut App) { app.init_resource::<PreviewAnimGraph>().add_systems(Update, (spawn_preview_rig, build_preview_anim_graph, attach_preview_anim_graph)); }
}
```
(adjust `"character.glb#Scene0"` + `SceneRoot`/`with_children` to what Step 1 confirms.)
- [ ] `lib.rs`: `pub mod preview_rig;`. `skill_designer.rs`: `app.add_plugins(crate::preview_rig::PreviewRigPlugin);`. `main.rs`: `.register_gltf_library("character.glb")`.
- [ ] Run PASS + build: `nix develop --command cargo test -p arena_editor --test preview_rig 2>&1 | tail -8 && nix develop --command cargo build -p arena_editor 2>&1 | tail -5`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): preview character.glb rig + PreviewAnimGraph + clip_node_for/drive_anim_clip"`

### Task 30: `apply_modulated_param`/`bake_bindings` — CPU-bake a binding value into an `EmitterDef`
**Files:** Create `crates/arena_editor/src/vfx_bind.rs`; Modify `src/lib.rs`, `Cargo.toml` (+`bevy_vfx`), workspace `Cargo.toml`.
**Interfaces:** Produces — `vfx_bind::{apply_modulated_param(&mut VfxSystem,&str,f32), bake_bindings(&mut VfxSystem,&[VfxParamBinding], impl Fn(&VfxParamBinding)->f32)}` — `"scale"`→`InitModule::SetSize(ScalarRange::Constant)`, `"emission"`→`SpawnModule::Rate`, `"color"`→scale `SetColor(ColorSource::Constant)` RGB. CPU-bake-before-insert (extract clones `EmitterDef`; prepare re-uploads on `PartialEq` change).
- [ ] Failing test (`vfx_bind.rs` `#[cfg(test)]`): `apply_modulated_param(default,"scale",0.7)` inserts `SetSize(Constant(0.7))`; `"emission",120.0`→`SpawnModule::Rate(120)`; `bake_bindings([Charge{0.2,1.0}], |_|0.5)`→`SetSize(Constant(0.6))`.
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --lib vfx_bind 2>&1 | tail -12`
- [ ] Workspace `Cargo.toml` `[workspace.dependencies]`: `bevy_vfx = { path = "../bevy_modal_editor/crates/bevy_vfx" }`. arena_editor `Cargo.toml` `[dependencies]`: `bevy_vfx = { workspace = true }`.
- [ ] Impl:
```rust
use arena_skills::{resolve_binding, VfxParamBinding};
use bevy::color::LinearRgba;
use bevy_vfx::data::{ColorSource, EmitterDef, InitModule, ScalarRange, SpawnModule, VfxSystem};
fn set_size(em: &mut EmitterDef, v: f32) { for m in em.init.iter_mut() { if let InitModule::SetSize(r) = m { *r = ScalarRange::Constant(v); return; } } em.init.push(InitModule::SetSize(ScalarRange::Constant(v))); }
fn scale_color(em: &mut EmitterDef, mult: f32) { for m in em.init.iter_mut() { if let InitModule::SetColor(ColorSource::Constant(c)) = m { *c = LinearRgba::rgb(c.red*mult, c.green*mult, c.blue*mult); return; } } em.init.push(InitModule::SetColor(ColorSource::Constant(LinearRgba::rgb(mult, mult, mult)))); }
pub fn apply_modulated_param(system: &mut VfxSystem, param: &str, value: f32) {
    let Some(em) = system.emitters.first_mut() else { return };
    match param { "scale" => set_size(em, value), "emission" => em.spawn = SpawnModule::Rate(value), "color" => scale_color(em, value), _ => {} }
}
pub fn bake_bindings(system: &mut VfxSystem, bindings: &[VfxParamBinding], source_for: impl Fn(&VfxParamBinding) -> f32) {
    for b in bindings { let v = resolve_binding(b, source_for(b)); apply_modulated_param(system, &b.param, v); }
}
```
- [ ] `lib.rs`: `pub mod vfx_bind;`. Run PASS: `nix develop --command cargo test -p arena_editor --lib vfx_bind 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): apply_modulated_param/bake_bindings CPU-bake VfxParam into EmitterDef"`

### Task 31: Preview cosmetics observer — `On<CueEvent>` → drive anim clip + spawn `bevy_vfx` at socket with baked params
**Files:** Create `crates/arena_editor/src/preview_cosmetics.rs`, `tests/preview_cosmetics.rs`; Modify `src/lib.rs`, `src/skill_designer.rs`.
**Interfaces:** Produces — `preview_cosmetics::{PreviewCharge(pub f32) (Resource, default 1.0), PreviewCosmetic (marker, GameEntity-tagged), on_preview_cue(...)}` — per `LaneEvent` bound to `CueEvent.cue_id` in `EditedSkillFx`: drive `lane.anim.clip` weight on the caster's `AnimationPlayer`; spawn `lane.particle` (clone the named `VfxLibrary` effect or a tagged placeholder, `bake_bindings` from `PreviewCharge`/stat, child of `resolve_socket(...)` at `offset`); same for `lane.projectile`. Consumes — `obelisk_bevy::events::CueEvent`, `bevy_vfx::data::VfxLibrary`.
- [ ] Step 1: read `obelisk-bevy/src/events.rs:140-160` to confirm `CueEvent{cue_id,source,position,kind}` is an observer `#[derive(Event)]` + the obelisk-arena re-export; decide `app.add_observer(on_preview_cue)` vs a message reader. Confirm the caster→`StatBlock` stat read for `VfxBindSource::Stat`; if non-trivial, route `Stat` through `bake_bindings` with a `0.0` fallback (math already tested in Task 25), keep `Charge` (PreviewCharge) fully wired.
- [ ] Failing test `tests/preview_cosmetics.rs`: spawn a `PreviewCaster`, init `RigSockets`/`PreviewAnimGraph`/`VfxLibrary`/`PreviewCharge`, insert an `EditedSkillFx` with an anim+particle lane on `"firebolt_cast"`, fire `CueEvent{cue_id:"firebolt_cast",source:caster,position,kind:OnCast}` (via `World::trigger` if observer), update, assert exactly one `PreviewCosmetic` child spawned. (Adjust the `CueEvent` constructor + observer registration to Step 1.)
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test preview_cosmetics 2>&1 | tail -16`
- [ ] Create `preview_cosmetics.rs`:
```rust
use bevy::prelude::*;
use bevy_editor_game::GameEntity;
use bevy_vfx::data::VfxLibrary;
use obelisk_bevy::events::CueEvent;
use arena_sim::preview::PreviewCaster;
use arena_skills::VfxBindSource;
use crate::model::EditedSkillFx;
use crate::preview_rig::{drive_anim_clip, PreviewAnimGraph};
use crate::socket::{resolve_socket, RigSockets};
use crate::vfx_bind::bake_bindings;
#[derive(Resource)] pub struct PreviewCharge(pub f32);
impl Default for PreviewCharge { fn default() -> Self { Self(1.0) } }
#[derive(Component)] pub struct PreviewCosmetic;
pub fn on_preview_cue(cue: On<CueEvent>, edited: Res<EditedSkillFx>, sockets: Res<RigSockets>, graph: Res<PreviewAnimGraph>, library: Res<VfxLibrary>, charge: Res<PreviewCharge>, caster_q: Query<Entity, With<PreviewCaster>>, mut players: Query<&mut AnimationPlayer>, children: Query<&Children>, mut commands: Commands) {
    let ev = cue.event();
    let Some(lanes) = edited.fx.lanes.get(&ev.cue_id).map(std::slice::from_ref) else { return };
    let caster = caster_q.single().unwrap_or(ev.source);
    for lane in lanes {
        if let Some(anim) = &lane.anim { if let Some(clip) = &anim.clip { if let Some(node) = graph.nodes.get(clip) {
            if let Some(pe) = find_anim_player(caster, &children, &players) { if let Ok(mut player) = players.get_mut(pe) { drive_anim_clip(&mut player, *node, anim.weight); } } } } }
        if let Some(p) = &lane.particle { let socket = resolve_socket(&sockets, p.socket.as_deref(), caster); spawn_effect(&mut commands, &library, p.effect.as_deref(), socket, p.offset, &p.param_bindings, charge.0); }
        if let Some(pr) = &lane.projectile { let socket = resolve_socket(&sockets, pr.socket.as_deref(), caster); spawn_effect(&mut commands, &library, pr.effect.as_deref(), socket, Vec3::ZERO, &[], charge.0); }
    }
}
fn find_anim_player(root: Entity, children: &Query<&Children>, players: &Query<&mut AnimationPlayer>) -> Option<Entity> {
    let mut stack = vec![root];
    while let Some(e) = stack.pop() { if players.contains(e) { return Some(e); } if let Ok(cs) = children.get(e) { stack.extend(cs.iter()); } }
    None
}
#[allow(clippy::too_many_arguments)]
fn spawn_effect(commands: &mut Commands, library: &VfxLibrary, effect: Option<&str>, socket: Entity, offset: Vec3, bindings: &[arena_skills::VfxParamBinding], charge: f32) {
    let child = if let Some(mut system) = effect.and_then(|n| library.effects.get(n).cloned()) {
        bake_bindings(&mut system, bindings, |b| match &b.source { VfxBindSource::Charge => charge, VfxBindSource::Stat { .. } => 0.0 });
        commands.spawn((system, Transform::from_translation(offset), PreviewCosmetic, GameEntity)).id()
    } else {
        commands.spawn((Transform::from_translation(offset), Visibility::default(), PreviewCosmetic, GameEntity)).id()
    };
    commands.entity(child).insert(ChildOf(socket));
}
```
(if Step 1 finds `on_preview_cue` must be a plain system/message reader, swap `cue: On<CueEvent>` accordingly; keep the per-lane body identical.)
- [ ] `lib.rs`: `pub mod preview_cosmetics; pub use preview_cosmetics::{PreviewCharge, PreviewCosmetic};`. `skill_designer.rs`: `app.init_resource::<crate::preview_cosmetics::PreviewCharge>().add_observer(crate::preview_cosmetics::on_preview_cue);` (or `.add_systems` per Step 1).
- [ ] Run PASS: `nix develop --command cargo test -p arena_editor --test preview_cosmetics 2>&1 | tail -12`
- [ ] Suite + clippy: `nix develop --command cargo test -p arena_editor 2>&1 | tail -12 && nix develop --command cargo clippy -p arena_editor --tests -- -D warnings 2>&1 | tail -5`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): preview cosmetics observer — cue->lanes drives anim + spawns bevy_vfx at socket with baked VfxParams"`

### Task 32: Windowed screenshot acceptance (author firebolt → Play → screenshot the cosmetics) + final M3 gates
**Files:** Modify `crates/arena_editor/src/main.rs` (an `ARENA_EDITOR_SCREENSHOT` headless-windowed capture hook); Create `tests/screenshot_acceptance.rs`.
**Interfaces:** Produces — `ARENA_EDITOR_SCREENSHOT=<path>` env hook (on boot loads firebolt, writes `PlayEvent`, advances N frames, captures to `<path>`); the test asserts the PNG exists + is non-trivial.
- [ ] Step 1: read `arena_game/src/client/harness.rs` for `ScreenshotConfig`/`screenshot_system`/`smoke_exit_after_frames` (the `bevy::render::view::screenshot::Screenshot` + `save_to_disk` idiom) + the `ARENA_*` env-hook style; confirm `bevy_editor_game::PlayEvent` is a `#[derive(Message)]`.
- [ ] Failing test `tests/screenshot_acceptance.rs` (skips cleanly with no display):
```rust
#[test] fn play_renders_firebolt_cosmetics_to_a_png() {
    if std::env::var_os("DISPLAY").is_none() && !cfg!(target_os = "macos") { eprintln!("no display; skipping"); return; }
    let out = std::env::temp_dir().join("arena_editor_firebolt_m3.png"); let _ = std::fs::remove_file(&out);
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_arena-editor"))
        .env("ARENA_EDITOR_SCREENSHOT", &out).env("ARENA_EDITOR_SCREENSHOT_FRAMES", "120").status().expect("spawn");
    assert!(status.success()); let meta = std::fs::metadata(&out).expect("written"); assert!(meta.len() > 1024);
}
```
- [ ] Run FAIL: `nix develop --command cargo test -p arena_editor --test screenshot_acceptance 2>&1 | tail -16`
- [ ] `main.rs`: when `ARENA_EDITOR_SCREENSHOT` is set, add a `Startup` system that writes `PlayEvent` (so `start_preview` casts + `on_preview_cue` renders cosmetics) and an `Update` capture system (mirroring `harness.rs::screenshot_system`) that after `ARENA_EDITOR_SCREENSHOT_FRAMES` frames captures to the path + exits. Behind the env var so the normal editor is unaffected.
- [ ] Run PASS (or clean skip): `nix develop --command cargo test -p arena_editor --test screenshot_acceptance 2>&1 | tail -10`
- [ ] Eyeball the PNG (manual): `Read` the `std::env::temp_dir()/arena_editor_firebolt_m3.png` path the test prints (firebolt muzzle burst + casting pose).
- [ ] Final M3 gate — both workspaces green: `nix develop --command cargo test -p arena_editor 2>&1 | tail -8 && cargo test -p arena_skills 2>&1 | tail -6 && cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -3 && bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -8`
- [ ] Commit: `git add -A && git commit -m "feat(arena_editor): windowed screenshot acceptance (author firebolt -> Play -> capture cosmetics); M3 complete"`

### Task 33: M1–M3 integration green-up + branch finish
**Files:** none (verification + branch finish across the three repos).
**Interfaces:** Consumes — all gates. Produces — the merged feature ready for review.
- [ ] obelisk-bevy: `cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --lib --tests 2>&1 | tail -8 && cargo test --features test-support --test golden 2>&1 | tail -8` (golden byte-identical).
- [ ] bevy_modal_editor: `cd /Users/luke/src/bevy_modal_editor && nix develop --command cargo test -p bevy_modal_editor 2>&1 | tail -8`.
- [ ] obelisk-arena: `cd /Users/luke/src/obelisk-arena && cargo test -p arena_sim 2>&1 | tail -4 && cargo test -p arena_skills 2>&1 | tail -4 && nix develop --command cargo test -p arena_editor 2>&1 | tail -6 && cargo build -p arena_game --bin arena-server --bin arena-client --bin arena-observer 2>&1 | tail -3 && bash crates/arena_game/tools/net-test/run_session.sh 2>&1 | tail -8`.
- [ ] Open the three PRs (`m1-cast-timeline-serialize` in obelisk-bevy; `m1-custom-mode-seam` in bevy_modal_editor — the upstream seam PR; the obelisk-arena branch chain `m1-arena-sim`→`m2-skill-timeline`→`m3-cosmetic-lanes`) via `gh pr create`, each body noting the gates that pass. (Per spec Decision 2: vendor the editor seam as a thin fork until the upstream PR merges.)
- [ ] No commit (verification task).