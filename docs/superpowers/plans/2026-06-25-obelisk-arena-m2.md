# obelisk-arena M2 Implementation Plan — 2-player online, server-authoritative, Stage-A prediction

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two clients connect to a dedicated headless server; each casts firebolt; both see the cast + server-authoritative damage; own movement is client-predicted; best-of-3 rounds resolve.

**Architecture:** Split the co-located M1 `arena_game` into a **shared lib** + **two binaries** (`arena-server` headless authority running `ObeliskSimPlugin` + `CombatRng` + obelisk→lightyear egress; `arena-client` windowed present + cosmetics + prediction) over **lightyear 0.26.4**. The server is the sole authority for combat/damage/RNG; clients predict their own movement + cast initiation + projectile motion, but **never** resolve hits or touch `CombatRng` (Stage A). Discrete combat events ride as **reliable messages** (not component mutations); pose rides the replicated/interpolated `NetworkedPosition` stream.

**Tech Stack:** Rust, Bevy 0.18, Avian 0.5 (pinned by lightyear), lightyear 0.26.4 (`netcode`+`udp`+`avian3d`+`interpolation`), obelisk-bevy (path dep), wisp source (copied).

---

## Context every task needs (read first)

- **The netcode integration guide (this plan's source of truth):** `../obelisk-bevy/docs/superpowers/specs/2026-06-25-m2-netcode-surface.md`. It contains the exact lightyear 0.26 API table (§1.2), the binary skeletons (§2), the full protocol (§3), the CueMessage refactor (§4), server/client pipelines (§5/§6), round flow (§7), the harness (§8), and the wisp copy-targets with line ranges. **Each task below references its guide section; read it.**
- **The spec:** `../obelisk-bevy/docs/superpowers/specs/2026-06-25-obelisk-arena-phase1-design.md` (§5 networking, §6.1–6.2 Stage-A prediction, §7 replication fidelity, §9 testing, §10 M2).
- **lightyear 0.26 is unproven in this lineage.** wisp *registered* prediction/rollback but never *enabled* it. So for M2.2 (prediction) and any 0.26 API call: **verify the exact API against the installed lightyear 0.26.4 source** (`~/.cargo/registry/src/*/lightyear*0.26*/`, its `examples/`) and wisp's usage — do not trust an API name that doesn't resolve. The guide's §1.2 table is the best-known shape; the compiler + the registry source are the authority.
- **Version pins (do not bump):** avian3d **0.5** is pinned by `lightyear_avian3d` (can't coexist with 0.6); Bevy 0.18; lightyear **0.26.4**. **Drop wisp's `input_bei` feature** (version-mismatch dead weight; the arena hand-rolls input).
- **Keep M1 green as a regression gate.** M2.0 refactors `CueMessage` while keeping the single-process M1 `arena-client` casting firebolt + spawning cosmetics. Don't break M1 until the wire replaces it.
- **Two non-obvious 0.26 traps** (guide §1.2): under `LightyearAvianPlugin::Position` write avian `Position`/`Rotation`, **never `Transform`**, on physics bodies; and per-tick **component mutation after insert is unreliable** → discrete combat events must be reliable **messages**, not component changes.
- **Work in `/Users/luke/src/obelisk-arena`** (M0+M1 is committed there on `master`). No worktree.

## File structure (M2 reshapes arena_game)

```
crates/arena_skills/src/lib.rs        # M2.0: CueMessage → serde; SkillFxRegistry; egress + consumer helpers
crates/arena_game/
  Cargo.toml                          # + lightyear 0.26.4; [lib] + 3 [[bin]]
  src/lib.rs                          # re-exports: net, server, client, skills, arena_root, add_avian_with_lightyear
  src/net/mod.rs                      # constants (TICK_HZ, PROTOCOL_ID, key, addrs) + arg parser (copy wisp)
  src/net/protocol.rs                 # ProtocolPlugin: components + channels + messages (guide §3)
  src/net/server.rs                   # ServerNetPlugin: listen, on_new_link, spawn_server
  src/net/client.rs                   # ClientNetPlugin: connect, ConnectTo
  src/server/mod.rs                   # ArenaServerPlugin: spawn players, controller, egress, rounds, late-joiner
  src/client/mod.rs                   # ArenaClientPlugin: controller(predicted), rig/anim, cosmetics, HUD, interp
  src/skills.rs                       # register_server_cue_egress / register_client_cue_binding (CueWireMessage)
  src/trace.rs                        # existing (M1); env-rename already done
  src/bin/server.rs                   # arena-server (guide §2.2)
  src/bin/client.rs                   # arena-client (guide §2.3)
  src/bin/observer.rs                 # arena-observer (M2.5; copy wisp)
```

---

# M2.0 — CueMessage serde refactor (unblocks the wire; keep M1 green)

### Task 1: Rewrite `CueMessage` as a serde wire type

**Files:** Modify `crates/arena_skills/src/lib.rs`; Test `crates/arena_skills/tests/cue_wire.rs`

- [ ] **Step 1: Write the failing round-trip test**

```rust
// crates/arena_skills/tests/cue_wire.rs
use arena_skills::{CueMessage, CueKind};
use bevy::math::Vec3;

#[test]
fn cue_message_round_trips_through_serde() {
    let m = CueMessage { cue_id: "on_cast".into(), source_id: "player".into(),
        position: Vec3::new(1.0, 2.0, 3.0), kind: CueKind::OnCast };
    let json = serde_json::to_string(&m).unwrap();
    let back: CueMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.cue_id, "on_cast");
    assert_eq!(back.source_id, "player");
    assert_eq!(back.kind, CueKind::OnCast);
}
```

- [ ] **Step 2: Run it — fails to compile (CueMessage isn't serde yet)**

Run: `cd /Users/luke/src/obelisk-arena && cargo test -p arena_skills cue_message_round_trips`
Expected: FAIL — `CueMessage` doesn't derive `Serialize`/`Deserialize` and still has `source: Entity`/`event: LaneEvent`.

- [ ] **Step 3: Rewrite `CueMessage`** (guide §4.2). Replace the M1 def:

```rust
// REMOVE: #[derive(Message, Debug, Clone)] struct with source: Entity + event: LaneEvent
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CueMessage {
    pub cue_id: String,        // the obelisk vfx_cues VALUE obelisk fires (e.g. "firebolt_cast")
    pub source_id: String,     // ObeliskId (stable string), NOT Entity
    pub position: Vec3,        // serde-serializable under bevy's serialize feature
    pub kind: CueKind,
}
```
Ensure `CueKind` and `LaneEvent` (+ its nested `ParticleSpec`/`ProjectileCosmetic`/`AnimLayer`) derive `Serialize` in addition to `Deserialize` (add `serde::Serialize`). Add `serde` to `arena_skills` deps if the bare `Serialize` import needs it (it's already a dep). Confirm `Vec3` serde works — `bevy` must have its `serialize` feature; if not, store `position: [f32;3]`.

- [ ] **Step 4: Run — passes**

Run: `cargo test -p arena_skills cue_message_round_trips`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "M2.0: CueMessage → serde wire type (cue_id/source_id/position/kind)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 2: Add `SkillFxRegistry` (cue_id → LaneEvents)

**Files:** Modify `crates/arena_skills/src/lib.rs`; Test `crates/arena_skills/tests/registry.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/arena_skills/tests/registry.rs
use arena_skills::SkillFxRegistry;
use std::path::Path;

#[test]
fn registry_loads_firebolt_cues() {
    let reg = SkillFxRegistry::load_dir(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/skills")));
    assert!(reg.lanes("firebolt_cast").map_or(false, |l| !l.is_empty()));
    assert!(reg.lanes("firebolt_impact").map_or(false, |l| !l.is_empty()));
    assert!(reg.lanes("nonexistent").is_none());
}
```

- [ ] **Step 2: Run — fails (no SkillFxRegistry)**

Run: `cargo test -p arena_skills registry_loads_firebolt`
Expected: FAIL — `SkillFxRegistry` undefined.

- [ ] **Step 3: Implement `SkillFxRegistry`** (guide §4.2). The M1 `SkillFx.lanes` is `HashMap<String, LaneEvent>` (one lane per cue id); flatten every `.skillfx.ron` into `by_cue: HashMap<String, Vec<LaneEvent>>`:

```rust
#[derive(bevy::prelude::Resource, Default)]
pub struct SkillFxRegistry { pub by_cue: std::collections::HashMap<String, Vec<LaneEvent>> }
impl SkillFxRegistry {
    pub fn load_dir(dir: &std::path::Path) -> Self {
        let mut by_cue: std::collections::HashMap<String, Vec<LaneEvent>> = Default::default();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.to_string_lossy().ends_with(".skillfx.ron") {
                    if let Ok(s) = std::fs::read_to_string(&p) {
                        if let Ok(fx) = ron::de::from_str::<SkillFx>(&s) {
                            for (cue_id, lane) in fx.lanes { by_cue.entry(cue_id).or_default().push(lane); }
                        }
                    }
                }
            }
        }
        Self { by_cue }
    }
    pub fn lanes(&self, cue_id: &str) -> Option<&[LaneEvent]> { self.by_cue.get(cue_id).map(|v| &v[..]) }
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test -p arena_skills registry_loads_firebolt`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "M2.0: SkillFxRegistry (cue_id -> [LaneEvent]) loaded from .skillfx.ron

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 3: Split the binding into egress helper + registry consumer

**Files:** Modify `crates/arena_skills/src/lib.rs`; Test `crates/arena_skills/tests/binding_split.rs`

- [ ] **Step 1: Write the failing test** for the consumer resolution:

```rust
// crates/arena_skills/tests/binding_split.rs
use arena_skills::{SkillFxRegistry, CueMessage, CueKind, resolve_cue};
use bevy::math::Vec3;
use std::path::Path;

#[test]
fn consumer_resolves_lanes_for_a_cue() {
    let reg = SkillFxRegistry::load_dir(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/skills")));
    let m = CueMessage { cue_id: "firebolt_cast".into(), source_id: "player".into(),
        position: Vec3::ZERO, kind: CueKind::OnCast };
    let lanes = resolve_cue(&reg, &m);
    assert!(!lanes.is_empty(), "firebolt_cast should resolve at least one lane");
    // missing cue → empty, no panic
    let miss = CueMessage { cue_id: "nope".into(), source_id: "x".into(), position: Vec3::ZERO, kind: CueKind::OnHit };
    assert!(resolve_cue(&reg, &miss).is_empty());
}
```

- [ ] **Step 2: Run — fails (no `resolve_cue`)**

Run: `cargo test -p arena_skills consumer_resolves_lanes`
Expected: FAIL.

- [ ] **Step 3: Implement the two helpers** (guide §4.3). Pure functions so they're lightyear-free + testable:

```rust
/// CueEvent → CueMessage (the egress half; the obelisk side supplies source_id via an index).
pub fn cue_event_to_message(cue_id: &str, source_id: &str, position: Vec3, kind: CueKind) -> CueMessage {
    CueMessage { cue_id: cue_id.into(), source_id: source_id.into(), position, kind }
}
/// CueMessage → the LaneEvents to play (the consumer half; re-looks up the registry).
pub fn resolve_cue<'a>(reg: &'a SkillFxRegistry, m: &CueMessage) -> &'a [LaneEvent] {
    reg.lanes(&m.cue_id).unwrap_or(&[])
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test -p arena_skills consumer_resolves_lanes`
Expected: PASS. (`arena_skills` stays lightyear-free — these are plain functions.)

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "M2.0: split cue binding into egress + registry-consumer helpers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4: Rewire M1's co-located path to the new CueMessage; re-key AimDirs by ObeliskId

**Files:** Modify `crates/arena_game/src/main.rs`, `crates/arena_game/src/cosmetics.rs`

- [ ] **Step 1: Re-key `AimDirs` to `ObeliskId`** (guide §4.4). Change `AimDirs(HashMap<Entity, Vec3>)` → `AimDirs(HashMap<String, Vec3>)`. Where M1 inserted by caster `Entity`, look up the caster's `ObeliskId` (via a query on the caster) and key by that string. In `spawn_cue_cosmetics`, look up the projectile aim by `source_id`.

- [ ] **Step 2: Rewire the M1 cosmetics consumer** to use a `SkillFxRegistry` + `resolve_cue`. Build the registry at startup (`SkillFxRegistry::load_dir(&root.join("assets/skills"))`, insert as a resource). The M1 `observe_cue` egress now emits the serde `CueMessage` (with `source_id` from an `ObeliskId` lookup on `cue.source`); the consumer reads `CueMessage`, calls `resolve_cue`, and spawns per `LaneEvent`. Because the cosmetics consumer needs `Res` access, route the `CueMessage` through a Bevy event/message channel inside `arena_game` (define an `arena_game`-local `#[derive(Message)] struct LocalCue(pub CueMessage)`), keeping the serde `CueMessage` itself plugin-free.

- [ ] **Step 3: Build + regression-run M1**

Run: `cd /Users/luke/src/obelisk-arena && cargo build`
Then the M1 regression (the gate must still fire a cue):
```bash
ARENA_AUTOCAST=1 ARENA_AUTOCAST_FRAME=30 ARENA_CAM_YAW=2.4 ARENA_SHOT=/tmp/m20_gate.png ARENA_SHOT_FRAME=36 ARENA_TRACE_FILE=/tmp/m20.jsonl cargo run -p arena_game --bin arena_game 2>/dev/null || cargo run -p arena_game
grep -c lane_event /tmp/m20.jsonl
```
Expected: build clean; the trace still shows `lane_event` dispatches (M1 cosmetics still fire through the refactored path). `cargo test -p arena_skills` still green.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M2.0: rewire M1 cosmetics to serde CueMessage + ObeliskId-keyed AimDirs

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# M2.1 — lightyear scaffold + 2 clients connect

### Task 5: Add lightyear + copy wisp net constants/arg-parser; split into lib + bins

**Files:** Modify `crates/arena_game/Cargo.toml`; Create `crates/arena_game/src/lib.rs`, `src/net/mod.rs`, `src/bin/server.rs`, `src/bin/client.rs`

- [ ] **Step 1: Add lightyear + restructure the crate to lib + bins** (guide §1.1, §2.1)

In `crates/arena_game/Cargo.toml`: add `lightyear = { version = "0.26.4", features = ["netcode", "udp", "avian3d", "interpolation"] }`, ensure `avian3d = { version = "0.5", features = ["default", "serialize"] }`. Add `[lib]` (`name="arena_game"`, `path="src/lib.rs"`) and three `[[bin]]` entries (`arena-server`→`src/bin/server.rs`, `arena-client`→`src/bin/client.rs` with `default-run`, `arena-observer`→`src/bin/observer.rs`). Move M1's `main.rs` content into the new module layout (the client bin will subsume it incrementally — for this task, a minimal lib + a stub for each bin that compiles).

- [ ] **Step 2: Copy wisp net constants + arg parser** into `src/net/mod.rs` (guide §2.1: copy `wisp/src/net/mod.rs:25-104` verbatim — `TICK_HZ=60`, `PROTOCOL_ID=0`, `NETCODE_KEY=[0u8;32]`, `default_server_addr`, `parse_addr_args`, `session_seed`). `pub mod net;` from lib.rs.

- [ ] **Step 3: Build**

Run: `cd /Users/luke/src/obelisk-arena && cargo build` (first lightyear fetch is slow — timeout 600000)
Expected: builds (the three bins may be stubs; the lib compiles with the net constants). Verify Cargo.lock has lightyear 0.26.4, a single avian3d 0.5.x, bevy 0.18.x.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M2.1: add lightyear 0.26.4; split arena_game into lib + 3 bins; net constants

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 6: Write `ProtocolPlugin` (components + channels + messages)

**Files:** Create `crates/arena_game/src/net/protocol.rs`

- [ ] **Step 1: Implement `ProtocolPlugin`** (guide §3 — use the exact code). Start with the connectivity-critical subset, then extend: register `NetworkedPlayer`, `NetworkOwner(u64)`, `NetworkedId(u64)`, `ObeliskNetId(String)`, `NetworkedPosition` (with `add_interpolation_with(lerp_networked_position)`), `NetworkedHealth`; the three channels (`InputChannel` UnorderedUnreliable C→S, `CastChannel` UnorderedReliable C→S, `EventChannel` UnorderedReliable S→C); the messages (`PlayerInputMessage`, `CastRequestMessage`, `NetEventMessage(obelisk_bevy::net::NetEvent)`, `CueWireMessage(arena_skills::CueMessage)`, `RoundStateMessage`). The component payloads are guide §3.2. **Verify each `register_component`/`register_message`/`add_channel` call against the installed lightyear 0.26.4 source** (the API table is guide §1.2; the compiler is the authority). The avian `Position`/`Rotation`/`LinearVelocity` prediction registration (guide §3.1) is added here but `disable: true` until M2.2 enables it per-entity.

- [ ] **Step 2: Build + a smoke test that the plugin adds cleanly**

Run: `cargo build` then a test:
```rust
// crates/arena_game/tests/protocol_smoke.rs
#[test]
fn protocol_plugin_builds() {
    let mut app = bevy::app::App::new();
    app.add_plugins(bevy::time::TimePlugin);
    app.add_plugins(arena_game::net::protocol::ProtocolPlugin);
    // building without panic is the assertion
}
```
Expected: builds; the protocol smoke test passes (no double-registration / missing-direction panic).

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.1: ProtocolPlugin — replicated components + channels + messages

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 7: `ServerNetPlugin` + `arena-server` boots and listens

**Files:** Create `crates/arena_game/src/net/server.rs`; write `crates/arena_game/src/bin/server.rs`

- [ ] **Step 1: Implement the server net plugin + listen setup** (guide §2.2, §5.1). `ServerNetPlugin` adds `ServerPlugins { tick_duration: Duration::from_secs_f64(1.0/60.0) }` + `ProtocolPlugin`; `on_new_link` observer (`Add<LinkOf>` → attach `ReplicationSender::default()`, copy `wisp/src/net/server.rs:171-176`); `spawn_server` (`NetcodeServer::new(...)` + `ServerUdpIo` + `LocalAddr(bind)` + `trigger(LinkStart{entity})`, copy `wisp/src/net/server.rs:154-167`). Write `src/bin/server.rs` per guide §2.2 (MinimalPlugins + LogPlugin + AssetPlugin + TransformPlugin + MeshPlugin + ScenePlugin + the net plugin + obelisk sim + config). For this task, the obelisk/round parts can be stubbed; the deliverable is **it boots and listens.**

- [ ] **Step 2: Run the server headless**

Run: `cd /Users/luke/src/obelisk-arena && ARENA_TRACE_SRC=server timeout-less: cargo run --bin arena-server > /tmp/m2_server.log 2>&1 &` then after ~4s `grep -iE "listen|bind|server|panic|error" /tmp/m2_server.log | head` then kill it.
Expected: the log shows the server bound/listening on the addr; no panic. (macOS has no `timeout` — background the process, sleep via a short poll loop, then `kill %1`.)

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.1: ServerNetPlugin + arena-server boots headless and listens

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 8: `ClientNetPlugin` + `arena-client` connects

**Files:** Create `crates/arena_game/src/net/client.rs`; write `crates/arena_game/src/bin/client.rs`

- [ ] **Step 1: Implement the client net plugin + connect** (guide §2.3, §6.1). `ClientNetPlugin` adds `ClientPlugins { tick_duration: 1/60 }` + `ProtocolPlugin`; `spawn_client` + `ConnectTo` (copy `wisp/src/net/client.rs:75-108`: `Authentication::Manual`, `LocalAddr(0.0.0.0:0)`, `PeerAddr(server)`, `ReplicationReceiver`, `trigger(Connect{entity})`); `client_id` from `ARENA_CLIENT_ID` env or wall-clock nanos. Write `src/bin/client.rs` per guide §2.3 (for this task the present/gameplay plugins can stay minimal — the deliverable is **it connects**).

- [ ] **Step 2: Run server + 2 clients, confirm connection**

Run the server (background), then two clients with distinct ids:
```bash
ARENA_CLIENT_ID=1 cargo run --bin arena-client > /tmp/m2_c1.log 2>&1 &
ARENA_CLIENT_ID=2 cargo run --bin arena-client > /tmp/m2_c2.log 2>&1 &
# poll ~6s, then:
grep -iE "connect|Connected|client|panic" /tmp/m2_server.log | head
```
Expected: the server log shows two clients connected (`on_client_connected` / two `ClientOf` / `Connected`). No panic. Kill all three.
(If running two *windowed* clients is intrusive, give the client bin an `ARENA_HEADLESS=1` mode that swaps `DefaultPlugins`→`MinimalPlugins`+LogPlugin for this connectivity check.)

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.1: ClientNetPlugin + arena-client connects to the server

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 9: Spawn `NetworkedPlayer` per client + late-joiner refresh

**Files:** Create `crates/arena_game/src/server/mod.rs` (begin `ArenaServerPlugin`)

- [ ] **Step 1: Implement player spawning + late-joiner refresh** (guide §5.1, §5.7). `sync_networked_players` (polling): one `NetworkedPlayer` per connected `ClientOf`, spawned with avian `RigidBody`+collider, `make_combatant(StatBlock::with_id(...))` + `Faction::Player` + `grant_skill("firebolt")` + `Hurtbox` + `NetworkedHealth` + `NetworkOwner(client_id)` + `NetworkedId` + `ObeliskNetId(obelisk_id)`, positioned at spawn markers by id, `Replicate::manual(current_senders)` (adapt `wisp/src/net/server.rs:284-360`). `refresh_replicate_on_connect` (copy `wisp/src/net/server.rs:208-232` verbatim) re-inserts `Replicate::manual` on the count delta.

- [ ] **Step 2: Headless verify — second client sees the first** (the late-joiner check)

Run server + 2 headless clients (or 2 observers once Task 20 exists; for now add temp logging on the client when it receives a replicated `NetworkedPlayer`). 
Expected: each client logs receiving a `NetworkedPlayer` for the *other* client_id (proving `Replicate::manual` + the refresh deliver to late joiners). No `NetworkTarget::All` (which silently breaks the 2nd client).

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.1: spawn NetworkedPlayer combatant per client + late-joiner refresh

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# M2.2 — movement replication + Stage-A own-movement prediction

### Task 10: Server movement controller + client input send + pose replication

**Files:** Modify `crates/arena_game/src/server/mod.rs`, `crates/arena_game/src/client/mod.rs`, `src/net/protocol.rs`

- [ ] **Step 1: Server controller + input drain** (guide §5.3a, §5.4, §5.6). `drain_player_inputs` (copy `wisp/src/net/server.rs:414-444`) into per-player `PlayerInputState`; `run_player_controller` + `apply_player_rotation` (copy `wisp/src/net/server.rs:453-514`, **write avian `Position`/`Rotation`, not Transform**), adapted to camera-relative movement; `sync_player_positions` (`Changed<Transform>` or post-physics → `NetworkedPosition`, derive `airborne` server-side; adapt `wisp/src/net/server.rs:519-551`). Client `send_local_player_input` packs WASD/yaw/pitch into `PlayerInputMessage`, sends on `InputChannel` (`Single<&mut MessageSender<PlayerInputMessage>>`).

- [ ] **Step 2: Headless verify movement replicates**

With the observer (or temp scripted input), move a player; confirm the server's `NetworkedPosition` for that player updates and the other client receives the change.
Expected: trace/logs show position deltas propagating server→other-client.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.2: server movement controller + client input + pose replication

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 11: Client interpolation of remote players

**Files:** Modify `crates/arena_game/src/client/mod.rs` (replication smoothing)

- [ ] **Step 1: Copy wisp's remote smoothing** (guide §6.2). `NetworkedPositionSmoothing` buffer + `capture_networked_position_samples` (`Changed<NetworkedPosition>`, teleport-snap >3m) + `smooth_networked_transforms` (render lerp at `now - one_tick`, clamp interp `t` to `[0,1]`), copy `wisp/src/net/replication.rs:82-217`, writing **both** Transform and avian `Position`/`Rotation`. Extend `lerp_networked_position` to carry `cast_phase`/`cast_elapsed` (snap phase at `t>=0.5`).

- [ ] **Step 2: Windowed verify [W]**

Run the server + 2 windowed clients (one on this machine; drive the other via the observer or a second `arena-client`). In client A's window, confirm client B's character moves **smoothly** (interpolated, no teleport jitter).
Expected: smooth remote movement. Capture a screenshot if a single frame is informative; otherwise this is a motion check the user can confirm.

- [ ] **Step 2b: If two windowed clients are impractical headlessly, defer the visual to a user check** and confirm headlessly that `NetworkedPositionSmoothing` samples accumulate + the lerp runs without panic (a unit test on `lerp_networked_position` clamping `t`).

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.2: client interpolation/smoothing of remote players

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 12: Enable Stage-A own-movement prediction (finish wisp's disabled rollback)

**Files:** Modify `crates/arena_game/src/net/protocol.rs`, `crates/arena_game/src/client/mod.rs`

- [ ] **Step 1: Verify the 0.26 prediction API before coding.** Read the installed `lightyear 0.26.4` source/examples for the real `add_prediction`/`add_should_rollback`/`Predicted` usage + the per-entity replication-enable override (the guide §6.3 + §1.2 give the shape; the registry source is the authority). wisp registered but never enabled this — **you are the first to exercise it.**

- [ ] **Step 2: Enable prediction on the local player** (guide §6.3): per-entity, opt the local player's `Position`/`Rotation`/`LinearVelocity` into replication (override the `disable: true` default), mark the local player `Predicted`, run the **same** movement controller on the client gated `With<Predicted>`, and implement the `position_should_rollback`/`rotation_should_rollback`/`velocity_should_rollback` fns (threshold `>= 0.01`, not reflexive — guide §1.2 trap).

- [ ] **Step 3: Verify — instant local movement + no persistent divergence**

Windowed [W]: local movement responds with zero perceptible latency. Headless [H]: with `RUST_LOG=lightyear=debug`, confirm rollbacks fire + correct (no silent persistent drift between predicted client position and server `NetworkedPosition`).
**Fallback (spec §6.1):** if rollback is unstable, fall back to wisp's snap-to-server reconciliation (run the controller locally for snappiness, snap to server `NetworkedPosition` each frame — adapt `wisp/src/player/controller.rs:92-123`) and document that Stage-A prediction landed as predict-locally-snap-to-server. Either way the milestone deliverable (responsive local movement, server-authoritative truth) is met.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M2.2: Stage-A own-movement prediction (rollback or snap fallback)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# M2.3 — combat over the wire

### Task 13: Run obelisk combat authoritatively on the server

**Files:** Modify `crates/arena_game/src/bin/server.rs`, `crates/arena_game/src/server/mod.rs`

- [ ] **Step 1: Wire obelisk on the server** (guide §2.2, §5.2). Add `ObeliskSimPlugin` + config/effects/skills load + `seed_combat_rng(session_seed())` to `arena-server` (mirror the M0 wiring, headless). Confirm `CombatRng` is server-only.

- [ ] **Step 2: Headless verify** — a server-side scripted `cast_skill_at` between two spawned players resolves damage.
Run the server with a temporary debug system that, once 2 players exist, casts firebolt from one at the other; grep the server log for `CastBegan` + `DamageResolved`.
Expected: server emits `CastBegan` then `DamageResolved` (20.0). Remove the debug cast after confirming.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.3: obelisk combat runs authoritatively on the server

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 14: Client cast_request → server validate → cast_skill_at

**Files:** Modify `crates/arena_game/src/client/mod.rs`, `crates/arena_game/src/server/mod.rs`

- [ ] **Step 1: Client sends `CastRequestMessage`** on the reliable `CastChannel` when the cast key fires (replacing M1's direct `cast_skill_at`): pack `skill_id`, the `target_hint` (nearest enemy's `ObeliskNetId`), and `aim_dir`.

- [ ] **Step 2: Server `drain_cast_requests`** (guide §5.3b) — drain on `CastChannel`, map `RemoteId`→caster via `ClientPlayerMap`, **re-validate server-side** (treat `target_hint` as a hint; re-acquire via `ObeliskSpatial::nearest_enemy`, check range/faction), then `cast_skill_at`. Obelisk's `validate_casts` gates the rest.

- [ ] **Step 3: Headless verify**

The observer sends a `CastRequestMessage`; the server emits `CastBegan`.
Expected: server log shows `CastBegan` for the requested cast.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M2.3: client cast_request -> server re-validate -> cast_skill_at

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 15: Egress bridge — NetEvent + CueEvent → wire messages

**Files:** Modify `crates/arena_game/src/server/mod.rs`, Create `crates/arena_game/src/skills.rs`

- [ ] **Step 1: NetEvent egress** (guide §5.5a): `egress_net_events` drains `MessageReader<obelisk_bevy::net::NetEvent>` and `send::<EventChannel>(NetEventMessage(ev.clone()))` to every `ClientOf` sender. (obelisk's `NetEvent` already uses stable string ids — wire-ready.)

- [ ] **Step 2: Cue egress** (guide §5.5b, §4): `register_server_cue_egress` registers `observe_cue` handlers on the server that convert `CueEvent { source: Entity, position, kind }` → `CueMessage { cue_id, source_id: ObeliskEntityIndex.id(source), position, kind }` and broadcast as `CueWireMessage` on `EventChannel`. (The server half of the M1 split; needs an `ObeliskEntityIndex` resource mapping Entity→ObeliskId, maintained server-side.)

- [ ] **Step 3: Headless verify**

Both observers receive `CastBegan` + `DamageResolved` (`NetEventMessage`) + a `CueWireMessage` for a cast.
Expected: trace shows both peers receive the combat events + cue.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M2.3: egress bridge — NetEvent + CueEvent -> lightyear messages

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 16: Client consumes CueWireMessage → cosmetics (+ de-dup)

**Files:** Modify `crates/arena_game/src/client/mod.rs`, `crates/arena_game/src/skills.rs`

- [ ] **Step 1: `register_client_cue_binding`** (guide §6.5, §4.3): consume replicated `CueWireMessage` → `resolve_cue(registry, &m)` → spawn cosmetics at `ObeliskEntityIndex.entity(&m.source_id)`'s socket/transform; de-dup by `source_id` (skip a replicated predicted-kind cue whose `source_id == local player` — the local predicted copy already played). Re-key `AimDirs` lookups by `source_id`.

- [ ] **Step 2: Windowed verify [W]** — firebolt cosmetics play on both clients; **[H]** the trace shows one cue per cast (de-dup works, not double-played for the caster).
Expected: VFX + projectile on both clients; no double cosmetic for the casting client.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.3: client consumes replicated cues -> cosmetics (+ source_id de-dup)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 17: Predicted cast initiation + projectile motion (no resolve/no RNG)

**Files:** Modify `crates/arena_game/src/client/mod.rs`

- [ ] **Step 1: `register_predicted_sim`** (guide §6.4, §2.3): on the client, run obelisk's **timeline + projectile** systems for the predicting player only (`validate_casts` optimistic + `advance_casts` + projectile motion), gated `With<Predicted>` / the local obelisk entity. **Hard-exclude `ObeliskSet::ResolveHits`, `Hitbox` spawn, and `CombatRng`** (Stage-A invariant; guide risk #2). The local cast fires its own `on_cast` + projectile cues immediately (a `LocalCue` stream feeding the same cosmetics consumer), reconciling its predicted `ActiveCast` against the replicated cast state; damage arrives from the server.

- [ ] **Step 2: Windowed verify [W]** — own windup animation + projectile start **instantly** on cast; the damage number/hp drop lands a few ms later (server-authoritative). Confirm the client never logs a local `DamageResolved`/RNG draw.
Expected: instant own-cast feedback; server-authoritative damage.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.3: predicted cast initiation + projectile motion (server-auth damage)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# M2.4 — HUD + best-of-3 rounds + late-joiner

### Task 18: NetworkedHealth mirror + HUD + floating damage

**Files:** Modify `crates/arena_game/src/server/mod.rs`, `crates/arena_game/src/client/mod.rs`

- [ ] **Step 1: Server hp mirror** (guide §5.6): each tick on change, mirror obelisk life (`ObeliskRead::life_of` + `StatBlock` max) → `NetworkedHealth { current, max }`. **Client HUD**: hp bars for own + opponent (bevy_ui `Node`s), driven by replicated `NetworkedHealth`; floating damage numbers + hit flash from `DamageResolved` (`NetEventMessage`), target looked up by `ObeliskNetId`.

- [ ] **Step 2: Windowed verify [W]** — hp bars drop on hit; damage numbers float; **[H]** `NetworkedHealth` replicates (trace).
Expected: HUD reflects server-authoritative hp.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.4: NetworkedHealth mirror + HUD bars + floating damage

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 19: Best-of-3 round state machine + respawn + reset

**Files:** Modify `crates/arena_game/src/server/mod.rs`, `crates/arena_game/src/net/protocol.rs`, `crates/arena_game/src/client/mod.rs`

- [ ] **Step 1: Server round machine** (guide §7): `RoundPhase` resource (`WaitingForPlayers`/`Countdown`/`Active`/`RoundOver{winner}`/`MatchOver{winner}`) + `RoundStateMessage` broadcast on `EventChannel`. Flow: wait for 2 `ClientOf` → countdown → `Active` (reset hp/cooldowns, respawn both at fixed spawn markers); a round ends on `EntityDied` (increment killer's wins); first to 2 → `MatchOver`; between rounds respawn + reset. Client renders the round/score state (a minimal HUD label + a countdown).

- [ ] **Step 2: Headless verify** — trace shows round transitions + best-of-3 resolves.
Run server + 2 observers scripting casts until one "dies"; confirm round increments and a `MatchOver` after 2 round wins.
Expected: `RoundStateMessage` transitions; best-of-3 resolves to `MatchOver`.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M2.4: best-of-3 round state machine + respawn + reset

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# M2.5 — trace harness + observer regression

### Task 20: Observer binary + the M2 net regression

**Files:** Create `crates/arena_game/src/bin/observer.rs`; Create `crates/arena_game/tools/net-test/run_session.sh`, `summarize.sh`

- [ ] **Step 1: Observer binary** (guide §8.2): copy `wisp/src/bin/observer.rs` as the template — a **headless** lightyear client that connects, receives replication, scripts `CastRequestMessage`s, and emits `trace::event` for every replicated `NetEventMessage`/`CueWireMessage`/`NetworkedPosition`. `ARENA_TRACE_SRC=client-0`/`client-1`.

- [ ] **Step 2: The regression harness** (guide §8.3): adapt wisp's `run_session.sh`/`summarize.sh` to launch `arena-server` + 2 `arena-observer`s (distinct trace srcs/files), script observer-0 to cast firebolt at observer-1, then assert over the merged JSONL:
  - server emits `CastBegan(caster=client-0, skill=firebolt)` and `DamageResolved(caster=client-0, target=client-1)`;
  - **both** observers receive a `CastBegan` and a `DamageResolved` for that cast;
  - the damage value both observers echo **matches the server's** (server-authoritative).

- [ ] **Step 3: Run the regression — fully headless**

Run: `bash crates/arena_game/tools/net-test/run_session.sh` (or the equivalent orchestration).
Expected: the assertions pass — 2 observers, firebolt, both see `CastBegan→DamageResolved`, damage matches the server. **This is the headless M2 gate proof.**

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M2.5: arena-observer + headless net regression (cast -> both see damage)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 21: M2 gate — windowed end-to-end confirmation

**Files:** none (verification + wrap)

- [ ] **Step 1: Full-stack gate.** Launch `arena-server` + two `arena-client` windows (the second can be driven by `arena-observer` for the headless half). Confirm the gate end-to-end:
  - two clients connect;
  - each casts firebolt (the other is targeted), both **see** the cast animation + VFX + projectile;
  - **server-authoritative damage** lands (hp bars drop on both);
  - own movement is **client-predicted** (instant);
  - **best-of-3 rounds resolve** (score updates, `MatchOver` on first-to-2).
  Capture a windowed screenshot of a cast-in-flight with the HUD for the record (`Read` it).

- [ ] **Step 2: Full workspace green**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`
Expected: clean (arena crates; stat_core dep warnings are cap-lints'd).

- [ ] **Step 3: Final commit**

```bash
git add -A && git commit -m "M2 GATE: 2-player online firebolt duel, server-auth damage, predicted movement, best-of-3

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage** (spec §5/§6.1-6.2/§7/§9/§10 M2):
- Dedicated server-auth loop (§5.1) → Tasks 7, 13; server pipeline (§5) → 10, 14, 15, 18. ✓
- CueMessage serde / cue contract (§4 / spec §4.3) → Tasks 1–4. ✓
- Replicated pose + cast state (§7) → Tasks 10, 11; NetworkedHealth (§5.6) → 18. ✓
- Movement prediction (§6.1, §6.3) → Task 12; predicted cast+projectile, server-auth damage (§6.2) → 17; cue de-dup (§5.3) → 16. ✓
- cast_request reliable + server re-validate (§5.2) → Task 14. ✓
- Best-of-3 rounds + respawn + late-joiner (spec §2, §5.7) → Tasks 9, 19. ✓
- Net trace regression (§9) → Tasks 20, 21. ✓
- Stage-A invariant (no client RNG/resolve) → Task 17 hard-gate + risk #2. ✓

**2. Placeholder scan:** The netcode tasks deliberately instruct *verify the exact 0.26 API against the installed lightyear source + wisp* rather than fabricate unproven verbatim code — this is honest (the guide flags the prediction path as unproven in this lineage) and each such task names the registry-source authority + the wisp copy-target with line ranges. The well-understood tasks (M2.0 CueMessage, protocol, egress) carry full code. No silent TBDs.

**3. Type/name consistency:** `CueMessage {cue_id, source_id, position, kind}` (Task 1) is used identically in the registry (2), helpers (3), egress (15), and consumer (16). `SkillFxRegistry`/`resolve_cue` consistent across 2/3/16. `NetEventMessage`/`CueWireMessage`/`CastRequestMessage`/`PlayerInputMessage`/`RoundStateMessage`/`NetworkedPosition`/`NetworkedHealth`/`ObeliskNetId`/`NetworkOwner` consistent between the protocol (6) and their users. `Replicate::manual` (not `to_clients`) used for all player replication (9). `EventChannel` reliable for combat/cues/rounds throughout.

**4. Scope:** M2 only (Stage A). Stage-B predicted damage + `resolve_seeded` is M3 (explicitly excluded; risk #2). One plan, six staged sub-milestones, 21 tasks each ending in a build/run/test check, tagged headless vs windowed. The headless net regression (Task 20) is the objective gate; Task 21 is the windowed confirmation.
