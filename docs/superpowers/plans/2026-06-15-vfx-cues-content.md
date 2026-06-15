# VFX Cues & Content Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give games a clean VFX-sequencing hook layer (skill-authored cue ids → `CueEvent`s with positions, consumed via `observe_cue`), fix two surfaced DoT minors, and wire obelisk's content systems in: skill-tree stat sources and loot/drop-table rolls on death.

**Architecture:** Skills carry a `vfx_cues` map (cue-id strings) in the `CastTimeline` asset. `ObeliskCuePlugin` (part of the sim, so it's headless-testable and servers can ignore it) observes the existing gameplay events (`CastBegan`/`HitWindowOpened`/`HitConfirmed`), looks up the skill's cues, and emits `CueEvent { cue_id, source, position, kind }`. Consumers bind a cue id to a handler with `App::observe_cue(id, handler)`. Content: an `apply_stat_sources` verb rebuilds a `StatBlock` from `StatSource`s (incl. `SkillTree::to_stat_source`); a `DropTables` resource + `DropTableId` component roll loot on `EntityDied` into a `LootDropped` event. Determinism + existing events untouched.

**Tech Stack:** Rust 2021, Bevy 0.17.3, the obelisk libs (`loot_core`, `tables_core`, `skill_tree`, `stat_core`), the existing obelisk-bevy sim.

---

## Verified facts (ground truth)

- `CastTimeline` (src/assets/mod.rs) currently: `skill_id, phase_durations, collision_windows, targeting, delivery` — **no `vfx_cues`** (this batch adds it). Loaded via the RON `CastTimelineLoader`; `CastTimelineHandles(HashMap<String, Handle<CastTimeline>>)`.
- Events (src/events.rs, all `#[derive(Event)]` observer events): `CastBegan{caster:Entity,skill_id,total_duration}`, `HitWindowOpened{caster:Entity,skill_id,window_id:String,hitbox:Entity}`, `HitConfirmed{caster:Entity,target:Entity,skill_id,window_id}`, `EntityDied{target:Entity,killer:Option<Entity>}`.
- `crate::spatial::boxes::Hitbox` has a `Transform` (the hitbox entity); `Transform` on combatants gives positions.
- `crate::core::tick::tick_effects_system` iterates `Query<(Entity, &mut Attributes)>`, calls `attrs.0.tick_effects(dt)`, emits `DotTicked`/`EffectExpired`/`EntityDied`. `StatBlock::is_alive() -> bool`.
- `stat_core::StatBlock::rebuild_from_sources(&mut self, sources: &[Box<dyn StatSource>])`; `stat_core::source::StatSource` trait (Send+Sync). `skill_tree::SkillTree::to_stat_source() -> SkillTreeSource` (a `StatSource`), and `SkillTree::apply_to(&mut StatBlock)`.
- `loot_core::Config::load_from_dir(&Path)`, `loot_core::Generator::new(Config)`, `Generator::generate(base_type_id, seed) -> Result<Item, _>`.
- `tables_core::DropTableRegistry::load_from_strings(&[(&str, &str)]) -> Result<Self, _>`, `roll<R: Rng>(table_id, rarity_mult: f64, quantity_mult: f64, level: u32, rng) -> Result<Vec<Drop>, _>`; `tables_core::Drop` (`Item{base_type,currencies}`/`Currency{id,count}`/`Unique{id}`), `DropsExt` (get_items/get_currencies/get_uniques). `Drop` derives `Debug, Clone, PartialEq, Eq`.
- `crate::core::config::CombatRng(pub ChaCha8Rng)` is the seeded RNG.
- The `ObeliskTestApp` harness drives the headless sim. Baseline at the start of this batch: **41 passing**. Loot deps (`loot_core`, `tables_core`, `skill_tree`) are already path-deps of the crate.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/assets/mod.rs` | modify | `CastTimeline` gains `#[serde(default)] vfx_cues: HashMap<String,String>`. |
| `src/events.rs` | modify | `CueEvent { cue_id, source, position, kind: CueKind }` + `CueKind` enum; `LootDropped { source, drops }`. |
| `src/vfx.rs` | create | `ObeliskCuePlugin` (cue emission observers) + `ObeliskCueExt::observe_cue`. |
| `src/core/tick.rs` | modify | skip already-dead entities (DoT-after-death fix). |
| `src/verbs.rs` | modify | `apply_stat_sources` verb (rebuild from `Box<dyn StatSource>`s). |
| `src/loot.rs` | create | `DropTables` resource, `DropTableId` component, `ItemGenerator` resource, `roll_drops_on_death` system, `ObeliskLootPlugin`. |
| `src/lib.rs` | modify | `pub mod vfx; pub mod loot;`; add `ObeliskCuePlugin` + `ObeliskLootPlugin` to `ObeliskSimPlugin`. |
| `src/prelude.rs` | modify | export the new surface. |
| `tests/vfx_content.rs` | create | Integration: firebolt fires `on_cast`/`on_hit` cues; loot rolls on death. |
| `README.md` | modify | "VFX cues" + "Content" sections. |

---

## Task 1: `vfx_cues` on the asset + `CueEvent` types

**Files:** Modify `src/assets/mod.rs`, `src/events.rs`.

- [ ] **Step 1: Add `vfx_cues` to `CastTimeline` in `src/assets/mod.rs`**

Add the field (serde default so existing `.cast.ron` files without it still parse):
```rust
    #[serde(default)]
    pub vfx_cues: std::collections::HashMap<String, String>,
```
as the last field of `CastTimeline`. Keys are cue slots (`"on_cast"`, `"on_hit"`, `"on_window_<window_id>"`); values are game-defined cue ids.

- [ ] **Step 2: Add `CueEvent` + `CueKind` to `src/events.rs`**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueKind { OnCast, OnWindow, OnHit }

/// A VFX/audio cue fired by a skill at a moment in its timeline. The presentation layer
/// (or game) binds `cue_id` to a handler via `App::observe_cue`.
#[derive(Event, Clone, Debug)]
pub struct CueEvent {
    pub cue_id: String,
    /// The entity the cue is anchored to (caster for OnCast/OnWindow, target for OnHit).
    pub source: Entity,
    /// World position to spawn the effect at.
    pub position: Vec3,
    pub kind: CueKind,
}
```

- [ ] **Step 3: Build + suite**

Run: `cargo test --features test-support --lib --tests` → all green (41). The firebolt/cleave `.cast.ron` files have no `vfx_cues` → default empty map → still parse.

- [ ] **Step 4: Commit**

```bash
git add src/assets/mod.rs src/events.rs
git commit -m "feat(vfx): vfx_cues on CastTimeline + CueEvent/CueKind types"
```
`git config commit.gpgsign false` if signing prompts.

---

## Task 2: `ObeliskCuePlugin` — emit `CueEvent`s from the timeline

**Files:** Create `src/vfx.rs`; Modify `src/lib.rs`.

- [ ] **Step 1: Write the cue-emission observers in `src/vfx.rs`**

```rust
use bevy::prelude::*;
use crate::assets::{CastTimeline, CastTimelineHandles};
use crate::events::{CastBegan, CueEvent, CueKind, HitConfirmed, HitWindowOpened};

/// Emits `CueEvent`s from a skill's authored `vfx_cues` at cast/window/hit moments.
/// Part of the sim (cheap + headless-testable); servers simply don't observe CueEvent.
pub struct ObeliskCuePlugin;

impl Plugin for ObeliskCuePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(cue_on_cast);
        app.add_observer(cue_on_window);
        app.add_observer(cue_on_hit);
    }
}

/// Look up a skill's cue id for a given slot, if the timeline is loaded.
fn cue_for(
    handles: &CastTimelineHandles,
    timelines: &Assets<CastTimeline>,
    skill_id: &str,
    slot: &str,
) -> Option<String> {
    let handle = handles.0.get(skill_id)?;
    let timeline = timelines.get(handle)?;
    timeline.vfx_cues.get(slot).cloned()
}

fn cue_on_cast(
    ev: On<CastBegan>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
    mut commands: Commands,
) {
    let e = ev.event();
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, "on_cast") {
        let position = transforms.get(e.caster).map(|t| t.translation).unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent { cue_id, source: e.caster, position, kind: CueKind::OnCast });
    }
}

fn cue_on_window(
    ev: On<HitWindowOpened>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
    mut commands: Commands,
) {
    let e = ev.event();
    let slot = format!("on_window_{}", e.window_id);
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, &slot) {
        let position = transforms.get(e.hitbox).map(|t| t.translation).unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent { cue_id, source: e.caster, position, kind: CueKind::OnWindow });
    }
}

fn cue_on_hit(
    ev: On<HitConfirmed>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
    mut commands: Commands,
) {
    let e = ev.event();
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, "on_hit") {
        let position = transforms.get(e.target).map(|t| t.translation).unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent { cue_id, source: e.target, position, kind: CueKind::OnHit });
    }
}
```

- [ ] **Step 2: Wire `pub mod vfx;` in `src/lib.rs` + add `ObeliskCuePlugin` to `ObeliskSimPlugin`**

Add `pub mod vfx;` (below the doc block). In `ObeliskSimPlugin::build`, add to the `add_plugins` chain:
```rust
        app.add_plugins(vfx::ObeliskCuePlugin);
```

- [ ] **Step 3: Build + suite**

Run: `cargo test --features test-support --lib --tests` → all green (41). (No skill has cues yet, so no CueEvents fire — just confirm nothing breaks.)

- [ ] **Step 4: Commit**

```bash
git add src/vfx.rs src/lib.rs
git commit -m "feat(vfx): ObeliskCuePlugin emits CueEvents from authored vfx_cues"
```

---

## Task 3: `observe_cue` ergonomic + cue integration test

**Files:** Modify `src/vfx.rs`, `assets/skills/firebolt.cast.ron`; Create `tests/vfx_content.rs`.

- [ ] **Step 1: Add `ObeliskCueExt::observe_cue` to `src/vfx.rs`**

```rust
use crate::events::CueEvent;

/// App-builder ergonomic: run `handler` whenever a `CueEvent` with `cue_id` fires.
pub trait ObeliskCueExt {
    fn observe_cue(
        &mut self,
        cue_id: impl Into<String>,
        handler: impl Fn(&CueEvent, &mut Commands) + Send + Sync + 'static,
    ) -> &mut Self;
}

impl ObeliskCueExt for App {
    fn observe_cue(
        &mut self,
        cue_id: impl Into<String>,
        handler: impl Fn(&CueEvent, &mut Commands) + Send + Sync + 'static,
    ) -> &mut Self {
        let cue_id = cue_id.into();
        self.add_observer(move |ev: On<CueEvent>, mut commands: Commands| {
            if ev.event().cue_id == cue_id {
                handler(ev.event(), &mut commands);
            }
        });
        self
    }
}
```

- [ ] **Step 2: Add cue ids to `assets/skills/firebolt.cast.ron`**

Add a `vfx_cues` map (the existing fields stay; add this entry inside the `( ... )`):
```ron
  vfx_cues: { "on_cast": "firebolt_cast", "on_hit": "firebolt_impact" },
```

- [ ] **Step 3: Write the cue integration test in `tests/vfx_content.rs`**

```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;
use std::sync::{Arc, Mutex};

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life; b.current_life = life;
    b.max_mana.base = mana; b.current_mana = mana;
    b
}

#[test]
fn firebolt_fires_cast_and_hit_cues() {
    let mut t = ObeliskTestApp::new(42);
    let handle: Handle<CastTimeline> =
        t.app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 {
        t.app.update();
        if t.app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() { break; }
    }
    t.app.world_mut().resource_mut::<CastTimelineHandles>().0.insert("firebolt".into(), handle);

    // Record which cue ids fired (shared via Arc<Mutex> since the handler is Fn).
    let fired: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let f1 = fired.clone();
    t.app.observe_cue("firebolt_cast", move |_cue, _cmds| f1.lock().unwrap().push("firebolt_cast".into()));
    let f2 = fired.clone();
    t.app.observe_cue("firebolt_impact", move |_cue, _cmds| f2.lock().unwrap().push("firebolt_impact".into()));

    let player = t.app.world_mut().spawn((Combatant, Attributes(make_block("player", 100.0, 100.0)), Faction::Player, ObeliskId("player".into()), Transform::from_xyz(0.0, 0.0, 0.0))).id();
    let dummy = t.app.world_mut().spawn((Combatant, Attributes(make_block("dummy", 25.0, 0.0)), Faction::Enemy, ObeliskId("dummy".into()), Transform::from_xyz(0.0, 0.0, 2.0))).id();
    {
        let mut c = t.app.world_mut().commands();
        insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();
    t.app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);
    t.advance_ticks(600);

    let fired = fired.lock().unwrap();
    assert!(fired.contains(&"firebolt_cast".to_string()), "on_cast cue should fire");
    assert!(fired.contains(&"firebolt_impact".to_string()), "on_hit cue should fire");
    let _ = dummy;
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test --features test-support --test vfx_content -- --nocapture` → PASS. Report which cues fired.
- **If the `observe_cue` `Fn(&CueEvent, &mut Commands)` bound doesn't satisfy bevy's `IntoObserverSystem`**, adjust the wrapper (e.g. drop the `Commands` param and make it `Fn(&CueEvent)`, or use the form that compiles) and update the test handler signature to match. Report the working form.
- Also run the full suite (`cargo test --features test-support --lib --tests`).
```bash
git add src/vfx.rs assets/skills/firebolt.cast.ron tests/vfx_content.rs
git commit -m "feat(vfx): observe_cue ergonomic + firebolt cue integration test"
```

---

## Task 4: Cleanup — stop DoT ticking dead entities

**Files:** Modify `src/core/tick.rs`.

- [ ] **Step 1: Skip already-dead entities in `tick_effects_system`**

In the per-entity loop of `tick_effects_system`, after the `effects_is_empty_fast` early-continue, add a dead-skip so a corpse stops emitting `DotTicked`/`EntityDied`:
```rust
        if !attrs.0.is_alive() {
            continue;
        }
```
(Place it right after the `if attrs.0.effects_is_empty_fast() { continue; }` line, before `tick_effects(dt)` is called.)

- [ ] **Step 2: Add a regression test to `src/core/tick.rs`** (`#[cfg(test)] mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use stat_core::StatBlock;

    #[test]
    fn dead_entities_stop_ticking_dots() {
        let mut t = ObeliskTestApp::new(1);
        // A dummy that is already dead (0 life) but still has effects shouldn't emit DotTicked.
        let mut block = StatBlock::with_id("corpse");
        block.max_life.base = 10.0;
        block.current_life = 0.0; // dead
        let e = t.app.world_mut().spawn((Combatant, Attributes(block), Faction::Enemy, ObeliskId("corpse".into()), Transform::default())).id();
        // Give it a damaging effect via the verb (burn from fixtures).
        t.app.world_mut().commands().entity(e).apply_obelisk_effect("burn");
        t.app.update();
        // Drain DotTicked into a counter.
        #[derive(Resource, Default)]
        struct Ticks(usize);
        t.app.init_resource::<Ticks>();
        t.app.add_observer(|_e: On<DotTicked>, mut c: ResMut<Ticks>| c.0 += 1);
        t.advance_ticks(120);
        assert_eq!(t.app.world().resource::<Ticks>().0, 0, "a dead entity must not emit DoT ticks");
        let _ = e;
    }
}
```

- [ ] **Step 3: Run + commit**

Run: `cargo test --features test-support --lib dead_entities_stop_ticking` → PASS, then full suite. (The firebolt slice/netcode tests previously saw post-death ticks; they assert on presence of events, not counts, so they stay green — but confirm.)
```bash
git add src/core/tick.rs
git commit -m "fix(tick): dead entities stop ticking DoTs (no post-death DotTicked)"
```

---

## Task 5: Skill-tree stat sources — `apply_stat_sources`

**Files:** Modify `src/verbs.rs`.

- [ ] **Step 1: Add `apply_stat_sources` to `ObeliskCommandsExt` in `src/verbs.rs`**

Add to the trait:
```rust
    /// Rebuild this entity's StatBlock from the given stat sources (e.g. a skill tree's
    /// `to_stat_source()`, equipped items). Replaces prior source-derived stats.
    fn apply_stat_sources(&mut self, sources: Vec<Box<dyn stat_core::source::StatSource>>) -> &mut Self;
```
Add to the impl (uses the confirmed `EntityWorldMut` command form):
```rust
    fn apply_stat_sources(&mut self, sources: Vec<Box<dyn stat_core::source::StatSource>>) -> &mut Self {
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut attrs) = entity.get_mut::<Attributes>() {
                attrs.0.rebuild_from_sources(&sources);
            }
        });
        self
    }
```

- [ ] **Step 2: Add a test to `src/verbs.rs`** (in the existing `#[cfg(test)] mod tests`)

The point is to prove `apply_stat_sources` rebuilds the `StatBlock` from `Box<dyn StatSource>`s. Use a **minimal hand-rolled `StatSource`** (no skill_tree-builder dependency). The `StatSource` trait is `fn id(&self) -> &str`, `fn priority(&self) -> i32` (default 0), `fn apply(&self, stats: &mut StatAccumulator)`. **Read `stat_core/src/stat_block/aggregator.rs` for how `StatAccumulator` accepts a flat stat** (e.g. the field/method gear & skill-tree sources use to add flat life — copy that pattern), and write the `apply` body accordingly:
```rust
    #[test]
    fn apply_stat_sources_rebuilds_from_a_source() {
        use stat_core::source::StatSource;
        use stat_core::stat_block::aggregator::StatAccumulator;

        // Minimal source that adds flat max-life. Fill the `apply` body using the accumulator's
        // real add-mechanism (read aggregator.rs — match how SkillTreeSource/gear add flat life).
        struct LifeSource;
        impl StatSource for LifeSource {
            fn id(&self) -> &str { "test_life" }
            fn apply(&self, stats: &mut StatAccumulator) {
                // e.g. add +50 flat life via the accumulator's flat-life field/method:
                //   stats.add_flat(StatType::AddedLife, 50.0);   // <- use the REAL API
            }
        }

        let mut t = ObeliskTestApp::new(1);
        let e = t.app.world_mut().spawn((Combatant, Attributes(StatBlock::with_id("hero")), Faction::Player, ObeliskId("hero".into()), Transform::default())).id();
        let base_life = t.app.world().entity(e).get::<Attributes>().unwrap().0.computed_max_life();

        t.app.world_mut().commands().entity(e).apply_stat_sources(vec![Box::new(LifeSource)]);
        t.app.update();

        let new_life = t.app.world().entity(e).get::<Attributes>().unwrap().0.computed_max_life();
        assert!(new_life > base_life, "a flat-life source should raise computed_max_life ({base_life} -> {new_life})");
    }
```
Fill `LifeSource::apply` with the accumulator's real add-API. (Skill-tree compatibility is then automatic: `skill_tree::SkillTree::to_stat_source()` returns a `StatSource`, so `apply_stat_sources(vec![Box::new(tree.to_stat_source())])` works the same way — note this in the README, no separate test needed.) Report the exact accumulator add-call you used.

- [ ] **Step 3: Run + commit**

Run: `cargo test --features test-support --lib apply_stat_sources_from_a_skill_tree` → PASS, then full suite.
```bash
git add src/verbs.rs
git commit -m "feat(verbs): apply_stat_sources (rebuild StatBlock from skill-tree/item sources)"
```

---

## Task 6: Loot — roll drop tables on death

**Files:** Create `src/loot.rs`; Modify `src/events.rs`, `src/lib.rs`.

- [ ] **Step 1: Add the `LootDropped` event to `src/events.rs`**

```rust
#[derive(Event, Clone, Debug)]
pub struct LootDropped {
    /// The entity that died and dropped loot.
    pub source: Entity,
    /// The rolled drops (item base types, currencies, uniques).
    pub drops: Vec<tables_core::Drop>,
}
```

- [ ] **Step 2: Write `src/loot.rs`**

```rust
use bevy::prelude::*;
use crate::core::config::CombatRng;
use crate::events::{EntityDied, LootDropped};

/// Drop tables (from `tables_core`). Load via `DropTableRegistry::load`/`load_from_strings`.
#[derive(Resource)]
pub struct DropTables(pub tables_core::DropTableRegistry);

/// Optional loot generator (from `loot_core`) for turning item drops into full `Item`s.
#[derive(Resource)]
pub struct ItemGenerator(pub loot_core::Generator);

/// Which drop table an entity rolls on death.
#[derive(Component, Clone, Debug)]
pub struct DropTableId(pub String);

/// On death, roll the dead entity's drop table (if any) and emit `LootDropped`.
pub fn roll_drops_on_death(
    death: On<EntityDied>,
    tables: Option<Res<DropTables>>,
    drop_ids: Query<&DropTableId>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
) {
    let Some(tables) = tables else { return };
    let target = death.event().target;
    let Ok(table_id) = drop_ids.get(target) else { return };
    // rarity_mult, quantity_mult, level — sensible defaults; games can scale these.
    if let Ok(drops) = tables.0.roll(&table_id.0, 1.0, 1.0, 1, &mut rng.0) {
        if !drops.is_empty() {
            commands.trigger(LootDropped { source: target, drops });
        }
    }
}

pub struct ObeliskLootPlugin;
impl Plugin for ObeliskLootPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(roll_drops_on_death);
    }
}
```

- [ ] **Step 3: Wire into `src/lib.rs`**

Add `pub mod loot;`. In `ObeliskSimPlugin::build`, add `app.add_plugins(loot::ObeliskLootPlugin);`. (`DropTables`/`ItemGenerator` resources are inserted by the consumer when they have loot config — the loot plugin's observer no-ops if `DropTables` is absent.)

- [ ] **Step 4: Add a loot test to `tests/vfx_content.rs`**

```rust
#[test]
fn dead_enemy_with_a_drop_table_drops_loot() {
    use obelisk_bevy::loot::{DropTableId, DropTables};
    let mut t = ObeliskTestApp::new(7);

    // Inline drop table (no file dependency): always rolls 1 entry, which drops "gold".
    // Schema confirmed from obelisk/config/tables/new_table.toml: [table] + [[table.rolls]] + [[entries]].
    let table_toml = r#"
[table]
id = "goblin"

[[table.rolls]]
count = 1
weight = 1

[[entries]]
type = "currency"
weight = 1
id = "gold"
"#;
    let registry = tables_core::DropTableRegistry::load_from_strings(&[("goblin", table_toml)])
        .expect("load drop table");
    t.app.insert_resource(DropTables(registry));

    // Collect LootDropped.
    #[derive(Resource, Default)]
    struct Loot(Vec<tables_core::Drop>);
    t.app.init_resource::<Loot>();
    t.app.add_observer(|e: On<obelisk_bevy::events::LootDropped>, mut l: ResMut<Loot>| {
        l.0.extend(e.event().drops.iter().cloned());
    });

    let goblin = t.app.world_mut().spawn((
        Combatant, Attributes(make_block("goblin", 10.0, 0.0)), Faction::Enemy, ObeliskId("goblin".into()), Transform::default(),
        DropTableId("goblin".into()),
    )).id();
    t.app.update();
    // Trigger death directly (programmatic) — exercises the loot-on-death observer.
    t.app.world_mut().commands().trigger(obelisk_bevy::events::EntityDied { target: goblin, killer: None });
    t.app.update();

    assert!(!t.app.world().resource::<Loot>().0.is_empty(), "a dead enemy with a drop table should drop loot");
}
```
The drop-table schema above is confirmed from `obelisk/config/tables/new_table.toml` (`[table]` + `[[table.rolls]]` + `[[entries]]`). Still **verify `roll(...)` returns a non-empty `Vec<Drop>`** — if a currency entry needs extra fields (e.g. `currencies = []`, `rarity_bonus`) to parse/roll, add them per `tables_core/src/config.rs` (`EntryConfig`). Report the working table format if you adjust it.

- [ ] **Step 5: Run + commit**

Run: `cargo test --features test-support --test vfx_content -- --nocapture` → both tests pass (cue + loot). Then full suite.
```bash
git add src/loot.rs src/events.rs src/lib.rs tests/vfx_content.rs
git commit -m "feat(loot): DropTables + roll_drops_on_death -> LootDropped"
```

---

## Task 7: Prelude exports + README + quality gates

**Files:** Modify `src/prelude.rs`, `README.md`.

- [ ] **Step 1: Export the new surface in `src/prelude.rs`**

```rust
pub use crate::events::{CueEvent, CueKind, LootDropped};
pub use crate::vfx::ObeliskCueExt;
pub use crate::loot::{DropTableId, DropTables, ItemGenerator};
```

- [ ] **Step 2: Add "VFX cues" + "Content" sections to `README.md`**

- VFX: skills author `vfx_cues` (slot → cue id) in their `.cast.ron`; the sim emits `CueEvent { cue_id, source, position, kind }` at cast/window/hit; bind with `app.observe_cue("firebolt_impact", |cue, cmds| { /* spawn particles at cue.position */ })`. Show the firebolt `vfx_cues` example.
- Content: `apply_stat_sources(vec![tree.to_stat_source()])` to fold a skill tree's stats into a combatant; insert a `DropTables` resource + a `DropTableId` component, and observe `LootDropped` to receive rolled drops on death (optionally generate full items via an `ItemGenerator`).
Keep it concise and accurate to the exported names.

- [ ] **Step 3: Quality gates**

- `cargo test --features test-support --lib --tests` → all green.
- `cargo clippy --features test-support --lib --tests -- -D warnings` → clean for `obelisk-bevy`.
- `cargo fmt` then `cargo fmt --check` → clean.
- `cargo build --no-default-features` → headless build still compiles (the new modules are sim-side, not present-gated).

- [ ] **Step 4: Commit**

```bash
git add src/prelude.rs README.md
git commit -m "docs: VFX cues + content README sections; export cue/loot surface"
```

---

## Self-review notes (coverage vs the batch scope)

- VFX cue-id layer (`vfx_cues` asset field + `CueEvent` emission): Tasks 1,2 ✅
- `observe_cue(cue_id, handler)` ergonomic: Task 3 ✅
- Cleanup — DoT-after-death: Task 4 ✅
- Skill-tree stat-source integration (`apply_stat_sources`): Task 5 ✅
- Loot/drop-table roll on death (`DropTables`/`DropTableId`/`LootDropped`/`ItemGenerator`): Task 6 ✅
- Prelude + docs + gates: Task 7 ✅
- Backward compat (slice/spatial/facade/netcode green): asserted each task ✅

## Deferred (out of this batch, noted to the user)

- **In-process `EntityEvent` propagation** (parent rig auto-observes child-volume events): a deeper refactor of the global observer-event system that the recorder/present/netcode all depend on — high churn, low marginal value (consumers can observe `CueEvent`/`HitConfirmed` globally and read `source`). Left as a focused refactor if needed.
- **Per-effect `DotTicked.effect_id`**: still the rollup (empty id) — `tick_effects` returns only total `dot_damage`, no per-effect breakdown; populating it needs a `stat_core` `TickResult` enhancement (an obelisk-side change), out of scope here.
- **Full `Item` generation pipeline on drop** (rolling `Drop::Item` base types through `ItemGenerator` + applying currencies): the `ItemGenerator` resource is wired; turning rolled item drops into fully-rolled `Item`s with affixes is a content-pipeline follow-on.
- **Full bevy render-feature trim** (server binary slimming): unchanged from the netcode batch's note.
