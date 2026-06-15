# Server-Authoritative Netcode Egress — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a server an authoritative, serializable, engine-neutral stream of "what happened" gameplay events that it can drain each tick and replicate to clients — plus a runnable headless-server example that proves the loop works with the presentation layer compiled out.

**Architecture:** A new `net` module defines a `NetEvent` enum keyed by **network-stable `String` actor ids** (not `Entity`) and deriving `serde` Serialize/Deserialize. `ObeliskNetPlugin` (part of the sim) registers `NetEvent` as a buffered Bevy message and adds **translation observers** that mirror the sim's in-process observer events (`DamageResolved`, `EffectApplied`, …) into `NetEvent`s, mapping `Entity → ObeliskId` via the existing `ObeliskEntityIndex`. A server system drains them with a `MessageReader`. Determinism + the existing observer events are untouched (dual-emit: observers for in-process VFX, messages for netcode egress).

**Tech Stack:** Rust 2021, Bevy 0.17.3 (buffered messages + observers), `serde`/`serde_json` (wire format + round-trip), the existing obelisk-bevy sim.

---

## Verified facts (ground truth)

- `crate::ids::ObeliskEntityIndex`: `id(&self, e: Entity) -> Option<&str>` (Entity→stable id), `entity(&self, id: &str) -> Option<Entity>`. It's a `Resource`, auto-synced, registered by `ObeliskCorePlugin`.
- `crate::events` (all `#[derive(Event)]` observer events, fired via `commands.trigger`, observed via `On<E>`):
  - `CastBegan { caster: Entity, skill_id: String, total_duration: f32 }`
  - `DamageResolved { caster: Entity, target: Entity, skill_id: String, total_damage: f64, is_killing_blow: bool, life_after: f64, mana_spent: f64 }`
  - `EffectApplied { target: Entity, effect_id: String, total_duration: f64, stacks: u32 }`
  - `EffectExpired { target: Entity, effect_id: String }`
  - `DotTicked { target: Entity, effect_id: String, dot_damage: f64, life_remaining: f64 }`
  - `EntityDied { target: Entity, killer: Option<Entity> }`
- `serde = { version = "1", features = ["derive"] }` is already a dependency. `serde_json` is NOT yet a dependency (Task 1 adds it as a dev-dependency for the round-trip test).
- `ObeliskSimPlugin` (in `src/lib.rs`) composes `ObeliskAssetsPlugin + ObeliskSpatialPlugin + ObeliskCorePlugin + ObeliskCombatPlugin`. This is where `ObeliskNetPlugin` gets added (Task 2).
- The `ObeliskTestApp` harness (`obelisk_bevy::testkit`) drives the headless sim deterministically; baseline at the start of this batch: **39 passing**.
- Headless app recipe (from the API notes in `src/lib.rs`): `MinimalPlugins + AssetPlugin + bevy::mesh::MeshPlugin + bevy::scene::ScenePlugin + ObeliskSimPlugin`, `TimeUpdateStrategy::ManualDuration(1/60s)`, `Time::<Fixed>::from_hz(60)`, `app.finish(); app.cleanup();` before the first `update()`.

## Bevy 0.17 buffered-event API (determine in Task 2)

Bevy split events ~0.16: **observer** events use `#[derive(Event)]` + `commands.trigger` + `On<E>`; **buffered/drainable** events use a separate API. In 0.17.3 this is expected to be `#[derive(Message)]` + `app.add_message::<M>()` + `MessageWriter<M>` (`.write(..)`) + `MessageReader<M>` (`.read()`). **Task 2 confirms the exact names**; if 0.17.3 still uses `#[derive(Event)]` + `app.add_event::<E>()` + `EventWriter`/`EventReader` for buffered events, use that instead (the code shapes are 1:1). This plan is written against the `Message` API with the `Event`/`EventReader` fallback noted.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/net.rs` | create | `NetEvent` wire enum (serde + buffered-message derive) + `ObeliskNetPlugin` + translation observers. |
| `src/lib.rs` | modify | `pub mod net;`; add `ObeliskNetPlugin` to `ObeliskSimPlugin`. |
| `src/prelude.rs` | modify | export `NetEvent`. |
| `Cargo.toml` | modify | add `serde_json` dev-dependency (round-trip test). |
| `tests/netcode.rs` | create | Headless cast → drain `NetEvent` stream → assert + serde round-trip. |
| `examples/headless_server.rs` | create | Runnable minimal headless authoritative server draining `NetEvent`s; builds with `--no-default-features` (present excluded). |
| `README.md` | modify | "Server / netcode" usage section. |

---

## Task 1: `NetEvent` wire types (serde) + round-trip test

**Files:** Create `src/net.rs`; Modify `src/lib.rs`, `Cargo.toml`.

- [ ] **Step 1: Add `serde_json` dev-dependency to `Cargo.toml`**

In `[dev-dependencies]`, add:
```toml
serde_json = "1"
```

- [ ] **Step 2: Write `src/net.rs` with the wire enum + a failing round-trip test**

```rust
use serde::{Deserialize, Serialize};

/// Engine-neutral, serializable gameplay event for server→client replication.
/// Actor references are network-stable `String` ids (obelisk `StatBlock.id`), NOT `Entity`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NetEvent {
    CastBegan { caster: String, skill_id: String, total_duration: f32 },
    DamageResolved {
        caster: String,
        target: String,
        skill_id: String,
        total_damage: f64,
        is_killing_blow: bool,
        life_after: f64,
    },
    EffectApplied { target: String, effect_id: String, total_duration: f64, stacks: u32 },
    EffectExpired { target: String, effect_id: String },
    DotTicked { target: String, effect_id: String, dot_damage: f64, life_remaining: f64 },
    EntityDied { target: String, killer: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netevent_serde_round_trips() {
        let events = vec![
            NetEvent::CastBegan { caster: "player".into(), skill_id: "firebolt".into(), total_duration: 0.6 },
            NetEvent::DamageResolved {
                caster: "player".into(), target: "goblin".into(), skill_id: "firebolt".into(),
                total_damage: 20.0, is_killing_blow: false, life_after: 30.0,
            },
            NetEvent::EntityDied { target: "goblin".into(), killer: Some("player".into()) },
        ];
        let json = serde_json::to_string(&events).expect("serialize");
        let back: Vec<NetEvent> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(events, back, "NetEvent must survive a JSON round-trip unchanged");
    }
}
```

- [ ] **Step 3: Wire `pub mod net;` in `src/lib.rs` (below the doc block, with the other module decls).**

- [ ] **Step 4: Run the round-trip test**

Run: `cargo test --features test-support --lib netevent_serde_round_trips`
Expected: PASS. (If `serde_json` isn't found, confirm Step 1's dev-dependency.)

- [ ] **Step 5: Commit**

```bash
git add src/net.rs src/lib.rs Cargo.toml
git commit -m "feat(net): serializable NetEvent wire format + serde round-trip"
```
`git config commit.gpgsign false` if signing prompts.

---

## Task 2: `ObeliskNetPlugin` — buffered message + translation observers

**Files:** Modify `src/net.rs`, `src/lib.rs`.

- [ ] **Step 1: Determine the bevy 0.17.3 buffered-event API**

Before writing code, confirm the buffered-message API by checking docs.rs/bevy/0.17 (or a quick `cargo doc`): is it `#[derive(Message)]` + `app.add_message::<M>()` + `MessageWriter::write` + `MessageReader::read`, or `#[derive(Event)]` + `app.add_event::<E>()` + `EventWriter::write/send` + `EventReader::read`? Use whichever 0.17.3 provides for BUFFERED events. The code below uses the `Message` form; if it's the `Event` form, substitute `Message`→`Event`, `add_message`→`add_event`, `MessageWriter`→`EventWriter`, `MessageReader`→`EventReader` (everything else is identical). **Record the confirmed API in your report** — Task 3 and the example reuse it.

- [ ] **Step 2: Make `NetEvent` a buffered message + add `ObeliskNetPlugin` in `src/net.rs`**

Add the buffered-message derive to `NetEvent` (keep the serde derives):
```rust
use bevy::prelude::*;

// Add `Message` to the existing derive list on NetEvent:
//   #[derive(Message, Serialize, Deserialize, Clone, Debug, PartialEq)]
// (If 0.17.3 uses buffered `Event` instead, derive `Event` here.)
```
Then add the plugin + translation observers below the enum:
```rust
use crate::events::{CastBegan, DamageResolved, DotTicked, EffectApplied, EffectExpired, EntityDied};
use crate::ids::ObeliskEntityIndex;

/// Mirrors the sim's in-process observer events into the buffered `NetEvent` stream for
/// server replication. Entity refs are translated to stable string ids via ObeliskEntityIndex.
pub struct ObeliskNetPlugin;

impl Plugin for ObeliskNetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<NetEvent>(); // (or add_event::<NetEvent>() if buffered Event)
        app.add_observer(mirror_cast_began);
        app.add_observer(mirror_damage_resolved);
        app.add_observer(mirror_effect_applied);
        app.add_observer(mirror_effect_expired);
        app.add_observer(mirror_dot_ticked);
        app.add_observer(mirror_entity_died);
    }
}

/// Stable id for an entity, or an empty string if unmapped (defensive).
fn id_of(index: &ObeliskEntityIndex, e: Entity) -> String {
    index.id(e).unwrap_or("").to_string()
}

fn mirror_cast_began(ev: On<CastBegan>, index: Res<ObeliskEntityIndex>, mut net: MessageWriter<NetEvent>) {
    let e = ev.event();
    net.write(NetEvent::CastBegan {
        caster: id_of(&index, e.caster),
        skill_id: e.skill_id.clone(),
        total_duration: e.total_duration,
    });
}

fn mirror_damage_resolved(ev: On<DamageResolved>, index: Res<ObeliskEntityIndex>, mut net: MessageWriter<NetEvent>) {
    let e = ev.event();
    net.write(NetEvent::DamageResolved {
        caster: id_of(&index, e.caster),
        target: id_of(&index, e.target),
        skill_id: e.skill_id.clone(),
        total_damage: e.total_damage,
        is_killing_blow: e.is_killing_blow,
        life_after: e.life_after,
    });
}

fn mirror_effect_applied(ev: On<EffectApplied>, index: Res<ObeliskEntityIndex>, mut net: MessageWriter<NetEvent>) {
    let e = ev.event();
    net.write(NetEvent::EffectApplied {
        target: id_of(&index, e.target),
        effect_id: e.effect_id.clone(),
        total_duration: e.total_duration,
        stacks: e.stacks,
    });
}

fn mirror_effect_expired(ev: On<EffectExpired>, index: Res<ObeliskEntityIndex>, mut net: MessageWriter<NetEvent>) {
    let e = ev.event();
    net.write(NetEvent::EffectExpired {
        target: id_of(&index, e.target),
        effect_id: e.effect_id.clone(),
    });
}

fn mirror_dot_ticked(ev: On<DotTicked>, index: Res<ObeliskEntityIndex>, mut net: MessageWriter<NetEvent>) {
    let e = ev.event();
    net.write(NetEvent::DotTicked {
        target: id_of(&index, e.target),
        effect_id: e.effect_id.clone(),
        dot_damage: e.dot_damage,
        life_remaining: e.life_remaining,
    });
}

fn mirror_entity_died(ev: On<EntityDied>, index: Res<ObeliskEntityIndex>, mut net: MessageWriter<NetEvent>) {
    let e = ev.event();
    net.write(NetEvent::EntityDied {
        target: id_of(&index, e.target),
        killer: e.killer.map(|k| id_of(&index, k)),
    });
}
```

- [ ] **Step 3: Add `ObeliskNetPlugin` to `ObeliskSimPlugin` in `src/lib.rs`**

In `ObeliskSimPlugin::build`, add it to the `add_plugins` chain (it's always-on — the egress is cheap; unread messages are auto-cleared by Bevy each frame):
```rust
        app.add_plugins(net::ObeliskNetPlugin);
```
(Add alongside the existing `ObeliskAssetsPlugin`/`ObeliskSpatialPlugin`/`ObeliskCorePlugin`/`ObeliskCombatPlugin` adds.)

- [ ] **Step 4: Build + suite**

Run: `cargo test --features test-support --lib --tests`
Expected: all green (39 + round-trip). **If observers can't take `MessageWriter`** (a system-param restriction in observers), the fallback is a normal system reading the events — but observers-with-MessageWriter is the expected pattern; report if it doesn't work.

- [ ] **Step 5: Commit**

```bash
git add src/net.rs src/lib.rs
git commit -m "feat(net): ObeliskNetPlugin mirrors gameplay events to the NetEvent egress"
```

---

## Task 3: Headless drain integration test

**Files:** Create `tests/netcode.rs`.

- [ ] **Step 1: Write the integration test**

```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::net::NetEvent;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life; b.current_life = life;
    b.max_mana.base = mana; b.current_mana = mana;
    b
}

/// A server runs the sim headless, casts firebolt, and drains the serializable NetEvent stream.
#[test]
fn server_drains_serializable_netevents() {
    let mut t = ObeliskTestApp::new(42);

    // Load the firebolt cast timeline (same as the slice).
    let handle: Handle<CastTimeline> =
        t.app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 {
        t.app.update();
        if t.app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() { break; }
    }
    t.app.world_mut().resource_mut::<CastTimelineHandles>().0.insert("firebolt".into(), handle);

    // A "netcode egress" system that drains NetEvents into a collector resource.
    #[derive(Resource, Default)]
    struct Collected(Vec<NetEvent>);
    t.app.init_resource::<Collected>();
    // Drain in Update (after the sim's FixedUpdate emits). Use the buffered-event reader
    // confirmed in Task 2 (MessageReader or EventReader).
    t.app.add_systems(Update, |mut reader: MessageReader<NetEvent>, mut c: ResMut<Collected>| {
        for ev in reader.read() {
            c.0.push(ev.clone());
        }
    });

    let player = t.app.world_mut().spawn((Combatant, Attributes(make_block("player", 100.0, 100.0)), Faction::Player, ObeliskId("player".into()), Transform::from_xyz(0.0, 0.0, 0.0))).id();
    let dummy = t.app.world_mut().spawn((Combatant, Attributes(make_block("dummy", 25.0, 0.0)), Faction::Enemy, ObeliskId("dummy".into()), Transform::from_xyz(0.0, 0.0, 2.0))).id();
    {
        let mut c = t.app.world_mut().commands();
        insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();
    t.app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);
    t.advance_ticks(600);

    let collected = &t.app.world().resource::<Collected>().0;
    // The stream must contain a CastBegan and a DamageResolved targeting the stable id "dummy".
    assert!(collected.iter().any(|e| matches!(e, NetEvent::CastBegan { caster, .. } if caster == "player")),
        "stream should contain CastBegan for player");
    assert!(collected.iter().any(|e| matches!(e, NetEvent::DamageResolved { target, caster, .. } if target == "dummy" && caster == "player")),
        "stream should contain DamageResolved player->dummy by stable id");
    assert!(collected.iter().any(|e| matches!(e, NetEvent::EntityDied { target, .. } if target == "dummy")),
        "stream should contain EntityDied for dummy");

    // The whole drained stream is serializable (the wire format).
    let json = serde_json::to_string(collected).expect("serialize the drained stream");
    let back: Vec<NetEvent> = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(collected, &back, "drained NetEvent stream round-trips");
    let _ = dummy;
}
```

Add `serde_json` to `[dev-dependencies]` is already done in Task 1. The test references `serde_json` — integration tests can use dev-dependencies.

- [ ] **Step 2: Run the integration test**

Run: `cargo test --features test-support --test netcode -- --nocapture`
Expected: PASS. Report the ACTUAL number of NetEvents drained and which variants appeared.
**Debug:** if `MessageReader` reads nothing, confirm (a) `ObeliskNetPlugin` is in `ObeliskSimPlugin`, (b) the drain system runs in `Update` AFTER the FixedUpdate that emits (it does), and (c) the buffered-event reader type matches Task 2's API. Note: Bevy buffered messages are double-buffered and cleared after ~2 frames of non-reading — since the drain system runs every `Update`, nothing is lost. If the harness's `advance_ticks` calls `update()` (which runs both FixedUpdate + Update), the drain keeps pace.

- [ ] **Step 3: Commit**

```bash
git add tests/netcode.rs
git commit -m "test(net): server drains serializable NetEvent stream (stable ids + round-trip)"
```

---

## Task 4: Runnable headless server example (presentation compiled out)

**Files:** Create `examples/headless_server.rs`.

- [ ] **Step 1: Write `examples/headless_server.rs`**

A minimal authoritative server: no window/render, drives the sim with `MinimalPlugins`, casts, advances ticks, and prints the drained `NetEvent` stream (what it would replicate to clients).
```rust
//! Minimal headless authoritative server. Run with the presentation layer compiled out:
//!   cargo run --example headless_server --no-default-features
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::net::NetEvent;
use stat_core::StatBlock;
use std::time::Duration;

#[derive(Resource, Default)]
struct Egress(Vec<NetEvent>);

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life; b.current_life = life;
    b.max_mana.base = mana; b.current_mana = mana;
    b
}

fn main() {
    // Init obelisk globals (constants + effects + skills) before building the app.
    if !stat_core::config::constants_initialized() { stat_core::init_constants_default().unwrap(); }
    if !stat_core::config::effect_registry_initialized() {
        stat_core::init_effect_registry(std::path::Path::new("tests/fixtures/effects")).unwrap();
    }

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(bevy::mesh::MeshPlugin)
        .add_plugins(bevy::scene::ScenePlugin)
        .add_plugins(ObeliskSimPlugin) // sim only — no presentation
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .init_resource::<Egress>();
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    app.seed_combat_rng(42);
    // Drain the NetEvent egress and stash it (a real server would serialize + send each).
    app.add_systems(Update, |mut reader: MessageReader<NetEvent>, mut e: ResMut<Egress>| {
        for ev in reader.read() { e.0.push(ev.clone()); }
    });
    app.finish();
    app.cleanup();

    // Load the firebolt timeline.
    let handle: Handle<CastTimeline> = app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 {
        app.update();
        if app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() { break; }
    }
    app.world_mut().resource_mut::<CastTimelineHandles>().0.insert("firebolt".into(), handle);

    // Spawn two combatants and cast.
    let player = app.world_mut().spawn((Combatant, Attributes(make_block("player", 100.0, 100.0)), Faction::Player, ObeliskId("player".into()), Transform::from_xyz(0.0, 0.0, 0.0))).id();
    let dummy = app.world_mut().spawn((Combatant, Attributes(make_block("dummy", 25.0, 0.0)), Faction::Enemy, ObeliskId("dummy".into()), Transform::from_xyz(0.0, 0.0, 2.0))).id();
    {
        let mut c = app.world_mut().commands();
        insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    app.update();
    app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);
    for _ in 0..600 { app.update(); }

    // Print the authoritative stream the server would replicate.
    let egress = &app.world().resource::<Egress>().0;
    println!("[headless server] authoritative NetEvent stream ({} events):", egress.len());
    for ev in egress {
        println!("  {}", serde_json::to_string(ev).unwrap());
    }
    let _ = dummy;
}
```

Add `serde_json` for the example: examples can use dev-dependencies, so the existing `serde_json` dev-dep covers it.

- [ ] **Step 2: Build the example with presentation compiled out**

Run: `cargo build --no-default-features --example headless_server`
Expected: PASS — proves the authoritative sim + netcode egress compile and run with the `present` module entirely excluded.
Then run it: `cargo run --no-default-features --example headless_server`
Expected: prints a non-empty NetEvent stream including `CastBegan`, `DamageResolved` (target "dummy"), and `EntityDied` ("dummy"). Report the printed stream.

- [ ] **Step 3: Commit**

```bash
git add examples/headless_server.rs
git commit -m "feat(net): runnable headless authoritative server example (--no-default-features)"
```

---

## Task 5: Prelude export + README netcode section + quality gates

**Files:** Modify `src/prelude.rs`, `README.md`.

- [ ] **Step 1: Export `NetEvent` in `src/prelude.rs`**

```rust
pub use crate::net::NetEvent;
```

- [ ] **Step 2: Add a "Server / netcode" section to `README.md`**

Append a section documenting:
- The dual-emit model: in-process `On<DamageResolved>` observers for VFX/UI vs. the buffered `NetEvent` stream for replication.
- How a server drains it: a system with the buffered-event reader (`MessageReader<NetEvent>` per Task 2) each frame, serializing + sending each `NetEvent`.
- That `NetEvent` uses stable `String` ids (obelisk `StatBlock.id`), so it's network-meaningful and serde-serializable (the wire format).
- The headless server pattern: `MinimalPlugins + AssetPlugin + ObeliskSimPlugin` (no `present`), built `--no-default-features`; point at `examples/headless_server.rs`.
Keep it concise and accurate to the implemented API (check `src/prelude.rs` / Task 2's confirmed reader type).

- [ ] **Step 3: Quality gates**

- `cargo test --features test-support --lib --tests` → all green (39 + round-trip + netcode integration).
- `cargo clippy --features test-support --lib --tests -- -D warnings` → clean for `obelisk-bevy`.
- `cargo fmt` then `cargo fmt --check` → clean.
- `cargo build --no-default-features` → the headless build (no present) compiles.

- [ ] **Step 4: Commit**

```bash
git add src/prelude.rs README.md
git commit -m "docs(net): server/netcode README section; export NetEvent"
```

---

## Self-review notes (coverage vs the batch scope)

- Serializable, engine-neutral wire format (stable string ids): Task 1 ✅
- Buffered `Message` dual-emit mirror of gameplay events (the egress): Task 2 ✅
- Server drains the stream + serde round-trip proof: Task 3 ✅
- Runnable headless authoritative server with presentation compiled out: Task 4 ✅
- Prelude export + docs + gates: Task 5 ✅

## Deferred (out of this batch, noted to the user)

- **In-process `EntityEvent` propagation** (parent rig observes child-volume events): a presentation/observer ergonomic, not a netcode requirement — fits the VFX batch better. The current global observer events already carry the target `Entity`.
- **Full bevy render-feature trim** (shrinking the server binary by setting `bevy = { default-features = false }` + a minimal feature set): a binary-size optimization. This batch proves the headless server *runs* with `present` compiled out (`--no-default-features`); trimming bevy's own render features further is fiddly feature-graph work best done as a focused follow-up.
- **Client-side replay/ingress** (applying a `NetEvent` stream on a client to drive VFX without re-simulating): the consumer side of replication; pairs with the VFX batch.
- **Network-transport** (actual sockets/serialization-on-the-wire): out of scope — obelisk-bevy provides the authoritative serializable stream; the game chooses a transport (e.g. `bevy_replicon`, `lightyear`).
