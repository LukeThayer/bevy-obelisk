# M2 Netcode Integration-Surface Guide

**Date:** 2026-06-25
**Status:** Integration-surface reference for the M2 implementation plan.
**Scope:** The exact, load-bearing surface a detailed M2 plan needs — real APIs, real file:line copy targets, real code skeletons — for obelisk-arena's 2-player online, dedicated-server, server-authoritative, **Stage-A-prediction** milestone (spec §5, §6.1–6.2, §9, §10 M2).
**Parent spec:** `docs/superpowers/specs/2026-06-25-obelisk-arena-phase1-design.md`.
**Primary integration sources:** wisp (`/Users/luke/src/wisp`, lightyear 0.26 reference), obelisk-bevy (`/Users/luke/src/obelisk-bevy`, the sim), obelisk-arena M1 (`/Users/luke/src/obelisk-arena`, the co-located base).

> This is plan *input*, not the plan. It pins the facts that are expensive to re-derive (the lightyear 0.26 API shapes, the co-located→split refactor, the CueMessage serde rewrite) and decomposes M2 into ordered, individually-verifiable tasks. The plan author turns §9 into batches.

**M2 gate (the definition of done):** *two clients connect to a dedicated headless server; each casts firebolt; both see the cast + server-authoritative damage; movement is client-predicted; best-of-3 rounds resolve.*

---

## 1. lightyear 0.26 baseline (get the API right — this is the load-bearing fact)

### 1.1 Confirmed version + features (from wisp's `Cargo.toml`, verified)

```toml
# arena_game/Cargo.toml — mirror wisp exactly
avian3d  = { version = "0.5", features = ["default", "serialize"] }   # 0.5 PINNED, see gotcha
bevy     = "0.18"
lightyear = { version = "0.26.4", features = ["netcode", "udp", "avian3d", "interpolation"] }
serde    = { version = "1", features = ["derive"] }
serde_json = "1"
ron      = "0.10"   # for .skillfx.ron + arena scene if RON-loaded
```

**Hard version facts (do not re-derive):**
- **lightyear `0.26.4`**, features `netcode` + `udp` + `avian3d` + `interpolation`. wisp also lists `input_bei` — **drop it for the arena**: native bei input replication is blocked by a bei version mismatch (wisp 0.25 / lightyear_inputs_bei 0.22). The arena hand-rolls input messages, so the feature is dead weight.
- **avian3d `0.5` is PINNED** by `lightyear_avian3d 0.26.x`. Cargo treats 0.5 and 0.6 as separate crates; you cannot have both. Do not bump avian to 0.6 until lightyear bumps its dep. (Note obelisk-bevy is already on Bevy 0.18 / Avian 0.5 per its Phase-0 carry, so this is consistent across the workspace.)
- **Bevy `0.18`** across the workspace.

### 1.2 The real 0.26 API names (the surface M2 calls)

These are the confirmed names — wrong guesses here cost the most. All from `lightyear::prelude` unless noted.

| Concern | API | Notes |
|---|---|---|
| Server plugin group | `ServerPlugins { tick_duration: Duration }` (`lightyear::prelude::server::ServerPlugins`) | `tick_duration = 1.0/60.0`. |
| Client plugin group | `ClientPlugins { tick_duration: Duration }` (`lightyear::prelude::client::ClientPlugins`) | Same `tick_duration` — mismatch = desync. |
| Register replicated component | `app.register_component::<C>()` → builder (`AppComponentExt`) | Chain `.add_prediction()`, `.add_should_rollback(fn)`, `.add_interpolation_with(fn)`, `.add_linear_interpolation()`, `.with_replication_config(cfg)`. |
| Register message | `app.register_message::<M>().add_direction(NetworkDirection::ClientToServer\|ServerToClient)` (`AppMessageExt`) | Unidirectional unless both directions registered. |
| Register channel | `app.add_channel::<C>(ChannelSettings{..}).add_direction(..)` (`AppChannelExt`) | `C` is a unit-struct tag (`pub struct InputChannel;`). |
| Channel modes | `ChannelMode::{UnorderedUnreliable, UnorderedReliable, Ordered, OrderedReliable}` | Unreliable for per-tick state; Reliable for one-shots (cast_request, round state). |
| Prediction enable | `.add_prediction()` + `.add_should_rollback(\|pred, conf\| bool)` | Must be paired — prediction w/o rollback handler panics on misprediction. |
| Interpolation | `.add_interpolation_with(\|start, end, t\| C)` or `.add_linear_interpolation()` | Custom lerp for non-linear types (yaw wrap); linear for Add/Sub/Mul types. |
| Replication gating | `.with_replication_config(ComponentReplicationConfig { disable: true, ..default() })` | Registered-but-not-replicated until per-entity override. wisp's avian-physics migration lever. |
| Replicate (late-join safe) | `Replicate::manual(senders: Vec<Entity>)` | Explicit sender list, refreshed on connect/disconnect. **Use this everywhere.** |
| Replicate (broadcast) | `Replicate::to_clients(NetworkTarget::All\|Single(PeerId)\|None)` | Snapshots senders at insert → **breaks late joiners.** Avoid except smoke tests. |
| Message I/O (send) | `Single<&mut MessageSender<M>>`; `sender.send::<Channel>(msg)` | Query as `Single` — multiple panics. |
| Message I/O (recv) | `Query<&mut MessageReceiver<M>>`; `for m in receiver.receive() {}` | `.receive()` drains since last call. |
| Markers (filter) | `Replicated` (arrived via replication), `Predicted` (client predicted replica), `Interpolated` (interpolated replica) | Use in `Query` filters; only `Replicated`/`Predicted`/`Interpolated` are added by lightyear. |
| Peer ids | `LocalId(PeerId)` (own), `RemoteId(PeerId)` (on `ClientOf` entities) | `PeerId::{Netcode\|Steam\|Local\|Entity}(u64)`; match all variants → `u64`. |
| Connection lifecycle | `Connected` (handshake done), `ClientOf` (server-side client entity), `LinkOf` (transport link) | `lightyear::prelude::server::ClientOf`; `lightyear::prelude::{Connected, LinkOf}`. |
| Transport | server: `NetcodeServer::new(cfg)` + `ServerUdpIo` + `LocalAddr`; client: `NetcodeClient::new(auth, cfg)` + `UdpIo` + `LocalAddr` + `PeerAddr` + `ReplicationReceiver` | Trigger `LinkStart{entity}` (server) / `Connect{entity}` (client) to open sockets. |
| Auth (dev) | `Authentication::Manual { server_addr, client_id, private_key, protocol_id }` (`lightyear::netcode::prelude::Authentication`) | Dev only: `protocol_id=0`, `key=[0u8;32]`. Real deploy needs a backend token service. |
| avian bridge | `LightyearAvianPlugin { replication_mode: AvianReplicationMode::Position }` (`lightyear::avian3d::plugin`) | Disables `PhysicsTransformPlugin`/`PhysicsInterpolationPlugin`/`IslandPlugin`/`IslandSleepingPlugin`; lightyear owns Transform↔Position sync. |

**0.26 traps that bite (carry these into the plan):**
- `register_component` interpolation lerp runs **every frame** — must be allocation-free.
- `add_should_rollback` is `>=` thresholded (`(pred-conf).length() >= 0.01`), not exact equality. Don't make it reflexive (comparing a value to itself must not always return false in a way that suppresses real rollbacks).
- Channels need **both** `.add_direction` calls to be bidirectional; one direction silently drops the other.
- Under `LightyearAvianPlugin::Position`: **write avian `Position`/`Rotation`, never `Transform`** on physics bodies — the per-tick sync clobbers Transform writes.
- **Component-update replication after insert was unreliable in wisp's setup.** Initial spawn via component works; *per-tick mutation* of an already-replicated component may not propagate. wisp's workaround: push per-tick state as **Messages** (e.g. `BeamCastBroadcast`) not component mutations. **This directly shapes M2's combat-event design (§3, §5).** The `NetworkedPosition` per-tick stream *does* work via the interpolation path, so position is fine; treat discrete combat events as messages.
- Rollback errors are **silent**; debug with `RUST_LOG=lightyear=debug`.

---

## 2. Server/client app split (co-located M1 → dedicated server + windowed client)

### 2.1 Binary structure — RECOMMENDATION: mirror wisp's two-binary layout

wisp ships **separate `client` and `server` binaries** sharing a `lib`, with a third `observer` test binary. Adopt the same for `arena_game`:

```toml
# crates/arena_game/Cargo.toml
[lib]
name = "arena_game"
path = "src/lib.rs"

[[bin]]
name = "arena-server"
path = "src/bin/server.rs"     # headless MinimalPlugins; runs ObeliskSimPlugin + CombatRng + net authority

[[bin]]
name = "arena-client"
path = "src/bin/client.rs"     # windowed DefaultPlugins; present + cosmetics + prediction
default-run = "arena-client"

[[bin]]
name = "arena-observer"        # M2.5 — scripted headless trace client (copy wisp/src/bin/observer.rs)
path = "src/bin/observer.rs"
```

**Why two bins, not one bin with `--server`/`--client`:** the server is `--no-default-features`-class (obelisk-bevy presentation compiled out; `MinimalPlugins`) and must *not* pull rendering; the client is `DefaultPlugins` + windowing. A single bin would have to compile both worlds and gate at runtime, fighting Bevy's plugin-group ergonomics and bloating the server binary. wisp proved the two-bin split; copy it. (A single bin is technically possible via feature flags, but the two-bin form is the lower-risk, already-validated path.)

The shared `lib.rs` holds: `ProtocolPlugin`, `ServerNetPlugin`, `ClientNetPlugin`, `add_avian_with_lightyear`, the net constants (`TICK_HZ=60`, `PROTOCOL_ID=0`, `NETCODE_KEY=[0u8;32]`, `default_server_addr`, `parse_addr_args`), and the copied `trace` module. **Copy `wisp/src/net/mod.rs:25-104` verbatim** for the constants + arg parser.

### 2.2 Server app skeleton (`src/bin/server.rs`)

```rust
// Headless authority. Copy structure from wisp/src/bin/server.rs:10-53.
use bevy::log::LogPlugin;
use bevy::mesh::MeshPlugin;
use bevy::prelude::*;
use bevy::scene::ScenePlugin;
use obelisk_bevy::prelude::*;       // ObeliskSimPlugin, ObeliskConfigExt, NetEvent, ...

fn main() {
    let root = arena_game::arena_root();
    let mut app = App::new();
    // Avian's collider cache reads AssetEvent<Mesh> + SceneSpawner; these
    // registrations are required even though the server renders nothing.
    app.add_plugins((
        MinimalPlugins, LogPlugin::default(), AssetPlugin {
            file_path: root.join("assets").to_string_lossy().into_owned(), ..default()
        }, TransformPlugin, MeshPlugin, ScenePlugin,
    ));
    app.insert_resource(Time::<Fixed>::from_hz(60.0));

    // 1. lightyear server stack (adds ServerPlugins { 1/60 } + ProtocolPlugin)
    app.add_plugins(arena_game::net::ServerNetPlugin);
    let bind = arena_game::net::parse_addr_args(arena_game::net::default_server_addr());
    app.insert_resource(arena_game::net::server::ServerBind { addr: bind });

    // 2. Physics AFTER ServerPlugins so LightyearAvianPlugin sees replication infra.
    arena_game::add_avian_with_lightyear(&mut app);

    // 3. Obelisk sim (HEADLESS — no `present`). Authority for combat.
    app.add_plugins(ObeliskSimPlugin);
    app.add_obelisk_config_constants_default();
    app.add_obelisk_effects(&root.join("config/effects"));
    app.add_obelisk_skills(SkillSource::Dir(root.join("config/skills")));
    // CombatRng seeded from the session seed (replicated to clients as match_seed, §5).
    let seed = arena_game::net::session_seed();   // env ARENA_MATCH_SEED or fixed for tests
    app.seed_combat_rng(seed);

    // 4. arena_skills CUE REGISTRATION lives HERE (server fires cues, egress → wire).
    //    NOTE: server does NOT spawn cosmetics; it converts CueEvent → CueMessage.
    arena_game::skills::register_server_cue_egress(&mut app);  // see §4/§5

    // 5. Arena scene + round state machine + server controllers + egress bridge.
    app.add_plugins(arena_game::server::ArenaServerPlugin);   // setup_arena, spawn markers, rounds
    app.run();
}
```

### 2.3 Client app skeleton (`src/bin/client.rs`)

```rust
// Windowed present. Copy structure from wisp/src/bin/client.rs:9-42.
use bevy::prelude::*;
use obelisk_bevy::prelude::*;

fn main() {
    let root = arena_game::arena_root();
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "obelisk-arena — client".into(), ..default() }),
        ..default()
    }).set(AssetPlugin {
        file_path: root.join("assets").to_string_lossy().into_owned(), ..default()
    }));
    app.insert_resource(Time::<Fixed>::from_hz(60.0));

    // Gameplay/present plugins shared with M1: controller, rig/anim, cosmetics, trace.
    app.add_plugins(arena_game::client::ArenaClientPlugin);   // controller + rig + cosmetics + HUD

    // lightyear client stack (adds ClientPlugins { 1/60 } + ProtocolPlugin)
    app.add_plugins(arena_game::net::ClientNetPlugin);
    let server_addr = arena_game::net::parse_addr_args(arena_game::net::default_server_addr());
    let client_id = app.world().resource::<arena_game::net::client::ConnectTo>().client_id;
    app.insert_resource(arena_game::net::client::ConnectTo { server: server_addr, client_id });

    // Physics after ClientPlugins (same reason as server).
    arena_game::add_avian_with_lightyear(&mut app);

    // CLIENT cue binding: from REPLICATED CueMessage, re-looking up LaneEvent (§4).
    arena_game::skills::register_client_cue_binding(&mut app);

    // Client runs obelisk's TIMELINE + PROJECTILE systems for the PREDICTED player only,
    // but NO hit resolution and NO CombatRng (Stage A, §6). See §6 for the gating.
    arena_game::client::register_predicted_sim(&mut app);

    app.run();
}
```

**What moves where (the M1→M2 decomposition of `main.rs`):**

| M1 (`arena_game/src/main.rs`, co-located) | M2 destination |
|---|---|
| `ObeliskSimPlugin` + `seed_combat_rng` + skills/effects load | **Server only.** Authority. |
| `register_cues_synchronously` (`App::observe_cue` at build) | **Split:** server registers cue→`CueMessage` egress; client registers `CueMessage`→cosmetics binding. (§4 — this is M2.0.) |
| `cast_on_input` (`commands.entity(player).cast_skill_at`) | **Split:** client sends `cast_request` message; server validates + `cast_skill_at`. (§5.2.) |
| `spawn_cue_cosmetics`, `fly_cosmetic_projectiles`, `age_lifetimes` | **Client only** (present). |
| `ArenaControllerPlugin` (writes Transform directly) | **Client (predicted) + Server (authoritative controller).** Rebuild on avian `Position`. (§6.) |
| `AimDirs` (HashMap<Entity, Vec3>) | **Client only**, re-keyed by `ObeliskId` (§4 gotcha). |
| `TracePlugin` | **Both** processes, distinct `ARENA_TRACE_SRC`. |

---

## 3. Protocol (`crates/arena_game/src/net/protocol.rs`)

Shared, identical on server and client. Copy the *shape* from `wisp/src/net/protocol.rs:23-160`.

### 3.1 Replicated components

```rust
use lightyear::prelude::*;
use avian3d::prelude::{Position, Rotation, LinearVelocity};
use serde::{Serialize, Deserialize};

pub struct ProtocolPlugin;
impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // --- Player identity / ownership (replicated on spawn) ---
        app.register_component::<NetworkedPlayer>();
        app.register_component::<NetworkOwner>();      // NetworkOwner(u64) == client_id (PeerId)
        app.register_component::<NetworkedId>();        // NetworkedId(u64) monotonic, cross-peer stable
        app.register_component::<ObeliskNetId>();       // wraps ObeliskId String (stable combat id)

        // --- Pose (the per-tick stream). Hand-rolled because Transform doesn't serde. ---
        // EXTENDED past wisp: carries obelisk cast state for remote cast animation (spec §7).
        app.register_component::<NetworkedPosition>()
            .add_interpolation_with(lerp_networked_position);

        // --- Health (replicated; HUD source of truth, server-authoritative). ---
        app.register_component::<NetworkedHealth>()     // { current: f64, max: f64 }
            .add_interpolation_with(lerp_health);        // or plain register if snapping is fine

        // --- avian physics for the PREDICTED local player (Stage A movement). ---
        // Registered-but-disabled by default; per-entity Replicate enables on the local rig.
        let disable = ComponentReplicationConfig { disable: true, ..default() };
        app.register_component::<Position>()
            .with_replication_config(disable.clone())
            .add_prediction()
            .add_should_rollback(position_should_rollback)
            .add_linear_interpolation();
        app.register_component::<Rotation>()
            .with_replication_config(disable.clone())
            .add_prediction()
            .add_should_rollback(rotation_should_rollback)
            .add_linear_interpolation();
        app.register_component::<LinearVelocity>()
            .with_replication_config(disable)
            .add_prediction()
            .add_should_rollback(velocity_should_rollback)
            .add_linear_interpolation();

        // --- Channels ---
        app.add_channel::<InputChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedUnreliable,        // movement: latest wins
            send_frequency: Default::default(),
            priority: 1.0,
        }).add_direction(NetworkDirection::ClientToServer);

        app.add_channel::<CastChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedReliable,          // cast_request: never drop
            send_frequency: Default::default(),
            priority: 1.0,
        }).add_direction(NetworkDirection::ClientToServer);

        app.add_channel::<EventChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedReliable,          // combat events + cues + round state: reliable
            send_frequency: Default::default(),
            priority: 1.0,
        }).add_direction(NetworkDirection::ServerToClient);

        // --- Messages ---
        app.register_message::<PlayerInputMessage>()
            .add_direction(NetworkDirection::ClientToServer);     // movement, on InputChannel
        app.register_message::<CastRequestMessage>()
            .add_direction(NetworkDirection::ClientToServer);     // cast, on CastChannel
        app.register_message::<NetEventMessage>()
            .add_direction(NetworkDirection::ServerToClient);     // wraps obelisk NetEvent (§5)
        app.register_message::<CueWireMessage>()
            .add_direction(NetworkDirection::ServerToClient);     // wraps arena_skills CueMessage (§4)
        app.register_message::<RoundStateMessage>()
            .add_direction(NetworkDirection::ServerToClient);     // best-of-3 round flow (§7)
    }
}
```

### 3.2 Message + component payloads

```rust
// Movement input — copy wisp/src/net/protocol.rs:164-186 (drop fields the arena doesn't use).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerInputMessage {
    pub movement: [f32; 2],   // camera-relative WASD axis
    pub yaw: f32,             // camera yaw (radians)
    pub pitch: f32,           // aim pitch (spine lean; cosmetic)
    pub jump: bool,
}

// Cast request (NEW — replaces M1's direct cast_skill_at on the local entity, spec §5.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CastRequestMessage {
    pub skill_id: String,
    pub target_hint: Option<u64>,   // ObeliskNetId of intended target (a HINT; server re-acquires)
    pub aim_dir: [f32; 3],          // for direction-targeted skills + cosmetic projectile aim
}

// Combat events on the wire — wrap obelisk's NetEvent so the server can broadcast verbatim.
// obelisk's NetEvent already uses STABLE STRING IDS (not Entity) — perfect for the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetEventMessage(pub obelisk_bevy::net::NetEvent);

// Pose component — EXTENDED with obelisk cast state (spec §7 "replicate the full ActiveCast").
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkedPosition {
    pub x: f32, pub y: f32, pub z: f32,
    pub yaw: f32, pub pitch: f32,
    pub airborne: bool,
    // cast state for remote cast-animation (Phase/elapsed/skill drive the upper-body layer):
    pub cast_phase: u8,        // 0 none, 1 windup, 2 active, 3 recovery
    pub cast_elapsed: f32,
    pub cast_skill: u16,       // interned skill index (skill_id table) or 0 = none
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkedHealth { pub current: f64, pub max: f64 }
```

**Channel rationale (per spec §5.2 + the 0.26 component-update unreliability gotcha §1.2):**
- Movement → **UnorderedUnreliable** (latest-wins, per-tick).
- `cast_request` → **UnorderedReliable** (a dropped cast is unacceptable).
- Combat events (`NetEvent`) + cues + round state → **UnorderedReliable** as **messages, not component mutations** (the 0.26 mutation-propagation gotcha forces this; matches wisp's `BeamCastBroadcast` pattern).
- Pose (`NetworkedPosition`) → replicated component via the **interpolation** path (this works in 0.26).

---

## 4. The CueMessage serde refactor (M2.0 — the opening task that unblocks everything)

This is the **first** M2 task because nothing else can go on the wire until cues are serializable. M1 embeds a full `LaneEvent` and captures closures — neither survives the network.

### 4.1 Before (M1, `crates/arena_skills/src/lib.rs:94-101`, `:174-196`)

```rust
// BEFORE — not serde, embeds LaneEvent, keyed by local Entity, registered via build-time closure.
#[derive(Message, Debug, Clone)]               // bevy Message, NOT serde
pub struct CueMessage {
    pub lane_id: String,
    pub kind: CueKind,
    pub source: Entity,                          // LOCAL ENTITY — meaningless across peers
    pub position: Vec3,
    pub event: LaneEvent,                        // ENTIRE authoring spec embedded
}

pub fn register_skill_cues(app: &mut App, fxs: &[SkillFx]) {
    for fx in fxs { for (cue_id, lane) in &fx.lanes {
        let lane = lane.clone();
        app.observe_cue(cue_id.clone(), move |cue: &CueEvent, commands: &mut Commands| {
            commands.write_message(CueMessage { /* captures lane.clone() — can't serialize a closure */ });
        });
    }}
}
```

### 4.2 After (M2 — serde wire type + registry lookup)

```rust
// AFTER — plain serde, stable id, NO embedded spec. arena_game wraps it as a lightyear message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CueMessage {
    pub cue_id: String,            // e.g. "on_cast", "on_window_bolt", "on_hit"
    pub source_id: String,         // ObeliskId (stable string) — NOT Entity (spec §4.2 wire shape)
    pub position: Vec3,            // Vec3 is serde-serializable under bevy's serialize feature
    pub kind: CueKind,             // serde enum (OnCast/OnWindow/OnHit)
}

// The registry resource: cue_id (or skill_id+cue_id) -> [LaneEvent], loaded from .skillfx.ron.
#[derive(Resource, Default)]
pub struct SkillFxRegistry {
    pub by_cue: HashMap<String, Vec<LaneEvent>>,   // keyed on the locked cue contract (spec §4.3)
}
impl SkillFxRegistry {
    pub fn load_dir(dir: &Path) -> Self { /* read every *.skillfx.ron, flatten bindings */ }
    pub fn lanes(&self, cue_id: &str) -> Option<&[LaneEvent]> { self.by_cue.get(cue_id).map(|v| &v[..]) }
}
```

### 4.3 Consumer re-looks-up `LaneEvent` by `cue_id`

```rust
// arena_game CLIENT: consume the REPLICATED CueWireMessage, resolve LaneEvent from the registry.
fn spawn_cue_cosmetics(
    mut msgs: MessageReceiver<CueWireMessage>,       // replicated wire cues
    mut local_cues: MessageReader<LocalCueMessage>,  // predicted local cues (own player, §6)
    registry: Res<SkillFxRegistry>,
    aim: Res<AimDirs>,                                // NOW keyed by ObeliskId (String)
    index: Res<ObeliskEntityIndex>,                  // ObeliskId -> Entity for socket/transform lookup
    mut commands: Commands,
) {
    for CueWireMessage(m) in msgs.receive() {
        let Some(lanes) = registry.lanes(&m.cue_id) else {
            warn!("cue {} has no skillfx binding — no-op", m.cue_id); continue;   // spec §12: never crash
        };
        for lane in lanes {
            if let Some(p) = lane.particle() { /* spawn particle at socket on index.entity(&m.source_id) */ }
            if let Some(proj) = lane.projectile() {
                let dir = aim.0.get(&m.source_id).copied().unwrap_or(Vec3::Z);     // keyed by ObeliskId now
                /* spawn cosmetic projectile flying `dir` */
            }
        }
    }
    // ... local predicted cues handled identically, de-duped by source_id (§6) ...
}
```

### 4.4 The three breakages this refactor must fix (from M1 gotchas)

1. **Embedded `LaneEvent` → registry lookup.** You cannot serialize closures or embed the full spec on the wire. The consumer re-queries `SkillFxRegistry` by `cue_id`.
2. **`source: Entity` → `source_id: ObeliskId` (String).** Replicated entity ids differ per peer; the stable obelisk id is the only thing both ends agree on.
3. **`AimDirs<Entity, Vec3>` → `AimDirs<String, Vec3>` keyed by `ObeliskId`.** Populated by *both* the local-predicted cast (own player) and replicated cues. If a replicated cue's `source_id` is missing from the map, the consumer recomputes aim from the replicated transform delta rather than defaulting to `Vec3::Z`.

**Verdict:** **mandatory and first.** It is small (a struct rewrite + a registry resource + the consumer split), it is independently testable headlessly (round-trip a `CueMessage` through serde, look up a `LaneEvent`), and **every other M2 wire task depends on it**. `arena_skills` stays lightyear-free (spec §3) — `arena_game` owns the lightyear `CueWireMessage` wrapper.

---

## 5. Server pipeline (`crates/arena_game/src/server/`)

The dedicated server is the sole authority. Per `FixedUpdate` tick + an Update egress bridge.

### 5.1 Connection + spawn (copy wisp)

- **`on_new_link`** (observer on `Add<LinkOf>`): attach `ReplicationSender::default()` to each new link, else replication doesn't flow. Copy `wisp/src/net/server.rs:171-176`.
- **`spawn_server`**: `NetcodeServer::new(NetcodeConfig::default().with_protocol_id(0).with_key([0u8;32]))` + `ServerUdpIo` + `LocalAddr(bind)`, then `trigger(LinkStart{entity})`. Copy `wisp/src/net/server.rs:154-167`.
- **`sync_networked_players`** (polling, not observer — avoids Replicate on-insert timing): one `NetworkedPlayer` per connected `ClientOf`. Spawn with avian `RigidBody` + collider, **and** `make_combatant(StatBlock::with_id(...))` + `Faction::Player` + `grant_skill("firebolt")` + `Hurtbox` + `NetworkedHealth` + `NetworkOwner(client_id)` + `NetworkedId` + `ObeliskNetId(obelisk_id)`. Position-spread by id at the round spawn markers. `Replicate::manual(current_senders)`. Adapt `wisp/src/net/server.rs:284-360`.

### 5.2 CombatRng seeded from the session seed

```rust
// match_seed is the session seed: env ARENA_MATCH_SEED for deterministic tests, else wall-clock.
// Server calls app.seed_combat_rng(match_seed) ONCE at build (server.rs §2.2).
// match_seed is REPLICATED to clients (a resource broadcast or RoundStateMessage field) so Stage B
// (M3) can reproduce rolls — in Stage A clients never touch CombatRng, so this is forward-prep only.
```
CombatRng is **server-only** (obelisk-bevy CLAUDE.md: never `thread_rng()` in combat paths; clients do not reseed or run `resolve_one_hit` in Stage A).

### 5.3 Input → cast_request → Validate → cast_skill_at (spec §5.2)

```rust
// (a) drain movement — copy wisp drain_player_inputs (server.rs:414-444) into per-player PlayerInputState.
// (b) drain cast requests on the RELIABLE CastChannel:
fn drain_cast_requests(
    mut receivers: Query<(&RemoteId, &mut MessageReceiver<CastRequestMessage>), With<ClientOf>>,
    client_map: Res<ClientPlayerMap>,        // client_id -> player Entity (wisp pattern)
    spatial: ObeliskSpatial,                  // server re-acquires the real target
    players: Query<(&Transform, &Faction)>,
    index: Res<ObeliskEntityIndex>,
    mut commands: Commands,
) {
    for (RemoteId(peer), mut rx) in &mut receivers {
        let client_id = peer_to_u64(peer);
        let Some(&caster) = client_map.0.get(&client_id) else { continue };
        for req in rx.receive() {
            // RE-VALIDATE server-side (spec §5.2): treat target_hint as a hint, re-acquire by spatial.
            let (tf, faction) = players.get(caster).unwrap();
            let target = req.target_hint
                .and_then(|nid| index_lookup(nid))
                .filter(|&t| /* still in range + correct faction */ true)
                .or_else(|| spatial.nearest_enemy(tf.translation, 20.0, *faction));
            let Some(target) = target else { continue };
            commands.entity(caster).cast_skill_at(req.skill_id.clone(), target);  // → PendingCast
        }
    }
}
```
Obelisk's `validate_casts` (FixedUpdate `ObeliskSet::Validate`) then gates range/LOS/mana/cooldown/already-casting and emits `CastBegan` or `CastRejected`. **Clients never validate** — they send a request, the server is authoritative.

### 5.4 Movement controller (server-authoritative, FixedUpdate)

Copy wisp's `apply_player_rotation` (server.rs:453-459, writes avian `Rotation`) → `run_player_controller` (server.rs:461-514, applies forces in FixedUpdate, ground-raycast, jump on rising edge). **Adapt to the arena's third-person camera-relative movement** (spec §7). Both write avian components, not Transform.

### 5.5 Egress bridge (Update) — obelisk events + cues → lightyear messages

```rust
// (a) NetEvent egress: obelisk's ObeliskNetPlugin already mirrors gameplay events into a buffered
//     MessageReader<NetEvent> stream with STABLE STRING IDS. Wrap + broadcast to all clients.
fn egress_net_events(
    mut net: MessageReader<obelisk_bevy::net::NetEvent>,
    mut senders: Query<&mut MessageSender<NetEventMessage>, With<ClientOf>>,
) {
    for ev in net.read() {
        for mut s in &mut senders { let _ = s.send::<EventChannel>(NetEventMessage(ev.clone())); }
    }
}

// (b) Cue egress: server has CueEvent observers (register_server_cue_egress, §2.2) that convert
//     obelisk CueEvent { source: Entity, position, kind } -> CueMessage { cue_id, source_id, position, kind }
//     via ObeliskEntityIndex.id(entity), then broadcast as CueWireMessage on EventChannel.
//     (This is the SERVER half of the M1 register_skill_cues split.)
```

### 5.6 Transform → replicated pose + hp → NetworkedHealth

- **`sync_player_positions`** (Update, `Changed<Transform>`): copy avian-integrated `Transform` → `NetworkedPosition`. Derive `airborne` server-side (raycast, exclude self — never trust client). **Extend** to stamp `cast_phase`/`cast_elapsed`/`cast_skill` from the player's `ActiveCast` (spec §7 replication fidelity). Adapt `wisp/src/net/server.rs:519-551`.
- **hp → `NetworkedHealth`**: mirror obelisk life (`ObeliskRead::life_of` + `StatBlock` max) into `NetworkedHealth { current, max }` each tick on change.

### 5.7 Late-joiner refresh (copy wisp verbatim)

`refresh_replicate_on_connect` (`wisp/src/net/server.rs:208-232`): poll connected `ClientOf` count; on delta, re-insert `Replicate::manual(current_senders.clone())` on every `NetworkedPlayer`. Required so the second client receives the first. **Copy as-is.**

---

## 6. Client pipeline (`crates/arena_game/src/client/`)

### 6.1 Connect + receive

- **`spawn_client`** + `ConnectTo`: copy `wisp/src/net/client.rs:75-108`. `Authentication::Manual`, bind `LocalAddr(0.0.0.0:0)`, `PeerAddr(server)`, `ReplicationReceiver`, `trigger(Connect{entity})`. `client_id` from `ARENA_CLIENT_ID` env (test-pin) or wall-clock nanos.
- Read `LocalId` on the `Connected` entity to learn own `client_id` → match against replicated `NetworkOwner` to find the local player.

### 6.2 Interpolate everything remote (Stage A: predict own, interpolate all else)

Copy wisp's `net/replication.rs` smoothing for **remote** players + props/cosmetics:
- `NetworkedPositionSmoothing` buffer (replication.rs:82-96), `capture_networked_position_samples` observer on `Changed<NetworkedPosition>` (replication.rs:101-146, teleport-snap >3m), `smooth_networked_transforms` per-frame render lerp at `now - span` one-tick delay (replication.rs:163-217). Writes **both** Transform and avian `Position`/`Rotation` so they don't fight.
- The `NetworkedPosition` `add_interpolation_with(lerp_networked_position)` handles yaw-wrap; extend the lerp to carry `cast_phase`/`cast_elapsed` (snap discrete phase at `t>=0.5`, lerp elapsed) so remote cast animation reads cleanly.

### 6.3 Predict own movement (FINISH wisp's disabled Position rollback — Stage A)

This is the M2.2 deliverable. wisp has Position/Rotation/LinearVelocity registered with `.add_prediction()` + `.add_should_rollback()` but `disable: true` and **never enables them** (Stage Q deferred). M2 **enables them on the local player**:
1. Per-entity, enable replication of `Position`/`Rotation`/`LinearVelocity` on the local player's predicted replica (override the `disable: true` default — the local rig gets a `Replicate` that opts these in).
2. Mark the local player `Predicted`; run the **same movement controller** (§5.4) on the client gated `With<Predicted>` for zero-latency feedback.
3. lightyear rolls back + replays from `position_should_rollback` (`>=0.01m`) when the server's authoritative `Position` arrives mismatched.

**Stage-A simplification (spec §6.1):** if full rollback is risky to land, the fallback is wisp's "run the same controller locally for snappiness, snap to server `NetworkedPosition` each frame" (the `sync_local_player_from_server` reconciliation, `player/controller.rs:92-123` — a dumb snap, acceptable at low latency). The plan should attempt real rollback (spec calls it "low risk, finish and enable it") and keep the snap as the fallback.

### 6.4 Predict own cast initiation + projectile motion (Stage A, spec §6.2)

- Client runs obelisk's **timeline + projectile** systems for the **predicting player only** (`register_predicted_sim`, §2.3): `validate_casts` (local castability pre-check, optimistic) + `advance_casts` (phases) + projectile motion — **but NO `ResolveHits`, NO `Hitbox` spawn, NO `CombatRng`** (spec §6.2: clients never resolve, never touch RNG in Stage A). Gate these systems `With<Predicted>` / on the local player's obelisk entity.
- The local cast fires its own `on_cast` + projectile-spawn cues from the predicted sim for instant feedback (a `LocalCueMessage` stream, §4.3 consumer).
- **Hit resolution + damage are server-authoritative** — the damage number/flash arrives via replicated `NetEventMessage(DamageResolved)` a few ms later. The client reconciles its predicted `ActiveCast` against the replicated cast state.

### 6.5 Cue de-dup by `source_id` (spec §5.3 — the predicted-vs-replicated rule)

```rust
// Remote entities' cues bind from replicated CueWireMessage.
// The LOCAL predicting player ALSO fires its own on_cast + projectile cues locally for zero latency.
// De-dup: when a replicated CueWireMessage arrives whose source_id == local player's ObeliskId AND
// it's a predicted cue kind (on_cast / projectile-spawn), SKIP it — the local predicted copy already played.
// Resolution-dependent cues (on_hit) come ONLY from the server (replicated) in Stage A.
fn dedup(local_obelisk_id: &str, m: &CueMessage) -> bool {
    m.source_id == local_obelisk_id && is_predicted_cue_kind(&m.cue_id)   // skip if true
}
```

### 6.6 HUD + server-auth damage display

- `NetworkedHealth` drives the HUD bars (own + opponent). Server-authoritative — the client renders what the server replicates, never computes damage.
- `DamageResolved` (from `NetEventMessage`) drives floating damage numbers + hit flash on the target (looked up by `ObeliskNetId` → local Entity via the replicated id, or `ObeliskEntityIndex` if the client mirrors it).

---

## 7. Round flow + late-joiner (server-driven best-of-3)

```rust
// Server-owned state machine (FixedUpdate or a dedicated system). Replicated via RoundStateMessage.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub enum RoundPhase { WaitingForPlayers, Countdown(f32), Active, RoundOver { winner: String }, MatchOver { winner: String } }

#[derive(Clone, Serialize, Deserialize)]
pub struct RoundStateMessage {
    pub phase: RoundPhase,
    pub scores: [(String /*obelisk_id*/, u8 /*round wins*/); 2],
    pub match_seed: u64,           // replicated for Stage B (M3); informational in Stage A
}
```

**Flow (spec §2):**
1. `WaitingForPlayers` until 2 `ClientOf` connected.
2. `Countdown` → `Active`. On `Active`, server resets hp/cooldowns and **respawns both players at fixed spawn markers** (hard-coded arena geometry, spec §11).
3. A round ends when one player's obelisk hp hits 0 (`EntityDied` → increment the killer's round-win count). Best-of-3: first to 2 wins → `MatchOver`.
4. Between rounds: respawn at markers, reset hp/cooldowns, re-broadcast `RoundStateMessage`.

**Late-joiner:** `refresh_replicate_on_connect` (§5.7) handles the *replication-target* side; the round machine handles the *gameplay* side (a joiner arriving mid-match enters as a spectator until the next round, or — simplest for M2 — the server only starts a match once exactly 2 are present and treats a disconnect as a forfeit). Keep M2's policy minimal; the gate only requires "best-of-3 rounds resolve" with two stable clients.

---

## 8. Net-test harness (M2.5 — adapt wisp's observer + trace)

### 8.1 Copy the trace module

Copy `wisp/src/trace.rs` (104 lines) → `arena_game/src/trace.rs`, renaming env vars: `ARENA_TRACE_FILE`, `ARENA_TRACE_SRC`. The implementation is process-agnostic (wall-clock `t`, `src` tag, JSONL append, `OnceLock` sink) — **no logic change needed**, only env-var names. M1 already uses `arena_game/src/trace.rs`; extend its `event(kind, extra)` call sites to cover the net path.

### 8.2 The observer binary (`src/bin/observer.rs`)

Copy `wisp/src/bin/observer.rs` as the template: a **headless** lightyear client (no window, no rendering) that connects to the server, receives replication, and **scripts cast commands** (sends `CastRequestMessage` directly, bypassing the input layer) while emitting trace events for every replicated `NetEventMessage` / `CueWireMessage` / `NetworkedPosition` it observes. Two observers (`ARENA_TRACE_SRC=client-0`, `client-1`) + the server (`ARENA_TRACE_SRC=server`) all write to the same JSONL (interleaved is fine; the harness reads line-by-line) or distinct files merged by the orchestrator.

### 8.3 The M2 regression assertion (spec §9)

```
Scenario: 2 headless observers connect to the dedicated server; observer-0 scripts a firebolt cast.
Assert over the merged JSONL trace:
  - server emits CastBegan(caster=client-0-id, skill=firebolt)
  - server emits DamageResolved(caster=client-0-id, target=client-1-id, skill=firebolt)
  - BOTH observers receive a CastBegan NetEvent AND a DamageResolved NetEvent for that cast
  - the damage value matches the server's (server-authoritative — clients echo, don't compute)
```
This is **fully headless** (server + 2 observers, no window). The orchestrator = an adapted `run_session.sh`/`summarize.sh` (spec §9; copy wisp's). The windowed client is needed only for the human-visual checks (cosmetics, rig animation, HUD), not the trace regression.

---

## 9. Staged task decomposition (ORDERED, bite-sized, each ends in a check)

Legend: **[H]** headlessly verifiable (server logs / trace / 2 headless observers); **[W]** needs the windowed client (visual).

### M2.0 — CueMessage serde refactor (unblocks the wire) — all [H]
1. `arena_skills`: rewrite `CueMessage` → `{ cue_id: String, source_id: String, position: Vec3, kind: CueKind }` serde; derive `Serialize/Deserialize` on `CueKind`/`LaneEvent`. **Check:** `arena_skills` compiles; a unit test round-trips a `CueMessage` through `serde_json`.
2. `arena_skills`: add `SkillFxRegistry` resource + `load_dir` (flatten every `.skillfx.ron` `bindings` into `by_cue`). **Check:** unit test loads `firebolt.skillfx.ron`, asserts `lanes("on_cast")` non-empty.
3. `arena_skills`: split the binding into (a) a `CueEvent → CueMessage` egress helper and (b) a `CueMessage → dispatch LaneEvent` consumer that re-looks-up `SkillFxRegistry`. **Check:** unit test: feed a `CueMessage`, assert it resolves the right `LaneEvent`s; missing cue_id no-ops + warns.
4. `arena_game` M1: rewire M1's co-located path to the new `CueMessage` (keep M1 single-process working). Re-key `AimDirs` to `HashMap<String, Vec3>` (ObeliskId). **Check:** M1 `arena-client` still casts firebolt + spawns cosmetics (regression — **[W]** for the visual, **[H]** for the trace that a cue fired).

### M2.1 — lightyear scaffold + 2 clients connect — all [H]
5. `arena_game`: copy wisp net constants/arg-parser (`net/mod.rs`); add `lightyear 0.26.4` + drop `input_bei`. **Check:** workspace builds.
6. `arena_game`: write `ProtocolPlugin` (§3) — components + channels + messages registered (start with just `NetworkedPlayer`/`NetworkOwner`/`NetworkedPosition` + `PlayerInputMessage`). **Check:** compiles; `ProtocolPlugin` adds cleanly to a `MinimalPlugins` app.
7. `arena_game`: `ServerNetPlugin` + `src/bin/server.rs` (copy wisp listen setup + `on_new_link` + `spawn_server`). **Check:** `arena-server` boots headless, logs "listening".
8. `arena_game`: `ClientNetPlugin` + `src/bin/client.rs` connect path. **Check:** `arena-client` connects; server logs `on_client_connected` for 2 clients.
9. `arena_game`: `sync_networked_players` (spawn `NetworkedPlayer` per `ClientOf`, `make_combatant`, `Replicate::manual`) + `refresh_replicate_on_connect`. **Check [H]:** 2 headless clients connect; trace shows each receives a `NetworkedPlayer` for the *other* (late-joiner refresh works).

### M2.2 — movement replication + prediction — [H] for replication, [W] for feel
10. Server `run_player_controller` + `apply_player_rotation` (FixedUpdate, avian forces) + `drain_player_inputs` + client `send_local_player_input`. **Check [H]:** observer moves; server `NetworkedPosition` updates; other observer sees it move.
11. Client interpolation of remote players (`net/replication.rs` smoothing copy). **Check [W]:** remote player moves smoothly in the window.
12. Enable Stage-A own-movement prediction: per-entity enable avian `Position`/`Rotation` replication on the local rig, mark `Predicted`, run the controller `With<Predicted>`, enable `add_should_rollback`. **Check [W]:** local movement is instant; **[H]:** trace shows no persistent position divergence vs server (rollback corrects). Fallback: the snap-to-server reconciliation if rollback is unstable.

### M2.3 — combat over the wire — all [H]
13. Server: obelisk sim on the server (`ObeliskSimPlugin` + skills/effects + `seed_combat_rng(match_seed)`). **Check [H]:** server-side scripted `cast_skill_at` emits `CastBegan`/`DamageResolved` in logs.
14. Client `CastRequestMessage` send + server `drain_cast_requests` → re-validate → `cast_skill_at`. **Check [H]:** observer sends a cast_request; server emits `CastBegan`.
15. Egress bridge: `egress_net_events` (NetEvent → `NetEventMessage` broadcast) + server cue egress (`CueEvent → CueWireMessage`). **Check [H]:** both observers receive `CastBegan` + `DamageResolved` + a `CueWireMessage`.
16. Client `register_client_cue_binding` consumes `CueWireMessage` → `SkillFxRegistry` → cosmetics; predicted own-cast cues + de-dup by `source_id`. **Check [W]:** firebolt VFX + cosmetic projectile play on both clients; own cast feels instant; **[H]:** trace shows one cue per cast (de-dup).
17. Predicted cast initiation + projectile motion on the client (obelisk timeline+projectile systems `With<Predicted>`, no ResolveHits/no CombatRng). **Check [W]:** own windup + projectile start instantly; damage lands a few ms later from server.

### M2.4 — HUD + rounds + late-joiner — [H] + [W]
18. `NetworkedHealth` mirror (server) + HUD bars + floating damage from `DamageResolved` (client). **Check [W]:** hp bars drop on hit; **[H]:** `NetworkedHealth` replicates.
19. Round state machine (server) + `RoundStateMessage` + respawn-at-markers + hp/cooldown reset. **Check [H]:** trace shows round transitions; **[H]:** best-of-3 resolves (first to 2 → `MatchOver`).

### M2.5 — trace harness — all [H]
20. Copy `trace.rs` (env-rename) + `observer.rs` (scripted cast) + adapt `run_session.sh`/`summarize.sh`. **Check [H]:** the M2 regression (§8.3) passes: 2 observers, firebolt, both see `CastBegan→DamageResolved`, damage matches server.

**M2 GATE (final check):** two clients connect to the dedicated server; each casts firebolt; both see the cast + server-authoritative damage; movement is client-predicted; best-of-3 rounds resolve. Headlessly proven by tasks 9, 12 (trace), 15, 17, 19, 20; visually confirmed (cosmetics/rig/HUD/feel) by tasks 11, 12, 16, 17, 18 with the windowed client.

---

## 10. Risks (netcode-specific)

1. **Getting the lightyear 0.26 prediction API right (highest).** `register_component().add_prediction().add_should_rollback()` must be paired and correctly thresholded; under `LightyearAvianPlugin::Position` you must write avian `Position`/`Rotation`, never Transform, or the per-tick sync clobbers writes. wisp *registered* but *never enabled* this path ("Stage Q deferred") — **M2 is the first time it's actually exercised**, so the 0.26 prediction/rollback path is unproven in this lineage. Mitigation: enable on the local player only; keep wisp's snap-to-server reconciliation (`player/controller.rs:92-123`) as the documented fallback; `RUST_LOG=lightyear=debug` to surface silent rollbacks.

2. **Server-only RNG under Stage A is fine but the seam to Stage B is sharp.** CombatRng is a stateful ChaCha8 *stream*; in Stage A only the server draws from it (clients never resolve), so M2 is safe. The risk is *accidentally* running `resolve_one_hit`/`ResolveHits` on the client (which would draw RNG and desync). Mitigation: hard-gate the client's obelisk systems to timeline+projectile only (`With<Predicted>`, explicitly exclude `ObeliskSet::ResolveHits` and Hitbox spawn); the `resolve_seeded` rollback-safe RNG is an M3 obelisk-bevy change, **not** needed for M2.

3. **0.26 component-update-after-insert is unreliable → combat must be messages.** Mutating an already-replicated component per-tick may not propagate (confirmed in wisp; their fix was the `BeamCastBroadcast` message pattern). M2's combat events therefore ride as **reliable Messages** (`NetEventMessage`, `CueWireMessage`), not component mutations. The pose stream uses the working interpolation path. Mitigation: do not try to replicate `ActiveCast` *changes* as component mutations — fold cast phase into the `NetworkedPosition` per-tick stream + use messages for discrete events.

4. **Message ordering + channel choice.** `cast_request` on an unreliable channel could drop a cast; combat events out of order could show damage before the cast. Mitigation: `cast_request` + combat events + round state on **reliable** channels (§3); movement stays unreliable (latest-wins). Late-arriving position packets make the interp `t` exceed 1 → clamp to `[0,1]` (wisp does this).

5. **The co-located→split refactor breaks M1's synchronous cue registration.** M1's `register_cues_synchronously` calls `App::observe_cue` at build time (single-process only) and `CueMessage` embeds a `LaneEvent` via a captured closure. Splitting into server-egress + client-binding while keeping M1 green is the trickiest non-net part. Mitigation: do the `CueMessage` serde refactor **first** (M2.0), keep M1 single-process working through that task as a regression gate, then layer the wire on top. Also re-key `AimDirs` to `ObeliskId` early — the Entity-keyed map is silently wrong the moment two peers exist.

Secondary: **avian 0.5 pin** (can't bump to 0.6 until lightyear does — keep the workspace on 0.5/Bevy 0.18); **late-joiner correctness** (copy `refresh_replicate_on_connect` verbatim — `NetworkTarget::All` silently breaks the second client); **`MessageSender` as `Single`** (multiple panics — query exactly one per message type per peer).
