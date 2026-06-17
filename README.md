# obelisk-bevy

A [Bevy](https://bevyengine.org/) plugin that exposes the [`obelisk`](../obelisk) ARPG
libraries (loot / stat / skill-tree / drop-table systems) to Bevy games, extended with a
**3D + temporal skill model**, **hit / hurt boxes**, **skill-usage primitives**, and
**VFX-sequencing hooks**.

obelisk provides the pure-Rust ARPG rules — skills, triggered effects, statuses/ailments,
damage resolution, stats. `obelisk-bevy` grafts an ECS + spatiotemporal + eventing layer
on top: a headless, deterministic, server-authoritative simulation that drives obelisk's
pipelines from Bevy schedules, plus a compile-outable presentation layer that consumes
gameplay events for VFX/audio/animation.

- **Bevy:** 0.17
- **Spatial backend:** Avian3d (sensors for hit/hurt detection, spatial queries for targeting)

## Status

Vertical slice complete. See the design spec:

- [docs/superpowers/specs/2026-06-04-obelisk-bevy-plugin-design.md](docs/superpowers/specs/2026-06-04-obelisk-bevy-plugin-design.md)

## Validating changes

A shared scenario library (`src/scenario/`) drives three validation surfaces:

- **Golden-trace regression** — `cargo test --features test-support --test golden` diffs every
  scenario in `feature_matrix()` (`src/scenario/library.rs`) against a committed event trace. Run it
  after any behavior change; if a trace change is intentional, regenerate with
  `UPDATE_GOLDEN=1 cargo test --features test-support --test golden` and **review the golden diff
  before committing** (never blind-regenerate).
- **Headless screenshots** — `cargo run --example screenshot --features debug-gizmos -- --scenario <name> --tick <n>`
  renders a scenario at a fixed tick to `screenshots/<name>-<tick>.png`.
- **Windowed playground** — `cargo run --example playground --features debug-gizmos` (keys `1`-`9`,
  `0`, `-` pick a scenario, `Space` free-casts, `R` resets).

See the "Validating changes" section of [CLAUDE.md](CLAUDE.md) for the full workflow and the
regression rule.

## Usage

### Add the dependency

```toml
[dependencies]
obelisk-bevy = { path = "../obelisk-bevy" }
# For headless / server builds (no window/render):
# obelisk-bevy = { path = "../obelisk-bevy", default-features = false }
```

### Register plugins

```rust
use obelisk_bevy::prelude::*;

App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(ObeliskPlugins)   // adds ObeliskSimPlugin (+ ObeliskPresentPlugin if "present" feature)
    // ... rest of your app
    .run();
```

### One-time global init (Startup or before first update)

```rust
use obelisk_bevy::prelude::{ObeliskConfigExt, SkillSource};

fn setup(app: &mut App) {
    app.add_obelisk_config_constants_default();          // load stat constants (idempotent)
    app.add_obelisk_effects(Path::new("config/effects")); // register DoT / buff effects
    app.add_obelisk_skills(SkillSource::Dir("config/skills".into())); // load *.toml skill rules
    app.seed_combat_rng(12345);                           // seed deterministic RNG
}
```

All four methods are idempotent — safe to call from tests and in-process client+server setups.

### Author a skill

**`config/skills/firebolt.toml`** — rules (parsed by `obelisk`/`stat_core`):

```toml
[[skills]]
id = "firebolt"
name = "Firebolt"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0

[skills.damage]
base_damages = [{ type = "fire", min = 20.0, max = 30.0 }]
```

**`assets/skills/firebolt.cast.ron`** — timeline (loaded by Bevy's asset server):

```ron
(
  skill_id: "firebolt",
  phase_durations: ( windup: 0.3, active: 0.1, recovery: 0.2 ),
  collision_windows: [
    ( id: "bolt", spawn_phase: Active, spawn_offset: 0.0, active_duration: 2.0,
      shape: Sphere( radius: 0.5 ), motion: Linear( speed: 20.0 ),
      hit_filter: Enemies, hit_mode: FirstOnly ),
  ],
  targeting: SingleEntity( range: 15.0 ),
  delivery: Projectile( speed: 20.0 ),
)
```

Register the handle at startup so the timeline is ready before casting:

```rust
fn load_timelines(asset_server: Res<AssetServer>, mut handles: ResMut<CastTimelineHandles>) {
    handles.0.insert(
        "firebolt".into(),
        asset_server.load("skills/firebolt.cast.ron"),
    );
}
```

### Spawn a combatant

Use `make_combatant` (from `ObeliskCommandsExt`) to insert `Combatant + Attributes + ObeliskId`
in one call. `ObeliskId` is derived automatically from `StatBlock.id`, so the two can never drift.

```rust
use obelisk_bevy::prelude::*;

fn spawn_player(mut commands: Commands) {
    let mut stats = stat_core::StatBlock::with_id("player");
    stats.max_life.base = 200.0;
    stats.current_life = 200.0;
    stats.max_mana.base = 100.0;
    stats.current_mana = 100.0;

    let player = commands
        .spawn_empty()
        .make_combatant(stats)      // inserts Combatant + Attributes + ObeliskId (derived from stats.id)
        .insert(Faction::Player)    // override the default Faction
        .grant_skill("firebolt")
        .id();

    // Attach a hurtbox (Avian3d sensor collider) so this entity can be hit.
    insert_hurtbox(&mut commands, player, 0.5, Vec3::ZERO);
}
```

> **Replication invariant:** `ObeliskId` must equal the entity's `StatBlock.id`. The netcode
> egress uses `ObeliskId` as the stable wire id when mapping `Entity → String` in `NetEvent`
> payloads. `make_combatant` enforces this automatically. If you ever insert `Attributes` and
> `ObeliskId` by hand, keep them in sync.

### Cast a skill

```rust
use obelisk_bevy::prelude::CastSkillExt;

fn fire_at_enemy(mut commands: Commands, player: Entity, enemy: Entity) {
    commands.entity(player).cast_skill_at("firebolt", enemy);
}
```

`PendingCast` is validated, resolved to an `ActiveCast`, and driven through
Windup → Active → Recovery automatically by `ObeliskSimPlugin` each `FixedUpdate`.

### React to gameplay events

All events are observer-triggered (`commands.trigger`). Add observers with `app.add_observer`:

```rust
use obelisk_bevy::prelude::DamageResolved;
use bevy::prelude::*;

fn on_damage(ev: On<DamageResolved>) {
    let e = ev.event();
    println!("Hit! {} damage to {:?}, killing={}", e.total_damage, e.target, e.is_killing_blow);
}

// In your plugin/setup:
app.add_observer(on_damage);
```

Other events: `CastBegan`, `CastRejected`, `CastPhaseChanged`, `HitWindowOpened`,
`HitConfirmed`, `EffectApplied`, `DotTicked`, `EffectExpired`, `EntityDied`.

### Headless / server builds

Exclude the presentation layer (no window, render, or audio):

```toml
obelisk-bevy = { path = "../obelisk-bevy", default-features = false }
```

This omits the `present` feature flag and `ObeliskPresentPlugin`. `ObeliskSimPlugin` and all
gameplay systems remain fully functional.

### Deterministic headless testing (`test-support` feature)

```toml
[dev-dependencies]
obelisk-bevy = { path = "../obelisk-bevy", features = ["test-support"] }
```

```rust
use obelisk_bevy::testkit::ObeliskTestApp;

#[test]
fn my_combat_test() {
    let mut app = ObeliskTestApp::new(42); // seed for reproducibility
    // spawn combatants, issue casts ...
    app.advance_ticks(60);
    let rec = app.rec();
    assert!(rec.damage_resolved.iter().any(|e| e.total_damage > 0.0));
}
```

`ObeliskTestApp` wires `MinimalPlugins` + physics + `ObeliskSimPlugin` with a fixed-step
`ManualDuration` time source so every tick is perfectly deterministic.

## Server / netcode

### Dual-emit model

obelisk-bevy uses a dual-emit design: every gameplay event fires **two** channels simultaneously.

- **In-process observers** (`app.add_observer(|e: On<DamageResolved>| ...)`) are for VFX, UI, audio, and local reactions — consumed by the presentation layer.
- **Buffered `NetEvent` stream** — a serializable, `MessageWriter`-backed queue — carries the same events as a network-stable wire format for server→client replication.

Both channels fire for the same underlying gameplay events. `ObeliskNetPlugin` (included in `ObeliskSimPlugin`) mirrors in-process observers into the `NetEvent` buffer automatically.

### Draining the egress on a server

Add a system that reads `MessageReader<NetEvent>` each frame:

```rust
use obelisk_bevy::prelude::NetEvent;
use bevy_eventwork::MessageReader; // or your transport's reader

fn replicate(mut reader: MessageReader<NetEvent>) {
    for ev in reader.read() {
        // serialize ev (serde::Serialize) and send to clients over your transport
        let json = serde_json::to_string(&ev).unwrap();
        send_to_all_clients(json);
    }
}
```

Register it as a normal Bevy system: `app.add_systems(Update, replicate)`.

### Wire format

`NetEvent` is `#[derive(serde::Serialize, serde::Deserialize)]` — it is directly serializable to any serde-compatible format (JSON, MessagePack, bincode, etc.).

Actor references are stable **`String` ids** taken from `obelisk`'s `StatBlock.id` field — not Bevy `Entity` values, which are meaningless across a network boundary.

Variants:

| Variant | Key fields |
|---|---|
| `CastBegan` | `caster`, `skill_id`, `total_duration` |
| `DamageResolved` | `caster`, `target`, `skill_id`, `total_damage`, `is_killing_blow`, `life_after` |
| `EffectApplied` | `target`, `effect_id`, `total_duration`, `stacks` |
| `EffectExpired` | `target`, `effect_id` |
| `DotTicked` | `target`, `effect_id`, `dot_damage`, `life_remaining` |
| `EntityDied` | `target`, `killer` (optional) |

### Headless server

Build and run the authoritative simulation without any presentation (no window, render, or audio):

```toml
# Cargo.toml
obelisk-bevy = { path = "../obelisk-bevy", default-features = false }
```

Plugin setup (no `DefaultPlugins`):

```rust
App::new()
    .add_plugins(MinimalPlugins)
    .add_plugins(bevy::asset::AssetPlugin::default())
    .add_plugins(bevy::mesh::MeshPlugin)
    .add_plugins(bevy::scene::ScenePlugin)
    .add_plugins(ObeliskSimPlugin)   // includes ObeliskNetPlugin; no present layer
    .run();
```

A working minimal example is at `examples/headless_server.rs`:

```bash
cargo run --example headless_server --no-default-features
```

### Transport

obelisk-bevy provides the **authoritative serializable event stream**; the game chooses its own transport. Drop-in options include [`bevy_replicon`](https://github.com/projectharmonia/bevy_replicon) and [`lightyear`](https://github.com/cBournhonesque/lightyear) — both accept custom serialized message types.

## VFX cues

Skills author a `vfx_cues` map in their `.cast.ron`; keys are slots (`on_cast`, `on_hit`,
`on_window_<window_id>`) and values are game-side cue ids. At the corresponding moment the
sim emits `CueEvent { cue_id, source, position, kind }` via `commands.trigger`. The
presentation layer binds handlers using `ObeliskCueExt`:

```rust
use obelisk_bevy::prelude::{ObeliskCueExt, CueEvent};

app.observe_cue("firebolt_impact", |cue: &CueEvent, commands: &mut Commands| {
    // spawn a particle or sound at cue.position, anchored to cue.source
});
```

Example firebolt timeline with authored cues:

```ron
(
  skill_id: "firebolt",
  phase_durations: ( windup: 0.3, active: 0.1, recovery: 0.2 ),
  vfx_cues: {
    "on_cast":  "firebolt_cast",
    "on_hit":   "firebolt_impact",
  },
  collision_windows: [ /* ... */ ],
  targeting: SingleEntity( range: 15.0 ),
  delivery: Projectile( speed: 20.0 ),
)
```

`CueKind` distinguishes when the cue fired: `OnCast`, `OnHit`, or `OnWindow`. Servers that
don't need VFX simply don't call `observe_cue` — `CueEvent` is cheap and fire-and-forget.

## Content

### Skill tree / gear → stats

Rebuild a combatant's `StatBlock` from any set of `StatSource`s (passive tree nodes, gear
sockets, buffs) using `apply_stat_sources`:

```rust
use obelisk_bevy::prelude::ObeliskCommandsExt;

commands.entity(player).apply_stat_sources(vec![
    Box::new(tree.to_stat_source()),
    Box::new(gear_socket.to_stat_source()),
]);
```

This replaces the entity's accumulated stats and recomputes derived values in one call.

### Loot on death

Insert a `DropTables` resource containing a `tables_core::DropTableRegistry`, then tag
enemies with a `DropTableId` component. When an entity with that component dies, the sim
rolls its table and emits `LootDropped { source, drops }` (where `drops` is a
`Vec<tables_core::Drop>`):

```rust
use obelisk_bevy::prelude::{DropTables, DropTableId, LootDropped};

// At startup — insert your drop table registry.
app.insert_resource(DropTables(my_registry));

// When spawning an enemy:
commands.spawn((
    Combatant,
    // ...
    DropTableId("goblin".into()),
));

// Observe drops:
app.add_observer(|ev: On<LootDropped>| {
    for drop in &ev.event().drops {
        println!("Dropped: {:?}", drop);
    }
});
```

Optionally insert an `ItemGenerator` resource (`loot_core::Generator`) to turn raw item
drops into fully-generated `Item`s with affixes and implicits.

## License

MIT
