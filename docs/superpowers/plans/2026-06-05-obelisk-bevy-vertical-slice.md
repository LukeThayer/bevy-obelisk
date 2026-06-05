# obelisk-bevy Vertical Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the end-to-end vertical slice from spec §11: cast `firebolt` → authoritative cast timeline → spatiotemporal projectile hitbox → overlap with an enemy hurtbox → deterministic obelisk damage/effect/trigger resolution → gameplay events → DoT tick → death, covered by a headless deterministic test and a minimal visual playground.

**Architecture:** A single `obelisk-bevy` crate wrapping the pure-Rust obelisk libraries as a Bevy 0.17 plugin. The authoritative simulation runs in `FixedUpdate` (deterministic, seeded RNG) and is the only writer of `StatBlock`. Overlap detection uses Avian3d `SpatialQuery::shape_intersections` (chosen over collision events because two kinematic sensors do not reliably emit contacts). The core damage funnel is a **pure function** (`combat::resolve_one_hit`) unit-testable without Bevy. Presentation is gated behind a `present` feature and only observes events.

**Tech Stack:** Rust 2021, Bevy `0.17` (default features for the slice), Avian3d `0.4` (the Avian release that targets Bevy 0.17), `serde` + `ron` (cast asset), `rand_chacha` (seeded RNG), obelisk crates via path deps (`stat_core`, `loot_core`, `skill_tree`, `tables_core`).

---

## Verified obelisk facts (ground truth for this plan)

These were confirmed by reading obelisk source; the code below depends on them.

- `StatBlock::new()` and `StatBlock::with_id(impl Into<String>) -> StatBlock`. Public mutable fields incl. `id: String`, `current_life: f64`, `max_life: StatValue`, `attack_speed: StatValue`, `cast_speed: StatValue`. `StatValue::with_base(f64)`, `StatValue.base: f64`, `StatValue::compute() -> f64`. `is_alive() -> bool`, `computed_max_life() -> f64`.
- `StatBlock::use_skill_against(&mut self, target: Option<&StatBlock>, skill: &Skill, registry: &HashMap<String,Skill>, source_id: String, rng: &mut impl Rng) -> Result<SkillResult, SkillUseError>`. Consumes resources + applies caster self-effects internally.
- `SkillResult { packets: Vec<DamagePacket>, mana_spent: f64, barrier_granted: f64, effects_consumed: Vec<ConsumedEffect>, .. }`.
- `SkillUseError { InsufficientMana { cost, available }, ConditionNotMet { condition } }` (impls `Display`).
- `stat_core::combat::resolve_damage_with_triggers(attacker: &StatBlock, defender: &StatBlock, packets: &[DamagePacket], skill: &Skill, registry: &HashMap<String,Skill>, rng: &mut impl Rng) -> TriggerResolutionResult`.
- `TriggerResolutionResult { defender: StatBlock, results: Vec<CombatResult>, on_kill_packets: Vec<DamagePacket>, defender_packets: Vec<DamagePacket> }`.
- `CombatResult { total_damage: f64, is_killing_blow: bool, damage_taken: Vec<DamageTaken>, effects_applied: Vec<Effect>, life_before: f64, life_after: f64, barrier_before: f64, barrier_after: f64, was_dodged: bool, .. }`.
- `StatBlock::tick_effects(&self, delta: f64) -> (StatBlock, TickResult)` (immutable: returns new block).
- `TickResult { dot_damage: f64, expired_effects: Vec<String>, stat_effects_expired: bool, life_remaining: f64, is_dead: bool, triggered_effects: Vec<TriggeredEffect> }`.
- `Effect { id: String, name: String, source_id: String, is_debuff: bool, total_duration: f64, timers: StackTimers, stacks: u32, max_stacks: u32, .. }` (NO flat `duration`).
- Skill loaders return `HashMap<String,Skill>`: `stat_core::config::skills::{load_skills, load_skills_dir, parse_skills}`. `parse_skills` expects a `[[skills]]` array; `load_skills_dir` accepts both `[[skills]]` files and single-skill top-level files. **Do NOT use `default_skills()` / `load_skill_configs()` — they return `HashMap<String,DamagePacketGenerator>`.**
- Globals (OnceLock): `stat_core::init_constants_default() -> Result<(),_>`, `stat_core::init_constants(&Path)`, `stat_core::config::constants_initialized() -> bool`; `stat_core::init_effect_registry(&Path) -> Result<(),_>`, `stat_core::config::effect_registry_initialized() -> bool`, `stat_core::effect_registry() -> &'static EffectRegistry`. `init_*` panic on second call → **always guard with the `*_initialized()` check.**
- Skill TOML (single-skill file): top-level `id`, `name`, `tags`, `targeting` (`"single_enemy"|"self"|"none"`), `delivery` (`"melee"|"projectile"|"instant"`), `mana_cost`, `cooldown`, `attack_speed_modifier`, `[[effect_applications]]` (`effect_id`, `target="target"|"self"`, `apply_chance="always"`, `[effect_applications.scaling.damage_driven] conversions={fire=0.4}`), `[damage]` with `base_damages = [{ type = "fire", min = 20.0, max = 20.0 }]`, `weapon_effectiveness`, `damage_effectiveness`.
- Effect TOML: `id`, `name`, `duration` (→ runtime `Effect.total_duration`), `is_debuff`, `stacking="strongest_only"`, `damage_type="fire"`, `base_damage_percent`, `tick_rate`, `max_stacks`, `[application] type="chance"`, `[[modifiers]]`.

---

## File Structure

```
obelisk-bevy/
  Cargo.toml
  src/
    lib.rs            # crate root: modules, ObeliskPlugins group, ObeliskSimPlugin, ObeliskSet
    prelude.rs        # public re-exports for consumers
    ids.rs            # ObeliskId, ObeliskEntityIndex
    core/
      mod.rs          # ObeliskCorePlugin
      config.rs       # add_obelisk_config app-ext, SkillSource, guarded global init, SkillRegistry, CombatRng
      components.rs    # Attributes, Faction, SkillSlots, Combatant (required components)
      tick.rs         # FixedUpdate tick_effects driver -> DotTicked/EffectExpired/EntityDied
    assets/
      mod.rs          # CastTimeline asset, authoring enums, RON loader, CastTimelineHandles, load-validation
    spatial/
      mod.rs          # ObeliskSpatialPlugin
      shapes.rs       # CollisionShape -> avian Collider
      boxes.rs        # Hurtbox, Hitbox components, HitFilter, HitMode
      detect.rs       # SpatialQuery overlap detection -> HitConfirmed
      projectile.rs   # Projectile motion (FixedUpdate transform integration)
    timeline/
      mod.rs          # ObeliskTimelinePlugin
      cast.rs         # CastSkillExt verb, PendingCast, CastRejectReason
      state.rs        # ActiveCast, SkillPhase, speed-scaling snapshot
      advance.rs      # validate + advance systems: phase transitions, window spawn
    combat/
      mod.rs          # ObeliskCombatPlugin
      resolve.rs      # PURE resolve_one_hit() + HitOutcome (no Bevy)
      system.rs       # ResolveHits system: HitConfirmed -> resolve_one_hit -> events + writeback
    events.rs         # all gameplay events (CastBegan, DamageResolved, ...)
    present/
      mod.rs          # (feature="present") ObeliskPresentPlugin: event-logging observers + debug gizmos
    testkit.rs        # (feature="test-support") init_test_obelisk(), ObeliskTestApp, EventRecorder
  assets/
    skills/firebolt.cast.ron
  tests/
    fixtures/
      skills/firebolt.toml
      effects/burn.toml
    vertical_slice.rs # the end-to-end headless scenario test
  examples/
    playground.rs     # visual 3D app
```

---

## Phase 0 — Scaffold & engine-API probe

### Task 1: Create the crate skeleton and confirm it builds

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "obelisk-bevy"
version = "0.1.0"
edition = "2021"
license = "MIT"

[features]
default = ["present"]
present = []
test-support = []

[dependencies]
bevy = "0.17"
avian3d = "0.4"
serde = { version = "1", features = ["derive"] }
ron = "0.8"
rand = "0.8"
rand_chacha = "0.3"
stat_core = { path = "../obelisk/stat_core" }
loot_core = { path = "../obelisk/loot_core" }
skill_tree = { path = "../obelisk/skill_tree" }
tables_core = { path = "../obelisk/tables_core" }

[dev-dependencies]
# enable the in-crate test helpers when running tests
obelisk-bevy = { path = ".", features = ["test-support"] }
```

- [ ] **Step 2: Write a placeholder `src/lib.rs`**

```rust
//! obelisk-bevy: a Bevy 0.17 plugin exposing the obelisk ARPG libraries.
```

- [ ] **Step 3: Build to confirm the dependency graph resolves**

Run: `cargo build`
Expected: PASS. If `avian3d = "0.4"` fails to resolve against `bevy = "0.17"`, run `cargo update` and, if still failing, `cargo search avian3d` / check https://github.com/avianphysics/avian README for the exact 0.4.x patch that lists Bevy 0.17, and pin it (e.g. `avian3d = "0.4.1"`). Record the working versions.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/lib.rs
git commit -m "chore: scaffold obelisk-bevy crate (bevy 0.17 + avian 0.4)"
```

### Task 2: API probe — pin the uncertain Bevy 0.17 / Avian 0.4 symbols

This task exists to confirm the exact symbol names the later tasks rely on. It is a throwaway probe; you delete it at the end.

**Files:**
- Create: `src/bin/probe.rs`

- [ ] **Step 1: Write the probe exercising every uncertain API**

```rust
// src/bin/probe.rs — throwaway. Confirms exact API shapes for later tasks.
use bevy::prelude::*;
use avian3d::prelude::*;

// (A) Required components with a non-Default value.
#[derive(Component, Default)]
#[require(Health)]
struct Unit;
#[derive(Component)]
struct Health(f32);
impl Default for Health { fn default() -> Self { Health(100.0) } }

// (B) Observer-triggered event.
#[derive(Event, Clone)]
struct Ping { who: Entity }

fn emit(mut commands: Commands) { commands.trigger(Ping { who: Entity::PLACEHOLDER }); }
fn on_ping(ev: On<Ping>) { let _ = ev.event().who; }

// (C) FixedUpdate fixed delta.
fn fixed(time: Res<Time<Fixed>>) { let _d: f32 = time.delta_secs(); }

// (D) Avian SpatialQuery shape intersection against the physics world.
fn detect(spatial: SpatialQuery) {
    let shape = Collider::sphere(0.5);
    let hits: Vec<Entity> = spatial.shape_intersections(
        &shape,
        Vec3::ZERO,
        Quat::IDENTITY,
        &SpatialQueryFilter::default(),
    );
    let _ = hits;
}

fn main() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(PhysicsPlugins::new(FixedUpdate))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_observer(on_ping)
        .add_systems(Startup, emit)
        .add_systems(FixedUpdate, (fixed, detect));
    // A static collider should be queryable by SpatialQuery:
    app.world_mut().spawn((RigidBody::Static, Collider::sphere(0.5), Transform::default()));
    app.update();
}
```

- [ ] **Step 2: Build the probe**

Run: `cargo build --bin probe`
Expected: PASS. **If any symbol is wrong, fix it against docs.rs and record the correct form in a scratch note** — these are the canonical names later tasks must use:
  - Required-component default form: is it `#[require(Health)]` (uses `Health::Default`) or does a literal need `#[require(Health = Health(100.0))]`? Record the form that compiles.
  - Observer event: confirm `#[derive(Event)]`, `commands.trigger(..)`, `On<E>`, `ev.event()`, `app.add_observer(..)`.
  - `Time::<Fixed>` delta accessor name (`delta_secs` vs `delta_seconds`).
  - `SpatialQuery::shape_intersections` argument order/types and `SpatialQueryFilter` constructor.
  - `PhysicsPlugins::new(FixedUpdate)` import path (`avian3d::prelude::*`).
  - Whether a `Collider` needs `RigidBody::Static` to appear in spatial queries (the probe spawns one with `RigidBody::Static`; if `hits` finds it when overlapping, that's the pattern hurtboxes use).

- [ ] **Step 3: Delete the probe and commit the recorded findings as a doc comment**

Delete `src/bin/probe.rs`. Add the confirmed symbol forms as a `//! API notes` comment block at the top of `src/lib.rs` so later tasks reference them.

```bash
git rm src/bin/probe.rs
git add src/lib.rs
git commit -m "chore: probe + record confirmed bevy 0.17 / avian 0.4 API forms"
```

---

## Phase 1 — Core: events, ids, components, config/init

### Task 3: Define the gameplay event set

**Files:**
- Create: `src/events.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write `src/events.rs`** (all global observer events; entity-targeted variants and the `Message` netcode mirror are deferred past the slice)

```rust
use bevy::prelude::*;

#[derive(Event, Clone, Debug)]
pub struct CastBegan { pub caster: Entity, pub skill_id: String, pub total_duration: f32 }

#[derive(Event, Clone, Debug)]
pub struct CastRejected { pub caster: Entity, pub skill_id: String, pub reason: CastRejectReason }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastRejectReason { UnknownSkill, TimelineMissing, InsufficientMana, ConditionNotMet, OutOfRange, NoTarget }

#[derive(Event, Clone, Debug)]
pub struct CastPhaseChanged { pub caster: Entity, pub skill_id: String, pub from: crate::timeline::SkillPhase, pub to: crate::timeline::SkillPhase, pub elapsed: f32 }

#[derive(Event, Clone, Debug)]
pub struct HitWindowOpened { pub caster: Entity, pub skill_id: String, pub window_id: String, pub hitbox: Entity }

#[derive(Event, Clone, Debug)]
pub struct HitConfirmed { pub caster: Entity, pub target: Entity, pub skill_id: String, pub window_id: String }

#[derive(Event, Clone, Debug)]
pub struct DamageResolved {
    pub caster: Entity, pub target: Entity, pub skill_id: String,
    pub total_damage: f64, pub is_killing_blow: bool,
    pub life_after: f64, pub mana_spent: f64,
}

#[derive(Event, Clone, Debug)]
pub struct EffectApplied { pub target: Entity, pub effect_id: String, pub total_duration: f64, pub stacks: u32 }

#[derive(Event, Clone, Debug)]
pub struct DotTicked { pub target: Entity, pub effect_id: String, pub dot_damage: f64, pub life_remaining: f64 }

#[derive(Event, Clone, Debug)]
pub struct EffectExpired { pub target: Entity, pub effect_id: String }

#[derive(Event, Clone, Debug)]
pub struct EntityDied { pub target: Entity, pub killer: Option<Entity> }

/// Register every event type so observers can attach. (Observer events don't strictly
/// require registration, but registering keeps reflection/inspection working.)
pub(crate) fn register_events(_app: &mut App) {
    // Observer-triggered events need no add_event; placeholder for future Message mirrors.
}

// re-export EffectApplied's Effect dependency users may want
pub use stat_core::Effect as ObeliskEffect;
```

- [ ] **Step 2: Wire the module in `src/lib.rs`**

```rust
//! obelisk-bevy: a Bevy 0.17 plugin exposing the obelisk ARPG libraries.
pub mod events;
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: FAIL — `crate::timeline::SkillPhase` does not exist yet. This is expected; Task 8 defines it. Temporarily change `from`/`to` field types to `String` to compile now, OR proceed to define a minimal `SkillPhase` stub. Use the stub:

Add to `src/lib.rs`:
```rust
pub mod timeline { #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum SkillPhase { Windup, Active, Recovery, Done } }
```
(Task 8 replaces this stub module with the real one.)

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/events.rs src/lib.rs
git commit -m "feat: gameplay event types"
```

### Task 4: ObeliskId + entity↔id index

**Files:**
- Create: `src/ids.rs`
- Modify: `src/lib.rs`
- Test: in `src/ids.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test in `src/ids.rs`**

```rust
use bevy::prelude::*;

/// Component mirroring `StatBlock.id`. Auto-registered into `ObeliskEntityIndex`.
#[derive(Component, Clone, Debug)]
pub struct ObeliskId(pub String);

/// Bidirectional Entity <-> obelisk String id map.
#[derive(Resource, Default)]
pub struct ObeliskEntityIndex {
    to_entity: std::collections::HashMap<String, Entity>,
    to_id: std::collections::HashMap<Entity, String>,
}

impl ObeliskEntityIndex {
    pub fn entity(&self, id: &str) -> Option<Entity> { self.to_entity.get(id).copied() }
    pub fn id(&self, e: Entity) -> Option<&str> { self.to_id.get(&e).map(|s| s.as_str()) }
    fn insert(&mut self, e: Entity, id: String) { self.to_entity.insert(id.clone(), e); self.to_id.insert(e, id); }
    fn remove(&mut self, e: Entity) { if let Some(id) = self.to_id.remove(&e) { self.to_entity.remove(&id); } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_syncs_on_spawn_and_despawn() {
        let mut app = App::new();
        app.init_resource::<ObeliskEntityIndex>();
        app.add_systems(Update, (sync_index_added, sync_index_removed));

        let e = app.world_mut().spawn(ObeliskId("goblin".into())).id();
        app.update();
        assert_eq!(app.world().resource::<ObeliskEntityIndex>().entity("goblin"), Some(e));

        app.world_mut().entity_mut(e).despawn();
        app.update();
        assert_eq!(app.world().resource::<ObeliskEntityIndex>().entity("goblin"), None);
    }
}
```

- [ ] **Step 2: Run the test — expect FAIL (systems undefined)**

Run: `cargo test --features test-support index_syncs`
Expected: FAIL — `sync_index_added` / `sync_index_removed` not found.

- [ ] **Step 3: Implement the sync systems in `src/ids.rs`**

```rust
pub fn sync_index_added(
    mut index: ResMut<ObeliskEntityIndex>,
    added: Query<(Entity, &ObeliskId), Added<ObeliskId>>,
) {
    for (e, id) in &added { index.insert(e, id.0.clone()); }
}

pub fn sync_index_removed(
    mut index: ResMut<ObeliskEntityIndex>,
    mut removed: RemovedComponents<ObeliskId>,
) {
    for e in removed.read() { index.remove(e); }
}
```

- [ ] **Step 4: Wire module in `src/lib.rs` and run the test — expect PASS**

Add `pub mod ids;` to `src/lib.rs`.
Run: `cargo test --features test-support index_syncs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ids.rs src/lib.rs
git commit -m "feat: ObeliskId + entity index with despawn cleanup"
```

### Task 5: Core components (Attributes, Faction, SkillSlots, Combatant)

**Files:**
- Create: `src/core/mod.rs`, `src/core/components.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write `src/core/components.rs`**

```rust
use bevy::prelude::*;
use stat_core::StatBlock;

/// The GAS-style AttributeSet: an obelisk StatBlock. Only sim systems hold `&mut`.
#[derive(Component, Clone, Debug)]
pub struct Attributes(pub StatBlock);

impl Default for Attributes {
    fn default() -> Self { Attributes(StatBlock::new()) }
}

/// Team / faction for hit filtering.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Faction { Player, Enemy, Neutral }
impl Default for Faction { fn default() -> Self { Faction::Neutral } }

/// Skill ids this entity can cast.
#[derive(Component, Clone, Debug, Default)]
pub struct SkillSlots(pub Vec<String>);

/// Ergonomic marker. Inserting `Combatant` auto-requires the rest.
/// NOTE: required components attach each type's `Default`, so `Combatant` alone yields a
/// valid-but-EMPTY StatBlock. Real stats come from the `make_combatant(stat_block)` command
/// (Task 16) or by inserting `Attributes(real_block)` at spawn.
#[derive(Component, Default)]
#[require(Attributes, Faction, SkillSlots, crate::ids::ObeliskId, Transform)]
pub struct Combatant;
```

`crate::ids::ObeliskId` needs a `Default`. Add to `src/ids.rs`:
```rust
impl Default for ObeliskId { fn default() -> Self { ObeliskId(String::new()) } }
```

- [ ] **Step 2: Write `src/core/mod.rs`**

```rust
pub mod components;
pub mod config;
pub mod tick;

pub use components::{Attributes, Combatant, Faction, SkillSlots};
```

- [ ] **Step 3: Stub `src/core/config.rs` and `src/core/tick.rs` so the module compiles**

`src/core/config.rs`:
```rust
// filled in Task 6
```
`src/core/tick.rs`:
```rust
// filled in Task 7
```

- [ ] **Step 4: Wire `pub mod core;` in `src/lib.rs`, build**

Run: `cargo build`
Expected: PASS. **If `#[require(...)]` rejects the empty-id default**, that's fine — `ObeliskId` has a `Default` now. If the `#[require]` syntax differs from your Task 2 probe finding, adjust to the confirmed form.

- [ ] **Step 5: Commit**

```bash
git add src/core src/ids.rs src/lib.rs
git commit -m "feat: core components (Attributes, Faction, SkillSlots, Combatant)"
```

### Task 6: Global init + registries (`add_obelisk_config`, SkillRegistry, CombatRng)

**Files:**
- Modify: `src/core/config.rs`
- Test: `src/core/config.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test in `src/core/config.rs`**

```rust
use bevy::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;
use stat_core::Skill;

/// Registry of obelisk skills (the REAL Skill type, not DamagePacketGenerator).
#[derive(Resource, Default)]
pub struct SkillRegistry(pub HashMap<String, Skill>);

/// The only RNG threaded into combat. Seeded for determinism.
#[derive(Resource)]
pub struct CombatRng(pub ChaCha8Rng);

/// Where skill rules come from.
pub enum SkillSource { Dir(PathBuf), Toml(String) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_loads_skills_and_seeds_rng() {
        let toml = r#"
[[skills]]
id = "firebolt"
name = "Firebolt"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0
[skills.damage]
base_damages = [{ type = "fire", min = 20.0, max = 20.0 }]
"#;
        let mut app = App::new();
        app.add_obelisk_config_constants_default();
        app.add_obelisk_skills(SkillSource::Toml(toml.into()));
        app.seed_combat_rng(42);

        assert!(app.world().resource::<SkillRegistry>().0.contains_key("firebolt"));
        assert!(app.world().get_resource::<CombatRng>().is_some());
        assert!(stat_core::config::constants_initialized());
    }
}
```

- [ ] **Step 2: Run — expect FAIL (extension methods undefined)**

Run: `cargo test --features test-support config_loads_skills`
Expected: FAIL — `add_obelisk_config_constants_default` etc. not found.

- [ ] **Step 3: Implement the App extension trait in `src/core/config.rs`**

```rust
/// App-builder verbs for obelisk setup. All global init is GUARDED (idempotent) because
/// obelisk's `init_*` panic on a second call — critical for tests and in-process client+server.
pub trait ObeliskConfigExt {
    fn add_obelisk_config_constants_default(&mut self) -> &mut Self;
    fn add_obelisk_config_constants(&mut self, path: &Path) -> &mut Self;
    fn add_obelisk_effects(&mut self, dir: &Path) -> &mut Self;
    fn add_obelisk_skills(&mut self, source: SkillSource) -> &mut Self;
    fn seed_combat_rng(&mut self, seed: u64) -> &mut Self;
}

impl ObeliskConfigExt for App {
    fn add_obelisk_config_constants_default(&mut self) -> &mut Self {
        if !stat_core::config::constants_initialized() {
            stat_core::init_constants_default().expect("init constants");
        }
        self
    }
    fn add_obelisk_config_constants(&mut self, path: &Path) -> &mut Self {
        if !stat_core::config::constants_initialized() {
            stat_core::init_constants(path).expect("init constants from path");
        }
        self
    }
    fn add_obelisk_effects(&mut self, dir: &Path) -> &mut Self {
        if !stat_core::config::effect_registry_initialized() {
            stat_core::init_effect_registry(dir).expect("init effect registry");
        }
        self
    }
    fn add_obelisk_skills(&mut self, source: SkillSource) -> &mut Self {
        let map = match source {
            SkillSource::Dir(d) => stat_core::config::skills::load_skills_dir(&d).expect("load skills dir"),
            SkillSource::Toml(s) => stat_core::config::skills::parse_skills(&s).expect("parse skills"),
        };
        self.insert_resource(SkillRegistry(map));
        self
    }
    fn seed_combat_rng(&mut self, seed: u64) -> &mut Self {
        self.insert_resource(CombatRng(ChaCha8Rng::seed_from_u64(seed)));
        self
    }
}
```

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test --features test-support config_loads_skills`
Expected: PASS. If `stat_core::config::skills` is not a public path, use the top-level guard: these functions may need re-export — if the path fails, add `pub use stat_core::config::skills::{load_skills_dir, parse_skills};` is not possible from here; instead reference the real public path the compiler accepts (try `stat_core::config::skills::parse_skills`; obelisk declares `pub mod config` and `config` declares `pub mod skills`).

- [ ] **Step 5: Commit**

```bash
git add src/core/config.rs
git commit -m "feat: guarded obelisk global init + SkillRegistry + seeded CombatRng"
```

---

## Phase 2 — Combat core (pure, no Bevy)

### Task 7: `resolve_one_hit` — the deterministic damage funnel (PURE)

This is the most important logic and is intentionally Bevy-free so it can be unit-tested directly.

**Files:**
- Create: `src/combat/mod.rs`, `src/combat/resolve.rs`
- Modify: `src/lib.rs`
- Test: `src/combat/resolve.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test in `src/combat/resolve.rs`**

```rust
use std::collections::HashMap;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;
use stat_core::{Skill, SkillUseError, Effect};

/// Outcome of resolving one hit, projected from obelisk's CombatResults.
#[derive(Debug, Clone)]
pub struct HitOutcome {
    pub total_damage: f64,
    pub is_killing_blow: bool,
    pub effects_applied: Vec<Effect>,
    pub mana_spent: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::StatBlock;

    fn firebolt_registry() -> HashMap<String, Skill> {
        let toml = r#"
[[skills]]
id = "firebolt"
name = "Firebolt"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0
[skills.damage]
base_damages = [{ type = "fire", min = 20.0, max = 20.0 }]
"#;
        stat_core::config::skills::parse_skills(toml).unwrap()
    }

    #[test]
    fn firebolt_deals_deterministic_damage_and_spends_mana() {
        if !stat_core::config::constants_initialized() { stat_core::init_constants_default().unwrap(); }
        let registry = firebolt_registry();
        let skill = registry.get("firebolt").unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0; caster.current_mana = 100.0;

        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 50.0; target.current_life = 50.0;

        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let outcome = resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();

        assert!(outcome.total_damage > 0.0, "should deal damage");
        assert!(target.current_life < 50.0, "target should have taken damage");
        assert_eq!(caster.current_mana, 95.0, "5 mana spent");
        assert_eq!(outcome.mana_spent, 5.0);
    }
}
```

- [ ] **Step 2: Run — expect FAIL (`resolve_one_hit` undefined)**

Run: `cargo test --features test-support firebolt_deals_deterministic`
Expected: FAIL — function not found.

- [ ] **Step 3: Implement `resolve_one_hit` in `src/combat/resolve.rs`**

Keep the top-of-file imports written in Step 1 (`HashMap`, `ChaCha8Rng`, `SeedableRng`, and `stat_core::{Skill, SkillUseError, Effect}`) — they are at file scope and cover the function. **Add these two imports at the top of the file**, then add the function below the `HitOutcome` struct:

```rust
// add to the existing top-of-file imports:
use stat_core::StatBlock;
use stat_core::combat::resolve_damage_with_triggers;

/// The ONE true deterministic resolve path. Never calls `receive_damage`/`resolve_damage`
/// (those use `thread_rng`). All randomness flows through the supplied `rng`.
///
/// Mutates both `caster` (mana/self-effects via `use_skill_against`) and `target`
/// (damage + applied effects via the write-back of `TriggerResolutionResult.defender`).
pub fn resolve_one_hit(
    caster: &mut StatBlock,
    target: &mut StatBlock,
    skill: &Skill,
    registry: &HashMap<String, Skill>,
    rng: &mut ChaCha8Rng,
) -> Result<HitOutcome, SkillUseError> {
    let source_id = caster.id.clone();
    // use_skill_against needs &mut caster + &target simultaneously: snapshot the target.
    let target_snapshot = target.clone();
    let skill_result = caster.use_skill_against(Some(&target_snapshot), skill, registry, source_id, rng)?;

    // Resolve the produced packets against the live target (deterministic, seeded).
    let tr = resolve_damage_with_triggers(caster, target, &skill_result.packets, skill, registry, rng);

    let total_damage: f64 = tr.results.iter().map(|r| r.total_damage).sum();
    let is_killing_blow = tr.results.iter().any(|r| r.is_killing_blow);
    let effects_applied: Vec<Effect> = tr.results.iter().flat_map(|r| r.effects_applied.clone()).collect();

    // Write the mutated defender back into the caller's target.
    *target = tr.defender;

    Ok(HitOutcome { total_damage, is_killing_blow, effects_applied, mana_spent: skill_result.mana_spent })
}
```

`src/combat/mod.rs`:
```rust
pub mod resolve;
pub use resolve::{resolve_one_hit, HitOutcome};
```
Add `pub mod combat;` to `src/lib.rs`.

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test --features test-support firebolt_deals_deterministic`
Expected: PASS. **If `resolve_damage_with_triggers`'s arg order rejects `(caster, target, ...)`**, the verified order is `(attacker, defender, packets, skill, registry, rng)` — match it exactly.

- [ ] **Step 5: Add a determinism test (same seed → identical damage)**

```rust
#[test]
fn resolution_is_deterministic_for_a_fixed_seed() {
    if !stat_core::config::constants_initialized() { stat_core::init_constants_default().unwrap(); }
    let registry = firebolt_registry();
    let skill = registry.get("firebolt").unwrap();
    let run = || {
        let mut c = StatBlock::with_id("p"); c.max_mana.base = 100.0; c.current_mana = 100.0;
        let mut t = StatBlock::with_id("d"); t.max_life.base = 50.0; t.current_life = 50.0;
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        resolve_one_hit(&mut c, &mut t, skill, &registry, &mut rng).unwrap().total_damage
    };
    assert_eq!(run(), run(), "same seed must produce identical damage");
}
```

Run: `cargo test --features test-support resolution_is_deterministic`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/combat src/lib.rs
git commit -m "feat: pure deterministic resolve_one_hit damage funnel + tests"
```

### Task 8: Burn DoT applies and ticks (effect registry + tick_effects)

**Files:**
- Create: `tests/fixtures/effects/burn.toml`
- Modify: `src/combat/resolve.rs` (test only)

- [ ] **Step 1: Write the burn fixture `tests/fixtures/effects/burn.toml`**

```toml
id = "burn"
name = "Burn"
duration = 5.0
is_debuff = true
stacking = "strongest_only"
damage_type = "fire"
base_damage_percent = 0.5
tick_rate = 0.5
max_stacks = 1
[application]
type = "chance"
```

- [ ] **Step 2: Add a firebolt-with-burn fixture skill `tests/fixtures/skills/firebolt.toml`**

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

- [ ] **Step 3: Write the failing test in `src/combat/resolve.rs`**

```rust
#[test]
fn burn_is_applied_and_ticks_down_life() {
    if !stat_core::config::constants_initialized() { stat_core::init_constants_default().unwrap(); }
    if !stat_core::config::effect_registry_initialized() {
        stat_core::init_effect_registry(std::path::Path::new("tests/fixtures/effects")).unwrap();
    }
    let registry = stat_core::config::skills::load_skills_dir(std::path::Path::new("tests/fixtures/skills")).unwrap();
    let skill = registry.get("firebolt").unwrap();

    let mut caster = StatBlock::with_id("player"); caster.max_mana.base = 100.0; caster.current_mana = 100.0;
    let mut target = StatBlock::with_id("dummy"); target.max_life.base = 500.0; target.current_life = 500.0;

    let mut rng = ChaCha8Rng::seed_from_u64(3);
    let outcome = resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();
    assert!(outcome.effects_applied.iter().any(|e| e.id == "burn"), "burn should be applied");

    // Tick 1 second of DoT (immutable API returns a new block).
    let (ticked, tick_result) = target.tick_effects(1.0);
    assert!(tick_result.dot_damage > 0.0, "burn should deal DoT this tick");
    assert!(ticked.current_life < target.current_life, "DoT should reduce life");
}
```

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test --features test-support burn_is_applied`
Expected: PASS. (Effect registry global init is guarded, so this is safe regardless of test order.)

- [ ] **Step 5: Commit**

```bash
git add tests/fixtures src/combat/resolve.rs
git commit -m "test: burn DoT applies via resolve and ticks via tick_effects"
```

### Task 9: Replace the `SkillPhase` stub with the real timeline state module

**Files:**
- Create: `src/timeline/mod.rs`, `src/timeline/state.rs`
- Modify: `src/lib.rs` (remove the stub `timeline` module)

- [ ] **Step 1: Remove the stub `pub mod timeline { ... }` from `src/lib.rs`; add `pub mod timeline;`**

- [ ] **Step 2: Write `src/timeline/state.rs`**

```rust
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillPhase { Windup, Active, Recovery, Done }

/// Per-cast runtime state. Effective (speed-scaled) durations are snapshotted at cast start.
#[derive(Component, Debug)]
pub struct ActiveCast {
    pub skill_id: String,
    pub target: Entity,
    pub phase: SkillPhase,
    pub elapsed: f32,
    /// Effective phase durations (seconds), already divided by the caster's speed rate.
    pub windup: f32,
    pub active: f32,
    pub recovery: f32,
    /// Window ids already spawned this cast (so we open each once).
    pub fired_windows: Vec<String>,
}

impl ActiveCast {
    pub fn total_duration(&self) -> f32 { self.windup + self.active + self.recovery }
    /// Which phase a given elapsed time falls in.
    pub fn phase_at(&self, t: f32) -> SkillPhase {
        if t < self.windup { SkillPhase::Windup }
        else if t < self.windup + self.active { SkillPhase::Active }
        else if t < self.total_duration() { SkillPhase::Recovery }
        else { SkillPhase::Done }
    }
}
```

- [ ] **Step 3: Write `src/timeline/mod.rs`**

```rust
pub mod state;
pub mod cast;     // Task 16
pub mod advance;  // Task 17
pub use state::{ActiveCast, SkillPhase};
```

Create empty `src/timeline/cast.rs` and `src/timeline/advance.rs` with `// filled later` so the module compiles.

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: PASS (events.rs now references the real `SkillPhase`).

- [ ] **Step 5: Commit**

```bash
git add src/timeline src/lib.rs
git commit -m "feat: timeline state (ActiveCast, SkillPhase) replacing stub"
```

### Task 10: Speed-scaling — effective phase durations from attack/cast speed (PURE)

**Files:**
- Modify: `src/timeline/state.rs`
- Test: `src/timeline/state.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::{Skill, StatBlock};

    fn spell(asm: f64) -> Skill {
        let toml = format!(r#"
[[skills]]
id = "s"
tags = ["spell"]
targeting = "single_enemy"
delivery = "instant"
attack_speed_modifier = {asm}
[skills.damage]
"#);
        stat_core::config::skills::parse_skills(&toml).unwrap().remove("s").unwrap()
    }

    #[test]
    fn cast_speed_halves_durations_at_double_speed() {
        let skill = spell(1.0);
        let mut caster = StatBlock::with_id("p");
        caster.cast_speed.base = 2.0; // 2x cast speed
        let rate = effective_rate(&caster, &skill);
        assert!((rate - 2.0).abs() < 1e-6);
        // base windup 0.4 -> effective 0.2
        assert!((0.4_f32 / rate as f32 - 0.2).abs() < 1e-4);
    }

    #[test]
    fn attack_speed_modifier_slows_a_slow_weapon() {
        let skill = spell(0.8); // slower
        let mut caster = StatBlock::with_id("p");
        caster.attack_speed.base = 1.0;
        // is_spell()? no Spell tag here would mean attack; but this skill is tagged spell -> cast_speed.
        // Override to an attack to exercise the modifier path:
        let attack_toml = r#"
[[skills]]
id = "a"
tags = ["attack"]
targeting = "single_enemy"
delivery = "melee"
attack_speed_modifier = 0.8
[skills.damage]
"#;
        let atk = stat_core::config::skills::parse_skills(attack_toml).unwrap().remove("a").unwrap();
        let rate = effective_rate(&caster, &atk);
        assert!((rate - 0.8).abs() < 1e-6, "rate = base(1.0) * modifier(0.8)");
    }
}
```

- [ ] **Step 2: Run — expect FAIL (`effective_rate` undefined)**

Run: `cargo test --features test-support cast_speed_halves`
Expected: FAIL.

- [ ] **Step 3: Implement `effective_rate` in `src/timeline/state.rs`**

```rust
use stat_core::{Skill, StatBlock};

/// The playback rate for a cast's timeline. `1.0` = play as authored; `2.0` = twice as fast.
/// Picks cast_speed for spells, attack_speed for attacks, then applies the skill's modifier.
pub fn effective_rate(caster: &StatBlock, skill: &Skill) -> f64 {
    let base = if skill.is_spell() {
        caster.cast_speed.compute()
    } else if skill.is_attack() {
        caster.attack_speed.compute()
    } else {
        1.0
    };
    skill.effective_speed(base).max(0.0001) // guard rate > 0
}

/// Build the speed-scaled ActiveCast from authored base durations.
pub fn scale_durations(base: (f32, f32, f32), rate: f64) -> (f32, f32, f32) {
    let r = rate as f32;
    (base.0 / r, base.1 / r, base.2 / r)
}
```

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test --features test-support cast_speed_halves attack_speed_modifier_slows`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/timeline/state.rs
git commit -m "feat: speed-scaled cast durations (effective_rate + scale_durations)"
```

---

## Phase 3 — Assets: the spatiotemporal cast timeline

### Task 11: `CastTimeline` asset + authoring enums + RON loader

**Files:**
- Create: `src/assets/mod.rs`
- Modify: `src/lib.rs`
- Create: `assets/skills/firebolt.cast.ron`
- Test: `src/assets/mod.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write `src/assets/mod.rs`** (asset + enums + loader)

```rust
use bevy::prelude::*;
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use serde::Deserialize;

#[derive(Asset, TypePath, Debug, Clone, Deserialize)]
pub struct CastTimeline {
    pub skill_id: String,
    pub phase_durations: PhaseDurations,
    #[serde(default)]
    pub collision_windows: Vec<CollisionWindow>,
    pub targeting: CastTargeting,
    pub delivery: CastDelivery,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhaseDurations { pub windup: f32, pub active: f32, pub recovery: f32 }

#[derive(Debug, Clone, Deserialize)]
pub struct CollisionWindow {
    pub id: String,
    pub spawn_phase: WindowPhase,
    #[serde(default)]
    pub spawn_offset: f32,
    pub active_duration: f32,
    pub shape: CollisionShape,
    #[serde(default)]
    pub motion: VolumeMotion,
    pub hit_filter: HitFilter,
    pub hit_mode: HitMode,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum WindowPhase { Windup, Active, Recovery }

#[derive(Debug, Clone, Deserialize)]
pub enum CollisionShape { Sphere { radius: f32 }, Capsule { radius: f32, height: f32 }, Cone { angle: f32, range: f32 } }

#[derive(Debug, Clone, Deserialize, Default)]
pub enum VolumeMotion { #[default] Static, Linear { speed: f32 } }

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum HitFilter { Caster, Allies, Enemies, All }

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum HitMode { OncePerTarget, FirstOnly, EveryTick }

#[derive(Debug, Clone, Deserialize)]
pub enum CastTargeting { SelfCast, SingleEntity { range: f32 }, Direction { range: f32 }, Cone { angle: f32, range: f32 } }

#[derive(Debug, Clone, Deserialize)]
pub enum CastDelivery { Melee, Instant, Projectile { speed: f32 } }

/// RON loader for `*.cast.ron`.
#[derive(Default)]
pub struct CastTimelineLoader;

impl AssetLoader for CastTimelineLoader {
    type Asset = CastTimeline;
    type Settings = ();
    type Error = CastLoadError;
    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<CastTimeline, CastLoadError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|e| CastLoadError::Io(e.to_string()))?;
        ron::de::from_bytes::<CastTimeline>(&bytes).map_err(|e| CastLoadError::Ron(e.to_string()))
    }
    fn extensions(&self) -> &[&str] { &["cast.ron"] }
}

#[derive(Debug, thiserror::Error)]
pub enum CastLoadError {
    #[error("io: {0}")] Io(String),
    #[error("ron: {0}")] Ron(String),
}

/// Maps skill_id -> loaded timeline handle.
#[derive(Resource, Default)]
pub struct CastTimelineHandles(pub std::collections::HashMap<String, Handle<CastTimeline>>);

pub struct ObeliskAssetsPlugin;
impl Plugin for ObeliskAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<CastTimeline>()
            .register_asset_loader(CastTimelineLoader)
            .init_resource::<CastTimelineHandles>();
    }
}
```

Add `thiserror = "1"` to `[dependencies]` in `Cargo.toml`.

- [ ] **Step 2: Write `assets/skills/firebolt.cast.ron`**

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

- [ ] **Step 3: Write the loader test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loads_firebolt_cast_ron() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
           .add_plugins(AssetPlugin { file_path: ".".into(), ..default() })
           .add_plugins(ObeliskAssetsPlugin);
        let handle: Handle<CastTimeline> = app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
        // Pump the asset server until loaded.
        for _ in 0..1000 {
            app.update();
            if app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() { break; }
        }
        let timeline = app.world().resource::<Assets<CastTimeline>>().get(&handle).expect("loaded");
        assert_eq!(timeline.skill_id, "firebolt");
        assert_eq!(timeline.collision_windows.len(), 1);
    }
}
```

- [ ] **Step 4: Wire `pub mod assets;` in `src/lib.rs`, run**

Run: `cargo test --features test-support loads_firebolt_cast_ron`
Expected: PASS. If `AssetLoader::load`'s exact async signature differs from your Task 2 finding, match the confirmed form. If asset loading never completes headless, ensure `AssetPlugin` + the default asset source are present (the test sets `file_path: "."` so `assets/...` resolves from the crate root).

- [ ] **Step 5: Commit**

```bash
git add src/assets src/lib.rs assets/skills/firebolt.cast.ron Cargo.toml
git commit -m "feat: CastTimeline asset + RON loader + firebolt fixture"
```

---

## Phase 4 — Spatial: hurtbox, hitbox, overlap detection

### Task 12: Shapes → Avian colliders, Hurtbox/Hitbox components

**Files:**
- Create: `src/spatial/mod.rs`, `src/spatial/shapes.rs`, `src/spatial/boxes.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write `src/spatial/shapes.rs`**

```rust
use avian3d::prelude::Collider;
use crate::assets::CollisionShape;

/// Convert an authoring shape to an Avian collider. (Cone approximated by a sphere for the
/// slice; a true cone/sector test is future work.)
pub fn to_collider(shape: &CollisionShape) -> Collider {
    match shape {
        CollisionShape::Sphere { radius } => Collider::sphere(*radius),
        CollisionShape::Capsule { radius, height } => Collider::capsule(*radius, *height),
        CollisionShape::Cone { range, .. } => Collider::sphere(*range), // slice approximation
    }
}
```

- [ ] **Step 2: Write `src/spatial/boxes.rs`**

```rust
use bevy::prelude::*;
use crate::assets::{HitFilter, HitMode};

/// Defensive volume on a combatant. Spawned as a static Avian collider so SpatialQuery can find it.
#[derive(Component, Debug)]
pub struct Hurtbox { pub owner: Entity }

/// Offensive volume spawned during an active collision window.
#[derive(Component, Debug)]
pub struct Hitbox {
    pub caster: Entity,
    pub skill_id: String,
    pub window_id: String,
    pub filter: HitFilter,
    pub mode: HitMode,
    pub remaining: f32,
    pub already_hit: Vec<Entity>,
}
```

- [ ] **Step 3: Write `src/spatial/mod.rs`**

```rust
pub mod shapes;
pub mod boxes;
pub mod detect;     // Task 14
pub mod projectile; // Task 15
pub use boxes::{Hitbox, Hurtbox};

use bevy::prelude::*;
pub struct ObeliskSpatialPlugin;
impl Plugin for ObeliskSpatialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(avian3d::prelude::PhysicsPlugins::new(FixedUpdate));
    }
}
```

Create empty `src/spatial/detect.rs` and `src/spatial/projectile.rs` with `// filled later`.

- [ ] **Step 4: Wire `pub mod spatial;` in `src/lib.rs`, build**

Run: `cargo build`
Expected: PASS. Confirm `Collider::capsule(radius, height)` arg order against avian3d 0.4 docs; adjust if needed.

- [ ] **Step 5: Commit**

```bash
git add src/spatial src/lib.rs
git commit -m "feat: shape->collider conversion + Hurtbox/Hitbox components"
```

### Task 13: Spawn a queryable hurtbox; confirm SpatialQuery finds it

**Files:**
- Modify: `src/spatial/boxes.rs` (add a spawn helper)
- Test: `src/spatial/detect.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Add a hurtbox-spawn helper to `src/spatial/boxes.rs`**

```rust
use avian3d::prelude::*;

/// Bundle of components that make `e` a SpatialQuery-discoverable hurtbox at `pos`.
/// Uses `RigidBody::Static` because (per the Task 2 probe) a static collider is included
/// in `SpatialQuery` shape intersections.
pub fn insert_hurtbox(commands: &mut Commands, owner: Entity, radius: f32, pos: Vec3) {
    commands.entity(owner).insert((
        Hurtbox { owner },
        RigidBody::Static,
        Collider::sphere(radius),
        Transform::from_translation(pos),
    ));
}
```

- [ ] **Step 2: Write the failing test in `src/spatial/detect.rs`**

```rust
use bevy::prelude::*;
use avian3d::prelude::*;
use crate::spatial::boxes::insert_hurtbox;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_query_finds_an_overlapping_hurtbox() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
           .add_plugins(avian3d::prelude::PhysicsPlugins::new(FixedUpdate))
           .insert_resource(Time::<Fixed>::from_hz(60.0));

        let dummy = app.world_mut().spawn_empty().id();
        {
            let world = app.world_mut();
            let mut commands = world.commands();
            insert_hurtbox(&mut commands, dummy, 0.5, Vec3::ZERO);
        }
        // Let physics register the collider.
        app.update();
        app.update();

        // Query a sphere at the origin; should find the dummy's hurtbox.
        let mut found = false;
        app.world_mut().run_system_once(move |spatial: SpatialQuery, q: Query<&Hurtbox>| {
            let hits = spatial.shape_intersections(&Collider::sphere(0.5), Vec3::ZERO, Quat::IDENTITY, &SpatialQueryFilter::default());
            return hits.iter().any(|e| q.get(*e).is_ok());
        }).map(|r| found = r).ok();
        assert!(found, "SpatialQuery should find the static hurtbox collider");
    }
}
```

(`run_system_once` is in `bevy::ecs::system::RunSystemOnce`; import it: `use bevy::ecs::system::RunSystemOnce;`. If the closure-return form is unsupported, write the result into a `Resource` instead and read it back.)

- [ ] **Step 3: Run — expect PASS or learn the real constraint**

Run: `cargo test --features test-support spatial_query_finds`
Expected: PASS. **If it fails**, the most likely causes (resolve in this order): (a) physics needs more `app.update()` cycles to register the collider — bump the loop; (b) the collider needs a different body type — try without `RigidBody::Static` or with `RigidBody::Kinematic`; (c) `shape_intersections` signature differs — match docs. Record the working pattern; Task 14 depends on it.

- [ ] **Step 4: Commit**

```bash
git add src/spatial/boxes.rs src/spatial/detect.rs
git commit -m "test: confirm SpatialQuery discovers static hurtbox colliders"
```

### Task 14: Overlap detection system → HitConfirmed

**Files:**
- Modify: `src/spatial/detect.rs`

(`ObeliskSet` and scheduling are introduced in Task 20; `detect_overlaps` is just a plain system for now.)

- [ ] **Step 1: Implement the detection system in `src/spatial/detect.rs`**

```rust
use bevy::prelude::*;
use avian3d::prelude::*;
use crate::spatial::{boxes::{Hitbox, Hurtbox}, shapes::to_collider};
use crate::core::components::Faction;
use crate::events::HitConfirmed;

/// For each active hitbox, query overlapping hurtboxes via SpatialQuery, apply the
/// faction filter + HitMode dedupe, and emit HitConfirmed. Detection is in FixedUpdate
/// so it is deterministic and authoritative.
pub fn detect_overlaps(
    mut commands: Commands,
    mut hitboxes: Query<(&mut Hitbox, &Transform)>,
    hurtboxes: Query<(Entity, &Hurtbox)>,
    factions: Query<&Faction>,
    spatial: SpatialQuery,
) {
    for (mut hitbox, hb_tf) in &mut hitboxes {
        let collider = Collider::sphere(0.5); // slice: bolt radius; future: store the hitbox's own collider
        let hits = spatial.shape_intersections(&collider, hb_tf.translation, hb_tf.rotation, &SpatialQueryFilter::default());

        let caster_faction = factions.get(hitbox.caster).copied().unwrap_or(Faction::Neutral);
        for hurt_e in hits {
            let Ok((owner_e, hurt)) = hurtboxes.get(hurt_e) else { continue };
            let target = hurt.owner;
            if target == hitbox.caster { continue; }
            if hitbox.already_hit.contains(&target) { continue; }

            // Faction filter (HitFilter::Enemies for the slice).
            let target_faction = factions.get(target).copied().unwrap_or(Faction::Neutral);
            let is_enemy = target_faction != caster_faction;
            if !is_enemy { continue; }

            hitbox.already_hit.push(target);
            commands.trigger(HitConfirmed {
                caster: hitbox.caster,
                target,
                skill_id: hitbox.skill_id.clone(),
                window_id: hitbox.window_id.clone(),
            });
            let _ = owner_e;
        }
    }
}
```

(Note: the slice hard-codes the bolt's 0.5 radius and an `Enemies` filter. Generalize `Hitbox` to store its own `Collider` and honor `HitFilter` in a follow-up.)

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/spatial/detect.rs
git commit -m "feat: SpatialQuery overlap detection -> HitConfirmed"
```

### Task 15: Projectile motion

**Files:**
- Modify: `src/spatial/projectile.rs`

- [ ] **Step 1: Write the projectile component + motion system**

```rust
use bevy::prelude::*;

/// Straight-line projectile motion for a moving hitbox. World-space speed (NOT speed-scaled).
#[derive(Component, Debug)]
pub struct Projectile { pub velocity: Vec3 }

/// Integrate projectile transforms each fixed step. Runs before ResolveHits so detection
/// uses the updated position.
pub fn move_projectiles(time: Res<Time<Fixed>>, mut q: Query<(&Projectile, &mut Transform)>) {
    let dt = time.delta_secs();
    for (proj, mut tf) in &mut q {
        tf.translation += proj.velocity * dt;
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/spatial/projectile.rs
git commit -m "feat: projectile motion (fixed-step transform integration)"
```

---

## Phase 5 — Timeline: cast verb, validation, advance

### Task 16: `cast_skill_at` verb + PendingCast + validation → ActiveCast

**Files:**
- Modify: `src/timeline/cast.rs`
- Modify: `src/timeline/advance.rs`
- Test: integration-style in `src/timeline/cast.rs`

- [ ] **Step 1: Write `src/timeline/cast.rs`**

```rust
use bevy::prelude::*;

/// Pending cast request, consumed by the Validate system.
#[derive(Component, Debug)]
pub struct PendingCast { pub skill_id: String, pub target: Entity }

/// EntityCommands verb: request a cast at a target entity.
pub trait CastSkillExt {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self;
}

impl CastSkillExt for EntityCommands<'_> {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self {
        let skill_id = skill_id.into();
        self.insert(PendingCast { skill_id, target });
        self
    }
}
```

- [ ] **Step 2: Write the Validate system in `src/timeline/advance.rs`**

```rust
use bevy::prelude::*;
use crate::assets::{CastTimeline, CastTimelineHandles};
use crate::core::components::Attributes;
use crate::core::config::SkillRegistry;
use crate::events::{CastBegan, CastRejected, CastRejectReason};
use crate::timeline::cast::PendingCast;
use crate::timeline::state::{effective_rate, scale_durations, ActiveCast, SkillPhase};

/// Validate pending casts: skill known? timeline loaded? mana/conditions ok? Then insert ActiveCast.
pub fn validate_casts(
    mut commands: Commands,
    pending: Query<(Entity, &PendingCast)>,
    casters: Query<&Attributes>,
    registry: Res<SkillRegistry>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
) {
    for (caster, req) in &pending {
        commands.entity(caster).remove::<PendingCast>();

        let Some(skill) = registry.0.get(&req.skill_id) else {
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::UnknownSkill });
            continue;
        };
        let Some(handle) = handles.0.get(&req.skill_id) else {
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::TimelineMissing });
            continue;
        };
        let Some(timeline) = timelines.get(handle) else {
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::TimelineMissing });
            continue;
        };
        let Ok(attrs) = casters.get(caster) else { continue };
        if !attrs.0.can_use_skill(skill) {
            commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason: CastRejectReason::InsufficientMana });
            continue;
        }

        // Speed-scale the authored phase durations at cast start.
        let rate = effective_rate(&attrs.0, skill);
        let base = (timeline.phase_durations.windup, timeline.phase_durations.active, timeline.phase_durations.recovery);
        let (windup, active, recovery) = scale_durations(base, rate);

        commands.entity(caster).insert(ActiveCast {
            skill_id: req.skill_id.clone(),
            target: req.target,
            phase: SkillPhase::Windup,
            elapsed: 0.0,
            windup, active, recovery,
            fired_windows: Vec::new(),
        });
        commands.trigger(CastBegan { caster, skill_id: req.skill_id.clone(), total_duration: windup + active + recovery });
    }
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS. Confirm `StatBlock::can_use_skill(&Skill) -> bool` is callable on `attrs.0`.

- [ ] **Step 4: Commit**

```bash
git add src/timeline/cast.rs src/timeline/advance.rs
git commit -m "feat: cast_skill_at verb + cast validation -> ActiveCast"
```

### Task 17: Advance the timeline — phase transitions + window spawn

**Files:**
- Modify: `src/timeline/advance.rs`

- [ ] **Step 1: Implement the advance system**

```rust
use crate::assets::{WindowPhase, VolumeMotion};
use crate::events::{CastPhaseChanged, HitWindowOpened};
use crate::spatial::boxes::Hitbox;
use crate::spatial::projectile::Projectile;

pub fn advance_casts(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut casts: Query<(Entity, &mut ActiveCast, &Transform)>,
    registry: Res<SkillRegistry>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
) {
    let dt = time.delta_secs();
    for (caster, mut cast, caster_tf) in &mut casts {
        let prev_phase = cast.phase;
        let prev_elapsed = cast.elapsed;
        cast.elapsed += dt;
        let new_phase = cast.phase_at(cast.elapsed);
        if new_phase != prev_phase {
            cast.phase = new_phase;
            commands.trigger(CastPhaseChanged { caster, skill_id: cast.skill_id.clone(), from: prev_phase, to: new_phase, elapsed: cast.elapsed });
        }

        // Spawn collision windows whose start time was crossed this tick.
        let Some(handle) = handles.0.get(&cast.skill_id) else { continue };
        let Some(timeline) = timelines.get(handle) else { continue };
        for win in &timeline.collision_windows {
            if cast.fired_windows.contains(&win.id) { continue; }
            let base = match win.spawn_phase { WindowPhase::Windup => 0.0, WindowPhase::Active => cast.windup, WindowPhase::Recovery => cast.windup + cast.active };
            let start = base + win.spawn_offset;
            if prev_elapsed < start && cast.elapsed >= start {
                cast.fired_windows.push(win.id.clone());
                // Spawn the hitbox at the caster, aimed at the target.
                let dir = Vec3::Z; // slice: forward; future: aim at target position
                let mut ent = commands.spawn((
                    Hitbox { caster, skill_id: cast.skill_id.clone(), window_id: win.id.clone(), filter: win.hit_filter, mode: win.hit_mode, remaining: win.active_duration, already_hit: Vec::new() },
                    Transform::from_translation(caster_tf.translation),
                ));
                if let VolumeMotion::Linear { speed } = win.motion {
                    ent.insert(Projectile { velocity: dir * speed });
                }
                let hitbox_entity = ent.id();
                commands.trigger(HitWindowOpened { caster, skill_id: cast.skill_id.clone(), window_id: win.id.clone(), hitbox: hitbox_entity });
            }
        }

        // End the cast.
        if cast.phase == SkillPhase::Done {
            commands.entity(caster).remove::<ActiveCast>();
        }
        let _ = &registry; // kept for future use_conditions re-checks
    }
}

/// Despawn hitboxes whose active window elapsed.
pub fn expire_hitboxes(mut commands: Commands, time: Res<Time<Fixed>>, mut q: Query<(Entity, &mut Hitbox)>) {
    let dt = time.delta_secs();
    for (e, mut hb) in &mut q {
        hb.remaining -= dt;
        if hb.remaining <= 0.0 { commands.entity(e).despawn(); }
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: PASS. (The `phase_start` no-op block is removed if your linter flags it; it's a leftover — delete it.)

- [ ] **Step 3: Commit**

```bash
git add src/timeline/advance.rs
git commit -m "feat: advance timeline (phase transitions + window/hitbox spawn + expiry)"
```

---

## Phase 6 — Combat system wiring + ticking + plugins + integration test

### Task 18: ResolveHits system — HitConfirmed → resolve_one_hit → events

**Files:**
- Create: `src/combat/system.rs`
- Modify: `src/combat/mod.rs`

- [ ] **Step 1: Write `src/combat/system.rs`**

```rust
use bevy::prelude::*;
use crate::combat::resolve::resolve_one_hit;
use crate::core::components::Attributes;
use crate::core::config::{CombatRng, SkillRegistry};
use crate::events::{DamageResolved, EffectApplied, EntityDied, HitConfirmed};

/// Observer: when a hit is confirmed, run the deterministic resolve funnel and emit results.
pub fn on_hit_confirmed(
    hit: On<HitConfirmed>,
    mut attrs: Query<&mut Attributes>,
    registry: Res<SkillRegistry>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
) {
    let ev = hit.event().clone();
    let Some(skill) = registry.0.get(&ev.skill_id) else { return };

    // Need &mut caster + &mut target disjointly.
    let [mut caster_attrs, mut target_attrs] = match attrs.get_many_mut([ev.caster, ev.target]) {
        Ok(pair) => pair,
        Err(_) => return, // same entity or missing
    };

    let outcome = match resolve_one_hit(&mut caster_attrs.0, &mut target_attrs.0, skill, &registry.0, &mut rng.0) {
        Ok(o) => o,
        Err(_) => return,
    };

    let life_after = target_attrs.0.current_life;
    commands.trigger(DamageResolved {
        caster: ev.caster, target: ev.target, skill_id: ev.skill_id.clone(),
        total_damage: outcome.total_damage, is_killing_blow: outcome.is_killing_blow,
        life_after, mana_spent: outcome.mana_spent,
    });
    for eff in &outcome.effects_applied {
        commands.trigger(EffectApplied { target: ev.target, effect_id: eff.id.clone(), total_duration: eff.total_duration, stacks: eff.stacks });
    }
    if outcome.is_killing_blow || !target_attrs.0.is_alive() {
        commands.trigger(EntityDied { target: ev.target, killer: Some(ev.caster) });
    }
}
```

- [ ] **Step 2: Update `src/combat/mod.rs`**

```rust
pub mod resolve;
pub mod system;
pub use resolve::{resolve_one_hit, HitOutcome};

use bevy::prelude::*;
pub struct ObeliskCombatPlugin;
impl Plugin for ObeliskCombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(system::on_hit_confirmed);
    }
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS. Confirm `Query::get_many_mut([a, b])` signature/return in bevy 0.17 (it may be `get_many_mut(...)` returning `Result<[Mut<T>;N], _>`); adjust the destructuring to the real return shape.

- [ ] **Step 4: Commit**

```bash
git add src/combat
git commit -m "feat: ResolveHits observer -> resolve_one_hit -> DamageResolved/EffectApplied/EntityDied"
```

### Task 19: TickEffects system — DoT/expiry/death each fixed step

**Files:**
- Modify: `src/core/tick.rs`

- [ ] **Step 1: Implement the tick driver**

```rust
use bevy::prelude::*;
use crate::core::components::Attributes;
use crate::events::{DotTicked, EffectExpired, EntityDied};

/// Drive obelisk's tick_effects once per fixed step. tick_effects is immutable (returns a
/// new block), so we replace the component's StatBlock and emit what happened.
pub fn tick_effects_system(
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Attributes)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs_f64();
    for (e, mut attrs) in &mut q {
        if attrs.0.effects_is_empty_fast() { continue; }
        let (new_block, result) = attrs.0.tick_effects(dt);
        let was_alive = attrs.0.is_alive();
        attrs.0 = new_block;

        if result.dot_damage > 0.0 {
            // Attribute DoT to the effects that ticked; for the slice, emit a single rollup.
            commands.trigger(DotTicked { target: e, effect_id: String::new(), dot_damage: result.dot_damage, life_remaining: result.life_remaining });
        }
        for id in &result.expired_effects {
            commands.trigger(EffectExpired { target: e, effect_id: id.clone() });
        }
        if was_alive && result.is_dead {
            commands.trigger(EntityDied { target: e, killer: None });
        }
    }
}
```

Add a cheap guard to `Attributes` to avoid cloning when no effects. In `src/core/components.rs`:
```rust
impl Attributes {
    /// True if the StatBlock has no active effects (skip tick clone).
    pub fn effects_is_empty_fast(&self) -> bool { self.0.effects.is_empty() }
}
```
(`StatBlock.effects: Vec<Effect>` is public per the verified facts.)

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: PASS. Confirm `tick_effects` takes `delta: f64` (it does) and `delta_secs_f64()` exists on `Time<Fixed>` (per Task 2 probe; if not, use `time.delta().as_secs_f64()`).

- [ ] **Step 3: Commit**

```bash
git add src/core/tick.rs src/core/components.rs
git commit -m "feat: FixedUpdate tick_effects driver -> DotTicked/EffectExpired/EntityDied"
```

### Task 20: Assemble plugins (ObeliskCorePlugin, ObeliskSimPlugin, ObeliskPlugins)

**Files:**
- Modify: `src/core/mod.rs`, `src/lib.rs`

- [ ] **Step 1: Write `ObeliskCorePlugin` in `src/core/mod.rs`**

```rust
pub mod components;
pub mod config;
pub mod tick;
pub use components::{Attributes, Combatant, Faction, SkillSlots};

use bevy::prelude::*;
use crate::ids::{sync_index_added, sync_index_removed, ObeliskEntityIndex};
use crate::ObeliskSet;

pub struct ObeliskCorePlugin;
impl Plugin for ObeliskCorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ObeliskEntityIndex>()
           .add_systems(Update, (sync_index_added, sync_index_removed))
           .add_systems(FixedUpdate, tick::tick_effects_system.in_set(ObeliskSet::TickEffects));
    }
}
```

- [ ] **Step 2: Assemble the schedule + groups in `src/lib.rs`**

```rust
//! obelisk-bevy: a Bevy 0.17 plugin exposing the obelisk ARPG libraries.
use bevy::prelude::*;
use bevy::app::{PluginGroup, PluginGroupBuilder};

pub mod events;
pub mod ids;
pub mod core;
pub mod assets;
pub mod spatial;
pub mod timeline;
pub mod combat;
pub mod prelude;
#[cfg(feature = "present")]
pub mod present;
#[cfg(feature = "test-support")]
pub mod testkit;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObeliskSet { Validate, Advance, SpawnVolumes, Projectiles, ResolveHits, TickEffects }

/// The headless authoritative simulation: core + assets + spatial + timeline + combat.
pub struct ObeliskSimPlugin;
impl Plugin for ObeliskSimPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(assets::ObeliskAssetsPlugin)
           .add_plugins(spatial::ObeliskSpatialPlugin)
           .add_plugins(core::ObeliskCorePlugin)
           .add_plugins(combat::ObeliskCombatPlugin);

        app.configure_sets(FixedUpdate, (
            ObeliskSet::Validate,
            ObeliskSet::Advance,
            ObeliskSet::Projectiles,
            ObeliskSet::ResolveHits,
            ObeliskSet::TickEffects,
        ).chain());

        app.add_systems(FixedUpdate, (
            timeline::advance::validate_casts.in_set(ObeliskSet::Validate),
            (timeline::advance::advance_casts, timeline::advance::expire_hitboxes).in_set(ObeliskSet::Advance),
            spatial::projectile::move_projectiles.in_set(ObeliskSet::Projectiles),
            spatial::detect::detect_overlaps.in_set(ObeliskSet::ResolveHits),
        ));
    }
}

/// Umbrella plugin group.
pub struct ObeliskPlugins;
impl PluginGroup for ObeliskPlugins {
    fn build(self) -> PluginGroupBuilder {
        let mut b = PluginGroupBuilder::start::<Self>().add(ObeliskSimPlugin);
        #[cfg(feature = "present")]
        { b = b.add(present::ObeliskPresentPlugin); }
        b
    }
}
```

- [ ] **Step 3: Write `src/prelude.rs`**

```rust
pub use crate::core::components::{Attributes, Combatant, Faction, SkillSlots};
pub use crate::core::config::{ObeliskConfigExt, SkillRegistry, SkillSource, CombatRng};
pub use crate::ids::{ObeliskId, ObeliskEntityIndex};
pub use crate::assets::{CastTimeline, CastTimelineHandles};
pub use crate::timeline::cast::CastSkillExt;
pub use crate::timeline::state::{ActiveCast, SkillPhase};
pub use crate::spatial::boxes::{Hitbox, Hurtbox, insert_hurtbox};
pub use crate::events::*;
pub use crate::{ObeliskPlugins, ObeliskSimPlugin, ObeliskSet};
```

- [ ] **Step 4: Stub `src/present/mod.rs`** (gated; real bodies in Task 22)

```rust
use bevy::prelude::*;
pub struct ObeliskPresentPlugin;
impl Plugin for ObeliskPresentPlugin { fn build(&self, _app: &mut App) {} }
```

- [ ] **Step 5: Build**

Run: `cargo build` then `cargo build --no-default-features`
Expected: both PASS (the second confirms the sim compiles without `present`).

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat: assemble ObeliskSimPlugin schedule + ObeliskPlugins group + prelude"
```

### Task 21: Testkit — `ObeliskTestApp`, `EventRecorder`, `init_test_obelisk`

**Files:**
- Modify: `src/testkit.rs`

- [ ] **Step 1: Write `src/testkit.rs`**

```rust
use bevy::prelude::*;
use std::path::Path;
use std::sync::Once;
use crate::events::*;

static INIT: Once = Once::new();

/// Idempotently init obelisk globals from test fixtures. Safe across parallel tests.
pub fn init_test_obelisk() {
    INIT.call_once(|| {
        if !stat_core::config::constants_initialized() { stat_core::init_constants_default().unwrap(); }
        if !stat_core::config::effect_registry_initialized() {
            stat_core::init_effect_registry(Path::new("tests/fixtures/effects")).unwrap();
        }
    });
}

/// Records every gameplay event for assertions.
#[derive(Resource, Default)]
pub struct EventRecorder {
    pub cast_began: Vec<CastBegan>,
    pub cast_rejected: Vec<CastRejected>,
    pub phase_changed: Vec<CastPhaseChanged>,
    pub hit_window_opened: Vec<HitWindowOpened>,
    pub hit_confirmed: Vec<HitConfirmed>,
    pub damage_resolved: Vec<DamageResolved>,
    pub effect_applied: Vec<EffectApplied>,
    pub dot_ticked: Vec<DotTicked>,
    pub died: Vec<EntityDied>,
}

pub struct EventRecorderPlugin;
impl Plugin for EventRecorderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EventRecorder>();
        app.add_observer(|e: On<CastBegan>, mut r: ResMut<EventRecorder>| r.cast_began.push(e.event().clone()));
        app.add_observer(|e: On<CastRejected>, mut r: ResMut<EventRecorder>| r.cast_rejected.push(e.event().clone()));
        app.add_observer(|e: On<CastPhaseChanged>, mut r: ResMut<EventRecorder>| r.phase_changed.push(e.event().clone()));
        app.add_observer(|e: On<HitWindowOpened>, mut r: ResMut<EventRecorder>| r.hit_window_opened.push(e.event().clone()));
        app.add_observer(|e: On<HitConfirmed>, mut r: ResMut<EventRecorder>| r.hit_confirmed.push(e.event().clone()));
        app.add_observer(|e: On<DamageResolved>, mut r: ResMut<EventRecorder>| r.damage_resolved.push(e.event().clone()));
        app.add_observer(|e: On<EffectApplied>, mut r: ResMut<EventRecorder>| r.effect_applied.push(e.event().clone()));
        app.add_observer(|e: On<DotTicked>, mut r: ResMut<EventRecorder>| r.dot_ticked.push(e.event().clone()));
        app.add_observer(|e: On<EntityDied>, mut r: ResMut<EventRecorder>| r.died.push(e.event().clone()));
    }
}

/// A headless app preconfigured for deterministic integration tests.
pub struct ObeliskTestApp { pub app: App }

impl ObeliskTestApp {
    pub fn new(seed: u64) -> Self {
        init_test_obelisk();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
           .add_plugins(AssetPlugin { file_path: ".".into(), ..default() })
           .add_plugins(crate::ObeliskSimPlugin)
           .add_plugins(EventRecorderPlugin)
           .insert_resource(Time::<Fixed>::from_hz(60.0));
        use crate::prelude::ObeliskConfigExt;
        app.seed_combat_rng(seed);
        Self { app }
    }
    /// Step `n` fixed ticks. Each `update()` runs one FixedUpdate at 60hz given the inserted Time<Fixed>.
    pub fn advance_ticks(&mut self, n: usize) { for _ in 0..n { self.app.update(); } }
    pub fn rec(&self) -> &EventRecorder { self.app.world().resource::<EventRecorder>() }
}
```

- [ ] **Step 2: Build the test-support feature**

Run: `cargo build --features test-support`
Expected: PASS. **If `update()` does not advance `FixedUpdate` deterministically headless** (FixedUpdate accumulates wall-clock time), insert `Time::<Virtual>` control: advance virtual time manually each step. Concretely, if needed, add before each update: `app.world_mut().resource_mut::<Time<Virtual>>().advance_by(Duration::from_secs_f64(1.0/60.0));` then `app.update();`. Record the working stepping method — the integration test depends on it.

- [ ] **Step 3: Commit**

```bash
git add src/testkit.rs
git commit -m "test: ObeliskTestApp + EventRecorder + idempotent init_test_obelisk"
```

### Task 22: Present layer — event-logging observers + debug gizmos (feature-gated)

**Files:**
- Modify: `src/present/mod.rs`

- [ ] **Step 1: Implement a minimal, read-only present plugin**

```rust
use bevy::prelude::*;
use crate::events::{DamageResolved, EntityDied, CastPhaseChanged};

pub struct ObeliskPresentPlugin;
impl Plugin for ObeliskPresentPlugin {
    fn build(&self, app: &mut App) {
        // Read-only: log gameplay events. Real VFX/audio observers attach here.
        app.add_observer(|e: On<DamageResolved>| {
            let d = e.event();
            info!("DamageResolved: {:.1} dmg to {:?}{}", d.total_damage, d.target, if d.is_killing_blow { " (KILL)" } else { "" });
        });
        app.add_observer(|e: On<CastPhaseChanged>| {
            let c = e.event();
            info!("Cast {} phase {:?} -> {:?} @ {:.2}s", c.skill_id, c.from, c.to, c.elapsed);
        });
        app.add_observer(|e: On<EntityDied>| info!("EntityDied: {:?}", e.event().target));
        // Avian debug gizmos for hit/hurtboxes (render-only; safe to add only in client builds).
        app.add_plugins(avian3d::prelude::PhysicsDebugPlugin::default());
    }
}
```

- [ ] **Step 2: Build with and without present**

Run: `cargo build` and `cargo build --no-default-features`
Expected: both PASS. (Confirm `PhysicsDebugPlugin::default()` exists in avian3d 0.4; if it's `PhysicsDebugPlugin` unit struct, drop `::default()`.)

- [ ] **Step 3: Commit**

```bash
git add src/present/mod.rs
git commit -m "feat: read-only present plugin (event logging + debug gizmos)"
```

### Task 23: The end-to-end headless scenario test

**Files:**
- Create: `tests/vertical_slice.rs`

- [ ] **Step 1: Write the integration test**

```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life; b.current_life = life;
    b.max_mana.base = mana; b.current_mana = mana;
    b
}

#[test]
fn firebolt_casts_hits_damages_and_burns_to_death() {
    let mut t = ObeliskTestApp::new(42);

    // Load skills (rules) + the cast timeline asset.
    t.app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    let handle: Handle<CastTimeline> = t.app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
    // Wait for the asset to load.
    for _ in 0..2000 {
        t.app.update();
        if t.app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() { break; }
    }
    t.app.world_mut().resource_mut::<CastTimelineHandles>().0.insert("firebolt".into(), handle);

    // Spawn player + dummy a short distance apart along +Z (the bolt travels +Z).
    let player = t.app.world_mut().spawn((Combatant, Attributes(make_block("player", 100.0, 100.0)), Faction::Player, ObeliskId("player".into()), Transform::from_xyz(0.0, 0.0, 0.0))).id();
    let dummy = t.app.world_mut().spawn((Combatant, Attributes(make_block("dummy", 60.0, 0.0)), Faction::Enemy, ObeliskId("dummy".into()), Transform::from_xyz(0.0, 0.0, 2.0))).id();
    {
        let mut commands = t.app.world_mut().commands();
        insert_hurtbox(&mut commands, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();

    // Cast firebolt at the dummy.
    t.app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);

    // Advance enough ticks for windup (0.3s) + projectile flight + burn ticks.
    t.advance_ticks(600); // 10 seconds at 60hz

    let rec = t.rec();
    assert!(!rec.cast_began.is_empty(), "cast should begin");
    assert!(rec.phase_changed.iter().any(|p| matches!(p.to, SkillPhase::Active)), "should reach Active phase");
    assert!(!rec.hit_window_opened.is_empty(), "bolt window should open");
    assert!(!rec.hit_confirmed.is_empty(), "bolt should hit the dummy");
    assert!(!rec.damage_resolved.is_empty(), "damage should resolve");
    assert!(rec.effect_applied.iter().any(|e| e.effect_id == "burn"), "burn should be applied");
    assert!(!rec.dot_ticked.is_empty(), "burn should tick");
    assert!(rec.died.iter().any(|d| d.target == dummy), "dummy should die from impact + burn");
}
```

- [ ] **Step 2: Run the full slice**

Run: `cargo test --features test-support firebolt_casts_hits_damages_and_burns_to_death -- --nocapture`
Expected: PASS. **Debugging order if it fails:** (1) assert `cast_began` first; if missing, the timeline asset wasn't in `CastTimelineHandles` or `can_use_skill` failed. (2) If `hit_confirmed` missing, the projectile didn't overlap — check the bolt's `+Z` direction reaches `(0,0,2)` within the window's `active_duration` and that `detect_overlaps`'s hard-coded 0.5 radius + the 0.6 hurtbox overlap; widen if needed. (3) If `died` missing, increase `firebolt` base damage or burn `base_damage_percent`, or add ticks.

- [ ] **Step 3: Add a determinism assertion**

```rust
#[test]
fn slice_is_deterministic_across_two_runs() {
    let run = || {
        let mut t = ObeliskTestApp::new(99);
        t.app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
        let h: Handle<CastTimeline> = t.app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
        for _ in 0..2000 { t.app.update(); if t.app.world().resource::<Assets<CastTimeline>>().get(&h).is_some() { break; } }
        t.app.world_mut().resource_mut::<CastTimelineHandles>().0.insert("firebolt".into(), h);
        let player = t.app.world_mut().spawn((Combatant, Attributes(make_block("player", 100.0, 100.0)), Faction::Player, ObeliskId("player".into()), Transform::from_xyz(0.0,0.0,0.0))).id();
        let dummy = t.app.world_mut().spawn((Combatant, Attributes(make_block("dummy", 60.0, 0.0)), Faction::Enemy, ObeliskId("dummy".into()), Transform::from_xyz(0.0,0.0,2.0))).id();
        { let mut c = t.app.world_mut().commands(); insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0,0.0,2.0)); }
        t.app.update();
        t.app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);
        t.advance_ticks(600);
        t.rec().damage_resolved.iter().map(|d| d.total_damage).sum::<f64>()
    };
    assert_eq!(run(), run(), "same seed -> identical total damage");
}
```

Run: `cargo test --features test-support slice_is_deterministic -- --nocapture`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add tests/vertical_slice.rs
git commit -m "test: end-to-end headless vertical slice + determinism"
```

### Task 24: The visual playground example

**Files:**
- Create: `examples/playground.rs`

- [ ] **Step 1: Write a minimal visual app**

```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use stat_core::StatBlock;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
       .add_plugins(ObeliskPlugins)
       .insert_resource(Time::<Fixed>::from_hz(60.0));
    use obelisk_bevy::prelude::ObeliskConfigExt;
    app.add_obelisk_config_constants_default();
    if !stat_core::config::effect_registry_initialized() {
        stat_core::init_effect_registry(std::path::Path::new("tests/fixtures/effects")).unwrap();
    }
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    app.seed_combat_rng(1);
    app.add_systems(Startup, setup);
    app.add_systems(Update, cast_on_space);
    app.run();
}

#[derive(Resource)]
struct Players { player: Entity, dummy: Entity }

fn setup(mut commands: Commands, assets: Res<AssetServer>, mut handles: ResMut<CastTimelineHandles>,
         mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    handles.0.insert("firebolt".into(), assets.load("assets/skills/firebolt.cast.ron"));
    commands.spawn((Camera3d::default(), Transform::from_xyz(6.0, 6.0, 6.0).looking_at(Vec3::new(0.0,0.0,1.0), Vec3::Y)));
    commands.spawn((DirectionalLight::default(), Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)));

    let mut pblock = StatBlock::with_id("player"); pblock.max_mana.base = 100.0; pblock.current_mana = 100.0; pblock.max_life.base = 100.0; pblock.current_life = 100.0;
    let player = commands.spawn((Combatant, Attributes(pblock), Faction::Player, ObeliskId("player".into()),
        Transform::from_xyz(0.0,0.0,0.0),
        Mesh3d(meshes.add(Capsule3d::new(0.3, 1.2))), MeshMaterial3d(mats.add(Color::srgb(0.2,0.5,1.0))))).id();

    let mut dblock = StatBlock::with_id("dummy"); dblock.max_life.base = 60.0; dblock.current_life = 60.0;
    let dummy = commands.spawn((Combatant, Attributes(dblock), Faction::Enemy, ObeliskId("dummy".into()),
        Transform::from_xyz(0.0,0.0,4.0),
        Mesh3d(meshes.add(Sphere::new(0.6))), MeshMaterial3d(mats.add(Color::srgb(1.0,0.3,0.2))))).id();
    insert_hurtbox(&mut commands, dummy, 0.6, Vec3::new(0.0,0.0,4.0));

    commands.insert_resource(Players { player, dummy });
}

fn cast_on_space(keys: Res<ButtonInput<KeyCode>>, players: Res<Players>, mut commands: Commands) {
    if keys.just_pressed(KeyCode::Space) {
        commands.entity(players.player).cast_skill_at("firebolt", players.dummy);
    }
}
```

- [ ] **Step 2: Build and (manually) run**

Run: `cargo build --example playground`
Expected: PASS. Manual check: `cargo run --example playground`, press Space, observe the cast log lines (from the present plugin) and the debug gizmos. Confirm `Mesh3d`/`MeshMaterial3d`/`Camera3d`/`DirectionalLight` are the correct bevy 0.17 component names (they are 0.15+); adjust if your patch differs.

- [ ] **Step 3: Commit**

```bash
git add examples/playground.rs
git commit -m "feat: visual playground example"
```

### Task 25: Full suite green + README usage

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Run the entire test suite**

Run: `cargo test --features test-support`
Expected: PASS (all unit + integration tests).

- [ ] **Step 2: Confirm clippy + fmt**

Run: `cargo clippy --all-targets --features test-support -- -D warnings` then `cargo fmt --check`
Expected: PASS. Fix any lints (notably remove the leftover `phase_start` no-op from Task 17).

- [ ] **Step 3: Add a Usage section to `README.md`** documenting the 8-step integration walkthrough (from spec §4) with the firebolt example.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: usage walkthrough; vertical slice complete"
```

---

## Self-review notes (coverage vs spec §11)

- Cast → validate → ActiveCast: Tasks 16. ✅
- Speed-scaled phase durations (spec §7.1): Task 10 + applied in Task 16. ✅
- Windup→Active→Recovery + CastPhaseChanged: Task 17. ✅
- Spatiotemporal projectile hitbox + HitWindowOpened: Task 17 + 15. ✅
- Avian overlap (SpatialQuery) → HitConfirmed: Tasks 13–14. ✅
- Deterministic resolve funnel (use_skill_against → resolve_damage_with_triggers → write-back), no thread_rng: Task 7. ✅
- DamageResolved / EffectApplied / EntityDied: Task 18. ✅
- Burn applied + tick_effects DoT + DotTicked: Tasks 8, 19. ✅
- Present observer logging + gizmos, feature-gated: Task 22; `--no-default-features` builds confirm the boundary (Task 20/22). ✅
- Headless deterministic scenario test + visual playground: Tasks 23, 24. ✅

## Deferred past the slice (tracked in spec, not in this plan)

Entity-targeted events + `Message` netcode mirror; `ObeliskCombat`/`ObeliskRead`/`ObeliskSpatial` SystemParam facades; cone/sector hit geometry + per-hitbox stored collider + true `HitFilter`/`HitMode::EveryTick` re-hit intervals; ground/direction targeting + range/LOS validation; cooldowns resource; loot/skill-tree/drop-table wiring; VFX cue id layer + `observe_cue`; trigger-cascade depth guard; bevy feature-trimming for a truly minimal headless server build.
