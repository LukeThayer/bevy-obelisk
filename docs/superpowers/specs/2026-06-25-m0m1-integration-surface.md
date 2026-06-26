# M0 + M1 BUILD GUIDE — Integration Surface

The exact integration surface an implementation plan needs to write **real code, not placeholders**.
Everything below is cross-checked against the live source on disk (not stale docs). Where the input
JSON or a `CLAUDE.md` disagreed with the actual `Cargo.toml` / source, the **live file wins** and the
discrepancy is flagged.

## Scope + the M1 gate

M0 + M1 are **co-located single-player** (no lightyear, no workspace netcode yet). The acceptance
gate is one sentence:

> `cargo run` shows a third-person character that **casts firebolt** with **animation** + a
> **particle** + a **flying projectile**.

Deferred and therefore OMITTED here:
- **lightyear / bevy_replicon** — not needed until M2 (multiplayer). No `net` deps in M0/M1.
- **bevy_landmass / rerecast / avian_rerecast** — not needed until M4 (navmesh/AI).
- obelisk's `NetEvent` egress exists in `obelisk-bevy` but we don't drain it in single-player.

### Confirmed version baseline (load-bearing)

| Crate | Version | Evidence |
|---|---|---|
| bevy | **0.18** | `/Users/luke/src/obelisk-bevy/Cargo.toml:15`, `/Users/luke/src/wisp/Cargo.toml:28`, `/Users/luke/src/bevy_modal_editor/Cargo.toml:10` all say `bevy = "0.18"` |
| avian3d | **0.5** | `obelisk-bevy/Cargo.toml:16`, `wisp/Cargo.toml:27`, `bevy_modal_editor/Cargo.toml:11` |
| ron | 0.8 | obelisk-bevy + bevy_modal_editor (wisp uses 0.10 only as dev-dep) |

> ⚠️ **`obelisk-bevy/CLAUDE.md` text says "Bevy 0.17 ↔ Avian 0.4". That text is STALE.** The live
> `obelisk-bevy/Cargo.toml` pins `bevy = "0.18"` + `avian3d = "0.5"`, and the API uses 0.18-isms
> (`Entity::from_raw_u32`, `#[derive(Message)]`, `On<E>` observers). Trust the manifest, pin to
> **0.18 / 0.5** everywhere. A single 0.18.0-vs-0.18.1 mismatch compiles but fails at runtime via
> type mismatch between path-dep crates — verify `Cargo.lock` after first build.

---

## 1. Workspace + crate skeletons

We create a new workspace `obelisk-arena/` as a **sibling** of `obelisk-bevy`, `wisp`, `obelisk`,
and `bevy_modal_editor` (path deps assume sibling layout). Two members for M0/M1:

- `arena_skills` — the SkillFx/cue-binding layer (depends on `obelisk-bevy`; headless-testable).
- `arena_game` — the runnable client (depends on `obelisk-bevy` + `arena_skills` + `bevy_vfx`).

Directory layout assumed:

```
src/
  obelisk/            (existing — stat_core, loot_core, ...)
  obelisk-bevy/       (existing)
  wisp/               (existing — copy rig/anim/trace from here)
  bevy_modal_editor/  (existing — bevy_vfx crate lives at crates/bevy_vfx)
  obelisk-arena/      (NEW)
    Cargo.toml        (workspace root)
    crates/
      arena_skills/Cargo.toml
      arena_game/Cargo.toml
    assets/
      skills/         (.cast.ron + .skillfx.ron live here)
      character.glb   (copied from wisp/assets/character.glb)
    config/
      skills/         (.toml obelisk rules live here)
```

### `obelisk-arena/Cargo.toml` (workspace root)

```toml
[workspace]
resolver = "2"
members = ["crates/arena_skills", "crates/arena_game"]

[workspace.package]
edition = "2021"          # NOT 2024 — obelisk-bevy is on edition 2021; matching avoids MSRV churn
license = "MIT"
authors = ["Luke"]

[workspace.dependencies]
# Pin EXACTLY to obelisk-bevy's versions. 0.18 / 0.5 — see version table above.
bevy = "0.18"
avian3d = "0.5"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ron = "0.8"
thiserror = "1"

# Path dep to the existing obelisk-bevy crate (sibling layout).
obelisk-bevy = { path = "../obelisk-bevy" }
# bevy_vfx lives inside the bevy_modal_editor workspace; reference it by its inner path.
bevy_vfx = { path = "../bevy_modal_editor/crates/bevy_vfx" }

# Internal members
arena_skills = { path = "crates/arena_skills" }
```

> **Edition note:** the input's example skeleton used `edition = "2024"`. obelisk-bevy is
> `edition = "2021"` (`obelisk-bevy/Cargo.toml:4`). Edition 2024 is Rust 1.82+; mixing editions
> across path deps is fine for Cargo but pointless churn here. **Use 2021** to stay aligned with the
> crate we depend on most. (wisp uses 2024, but we're not depending on wisp as a crate — only
> copying source from it.)

### `crates/arena_skills/Cargo.toml`

```toml
[package]
name = "arena_skills"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
bevy.workspace = true
obelisk-bevy.workspace = true
serde.workspace = true
serde_json.workspace = true
ron.workspace = true
thiserror.workspace = true

[dev-dependencies]
# obelisk-bevy's headless test harness is behind the test-support feature.
obelisk-bevy = { path = "../../../obelisk-bevy", features = ["test-support"] }
```

> `arena_skills` does NOT depend on `bevy_vfx` or have a render backend — the cue→LaneEvent
> dispatch is pure data, headless-testable with `obelisk_bevy::testkit::ObeliskTestApp`. Keep VFX
> spawning in `arena_game` so the binding layer stays render-free.

### `crates/arena_game/Cargo.toml`

```toml
[package]
name = "arena_game"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
bevy.workspace = true            # default features ON — we need the render backend + DefaultPlugins
avian3d.workspace = true
obelisk-bevy.workspace = true    # default features ON — pulls "present"
arena_skills.workspace = true
bevy_vfx.workspace = true        # the particle plugin (render-coupled — see §5)
serde.workspace = true
serde_json.workspace = true
```

> **`obelisk-bevy` default feature = `present`** (`obelisk-bevy/Cargo.toml:8`). For the windowed
> client that's what we want. The headless server build (`--no-default-features`) is an M2+ concern.

---

## 2. `arena_skills` module map

This crate owns the **SkillFx authoring format** (`.skillfx.ron`) and the **binding layer** that
turns obelisk `CueEvent`s into `LaneEvent`s the game consumes. It mirrors obelisk's own
`.cast.ron` loader pattern but does NOT touch the sim — it's a pure consumer of `observe_cue`.

### The obelisk cue contract (verified from source)

From `/Users/luke/src/obelisk-bevy/src/vfx.rs` (read in full):

- `app.observe_cue(cue_id, handler)` — `handler: Fn(&CueEvent, &mut Commands) + Send + Sync + 'static`.
  Internally `app.add_observer(|ev: On<CueEvent>, mut commands| { if ev.cue_id == cue_id { handler(...) } })`.
- `CueEvent { cue_id: String, source: Entity, position: Vec3, kind: CueKind }`
  (`/Users/luke/src/obelisk-bevy/src/events.rs:149-156`).
- `CueKind { OnCast, OnWindow, OnHit }`.
- The `vfx_cues` slot keys are **exact strings**: `"on_cast"`, `"on_hit"`, and
  `"on_window_<window_id>"` where `<window_id>` is the collision window's `id`.
- `CueEvent.source` differs by kind: **OnCast=caster, OnWindow=caster, OnHit=target**. `position`
  is caster translation for OnCast, **hitbox (moving projectile) translation** for OnWindow, target
  translation for OnHit.

### Rust types (`crates/arena_skills/src/lib.rs`)

```rust
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

/// Authored cosmetic layer for one skill, loaded from `<skill>.skillfx.ron`.
/// Mirrors obelisk's CastTimeline: a serde-deserialized RON asset keyed by skill_id.
#[derive(Asset, TypePath, Debug, Clone, Deserialize)]
pub struct SkillFx {
    pub skill_id: String,
    /// Maps an obelisk cue slot key ("on_cast" | "on_hit" | "on_window_<id>") to a lane reaction.
    #[serde(default)]
    pub lanes: HashMap<String, LaneEvent>,
}

/// One cosmetic reaction bound to a cue slot. The game turns this into particles / projectile /
/// anim-layer changes. Authored, not code.
#[derive(Debug, Clone, Deserialize)]
pub struct LaneEvent {
    /// Stable lane id for tracing / debugging (e.g. "firebolt_muzzle").
    pub lane_id: String,
    /// What cosmetic to fire. Keep this an open enum so M2+ can extend it.
    pub kind: CueKind,
    /// Particle burst params (M1 uses this for muzzle + impact).
    #[serde(default)]
    pub particle: Option<ParticleSpec>,
    /// Cosmetic (non-authoritative) projectile to spawn for OnCast/OnWindow lanes.
    #[serde(default)]
    pub projectile: Option<ProjectileCosmetic>,
    /// Animation layer to drive on the source rig (e.g. "cast_release").
    #[serde(default)]
    pub anim: Option<AnimLayer>,
}

/// Mirror of obelisk's CueKind so .skillfx.ron can declare which moment a lane reacts to.
/// (We do NOT re-use obelisk's CueKind in the RON to keep the authoring format decoupled, but the
/// dispatcher maps obelisk CueKind -> this 1:1.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum CueKind {
    OnCast,
    OnWindow,
    OnHit,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParticleSpec {
    pub count: u32,
    #[serde(default = "default_lifetime")]
    pub lifetime: f32,
    /// Local-space color (RGB 0..1) for the emissive stand-in / billboard tint.
    #[serde(default)]
    pub color: [f32; 3],
    #[serde(default = "default_speed")]
    pub speed: f32,
}
fn default_lifetime() -> f32 { 0.5 }
fn default_speed() -> f32 { 3.0 }

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectileCosmetic {
    /// World units/sec. MUST match the .cast.ron window motion speed so the cosmetic mesh tracks
    /// the authoritative hitbox (firebolt = 20.0). NOT speed-scaled.
    pub speed: f32,
    #[serde(default)]
    pub color: [f32; 3],
    #[serde(default = "default_proj_radius")]
    pub radius: f32,
}
fn default_proj_radius() -> f32 { 0.2 }

#[derive(Debug, Clone, Deserialize)]
pub struct AnimLayer {
    /// Logical anim state name, mapped to a clip node in arena_game's AnimationGraph.
    pub state: String,
}

/// A dispatched lane reaction, surfaced as a Bevy Message the game's spawn systems read.
/// Carries the resolved world position + source entity so the consumer has no obelisk dependency.
#[derive(Message, Debug, Clone)]
pub struct CueMessage {
    pub lane_id: String,
    pub kind: CueKind,
    pub source: Entity,
    pub position: Vec3,
    pub event: LaneEvent,
}
```

### RON loader (mirror of obelisk's hand-rolled `.cast.ron` loader)

obelisk uses a hand-rolled `ron`-crate `AssetLoader` matched on the `.cast.ron` extension
(`obelisk-bevy/src/assets/mod.rs`). Mirror it for `.skillfx.ron`:

```rust
use bevy::asset::{io::Reader, AssetLoader, LoadContext};

#[derive(Default)]
pub struct SkillFxLoader;

impl AssetLoader for SkillFxLoader {
    type Asset = SkillFx;
    type Settings = ();
    type Error = anyhow::Error; // or a thiserror enum, mirroring obelisk's loader error type

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _ctx: &mut LoadContext<'_>,
    ) -> Result<SkillFx, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let fx: SkillFx = ron::de::from_bytes(&bytes)?;
        Ok(fx)
    }

    fn extensions(&self) -> &[&str] {
        &["skillfx.ron"]
    }
}
```

A `SkillFxHandles(pub HashMap<String, Handle<SkillFx>>)` resource + a poll-until-loaded system
mirrors obelisk's `CastTimelineHandles` + `poll_cast_assets` exactly (copy that shape from the
input's `load_cast_assets` / `poll_cast_assets` example).

### Binding layer — register cues, dispatch LaneEvents

The binding fn registers ONE `observe_cue` per cue_id that any loaded `.skillfx.ron` references, and
the handler writes a `CueMessage`. Because `observe_cue`'s handler is `Fn + Send + Sync + 'static`
and gets only `&CueEvent` + `&mut Commands` (no resource access), we look the `LaneEvent` up by
cloning it into the closure at registration time.

```rust
use obelisk_bevy::prelude::*; // ObeliskCueExt::observe_cue, CueEvent, CueKind as ObCueKind

pub struct ArenaSkillsPlugin;

impl Plugin for ArenaSkillsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SkillFx>()
            .register_asset_loader(SkillFxLoader)
            .add_message::<CueMessage>(); // bevy 0.18: add_message, not add_event
    }
}

/// Call AFTER .skillfx.ron assets are loaded (so we know which cue_ids exist).
/// For each (slot -> LaneEvent) in every SkillFx, register an obelisk cue observer that emits a
/// CueMessage. obelisk fires CueEvent with cue_id == the value stored in the .cast.ron vfx_cues map;
/// so the LaneEvent must be keyed by that SAME cue_id (see firebolt fixture below).
pub fn register_skill_cues(app: &mut App, fxs: &[SkillFx], cue_ids: &HashMap<String, (String, LaneEvent)>) {
    // cue_ids: cue_id -> (resolved lane info). Built by walking the skill's .cast.ron vfx_cues to
    // discover the cue_id strings, then matching slot keys to .skillfx.ron lanes.
    for (cue_id, (_skill, lane)) in cue_ids.clone() {
        let lane = lane.clone();
        app.observe_cue(cue_id, move |cue: &CueEvent, commands: &mut Commands| {
            let kind = match cue.kind {
                ObCueKind::OnCast => CueKind::OnCast,
                ObCueKind::OnWindow => CueKind::OnWindow,
                ObCueKind::OnHit => CueKind::OnHit,
            };
            commands.queue({
                let lane = lane.clone();
                let msg = CueMessage {
                    lane_id: lane.lane_id.clone(),
                    kind,
                    source: cue.source,
                    position: cue.position,
                    event: lane,
                };
                move |world: &mut World| {
                    world.write_message(msg);
                }
            });
        });
    }
}
```

> **Why route through a `CueMessage` instead of spawning VFX directly in the observer?** The
> `observe_cue` handler can't take `Res<AssetServer>` / `ResMut<Assets<Mesh>>`. Emitting a Message
> lets a normal `arena_game` system (with full SystemParam access) read it and spawn particles /
> projectiles / drive anim. This keeps `arena_skills` render-free and headless-testable.
>
> **Alternative (simpler) discovery:** rather than the `cue_ids` map plumbing, the game can store the
> `SkillFx` assets in a resource and register a single broad observer per cue_id by reading the
> `vfx_cues` from `CastTimelineHandles` at startup. The plan should pick ONE; the map approach above
> is the explicit, testable version.

### `assets/skills/firebolt.skillfx.ron` (matching the REAL fixture cue keys)

The live firebolt `.cast.ron` (`/Users/luke/src/obelisk-bevy/assets/skills/firebolt.cast.ron`, read
verbatim) declares:

```ron
vfx_cues: { "on_cast": "firebolt_cast", "on_hit": "firebolt_impact" },
```

So obelisk fires `CueEvent.cue_id == "firebolt_cast"` (OnCast) and `"firebolt_impact"` (OnHit).
There is **NO `on_window_bolt` key** in the fixture — so no OnWindow cue fires for firebolt out of
the box. The `.skillfx.ron` MUST key its lanes by those exact cue_id strings:

```ron
(
    skill_id: "firebolt",
    lanes: {
        "firebolt_cast": (
            lane_id: "firebolt_muzzle",
            kind: OnCast,
            particle: Some(( count: 12, lifetime: 0.4, color: (1.0, 0.5, 0.1), speed: 4.0 )),
            projectile: Some(( speed: 20.0, color: (1.0, 0.4, 0.05), radius: 0.2 )),
            anim: Some(( state: "cast_release" )),
        ),
        "firebolt_impact": (
            lane_id: "firebolt_impact",
            kind: OnHit,
            particle: Some(( count: 20, lifetime: 0.5, color: (1.0, 0.3, 0.05), speed: 5.0 )),
        ),
    },
)
```

> ⚠️ **Cue-key gotcha (silent miss):** if a lane tries to consume `"on_window_bolt"` but the
> firebolt window id is `"bolt"` AND the `.cast.ron` has no `on_window_bolt` entry in `vfx_cues`,
> NO cue fires — silently. To get a per-window cue (e.g. a trail), you must ALSO add
> `"on_window_bolt": "firebolt_trail"` to the `.cast.ron`'s `vfx_cues` map. For M1 the cosmetic
> projectile is spawned on the **OnCast** lane (`firebolt_cast`) and flown by the game, so we don't
> need the window cue.

---

## 3. obelisk spawn + cast recipe (M1) — verbatim minimal code

All signatures verified against the input JSON + `obelisk-bevy/src/vfx.rs`. The spawn/cast patterns
are copied from `examples/screenshot.rs` and `examples/playground.rs`.

### App initialization (windowed client)

```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use std::path::Path;

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);          // sets AssetServer file_path = "assets"
    app.add_plugins(ObeliskSimPlugin);         // FixedUpdate: Validate->Advance->Projectiles->ResolveHits->TickEffects
    app.add_plugins(obelisk_bevy::present::ObeliskPresentPlugin); // present feature; omit for headless
    app.add_plugins(arena_skills::ArenaSkillsPlugin);

    app.add_obelisk_config_constants_default(); // idempotent obelisk stat_core constants
    if !stat_core::config::effect_registry_initialized() {
        stat_core::init_effect_registry(Path::new("../obelisk-bevy/tests/fixtures/effects")).unwrap();
    }
    // obelisk Skill rules (the .toml WHAT layer). SingleSource::Dir loads a directory of *.toml.
    app.add_obelisk_skills(SkillSource::Dir("config/skills".into()));
    app.seed_combat_rng(0xC0FFEE);             // deterministic ChaCha8 seed
    app.insert_resource(Time::<Fixed>::from_hz(60.0));
    app
}
```

> The `.cast.ron` timelines (the WHEN layer) load through the AssetServer + `CastTimelineHandles`
> registry — copy the `load_cast_assets` / `poll_cast_assets` pair from the input JSON's
> `exampleCode` verbatim (it's the real screenshot.rs/playground.rs recipe). Put firebolt's
> `.cast.ron` at `assets/skills/firebolt.cast.ron`.

### Spawn a Combatant (verbatim from `examples/screenshot.rs:270-283`)

```rust
fn spawn_actor(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    actor: &ActorSpec,
) {
    let color = match actor.faction {
        Faction::Player => Color::srgb(0.2, 0.5, 1.0),
        Faction::Enemy => Color::srgb(1.0, 0.3, 0.2),
        Faction::Neutral => Color::srgb(0.7, 0.7, 0.7),
    };
    let e = commands
        .spawn_empty()
        .make_combatant(actor.stat_block())   // inserts Combatant+Attributes+ObeliskId (== block.id)
        .insert((
            actor.faction,
            Transform::from_translation(actor.pos),
            Mesh3d(meshes.add(Capsule3d::new(0.3, 1.0))),
            MeshMaterial3d(mats.add(color)),
        ))
        .id();

    for skill in &actor.skills {
        commands.entity(e).grant_skill(skill.clone());  // appends to SkillSlots
    }
}
```

Verb signatures (from `verbs.rs` / `timeline/cast.rs`):
- `make_combatant(&mut self, block: StatBlock) -> &mut Self` — `verbs.rs:28-31`. **Invariant:
  `ObeliskId == block.id`** (enforced here).
- `grant_skill(&mut self, skill_id: impl Into<String>) -> &mut Self` — `verbs.rs:33-43`.
- `Combatant` is a required-components root: `#[require(Attributes, Faction, SkillSlots, ObeliskId, Transform)]`.

### Cast firebolt (verbatim from `examples/screenshot.rs:366-384`)

```rust
match action {
    Action::Cast { caster, skill, aim } => {
        let Some(c) = index.entity(caster) else { return; };
        match aim {
            Aim::Entity(target) => {
                if let Some(t) = index.entity(target) {
                    commands.entity(c).cast_skill_at(skill.clone(), t);
                }
            }
            Aim::Point(p) => { commands.entity(c).cast_skill_at_point(skill.clone(), *p); }
            Aim::Dir(d) => {
                if let Ok(dir) = Dir3::new(*d) {
                    commands.entity(c).cast_skill_dir(skill.clone(), dir);
                }
            }
        }
    }
}
```

Cast verb signatures (`timeline/cast.rs:34-54`):
- `cast_skill_at(skill_id: impl Into<String>, target: Entity)` → inserts `PendingCast { skill_id, aim: CastAim::Entity(target) }`.
- `cast_skill_at_point(skill_id, point: Vec3)` → `CastAim::Point`.
- `cast_skill_dir(skill_id, dir: Dir3)` → `CastAim::Direction`.

For the M1 demo, the simplest path is: on a key press, `cast_skill_at("firebolt", nearest_enemy)`
using `ObeliskSpatial::nearest_enemy` (the playground's `Space` behavior).

### firebolt `.cast.ron` (the live fixture — copy verbatim)

`/Users/luke/src/obelisk-bevy/assets/skills/firebolt.cast.ron`:

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
  vfx_cues: { "on_cast": "firebolt_cast", "on_hit": "firebolt_impact" },
)
```

- Phases: Windup 0.3s → Active 0.1s → Recovery 0.2s (total 0.6s base; speed-scaled at cast start).
- Window `bolt` spawns at t=0.3s (Active start), lives 2.0s, moves `Linear(20.0)` → the hitbox gets a
  `Projectile { velocity: aim_dir * 20.0 }` component (NOT speed-scaled).
- **vfx_cues keys (ACTUAL):** `"on_cast" -> "firebolt_cast"`, `"on_hit" -> "firebolt_impact"`. No
  `on_window_*` key.

### firebolt `.toml` (the live fixture — the WHAT layer)

`/Users/luke/src/obelisk-bevy/tests/fixtures/skills/firebolt.toml` — copy to `config/skills/firebolt.toml`:

```toml
id = "firebolt"
name = "Firebolt"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0

[[effect_applications]]
effect_id = "burn"
target = "target"
apply_chance = "always"
[effect_applications.scaling.damage_driven]
conversions = { fire = 0.5 }

[damage]
base_damages = [{ type = "fire", min = 20.0, max = 20.0 }]
```

> **Two-file authoring is mandatory:** every castable skill = `config/skills/<id>.toml` (obelisk
> rules) + `assets/skills/<id>.cast.ron` (timeline). For arena we add a THIRD optional file
> `assets/skills/<id>.skillfx.ron` (cosmetics, §2) — but obelisk only knows about the first two.

---

## 4. wisp rig / anim / controller copy plan

All copied from `/Users/luke/src/wisp/src/player/`. The character glb is
`/Users/luke/src/wisp/assets/character.glb` (copy it to `obelisk-arena/assets/character.glb`).

> Naming note: wisp's marker is `LocalWizardBody` and asset resource is `WizardAssets` — rename to
> `ArenaBody` / `RigAssets` (cosmetic). Functionally identical.

### Exact source ranges to copy (verified line numbers)

| Source | Lines | What | Adaptation |
|---|---|---|---|
| `visuals.rs` | `71-81` | clip-name constants (`IDLE_CLIP`..`CAST_WALK_R_CLIP`) | copy verbatim — these are REAL named tracks in `character.glb` |
| `visuals.rs` | `211-249` | `build_graph_when_loaded` — loads named clips into `AnimationGraph`, stores `AnimationNodeIndex` | copy structure; drive cast clips from `ActiveCast.phase` instead of `CastState` |
| `visuals.rs` | `364-...` | `attach_animation_graph` — attaches graph + starts clips muted except idle | wait on `Combatant` instead of `WizardAssets`-ready |
| `visuals.rs` | `437-...` | `locomotion_blend` + `step_airborne_blend` + `step_casting_blend` | keep exponential-smooth helpers; replace velocity walk-selection with `ActiveCast.phase`-based selection; drop airborne |
| `visuals.rs` | `524-...` | `apply_locomotion_blend` — `player.play(node).repeat().set_weight(w)` | keep verbatim |
| `visuals.rs` | `548-...` | `drive_animation` per-frame system | replace `LinearVelocity + CastState` reads with `ActiveCast` existence + `.phase` |
| `controller.rs` | `212` | `pub const AIM_PITCH_BONE: &str = "chest_joint";` | copy verbatim |
| `controller.rs` | `219-249` | `apply_aim_pitch_to_local_spine` + `ancestor_has_body_marker` | COPY VERBATIM, rename marker |
| `mod.rs` | `207-...` | `spawn_player` hierarchy (Player + camera + body `SceneRoot`) | keep scene load + hierarchy; swap physics body to match obelisk's spatial model |

### AnimationGraph loader (copy verbatim, adapt cast-clip driving)

The clip names below are REAL tracks in `character.glb` (verified `visuals.rs:71-81`). Query
`gltf.named_animations.keys()` to confirm which exist before relying on cast-walk variants.

```rust
const IDLE_CLIP: &str = "idle";
const WALK_F_CLIP: &str = "walk_forward";
const WALK_B_CLIP: &str = "walk_backward";
const WALK_L_CLIP: &str = "walk_left";
const WALK_R_CLIP: &str = "walk_right";
const FALL_CLIP: &str = "falling";
const CAST_IDLE_CLIP: &str = "casting_idle";
const CAST_WALK_F_CLIP: &str = "casting_walk_forward";
const CAST_WALK_B_CLIP: &str = "casting_walk_backward";
const CAST_WALK_L_CLIP: &str = "casting_walk_left";
const CAST_WALK_R_CLIP: &str = "casting_walk_right";

pub fn build_graph_when_loaded(
    mut rig: ResMut<RigAssets>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if rig.ready() { return; }
    let Some(gltf) = gltfs.get(&rig.gltf) else { return; };

    let mut graph = AnimationGraph::new();
    let root = graph.root;
    let mut add = |name: &str| -> Option<AnimationNodeIndex> {
        gltf.named_animations.get(name).map(|clip| graph.add_clip(clip.clone(), 1.0, root))
    };
    rig.idle = add(IDLE_CLIP);
    rig.cast_idle = add(CAST_IDLE_CLIP);
    // ... (the rest as in visuals.rs:231-241)
    if rig.idle.is_none() {
        warn!("character.glb is missing animation \"{IDLE_CLIP}\"");
        return;
    }
    rig.graph = Some(graphs.add(graph));
}
```

### Bone-by-Name spine pitch (copy VERBATIM)

`character.glb`'s rig is `pelvis_joint → waist_joint → chest_joint → neck_joint`. `chest_joint` is
the ONLY spine bone used for aim pitch. `ancestor_has_body_marker` walks the `ChildOf` parent chain
(NOT `Transform` parents) to confirm a bone belongs to the local body before mutating it.

```rust
pub const AIM_PITCH_BONE: &str = "chest_joint";

fn ancestor_has_body_marker(
    entity: Entity,
    parents: &Query<&ChildOf>,
    marker: &Query<(), With<ArenaBody>>, // renamed from LocalWizardBody
) -> bool {
    let mut cur = entity;
    loop {
        if marker.contains(cur) { return true; }
        match parents.get(cur) {
            Ok(p) => cur = p.0,
            Err(_) => return false,
        }
    }
}

// Adapted: read pitch from the third-person aim instead of wisp's first-person Facing.pitch.
pub fn apply_aim_pitch_to_local_spine(
    aim_pitch: Res<AimPitch>,            // arena's own resource (camera/controller-derived)
    bones: Query<(Entity, &Name)>,
    parents: Query<&ChildOf>,
    body_marker: Query<(), With<ArenaBody>>,
    mut transforms: Query<&mut Transform>,
) {
    let pitch_quat = Quat::from_axis_angle(Vec3::Z, -aim_pitch.0);
    for (entity, name) in &bones {
        if name.as_str() != AIM_PITCH_BONE { continue; }
        if !ancestor_has_body_marker(entity, &parents, &body_marker) { continue; }
        if let Ok(mut tf) = transforms.get_mut(entity) {
            tf.rotation = tf.rotation * pitch_quat; // post-multiply onto the animated rotation
        }
    }
}
```

> **PostUpdate scheduling is critical:** `apply_aim_pitch_to_local_spine` MUST run AFTER
> `AnimationSystems` (bone Transforms animated first) and BEFORE `TransformSystems::Propagate` (so
> the modification is included in `GlobalTransform`). Violating the order makes the pitch invisible
> or makes it fight the animation. Schedule with `.after(AnimationSystems).before(TransformSystems::Propagate)`.

### The cast-anim adaptation (the M1 substance)

wisp drives casting clips from a velocity-based `CastState`. Arena drives them from obelisk's
`ActiveCast.phase` (`SkillPhase { Windup, Active, Recovery, Done }`,
`/Users/luke/src/obelisk-bevy/src/timeline/state.rs:4-10`). The mapping:

```rust
// In drive_animation, replace the (LinearVelocity, CastState) read with:
//   active_cast: Option<&ActiveCast>  on the player combatant entity
let casting_blend = match active_cast.map(|c| c.phase) {
    Some(SkillPhase::Windup)   => /* ease toward cast_idle (charging pose) */ 1.0,
    Some(SkillPhase::Active)   => /* release pose */ 1.0,
    Some(SkillPhase::Recovery) => /* ease back out */ 0.5,
    _ /* None | Done */        => 0.0,
};
// Keep step_casting_blend (exponential follower) so the pose doesn't pop.
apply_locomotion_blend(&mut player, &rig, world_velocity, yaw, /*airborne*/ 0.0, casting_blend);
```

> **Why a third-person controller (not wisp's FPS controller):** the M1 gate is "third-person
> character". Reuse the rig + anim graph + spine code, but the camera is an over-the-shoulder follow
> cam, not wisp's first-person. The spine-pitch input is the aim pitch derived from the
> camera/controller, fed via an `AimPitch` resource (replacing wisp's `Facing.pitch`).
>
> **Physics body:** wisp's `spawn_player` uses `RigidBody::Kinematic` + `Collider::capsule`. obelisk
> puts a hurtbox collider (`RigidBody::Static`) on the combatant entity itself via `insert_hurtbox`.
> For M1 single-player, keep the player as a `Combatant` (so it can cast), give it a simple kinematic
> capsule for movement, and don't double-insert a hurtbox unless you want the player hittable. Drop
> `LinearVelocity` reads in `drive_animation` — compute `world_velocity` from frame delta of the
> controller's own movement instead.

---

## 5. M1 particle + cosmetic projectile

**Chosen approach: the simple emissive-billboard stand-in, NOT the `bevy_vfx` plugin.**

Rationale:
- `bevy_vfx::VfxPlugin` wires a billboard render node into `Core3d` (`bevy_vfx/src/lib.rs`) and
  GPU compute pipelines — heavy coupling for a single muzzle/impact burst, and it auto-registers a
  large reflect surface. The M1 gate only needs "a particle" to be visible.
- The emissive stand-in is **100% pure Bevy**, no GPU compute, no external textures, and works in
  `DefaultPlugins` without feature surgery. Trade-off: static billboards (no curl/drift), CPU
  per-frame material fade, scales poorly past ~50 simultaneous — fine for M1.
- The plan can swap to `bevy_vfx::VfxSystem`/`EmitterDef::Once { count, offset }` later without
  changing the LaneEvent contract (the `ParticleSpec` fields map straight onto a `SpawnModule::Once`).

### Particle spawn (consume `CueMessage` in `arena_game`)

```rust
#[derive(Component)]
struct ParticleLifetime { elapsed: f32, duration: f32 }

fn spawn_cue_cosmetics(
    mut msgs: MessageReader<CueMessage>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for m in msgs.read() {
        // 1) particle burst (emissive billboard stand-in)
        if let Some(p) = &m.event.particle {
            let c = LinearRgba::rgb(p.color[0], p.color[1], p.color[2]);
            let material = materials.add(StandardMaterial {
                emissive: c * 2.0,
                base_color: Color::from(c),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            });
            commands.spawn((
                Mesh3d(meshes.add(Rectangle::new(0.25, 0.25))),
                MeshMaterial3d(material),
                Transform::from_translation(m.position),
                ParticleLifetime { elapsed: 0.0, duration: p.lifetime },
            ));
        }
        // 2) cosmetic flying projectile (OnCast lane only)
        if let Some(proj) = &m.event.projectile {
            let c = LinearRgba::rgb(proj.color[0], proj.color[1], proj.color[2]);
            commands.spawn((
                Mesh3d(meshes.add(Sphere::new(proj.radius))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    emissive: c * 3.0, unlit: true, ..default()
                })),
                Transform::from_translation(m.position),
                CosmeticProjectile { velocity: Vec3::ZERO /* set from aim — see note */, speed: proj.speed },
                ParticleLifetime { elapsed: 0.0, duration: 2.0 }, // matches window active_duration
            ));
        }
    }
}

#[derive(Component)]
struct CosmeticProjectile { velocity: Vec3, speed: f32 }

fn fly_cosmetic_projectiles(time: Res<Time>, mut q: Query<(&CosmeticProjectile, &mut Transform)>) {
    for (proj, mut tf) in &mut q {
        tf.translation += proj.velocity * time.delta_secs();
    }
}

fn age_lifetimes(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut ParticleLifetime)>,
) {
    for (e, mut life) in &mut q {
        life.elapsed += time.delta_secs();
        if life.elapsed >= life.duration {
            commands.entity(e).despawn();
        }
    }
}
```

> **Projectile direction:** the OnCast `CueEvent.position` is the caster's translation, but it
> carries no aim direction. For the cosmetic projectile to fly the right way, compute the aim from
> the caster→target vector at spawn (the same target you passed to `cast_skill_at`), or — cleaner —
> add an `"on_window_bolt"` cue to firebolt's `.cast.ron` so the cosmetic spawns AT the hitbox with
> the hitbox already moving (OnWindow position is the moving hitbox translation). For M1 the
> simplest correct version: in the cast system, stash the aim dir keyed by caster, and read it in
> `spawn_cue_cosmetics`. Either way the cosmetic is purely visual — the authoritative hit is the
> obelisk `Hitbox`/`Projectile`.

> **If you instead want real `bevy_vfx`:** add `bevy_vfx.workspace = true`, `app.add_plugins(VfxPlugin)`,
> and replace the billboard spawn with `VfxSystem { emitters: vec![EmitterDef { spawn:
> SpawnModule::Once { count: p.count, offset: 0.0 }, ... }], .. }` at `Transform::from_translation(m.position)`.
> Verified types live in `/Users/luke/src/bevy_modal_editor/crates/bevy_vfx/src/data.rs`
> (`VfxSystem` L20, `EmitterDef` L58, `SpawnModule` L223, `InitModule` L422, `RenderModule` L674).
> SetPosition(Point(Vec3::ZERO)) is correct when the VfxSystem entity is AT the socket world pos.

---

## 6. Trace harness

Copy `/Users/luke/src/wisp/src/trace.rs` (read in full, 105 lines) verbatim into a small
`arena_game/src/trace.rs` (or a tiny `arena_trace` crate). It's a self-contained, env-gated JSONL
emitter — no wisp-specific deps. Key facts (verified):

- `static SINK: OnceLock<Option<Mutex<File>>>` — opened once from `WISP_TRACE_FILE` env var; no-op
  when unset.
- `now_secs()` = wall-clock seconds since unix epoch (cross-process correlation; NOT monotonic).
- `src()` = `WISP_TRACE_SRC` env (default `"unknown"`).
- `pub fn event(kind: &str, extra: serde_json::Value)` — merges `{t, src, kind}` + `extra` into one
  JSON line, flushes immediately.
- `TracePlugin` emits a `"start"` sentinel at `Startup`.

> Rename the env vars to `ARENA_TRACE_FILE` / `ARENA_TRACE_SRC` if you want arena-scoped traces (or
> keep `WISP_*` to reuse wisp's net-test tooling later). For M1 single-player the trace is just a
> determinism/debug aid — emit a `lane_event` whenever a `CueMessage` is dispatched.

### Sample emit (in `spawn_cue_cosmetics`)

```rust
use serde_json::json;

trace::event("lane_event", json!({
    "lane_id": m.lane_id,
    "kind": format!("{:?}", m.kind),
    "source": format!("{:?}", m.source),
    "pos_x": m.position.x,
    "pos_y": m.position.y,
    "pos_z": m.position.z,
    "has_particle": m.event.particle.is_some(),
    "has_projectile": m.event.projectile.is_some(),
}));
```

---

## 7. Task-decomposition hints (ordered, bite-sized)

Each task ends in a build/run check. M0 = foundation that compiles + runs an empty window with the
sim wired. M1 = the gate ("third-person character casts firebolt with anim + particle + projectile").

### M0 — foundation

1. **Create `obelisk-arena` workspace + two empty crates.** Write the three manifests from §1
   (pin 0.18 / 0.5, edition 2021, path dep to `../obelisk-bevy` + `../bevy_modal_editor/crates/bevy_vfx`).
   ✅ `cargo build` (empty `lib.rs`/`main.rs`) succeeds; check `Cargo.lock` has a SINGLE bevy 0.18.x + avian 0.5.x.
2. **Copy the trace harness** (§6) into arena. ✅ `cargo build`; run with `ARENA_TRACE_FILE=/tmp/t.jsonl`
   and confirm a `"start"` line appears.
3. **Stand up a minimal `arena_game` window** with `DefaultPlugins` + a camera + a ground plane +
   `ObeliskSimPlugin` + `Time::<Fixed>::from_hz(60)`. ✅ `cargo run` shows an empty 3D scene, no panics.
4. **Wire obelisk config + skill loading.** Copy firebolt `.toml` → `config/skills/`, firebolt
   `.cast.ron` → `assets/skills/`. Add `add_obelisk_config_constants_default`,
   `init_effect_registry`, `add_obelisk_skills(SkillSource::Dir(...))`, `seed_combat_rng`, and the
   `load_cast_assets`/`poll_cast_assets` pair. ✅ `cargo run`; log confirms `CastTimelineHandles`
   contains `"firebolt"` after a few frames.
5. **Spawn a player Combatant + a dummy enemy Combatant** via `make_combatant` + `grant_skill("firebolt")`
   (§3). Render them as capsules. ✅ `cargo run` shows two capsules; ObeliskId == StatBlock.id (assert).
6. **Headless determinism smoke test in `arena_skills`** using `obelisk_bevy::testkit::ObeliskTestApp`:
   spawn caster+target, `cast_skill_at("firebolt", target)`, advance ~24 ticks, assert a
   `DamageResolved`/`EntityDied` event. ✅ `cargo test -p arena_skills` green.

### M1 — the gate

7. **Define `arena_skills` types + `.skillfx.ron` loader** (§2): `SkillFx`, `LaneEvent`, `CueKind`,
   `ParticleSpec`, `ProjectileCosmetic`, `AnimLayer`, `CueMessage`; `SkillFxLoader`; `ArenaSkillsPlugin`.
   Author `firebolt.skillfx.ron` keyed by the REAL cue ids `"firebolt_cast"` / `"firebolt_impact"`.
   ✅ `cargo test -p arena_skills` loads the RON asset (round-trip test).
8. **Cue binding layer:** `register_skill_cues` registers `observe_cue("firebolt_cast", ...)` /
   `observe_cue("firebolt_impact", ...)` that emit `CueMessage`. ✅ headless test: cast firebolt,
   assert a `CueMessage` with `lane_id == "firebolt_muzzle"` is written.
9. **Particle + cosmetic projectile spawn systems** in `arena_game` (§5): `spawn_cue_cosmetics`,
   `fly_cosmetic_projectiles`, `age_lifetimes`, emit `trace::event("lane_event", ...)`. ✅ `cargo run`,
   trigger a cast (debug key), see emissive bursts at muzzle + a flying sphere + an impact burst.
10. **Copy the character rig + AnimationGraph** (§4): copy `character.glb`, copy `build_graph_when_loaded`
    + `attach_animation_graph` + clip constants; attach the graph to the player body `SceneRoot`.
    ✅ `cargo run` shows the rigged character playing the idle clip (no T-pose).
11. **Third-person controller + camera + spine pitch:** over-the-shoulder follow cam; WASD movement
    (kinematic capsule); copy `apply_aim_pitch_to_local_spine` + `ancestor_has_body_marker` (rename
    marker), feed an `AimPitch` resource; schedule in PostUpdate after `AnimationSystems`, before
    `TransformSystems::Propagate`. ✅ `cargo run`: character walks, torso leans with aim.
12. **Cast-phase-driven anim:** adapt `drive_animation` to read `ActiveCast.phase` (Windup→charge,
    Active→release, Recovery→ease-out) via `step_casting_blend`. Bind the cast to a key
    (`cast_skill_at("firebolt", nearest_enemy)` via `ObeliskSpatial`). ✅ **GATE:** `cargo run` shows a
    third-person character that casts firebolt with a cast animation + a particle + a flying projectile.
13. **(polish)** confirm determinism: same `seed_combat_rng` + same inputs ⇒ identical `lane_event`
    trace; run the `arena_skills` golden-style headless test once more. ✅ green.

---

## Top gotchas (carry into the plan)

1. **Version pinning.** 0.18 / 0.5 EVERYWHERE (the `obelisk-bevy/CLAUDE.md` "0.17/0.4" text is
   stale — the manifest is 0.18/0.5). One mismatched patch version compiles but breaks at runtime.
   Verify `Cargo.lock` after the first build.
2. **vfx_cues keys are exact strings.** firebolt's REAL keys are `"firebolt_cast"` (on_cast) and
   `"firebolt_impact"` (on_hit). There is NO `on_window_*` cue — a `.skillfx.ron` lane keyed
   `"on_window_bolt"` fires NOTHING unless you also add that key to the `.cast.ron`. Silent miss.
3. **`observe_cue` handler can't take resources.** It gets `&CueEvent` + `&mut Commands` only. Route
   through a `CueMessage` (Bevy `Message`) so a normal system spawns the VFX. Keeps `arena_skills`
   headless-testable.
4. **PostUpdate ordering for spine pitch.** `apply_aim_pitch_to_local_spine` MUST run after
   `AnimationSystems` and before `TransformSystems::Propagate`, or the lean is invisible / fights the
   clip. `ancestor_has_body_marker` walks `ChildOf` (NOT Transform) parents.
5. **FixedUpdate schedule order is sacred + determinism.** obelisk's sim is
   `Validate→Advance→Projectiles→ResolveHits→TickEffects` (chained). Don't insert systems into the
   wrong set. Combat RNG is the seeded `CombatRng` (ChaCha8) only — never call obelisk's
   `receive_damage`/`resolve_damage` (they use `thread_rng`). The cosmetic projectile is purely
   visual; the authoritative hit is obelisk's `Hitbox`/`Projectile`.

---

## Concise return (plan input)

**Crate manifest decisions:** new sibling workspace `obelisk-arena` (resolver 2, edition **2021** to
match obelisk-bevy, NOT 2024), members `arena_skills` + `arena_game`. Pin **bevy 0.18 / avian3d 0.5
/ ron 0.8** in `[workspace.dependencies]` (the obelisk-bevy/CLAUDE.md "0.17/0.4" claim is stale —
the live Cargo.toml is 0.18/0.5). Path deps: `obelisk-bevy = { path = "../obelisk-bevy" }`,
`bevy_vfx = { path = "../bevy_modal_editor/crates/bevy_vfx" }`. `arena_skills` is render-free (dev-dep
`obelisk-bevy` with `test-support`); `arena_game` carries default features (`present`) + `bevy_vfx`.
lightyear/landmass/rerecast OMITTED (M2/M4).

**firebolt vfx_cues keys (ACTUAL, from the live fixture):**
`{ "on_cast": "firebolt_cast", "on_hit": "firebolt_impact" }`. No `on_window_*` key — so the
`.skillfx.ron` lanes must be keyed `"firebolt_cast"` and `"firebolt_impact"`, and OnWindow cues
don't fire unless you add `"on_window_bolt"` to the `.cast.ron`.

**Particle approach chosen:** the **simple emissive-billboard stand-in** (pure Bevy, no GPU compute,
works under DefaultPlugins) — NOT `bevy_vfx::VfxPlugin` (heavy Core3d render-node + compute
coupling). LaneEvent's `ParticleSpec` maps 1:1 onto `bevy_vfx::SpawnModule::Once` for a later
upgrade with no contract change. Cosmetic projectile = an emissive sphere flown by the game;
authoritative hit stays in obelisk.

**Top 5 gotchas:** (1) pin 0.18/0.5 everywhere, verify Cargo.lock; (2) vfx_cues keys are exact
strings, no firebolt `on_window` cue; (3) `observe_cue` handler can't take resources — route via a
`CueMessage`; (4) spine-pitch must run PostUpdate after AnimationSystems, before Propagate, walking
`ChildOf` not Transform; (5) FixedUpdate set order + seeded `CombatRng` are sacred, cosmetic
projectile is non-authoritative.

**Ordered M0 tasks:** (1) create workspace + 3 manifests, build empty; (2) copy trace harness;
(3) minimal window + ObeliskSimPlugin; (4) wire obelisk config + load firebolt `.toml`/`.cast.ron`;
(5) spawn player+enemy Combatants via make_combatant+grant_skill; (6) headless determinism test in
arena_skills.

**Ordered M1 tasks:** (7) SkillFx/LaneEvent/CueKind/CueMessage types + `.skillfx.ron` loader +
firebolt fixture; (8) cue binding (observe_cue → CueMessage); (9) particle + cosmetic-projectile
spawn systems + trace emit; (10) copy character.glb + AnimationGraph loader + clip constants;
(11) third-person controller + camera + spine-pitch (chest_joint); (12) cast-phase-driven anim
(ActiveCast.phase) + bind cast key — **GATE**; (13) determinism polish.

**Files:** guide at
`/Users/luke/src/obelisk-bevy/docs/superpowers/specs/2026-06-25-m0m1-integration-surface.md`.
Copy sources: `/Users/luke/src/wisp/src/trace.rs`, `/Users/luke/src/wisp/src/player/visuals.rs`
(L71-81 clips, L211-249 graph, L437+ blend, L548+ drive), `/Users/luke/src/wisp/src/player/controller.rs`
(L212 bone const, L219-249 spine pitch), `/Users/luke/src/wisp/assets/character.glb`. obelisk surface:
`/Users/luke/src/obelisk-bevy/src/vfx.rs` (observe_cue), `.../assets/skills/firebolt.cast.ron`,
`.../tests/fixtures/skills/firebolt.toml`. Particle types:
`/Users/luke/src/bevy_modal_editor/crates/bevy_vfx/src/data.rs`.
