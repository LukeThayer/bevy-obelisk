# Obelisk-Arena → Lightyear-Native Prediction + Interpolation Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace arena's hand-rolled netcode (a `NetworkedPosition` pose stream + manual two-sample smoothing for remotes + predict-locally-snap-to-server for the local player + a `PlayerInputMessage` wire) with **lightyear-native client-side prediction (with rollback) for the local player + native interpolation for remotes**, following lightyear 0.26.4's canonical `avian_3d_character` example (Dynamic body + force controller) with **native input** (the `simple_box` pattern, since bei is unusable). This eliminates the network jitter (the hand-rolled smoother renders one variable-span sample behind, and the local snap fights prediction).

**Decisions (locked with the user):** FULL migration in one go; **`RigidBody::Dynamic` + force-based controller** (canonical), accepting the movement/jump feel re-tune; native input (no leafwing/bei).

**Architecture:** The server spawns ONE player entity per client: a Dynamic avian character with `ActionState<ArenaInput>` (native input) + `Replicate::to_clients(All)` + `PredictionTarget::Single(owner)` + `InterpolationTarget::AllExceptSingle(owner)` + `ControlledBy`. lightyear auto-creates a **Predicted** entity on the owner's client and **Interpolated** entities on the others. ONE shared `apply_arena_action` force-controller runs in `FixedUpdate` (server: all chars; client: `With<Predicted>` — lightyear re-runs it during rollback). Visual smoothing via `FrameInterpolationPlugin<Position/Rotation>`. The obelisk combatant, hurtbox, cast pipeline, health, customization, and cast-state replication ride the server-authoritative entity; the rig attaches to the Predicted (local, hidden) / Interpolated (remote, visible) entities via `Added<Predicted>`/`Added<Interpolated>` observers.

**Tech Stack:** Bevy 0.18.1, Avian3d 0.5.0, lightyear 0.26.4 (add features `input_native` + `prediction`). Rust.

---

## Canonical reference (READ THESE before each milestone — they are the recipe)

Worktree at `/tmp/lightyear-0.26.4/examples/`:
- **`avian_3d_character/src/{protocol,shared,server,client,renderer}.rs`** — the PRIMARY pattern: avian Dynamic + prediction + rollback + interpolation + `FrameInterpolationPlugin`. The force controller is `shared.rs::apply_character_action`. The client Confirmed/Predicted observer is `client.rs::handle_new_character` (`Added<Predicted>` + `Has<Controlled>` → `InputMap` + physics). `LightyearAvianPlugin { replication_mode: Position }` + disabled physics plugins in `shared.rs:85-97`.
- **`simple_box/src/{protocol,client,server,shared}.rs`** — the NATIVE-INPUT pattern (no leafwing/bei): `input::native::InputPlugin::<Inputs>::default()`, `ActionState<Inputs>`, `InputMarker<Inputs>`, `buffer_input` in `FixedPreUpdate.in_set(InputSystems::WriteClientInputs)`, server spawn with `PredictionTarget::Single(owner)` + `InterpolationTarget::AllExceptSingle(owner)`, client observers `handle_predicted_spawn` (`On<Add, ...>` → `InputMarker`) / `handle_interpolated_spawn`.

**Combine them:** avian Dynamic + force controller (from `avian_3d_character`) DRIVEN BY native input (from `simple_box`). i.e. swap `avian_3d_character`'s leafwing `ActionState<CharacterAction>` for native `ActionState<ArenaInput>`.

## Arena surface (current — what changes)
- `crates/arena_game/src/net/protocol.rs`: avian `Position`/`Rotation`/`LinearVelocity` registered with `add_prediction()`+`add_should_rollback()`+`add_linear_interpolation()` but `ComponentReplicationConfig { disable: true }` (DORMANT — enable it). `NetworkedPosition` (pose+cast fields) with `add_interpolation_with(lerp_networked_position)`. Channels `InputChannel`/`CastChannel`/`EventChannel`; messages `PlayerInputMessage`/`CastRequestMessage`/`NetEventMessage`/`CueWireMessage`/`RoundStateMessage`/`Customize*`.
- `crates/arena_game/src/server/mod.rs`: `sync_networked_players` spawns the monolith (obelisk combatant + identity + `NetworkedHealth`/`PlayerCustomization`/`NetworkedPosition`/`PlayerInputState` + KINEMATIC avian body + `Replicate::manual` + `Hurtbox`). `drain_player_inputs`/`apply_player_rotation`/`run_player_controller` (kinematic) + `sync_player_positions` (ships `NetworkedPosition`) + `sync_cast_state` (stamps cast_phase) + `sync_networked_health`. `drain_cast_requests` (reliable cast, keyed by `ClientPlayerMap[client_id]`). Round machine reads/writes player Position + `NetworkedPosition`.
- `crates/arena_game/src/client/prediction.rs` (`predict_local_movement`+`snap_local_to_server`) and `client/replication.rs` (`smooth_networked_transforms`+`NetworkedPositionSmoothing`): **DELETE** — lightyear replaces both.
- `crates/arena_game/src/client/net.rs`: `materialize_replicated_players` (Kinematic body + `LocalNetPlayer`), `send_local_player_input` (PlayerInputMessage), `ChargeState`, `send_cast_requests`, customization. The materialize → replaced by Predicted/Interpolated observers.
- `crates/arena_game/src/client/present.rs`: `attach_rig_to_players` (rig child of player). `client/controller.rs`: `CameraYaw`/`AimPitch`/`follow_local_net_player` (camera follows `LocalNetPlayer`). `client/rig.rs`: `drive_animation` reads root + `NetworkedPosition.cast_phase`.
- `crates/arena_game/src/lib.rs`: `add_avian_with_lightyear` (LightyearAvianPlugin::Position + disabled plugins — ALREADY matches the example), `add_obelisk_sim_client`/`add_obelisk_sim_headless`.
- Workspace `Cargo.toml`: lightyear features → add `input_native`, `prediction`.
- `crates/arena_game/tools/net-test/`: harness asserts cast+damage replication; movement traces read `NetworkedPosition`.

---

## Milestone 1 — Feature flags + native input type + protocol

**Files:** workspace `Cargo.toml`, `crates/arena_game/src/net/protocol.rs`, a new `crates/arena_game/src/net/input.rs`.

### Task 1.1: Add lightyear features
- [ ] Edit workspace `/Users/luke/src/obelisk-arena/Cargo.toml` lightyear features: `["netcode", "udp", "avian3d", "interpolation", "input_native", "prediction"]`. Build to confirm the crate resolves: `cargo build`.
- [ ] Commit `chore(arena): enable lightyear input_native + prediction features`.

### Task 1.2: Define the native `ArenaInput`
- [ ] Create `src/net/input.rs`. Define the per-tick input the controller needs (movement + look + jump + charging). Native input requires the type be `MapEntities + Message`-able per lightyear; mirror `simple_box`'s `Inputs`:
```rust
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Per-tick native input replicated by lightyear (`ActionState<ArenaInput>`), consumed by the shared
/// FixedUpdate controller on the server + the client `Predicted` entity (re-run during rollback).
/// Cast is NOT here — it stays a discrete reliable `CastRequestMessage` (movement prediction doesn't
/// depend on it). `Default` = no input (idle).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default, Reflect)]
pub struct ArenaInput {
    pub movement: Vec2, // camera-relative WASD: x strafe(+right), y forward
    pub yaw: f32,
    pub pitch: f32,
    pub jump: bool,
}
```
  Re-read `simple_box/src/protocol.rs` for the EXACT trait bounds lightyear 0.26.4's `input::native::InputPlugin` requires of the input type (it may need `Reflect`/`MapEntities`/a marker). Match them.
- [ ] Commit `feat(arena): native ArenaInput type for lightyear input`.

### Task 1.3: Protocol — register native input + enable avian prediction/interpolation
- [ ] In `ProtocolPlugin::build` (`src/net/protocol.rs`):
  - Add `app.add_plugins(lightyear::prelude::input::native::InputPlugin::<ArenaInput>::default());` (verify the exact path/config against `simple_box/src/protocol.rs`; it may take an `InputConfig` with `rebroadcast_inputs: true` so the server receives inputs — the avian example sets that).
  - Change the avian `Position`/`Rotation`/`LinearVelocity` registration: REMOVE `with_replication_config(ComponentReplicationConfig{disable:true})`, ADD `.add_linear_correction_fn()` on Position+Rotation (per the avian example). Add `AngularVelocity` registration with `add_prediction()` + `add_should_rollback()` (the example registers it; LockedAxes keeps it ~0 but lightyear wants it predicted). Keep the `*_should_rollback` thresholds (already match the example at 0.01).
  - Keep `NetworkedHealth`, `PlayerCustomization` registrations. Keep `EventChannel` + `CastChannel` + the S→C messages (`NetEventMessage`/`CueWireMessage`/`RoundStateMessage`/`Customize*`) + `CastRequestMessage` on `CastChannel`.
  - REMOVE `PlayerInputMessage` + `InputChannel` (native input owns its channel now).
  - REPLACE `NetworkedPosition` (the whole pose stream) with a small replicated **`NetworkedCastState`** carrying ONLY what avian Position/Rotation can't: `{ cast_phase: u8, cast_skill: u16 }` (cast animation for remotes). Register it (no interpolation needed — discrete; snap). Drop `airborne`/`pitch`/`yaw`/`x/y/z` from the wire (pose = avian Position/Rotation now; pitch for the spine-lean can ride `NetworkedCastState` if needed, else derive). Re-home `lerp_networked_position` deletion.
- [ ] `cargo build` (will break downstream until later milestones — that's expected; keep this milestone's edits to protocol/input/Cargo and stub the rest minimally so it compiles, OR land M1+M2+M3 together if the borrow is too tangled — use judgment, but COMMIT working increments).
- [ ] Commit `feat(arena): register native input + enable avian Position/Rotation/Velocity prediction+interpolation`.

---

## Milestone 2 — Server: Dynamic character + targets + force controller

**Files:** `crates/arena_game/src/server/mod.rs`, `crates/arena_game/src/net/mod.rs` (constants), `crates/arena_game/src/shared_controller.rs` (new, shared by server+client).

### Task 2.1: Shared force controller
- [ ] Create `src/shared_controller.rs` porting `avian_3d_character/src/shared.rs::apply_character_action`, driven by `ArenaInput` instead of leafwing `ActionState<CharacterAction>`:
```rust
// reads ArenaInput (movement/yaw/jump), applies FORCES to a Dynamic avian body. Ground check via
// SpatialQuery raycast under the capsule (exclude self). Jump = apply_linear_impulse(Vec3::Y*JUMP_IMPULSE)
// when grounded. Move = accelerate toward desired ground velocity (move_towards + force = accel*mass).
// Yaw -> set Rotation (LockedAxes locks physical rotation; set Rotation directly each tick like the
// example sets it, OR via torque — match the example).
```
  Use avian's global `Gravity` resource (set it in `add_avian_with_lightyear` or a shared build) instead of the manual `GRAVITY` constant. Tune `MAX_SPEED`/`MAX_ACCELERATION`/`JUMP_IMPULSE`/`Gravity`/`Friction` to roughly match the current feel (MAX_SPEED ≈ 4 m/s, jump apex ≈ 1.0–1.2 m). Keep `EYE_HEIGHT`/`GROUND_Y`/spawn constants (`net/mod.rs`) — but note: with a Dynamic body on a real floor collider, `GROUND_Y` becomes emergent (the floor height), NOT a clamp. **Add a floor collider** (see 2.3).
- [ ] Unit-test the desired-velocity math if practical. Commit `feat(arena): shared force-based character controller (ArenaInput-driven)`.

### Task 2.2: Server spawn — Dynamic + targets + ControlledBy + ActionState
- [ ] In `sync_networked_players`, replace the kinematic body + `Replicate::manual` with the canonical Dynamic spawn (mirror `avian_3d_character/src/server.rs::handle_connected` + `CharacterPhysicsBundle`):
```rust
.insert((
    Position(spawn), Rotation::default(), LinearVelocity::default(), AngularVelocity::default(),
    RigidBody::Dynamic,
    Collider::capsule(0.4, 1.2),
    LockedAxes::default().lock_rotation_x().lock_rotation_y().lock_rotation_z(),
    Friction::new(0.0).with_combine_rule(CoefficientCombine::Min),
    ActionState::<ArenaInput>::default(),
    Replicate::to_clients(NetworkTarget::All),
    PredictionTarget::to_clients(NetworkTarget::Single(client_id_peer)),
    InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(client_id_peer)),
    ControlledBy { owner: connection_entity, lifetime: Default::default() },
))
```
  Keep the obelisk combatant (`make_combatant`/`grant_skill`/faction), identity (`NetworkedPlayer`/`NetworkOwner`/`NetworkedId`/`ObeliskNetId`), `NetworkedHealth`, `PlayerCustomization`, `NetworkedCastState`. Keep the `Hurtbox` (separate insert; it stays server-authoritative — but it currently OVERWRITES RigidBody to Static; FIX: put the hurtbox `Collider` on a CHILD entity so the player stays `RigidBody::Dynamic` — a Static hurtbox collider on the same entity now conflicts with the Dynamic body. Child hurtbox entity with `Hurtbox{owner}` + the capsule(0.35,0.48) + a Transform, parented to the player so it tracks). Verify obelisk `detect_overlaps` finds a child-entity hurtbox (it queries `Hurtbox`+`Collider`; should be fine).
  - `Replicate::to_clients(NetworkTarget::All)` REPLACES `Replicate::manual`. VERIFY late-join in the harness (the prior `manual` workaround existed because `All` snapshotted senders — confirm 0.26 `to_clients(All)` widens to new clients; the canonical example relies on it).
- [ ] Commit `feat(arena): spawn Dynamic predicted+interpolated character (drops Replicate::manual + kinematic)`.

### Task 2.3: Floor collider + server controller wiring
- [ ] Add a STATIC floor collider on the server (and client) so the Dynamic body rests on it (the green plane is currently visual-only). E.g. a large `Collider::cuboid` (or half-space) at the platform height so feet rest at world 0. Mirror `avian_3d_character`'s floor (`FloorPhysicsBundle`). Re-derive `GROUND_Y`/spawn Y so the capsule rests with feet at world 0.
- [ ] Replace `drain_player_inputs`/`apply_player_rotation`/`run_player_controller` (kinematic) with: the shared `apply_arena_action` system in `FixedUpdate` over all character entities (server reads the replicated `ActionState<ArenaInput>` lightyear maintains — NO manual drain needed; lightyear delivers input to `ActionState`). DELETE `PlayerInputState` + `drain_player_inputs` + `sync_player_positions` (pose now replicates via avian Position/Rotation natively). Keep `sync_cast_state` but write to `NetworkedCastState` (+ charging telegraph) instead of `NetworkedPosition`. Keep `sync_networked_health`.
- [ ] Update the round machine (`run_round_machine`/`reset_for_new_round`): it writes `Position`/`Transform`/`NetworkedPosition` on respawn — now write `Position`/`Rotation` (+ zero velocity) only; drop the `NetworkedPosition` writes. Spawn-slot logic unchanged.
- [ ] `cargo build` (server side). Commit `feat(arena): server runs shared force controller; floor collider; cast-state via NetworkedCastState`.

---

## Milestone 3 — Client: native input, Predicted/Interpolated observers, delete hand-rolled

**Files:** `crates/arena_game/src/client/{mod,net,prediction,replication,present,controller,rig}.rs`.

### Task 3.1: Native input buffer
- [ ] Add `buffer_arena_input` in `FixedPreUpdate.in_set(InputSystems::WriteClientInputs)` (mirror `simple_box/src/client.rs::buffer_input`): read the windowed keyboard/mouse (the existing `bridge_windowed_input_to_local_input` logic — WASD → movement, `CameraYaw`→yaw, `AimPitch`→pitch, Space→jump) and write `ActionState::<ArenaInput>` on the entity carrying `InputMarker<ArenaInput>`. For the headless `ARENA_AUTOMOVE` path, write the automove input the same way. DELETE `send_local_player_input` + `PlayerInputMessage` send + the `LocalInput` resource bridge (native input owns the wire).
- [ ] Commit `feat(arena): buffer native ArenaInput from keyboard/mouse`.

### Task 3.2: Predicted/Interpolated observers (rig, camera, input, markers)
- [ ] Replace `materialize_replicated_players` with observers (mirror `avian_3d_character/src/client.rs::handle_new_character` + `simple_box` predicted/interpolated handlers):
  - `On<Add, Predicted>` (or `Added<Predicted>` system) `With<NetworkedPlayer>`: if `Has<Controlled>` (the local owner) → add `InputMarker::<ArenaInput>::default()`, the `CharacterPhysicsBundle` (Collider for the predicted body), tag `LocalNetPlayer`, attach the camera (follow this entity), attach the LOCAL rig (`Visibility::Hidden`). Add `FrameInterpolate<Position>`/`<Rotation>` (see 3.4).
  - `On<Add, Interpolated>` `With<NetworkedPlayer>`: attach the REMOTE rig (visible). Interpolated entities get Position/Rotation from lightyear interpolation; `drive_animation` reads them.
  - Re-home `attach_rig_to_players` logic into these observers (rig is now a child of the Predicted/Interpolated entity). Keep `LocalPlayerBody`/`ArenaBody`/`RIG_FOOT_OFFSET`.
- [ ] Update `controller.rs::follow_local_net_player` to follow the `Predicted` local entity (it already follows `LocalNetPlayer` — keep that, now set on the Predicted entity).
- [ ] Commit `feat(arena): Predicted/Interpolated observers attach rig+camera+input (replaces materialize)`.

### Task 3.3: Client controller + DELETE hand-rolled prediction/smoothing
- [ ] Add the shared `apply_arena_action` system on the client in `FixedUpdate`, gated `With<Predicted>` (mirror `avian_3d_character/src/client.rs::handle_character_actions` — lightyear re-runs it during rollback; the `ActionState<ArenaInput>` lightyear maintains is correct per tick).
- [ ] DELETE `client/prediction.rs` (`LocalPredictionPlugin`, `predict_local_movement`, `snap_local_to_server`) and `client/replication.rs` (`ReplicationSmoothingPlugin`, `smooth_networked_transforms`, `NetworkedPositionSmoothing`, capture system). Remove their plugin registrations from `run_windowed_client`/`run_headless_client`.
- [ ] Commit `feat(arena): client predicts via shared controller (With<Predicted>); delete hand-rolled predict+smooth`.

### Task 3.4: Visual frame-interpolation
- [ ] Add `FrameInterpolationPlugin::<Position>::default()` + `<Rotation>` and the `On<Add, Position>` observer that inserts `FrameInterpolate<Position/Rotation> { trigger_change_detection: true, .. }` on `With<Predicted>` entities (mirror `avian_3d_character/src/renderer.rs`). This smooths the predicted local player's render between FixedUpdate ticks. (Interpolated remotes are already smooth via lightyear interpolation.)
- [ ] Commit `feat(arena): FrameInterpolation for the predicted local player`.

### Task 3.5: Re-home cast animation + customization reads
- [ ] `rig.rs::drive_animation`: read `NetworkedCastState.cast_phase` (instead of `NetworkedPosition.cast_phase`) on remote (Interpolated) players to drive the casting blend. Local (Predicted) keeps the `ChargeState` windup. Remote velocity for the walk blend: derive from the entity's `LinearVelocity` (now replicated/interpolated) or Position delta. Use `Rotation` (not `NetworkedPosition.yaw`) for the remote body yaw.
- [ ] `client/net.rs` customization drain: key on the same entities (`NetworkedId`); unchanged except it now matches Confirmed/Predicted/Interpolated entities (apply to the visible rig's entity).
- [ ] `send_cast_requests`: unchanged (reliable `CastChannel`), but read aim from `CameraYaw`/`AimPitch` as today; keep `PredictedCast` local-cosmetic. Cast still server-authoritative.
- [ ] Commit `feat(arena): remote cast/walk animation from replicated cast-state + avian velocity/rotation`.

---

## Milestone 4 — Harness + verification

**Files:** `crates/arena_game/tools/net-test/{run_session.sh,summarize.py}`, the observer bin.

### Task 4.1: Update traces to avian pose
- [ ] The harness's `server_pose`/`remote_pose` traces read `NetworkedPosition` — update them to read avian `Position`/`Rotation` (server) and the Interpolated entity's `Position` (observer). Keep the cast+damage assertions (they key on obelisk `NetEvent`s — unchanged). Update `summarize.py` accordingly.
- [ ] The headless observer must drive native input (`ARENA_AUTOMOVE`/`ARENA_AUTOCAST` now write `ActionState<ArenaInput>` / send `CastRequestMessage`). Update the observer bin.
- [ ] Run the harness: assert (a) both players spawn + replicate (late-join: 2nd client sees 1st), (b) cast + damage resolve + echo on both observers, (c) a moving player's avian Position propagates to the other observer (interpolation), (d) NO position divergence beyond interpolation delay.
- [ ] Commit `test(arena): net-test harness reads avian pose + drives native input`.

### Task 4.2: Windowed jitter verification
- [ ] Two-client windowed run (one auto-moving + casting, one observing): capture the observer's view of the mover. Confirm: opponent moves SMOOTHLY (no stutter — the native-interpolation goal), cast animation + projectile correct, own movement responsive (prediction), feet planted, level shots land. Read the screenshot(s). Compare felt smoothness vs the old hand-rolled path.
- [ ] Bump `PROTOCOL_ID` (`net/mod.rs`) since the wire changed.
- [ ] Commit `chore(arena): bump PROTOCOL_ID for native-prediction wire`.

---

## Final Review
- [ ] Full arena gate: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`.
- [ ] obelisk-bevy gate unaffected (no obelisk change this migration) — confirm goldens still green if obelisk touched at all (it shouldn't be).
- [ ] Dispatch a final code-reviewer over the whole diff.
- [ ] Delete dead code: `NetworkedPosition`, `lerp_networked_position`, `PlayerInputMessage`, `PlayerInputState`, `InputChannel`, `LocalInput`, the prediction/replication modules.

## Risks (watch these)
1. **Late-join with `Replicate::to_clients(All)`** — the prior `manual` workaround. Verify the 2nd client sees the 1st player. If it breaks, use the per-client `Replicate` refresh the example/region uses.
2. **Dynamic-body feel** — the kinematic→Dynamic switch changes movement/jump feel; budget tuning time (friction/accel/gravity/impulse). The documented "velocity clobber" footgun: the canonical example AVOIDS it with `AvianReplicationMode::Position` + disabled `PhysicsTransformPlugin`/`PhysicsInterpolationPlugin` (arena's `add_avian_with_lightyear` already does this) + applying FORCES in FixedUpdate. Confirm the body actually moves (the arena's earlier Dynamic attempt failed — follow the example exactly).
3. **Hurtbox vs Dynamic** — the hurtbox `RigidBody::Static` must move OFF the player entity (child entity) so the player stays Dynamic.
4. **obelisk on Predicted** — keep `ActiveCast`/combat server-authoritative; the client Predicted entity predicts ONLY movement. cast_phase is replicated (`NetworkedCastState`). Don't let rollback touch obelisk components (they're not registered for prediction → lightyear won't roll them back; fine).
5. **Cast aim from the predicted entity** — `send_cast_requests` reads `CameraYaw`/`AimPitch`; the muzzle offset + eye height still apply. Verify shots still land post-migration.

## Self-Review Notes
- Spec coverage: native input (M1), Dynamic+targets+controller+floor (M2), client observers+native-input-buffer+delete-handrolled+frame-interp+anim-rehome (M3), harness+windowed (M4). All hand-rolled systems deleted; all canonical pieces added.
- Type consistency: `ArenaInput` shared M1→M3; `NetworkedCastState{cast_phase,cast_skill}` shared M1(register)/M2(server write)/M3(client read).
