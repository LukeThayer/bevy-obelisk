# obelisk-arena M0 + M1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the `obelisk-arena` Cargo workspace + the `arena_skills` and `arena_game` crates, and ship the single-player M1 gate: `cargo run` shows a third-person character that **casts firebolt** with a cast **animation** + a **particle burst** + a **flying projectile** — co-located, no networking.

**Architecture:** A new sibling workspace (`../obelisk-arena`) path-deps the migrated `obelisk-bevy` (Bevy 0.18.1 / Avian 0.5.0) for deterministic combat and `bevy_vfx` (from the editor) for an optional later particle upgrade. `arena_skills` is a render-free crate owning the `.skillfx.ron` cosmetic-binding format and the `observe_cue → CueMessage` bridge; `arena_game` is the windowed client that copies wisp's rig/animation/controller and turns `CueMessage`s into particles/projectiles, with the cast animation driven by obelisk's `ActiveCast.phase`.

**Tech Stack:** Rust (edition 2021), Bevy 0.18, Avian3d 0.5, `ron`/`serde`, obelisk-bevy (path dep), wisp source (copied), `character.glb` rig.

---

## Context every task needs (read first)

- **The build guide (this plan's source of truth for code + paths):** `../obelisk-bevy/docs/superpowers/specs/2026-06-25-m0m1-integration-surface.md`. It contains the verbatim manifests, type defs, the obelisk spawn/cast recipe, the wisp copy-targets with exact line ranges, the particle stand-in, and the firebolt fixture contents. **When this plan says "copy `wisp/src/player/visuals.rs:211-249`", that range is in the guide §4.** Read the guide section referenced by each task.
- **The spec:** `../obelisk-bevy/docs/superpowers/specs/2026-06-25-obelisk-arena-phase1-design.md` (§3 crates, §4 `.skillfx.ron`, §7 characters/anim).
- **Version pinning is load-bearing:** bevy **0.18**, avian3d **0.5**, ron **0.8** everywhere. The `obelisk-bevy/CLAUDE.md` text saying "0.17/0.4" is **stale** — the live `obelisk-bevy/Cargo.toml` is 0.18/0.5. After the first build, verify `Cargo.lock` has a single bevy 0.18.x + a single avian3d 0.5.x (a patch mismatch compiles but fails at runtime with cross-crate type errors).
- **Never edit obelisk-bevy's fixtures.** The arena gets its **own copies** of `firebolt.toml` / `firebolt.cast.ron` under `obelisk-arena/`. Editing `obelisk-bevy/assets/skills/firebolt.cast.ron` would break Phase 0's 39 byte-identical goldens.
- **Verify inferred obelisk APIs against the real examples.** The guide *inferred* some app-config calls (`add_obelisk_config_constants_default`, `add_obelisk_skills`, `SkillSource::Dir`, `init_effect_registry`, the `load_cast_assets`/`poll_cast_assets` pair). Before using them, open `../obelisk-bevy/examples/screenshot.rs` and `examples/playground.rs` and copy the **actual** call names/signatures — those examples are the working single-player recipe. If a name differs, use the real one.
- **Workspace location:** create `obelisk-arena` as a sibling of `obelisk-bevy` (i.e. `/Users/luke/src/obelisk-arena`). Path deps assume that sibling layout. Work directly there (it's a fresh repo; `git init` it in Task 1).

## File structure (created by this plan)

```
/Users/luke/src/obelisk-arena/
  Cargo.toml                         # workspace root (guide §1)
  .gitignore                         # target/, Cargo.lock kept (it's a bin workspace)
  crates/
    arena_skills/
      Cargo.toml
      src/lib.rs                     # SkillFx/LaneEvent/CueMessage types, loader, ArenaSkillsPlugin, binding
    arena_game/
      Cargo.toml
      src/main.rs                    # app wiring, spawn/cast, the M1 demo
      src/trace.rs                   # copied from wisp/src/trace.rs
      src/rig.rs                     # copied AnimationGraph loader + clip consts + cast-anim driving
      src/controller.rs              # third-person controller + camera + spine-pitch
      src/cosmetics.rs               # CueMessage → particle + cosmetic projectile systems
  config/skills/firebolt.toml        # copied from obelisk-bevy fixtures
  assets/skills/firebolt.cast.ron    # copied from obelisk-bevy
  assets/skills/firebolt.skillfx.ron # NEW (guide §2)
  assets/character.glb               # copied from wisp/assets/character.glb
```

---

# M0 — foundation (compiles + runs an empty scene with the sim wired)

### Task 1: Create the workspace + two crate skeletons

**Files:**
- Create: `/Users/luke/src/obelisk-arena/Cargo.toml`, `crates/arena_skills/Cargo.toml`, `crates/arena_skills/src/lib.rs`, `crates/arena_game/Cargo.toml`, `crates/arena_game/src/main.rs`, `.gitignore`

- [ ] **Step 1: Init the repo and directory tree**

```bash
mkdir -p /Users/luke/src/obelisk-arena/crates/arena_skills/src /Users/luke/src/obelisk-arena/crates/arena_game/src
mkdir -p /Users/luke/src/obelisk-arena/assets/skills /Users/luke/src/obelisk-arena/config/skills
cd /Users/luke/src/obelisk-arena && git init -q
printf 'target/\n' > .gitignore
```

- [ ] **Step 2: Write the workspace root `Cargo.toml`** (guide §1)

```toml
[workspace]
resolver = "2"
members = ["crates/arena_skills", "crates/arena_game"]

[workspace.package]
edition = "2021"
license = "MIT"
authors = ["Luke"]

[workspace.dependencies]
bevy = "0.18"
avian3d = "0.5"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ron = "0.8"
thiserror = "1"
anyhow = "1"
obelisk-bevy = { path = "../obelisk-bevy" }
bevy_vfx = { path = "../bevy_modal_editor/crates/bevy_vfx" }
arena_skills = { path = "crates/arena_skills" }
```

- [ ] **Step 3: Write `crates/arena_skills/Cargo.toml`** (guide §1)

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
anyhow.workspace = true

[dev-dependencies]
obelisk-bevy = { path = "../../../obelisk-bevy", features = ["test-support"] }
```

- [ ] **Step 4: Write `crates/arena_game/Cargo.toml`** (guide §1)

```toml
[package]
name = "arena_game"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
bevy.workspace = true
avian3d.workspace = true
obelisk-bevy.workspace = true
arena_skills.workspace = true
bevy_vfx.workspace = true
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 5: Write placeholder crate roots**

`crates/arena_skills/src/lib.rs`:
```rust
//! arena_skills — the .skillfx.ron cosmetic-binding layer for obelisk skills.
```
`crates/arena_game/src/main.rs`:
```rust
fn main() {
    println!("arena_game placeholder");
}
```

- [ ] **Step 6: Build the empty workspace and verify versions**

Run: `cd /Users/luke/src/obelisk-arena && cargo build`
Expected: builds (fetches/compiles obelisk-bevy + bevy + bevy_vfx the first time — slow). Then:
Run: `grep -E '^name = "(bevy|avian3d)"' Cargo.lock -A1 | grep version`
Expected: exactly one `bevy` 0.18.x and one `avian3d` 0.5.x. If there are two versions of either, fix the manifests before continuing.

- [ ] **Step 7: Commit**

```bash
cd /Users/luke/src/obelisk-arena
git add -A
git commit -m "M0: obelisk-arena workspace + arena_skills/arena_game skeletons

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 2: Copy the trace harness

**Files:**
- Create: `crates/arena_game/src/trace.rs` (copied from `/Users/luke/src/wisp/src/trace.rs`)
- Modify: `crates/arena_game/src/main.rs`

- [ ] **Step 1: Copy `wisp/src/trace.rs` verbatim into `crates/arena_game/src/trace.rs`**

Read `/Users/luke/src/wisp/src/trace.rs` (≈105 lines, self-contained JSONL emitter) and copy it. Rename the env vars `WISP_TRACE_FILE`/`WISP_TRACE_SRC` → `ARENA_TRACE_FILE`/`ARENA_TRACE_SRC` (guide §6). It exposes `pub fn event(kind: &str, extra: serde_json::Value)` and `TracePlugin` (emits a `"start"` line at `Startup`).

- [ ] **Step 2: Wire it into a minimal main**

`crates/arena_game/src/main.rs`:
```rust
mod trace;
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugins(trace::TracePlugin)
        .add_systems(Startup, || info!("arena_game up"))
        .run();
}
```

- [ ] **Step 3: Build and smoke-test the trace**

Run: `cd /Users/luke/src/obelisk-arena && cargo build`
Expected: builds.
Run: `ARENA_TRACE_FILE=/tmp/arena_t.jsonl cargo run -p arena_game` then (after it starts, Ctrl-C if it doesn't exit) `head -1 /tmp/arena_t.jsonl`
Expected: a JSON line with `"kind":"start"`.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M0: copy wisp trace harness (ARENA_TRACE_* env-gated JSONL)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 3: Minimal window + ObeliskSimPlugin

**Files:**
- Modify: `crates/arena_game/src/main.rs`

- [ ] **Step 1: Replace main with a windowed app + the obelisk sim + a 3D scene**

```rust
mod trace;
use bevy::prelude::*;
use obelisk_bevy::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.add_plugins(ObeliskSimPlugin);
    app.add_plugins(trace::TracePlugin);
    app.insert_resource(Time::<Fixed>::from_hz(60.0));
    app.add_systems(Startup, setup_scene);
    app.run();
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 6.0, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((DirectionalLight::default(), Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
    ));
}
```

(If `ObeliskSimPlugin` / `obelisk_bevy::prelude` differ, confirm the real names from `../obelisk-bevy/examples/screenshot.rs` per the context note.)

- [ ] **Step 2: Run it**

Run: `cd /Users/luke/src/obelisk-arena && cargo run -p arena_game`
Expected: a window opens showing a green ground plane under a light; no panics. Close the window to exit.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M0: windowed arena_game with ObeliskSimPlugin + 3D scene

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4: Wire obelisk config + load the firebolt skill

**Files:**
- Create: `config/skills/firebolt.toml` (copy from `../obelisk-bevy/tests/fixtures/skills/firebolt.toml`), `assets/skills/firebolt.cast.ron` (copy from `../obelisk-bevy/assets/skills/firebolt.cast.ron`)
- Modify: `crates/arena_game/src/main.rs`

- [ ] **Step 1: Copy the two firebolt fixture files into the arena**

```bash
cp /Users/luke/src/obelisk-bevy/tests/fixtures/skills/firebolt.toml /Users/luke/src/obelisk-arena/config/skills/firebolt.toml
cp /Users/luke/src/obelisk-bevy/assets/skills/firebolt.cast.ron /Users/luke/src/obelisk-arena/assets/skills/firebolt.cast.ron
```
(If those source paths differ, `find /Users/luke/src/obelisk-bevy -name 'firebolt.*'` to locate the real `.toml` + `.cast.ron`.)

- [ ] **Step 2: Add the obelisk config + skill-loading wiring to `setup` / app build**

Open `../obelisk-bevy/examples/screenshot.rs` + `examples/playground.rs` and **copy the actual** config/skill/cast-asset wiring (the guide §3 shows the inferred shape — use the real names):
- obelisk stat_core constants init,
- effect registry init (points at obelisk's `tests/fixtures/effects` or an arena copy),
- skill `.toml` loading from `config/skills`,
- `seed_combat_rng(<u64>)`,
- the `CastTimeline` `.cast.ron` asset load + the poll-until-registered system.

Add them to the app builder. Use the exact function names from the examples.

- [ ] **Step 3: Run and confirm firebolt registers**

Run: `cd /Users/luke/src/obelisk-arena && cargo run -p arena_game`
Expected: no panics; add a temporary log that prints the loaded skill ids (or the `CastTimelineHandles` keys) after a few frames and confirm `"firebolt"` appears. Remove the temp log after confirming.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M0: wire obelisk config + load firebolt skill (.toml + .cast.ron)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 5: Spawn player + dummy Combatants

**Files:**
- Modify: `crates/arena_game/src/main.rs`

- [ ] **Step 1: Add a spawn helper + spawn two combatants** (guide §3, mirrors `screenshot.rs`)

```rust
use obelisk_bevy::prelude::*;

fn spawn_combatants(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Player at origin, enemy dummy out along +Z. Use the real StatBlock builder from the examples.
    spawn_one(&mut commands, &mut meshes, &mut materials, "player", Faction::Player, Vec3::ZERO, &["firebolt"]);
    spawn_one(&mut commands, &mut meshes, &mut materials, "dummy", Faction::Enemy, Vec3::new(0.0, 0.0, 6.0), &[]);
}

fn spawn_one(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    id: &str,
    faction: Faction,
    pos: Vec3,
    skills: &[&str],
) {
    let color = match faction {
        Faction::Player => Color::srgb(0.2, 0.5, 1.0),
        Faction::Enemy => Color::srgb(1.0, 0.3, 0.2),
        _ => Color::srgb(0.7, 0.7, 0.7),
    };
    // Build a StatBlock with id == `id` using the example's helper (verify the real API).
    let block = make_stat_block(id); // <- replace with the real StatBlock construction from the examples
    let e = commands
        .spawn_empty()
        .make_combatant(block)
        .insert((
            faction,
            Transform::from_translation(pos),
            Mesh3d(meshes.add(Capsule3d::new(0.3, 1.0))),
            MeshMaterial3d(materials.add(color)),
        ))
        .id();
    for s in skills {
        commands.entity(e).grant_skill(s.to_string());
    }
}
```

Replace `make_stat_block` with the example's real `StatBlock` construction (the guide notes `make_combatant` enforces `ObeliskId == block.id`). Register `spawn_combatants` on `Startup` after the scene.

- [ ] **Step 2: Run and confirm two capsules**

Run: `cd /Users/luke/src/obelisk-arena && cargo run -p arena_game`
Expected: a blue capsule at origin and a red capsule ahead of it. No panics.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M0: spawn player + dummy obelisk Combatants

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 6: Headless determinism smoke test in `arena_skills`

**Files:**
- Create: `crates/arena_skills/tests/cast_smoke.rs`

- [ ] **Step 1: Write a headless test that casts firebolt and asserts a damage event** (guide §3 + obelisk testkit)

```rust
// Uses obelisk-bevy's test-support harness. Verify the real testkit API names from
// ../obelisk-bevy/src/testkit.rs and ../obelisk-bevy/tests/*.rs.
use obelisk_bevy::testkit::*;

#[test]
fn firebolt_cast_resolves_damage() {
    let mut app = ObeliskTestApp::new();        // <- real harness type from testkit.rs
    app.load_skill_dir("../../obelisk-bevy/tests/fixtures/skills");
    app.load_cast_dir("../../obelisk-bevy/assets/skills");
    app.seed_combat_rng(0xC0FFEE);
    let caster = app.spawn_combatant("caster", /*faction*/ Enemy_or_Player, Vec3::ZERO, &["firebolt"]);
    let target = app.spawn_combatant("target", /*enemy*/ Faction::Enemy, Vec3::new(0.0, 0.0, 3.0), &[]);
    app.cast(caster, "firebolt", target);
    app.advance_ticks(40);
    assert!(app.saw_event_kind("DamageResolved"), "firebolt should resolve damage by tick 40");
}
```

The exact testkit method names (`ObeliskTestApp`, `spawn_combatant`, `cast`, `advance_ticks`, event assertions) **must be taken from `../obelisk-bevy/src/testkit.rs` and the existing `../obelisk-bevy/tests/` files** — copy their real patterns; the names above are placeholders to be replaced with the actual API.

- [ ] **Step 2: Run the test**

Run: `cd /Users/luke/src/obelisk-arena && cargo test -p arena_skills`
Expected: PASS — firebolt resolves damage deterministically.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "M0: headless determinism smoke test (firebolt resolves damage)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

# M1 — the gate (third-person character casts firebolt with anim + particle + projectile)

### Task 7: `arena_skills` types + `.skillfx.ron` loader + firebolt fixture

**Files:**
- Modify: `crates/arena_skills/src/lib.rs`
- Create: `assets/skills/firebolt.skillfx.ron`, `crates/arena_skills/tests/skillfx_load.rs`

- [ ] **Step 1: Add the SkillFx types + loader + plugin** (guide §2 — copy verbatim)

Put into `crates/arena_skills/src/lib.rs` the full set from guide §2: `SkillFx` (`#[derive(Asset, TypePath, Debug, Clone, Deserialize)]`), `LaneEvent`, `CueKind`, `ParticleSpec`, `ProjectileCosmetic`, `AnimLayer`, `CueMessage` (`#[derive(Message, Debug, Clone)]`), the `SkillFxLoader` (`AssetLoader` for `.skillfx.ron`), and `ArenaSkillsPlugin` (`init_asset::<SkillFx>()` + `register_asset_loader(SkillFxLoader)` + `add_message::<CueMessage>()`). Use the exact code in guide §2.

- [ ] **Step 2: Author `assets/skills/firebolt.skillfx.ron`** (guide §2 — keyed by the REAL cue ids)

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

The lane keys (`firebolt_cast`, `firebolt_impact`) are the **values** in firebolt's `.cast.ron` `vfx_cues` map — that is the `cue_id` obelisk fires. The projectile speed `20.0` matches the `.cast.ron` window `motion: Linear(speed: 20.0)`.

- [ ] **Step 3: Write a load round-trip test**

```rust
// crates/arena_skills/tests/skillfx_load.rs
use arena_skills::SkillFx;

#[test]
fn firebolt_skillfx_deserializes() {
    let ron = std::fs::read_to_string("../../assets/skills/firebolt.skillfx.ron").unwrap();
    let fx: SkillFx = ron::de::from_str(&ron).unwrap();
    assert_eq!(fx.skill_id, "firebolt");
    assert!(fx.lanes.contains_key("firebolt_cast"));
    assert!(fx.lanes.contains_key("firebolt_impact"));
    assert!(fx.lanes["firebolt_cast"].projectile.is_some());
}
```

- [ ] **Step 4: Run the test**

Run: `cd /Users/luke/src/obelisk-arena && cargo test -p arena_skills skillfx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "M1: arena_skills .skillfx.ron types + loader + firebolt fixture

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 8: Cue binding layer (`observe_cue → CueMessage`)

**Files:**
- Modify: `crates/arena_skills/src/lib.rs`
- Create: `crates/arena_skills/tests/cue_binding.rs`

- [ ] **Step 1: Add the binding registration** (guide §2 "Binding layer")

Add `register_skill_cues` (guide §2): for each loaded `SkillFx` lane keyed by `cue_id`, call obelisk's `app.observe_cue(cue_id, move |cue: &CueEvent, commands: &mut Commands| { ... commands.queue(|world| world.write_message(CueMessage{...})) })`, mapping obelisk `CueKind`→arena `CueKind`. Use the exact code in guide §2. (Confirm `observe_cue`'s real signature in `../obelisk-bevy/src/vfx.rs`: `Fn(&CueEvent, &mut Commands)`.)

- [ ] **Step 2: Headless test — cast fires a CueMessage**

```rust
// crates/arena_skills/tests/cue_binding.rs
// Build a headless app with ObeliskSimPlugin + ArenaSkillsPlugin, register firebolt's cues,
// cast firebolt, advance, and assert a CueMessage with lane_id "firebolt_muzzle" was written.
// Use the real testkit + the MessageReader drain pattern from ../obelisk-bevy/tests.
```

Write the concrete test against the real testkit API (mirror Task 6's harness). Assert `lane_id == "firebolt_muzzle"` appears after the cast's `on_cast` cue fires.

- [ ] **Step 3: Run**

Run: `cd /Users/luke/src/obelisk-arena && cargo test -p arena_skills cue`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M1: cue binding (observe_cue -> CueMessage)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 9: Particle + cosmetic projectile spawn systems

**Files:**
- Create: `crates/arena_game/src/cosmetics.rs`
- Modify: `crates/arena_game/src/main.rs`

- [ ] **Step 1: Add the cosmetics systems** (guide §5 — emissive stand-in, copy verbatim)

Put into `crates/arena_game/src/cosmetics.rs` the guide §5 systems: `ParticleLifetime`, `CosmeticProjectile`, `spawn_cue_cosmetics` (reads `MessageReader<CueMessage>`, spawns an emissive `Rectangle` billboard per particle + an emissive `Sphere` per projectile), `fly_cosmetic_projectiles`, `age_lifetimes`. Add the `trace::event("lane_event", …)` emit (guide §6 sample) inside `spawn_cue_cosmetics`.

For the projectile **direction** (guide §5 note): in the cast system, stash the aim dir keyed by caster (a small resource `HashMap<Entity, Vec3>`), and set `CosmeticProjectile.velocity = aim_dir * proj.speed` in `spawn_cue_cosmetics`. (The `on_cast` `CueEvent.position` is the caster; it carries no direction.)

- [ ] **Step 2: Register the systems + `ArenaSkillsPlugin` + `register_skill_cues`**

In `main.rs`: `app.add_plugins(arena_skills::ArenaSkillsPlugin)`, call `register_skill_cues` after the `.skillfx.ron` assets load, and `app.add_systems(Update, (spawn_cue_cosmetics, fly_cosmetic_projectiles, age_lifetimes))`.

- [ ] **Step 3: Temporary cast key to exercise it**

Add a temporary system: on `KeyCode::Space`, `cast_skill_at("firebolt", dummy)` from the player (you'll replace this with the real bind in Task 12).

Run: `cd /Users/luke/src/obelisk-arena && cargo run -p arena_game`, press Space.
Expected: an emissive burst at the player, a glowing sphere flies toward the dummy, an impact burst at the dummy when it hits. Trace (`ARENA_TRACE_FILE=…`) shows `lane_event` lines.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M1: cue cosmetics — emissive particle burst + flying projectile + trace

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 10: Copy the character rig + AnimationGraph

**Files:**
- Create: `crates/arena_game/src/rig.rs`, `assets/character.glb` (copied)
- Modify: `crates/arena_game/src/main.rs`

- [ ] **Step 1: Copy the rig asset**

```bash
cp /Users/luke/src/wisp/assets/character.glb /Users/luke/src/obelisk-arena/assets/character.glb
```

- [ ] **Step 2: Copy the AnimationGraph loader + clip constants** (guide §4)

Create `crates/arena_game/src/rig.rs`. Copy from `/Users/luke/src/wisp/src/player/visuals.rs`: the clip-name constants (`visuals.rs:71-81` — `idle`/`walk_forward`/…/`casting_idle`/…), `build_graph_when_loaded` (`visuals.rs:211-249` — load named clips into an `AnimationGraph`, store `AnimationNodeIndex`es in a `RigAssets` resource), and `attach_animation_graph` (`visuals.rs:364+` — attach the graph to the body `SceneRoot`, start clips muted except idle). Rename `WizardAssets`→`RigAssets`, `LocalWizardBody`→`ArenaBody`. Guide §4 has the verbatim loader.

- [ ] **Step 3: Spawn the rig on the player + register the loader systems**

Replace the player's capsule mesh with a `SceneRoot` loading `character.glb`'s scene + the `ArenaBody` marker (mirror `wisp/src/player/mod.rs:207+` `spawn_player` hierarchy). Register `build_graph_when_loaded` + `attach_animation_graph` in `Update`.

Run: `cd /Users/luke/src/obelisk-arena && cargo run -p arena_game`
Expected: the player is the rigged character playing the **idle** clip (not a T-pose). The dummy can stay a capsule.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M1: copy character.glb rig + AnimationGraph loader (idle plays)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 11: Third-person controller + camera + spine pitch

**Files:**
- Create: `crates/arena_game/src/controller.rs`
- Modify: `crates/arena_game/src/main.rs`

- [ ] **Step 1: Add an over-the-shoulder follow camera + WASD movement**

Create `crates/arena_game/src/controller.rs`: an `AimPitch(pub f32)` resource (mouse-Y), camera yaw from mouse-X, camera positioned behind/above the player; camera-relative WASD moves the player's `Transform` (kinematic — translate directly, compute `world_velocity` from frame delta per guide §4 note, since we drop `LinearVelocity`).

- [ ] **Step 2: Copy the spine-pitch system** (guide §4 — copy verbatim)

Copy `AIM_PITCH_BONE = "chest_joint"` (`controller.rs:212`), `apply_aim_pitch_to_local_spine` + `ancestor_has_body_marker` (`controller.rs:219-249`), renaming the marker to `ArenaBody` and reading pitch from the `AimPitch` resource (guide §4 verbatim block).

- [ ] **Step 3: Schedule spine-pitch correctly** (guide gotcha #4)

```rust
app.add_systems(
    PostUpdate,
    apply_aim_pitch_to_local_spine
        .after(bevy::animation::AnimationSystems)
        .before(bevy::transform::TransformSystems::Propagate),
);
```
(Confirm the exact 0.18 system-set names — they may be `AnimationSystems`/`TransformSystems` or similar; check a wisp/editor `PostUpdate` ordering site.)

- [ ] **Step 4: Run**

Run: `cd /Users/luke/src/obelisk-arena && cargo run -p arena_game`
Expected: WASD walks the character (locomotion clips blend with movement direction); moving the mouse aims, and the character's torso (`chest_joint`) leans with the aim pitch. No T-pose, no jitter fighting the animation.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "M1: third-person controller + follow cam + chest_joint aim lean

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 12: Cast-phase-driven animation + bind the cast — THE GATE

**Files:**
- Modify: `crates/arena_game/src/rig.rs`, `crates/arena_game/src/controller.rs`, `crates/arena_game/src/main.rs`

- [ ] **Step 1: Drive the cast animation from `ActiveCast.phase`** (guide §4 "cast-anim adaptation")

In the per-frame anim driver (copied `drive_animation`, `visuals.rs:548+`), replace the velocity/`CastState` read with `Option<&ActiveCast>` on the player and map `SkillPhase` → a casting blend (guide §4):
```rust
let casting_blend = match active_cast.map(|c| c.phase) {
    Some(SkillPhase::Windup)   => 1.0,
    Some(SkillPhase::Active)   => 1.0,
    Some(SkillPhase::Recovery) => 0.5,
    _                          => 0.0,
};
// keep step_casting_blend (exponential follower) so the pose doesn't pop
apply_locomotion_blend(&mut player, &rig, world_velocity, yaw, 0.0, casting_blend);
```
Confirm `ActiveCast` + `SkillPhase` (Windup/Active/Recovery/Done) from `../obelisk-bevy/src/timeline/state.rs`.

- [ ] **Step 2: Replace the temporary Space key with the real cast bind**

On the cast key, `cast_skill_at("firebolt", target)` where `target` = `ObeliskSpatial::nearest_enemy(player)` (guide §3). Stash the aim dir (player→target) for the cosmetic projectile (Task 9 Step 1). Remove the Task 9 temporary cast system.

- [ ] **Step 3: THE GATE — run and verify the full loop**

Run: `cd /Users/luke/src/obelisk-arena && cargo run -p arena_game`
Expected: a **third-person character** that, on the cast key, plays a **cast animation** (windup→release→recovery on the upper body while locomotion still blends), spawns a **particle burst** at the muzzle, launches a **flying projectile** toward the enemy, and shows an **impact burst** when it connects. This is the M1 definition of done.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "M1 GATE: cast-phase-driven animation + firebolt cast bind

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 13: Determinism polish + M0/M1 wrap

**Files:**
- Modify: `crates/arena_skills/tests/*` (as needed)

- [ ] **Step 1: Re-confirm determinism**

Run: `cd /Users/luke/src/obelisk-arena && cargo test`
Expected: all `arena_skills` tests green (the cast-resolves-damage + skillfx-load + cue-binding tests).

- [ ] **Step 2: Confirm a clean build of the whole workspace**

Run: `cargo build && cargo build --tests`
Expected: clean. Optionally `cargo clippy -- -D warnings` and `cargo fmt --check`; fix any arena warnings.

- [ ] **Step 3: Final commit**

```bash
git add -A && git commit -m "M1: determinism polish; M0+M1 complete

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage (against the Phase 1 spec §3/§4/§7 + the M0/M1 milestones):**
- Workspace + `arena_skills` + `arena_game` crates (spec §3) → Task 1. ✓
- `.skillfx.ron` format + loader + binding + `CueMessage`/cue contract (spec §4) → Tasks 7, 8. ✓
- obelisk combat spawn/cast (spec §10 M0/M1) → Tasks 4, 5, 6. ✓
- Third-person controller + rig + `ActiveCast.phase`-driven animation (spec §7) → Tasks 10, 11, 12. ✓
- Cue→particle+cosmetic-projectile (spec §10 M1) → Task 9. ✓
- Trace harness front-loaded (spec §9) → Task 2. ✓
- Determinism (spec §9) → Tasks 6, 13. ✓
- Out-of-scope honored: no lightyear/landmass/rerecast (M2/M4), no editing obelisk goldens. ✓

**2. Placeholder scan:** The plan deliberately marks places where the engineer must substitute the **real** API name from the obelisk examples/testkit (`make_stat_block`, the testkit method names, the config-wiring calls) — these are flagged as "verify against `../obelisk-bevy/examples|src/testkit.rs`", not silent TBDs, because the guide honestly inferred them and the source is the authority. All genuinely-new code (manifests, `arena_skills` types, loader, binding, cosmetics systems, the `.skillfx.ron`/`.cast.ron`/`.toml` fixtures) is given verbatim. The wisp copy-targets are exact file:line ranges (the correct granularity for "copy this function").

**3. Type/name consistency:** `SkillFx`/`LaneEvent`/`CueKind`/`ParticleSpec`/`ProjectileCosmetic`/`AnimLayer`/`CueMessage` (Task 7) are used consistently in Tasks 8–9; `RigAssets`/`ArenaBody`/`AimPitch`/`AIM_PITCH_BONE` (Tasks 10–11) reused in Task 12; lane keys `firebolt_cast`/`firebolt_impact` and `lane_id firebolt_muzzle` consistent across Tasks 7–9. Cosmetic projectile speed `20.0` matches the `.cast.ron` window motion.

**4. Scope:** M0+M1 only (single-player, co-located). M2–M4 are separate plan cycles. Each task ends in a build/run/test check; the M1 gate (Task 12) is the visual acceptance.
