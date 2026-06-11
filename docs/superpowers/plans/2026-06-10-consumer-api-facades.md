# Consumer API Facades & Verbs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give game developers an ergonomic, one-stop consumer API on top of the obelisk-bevy sim: cooldowns, read-only HUD/AI queries (`ObeliskRead`), spatial target acquisition (`ObeliskSpatial`), an authoritative programmatic combat entry (`ObeliskCombat`), and EntityCommands verbs (`make_combatant`, `apply_obelisk_effect`, `grant_skill`, `grant_barrier`, `grant_elude`).

**Architecture:** Add custom `#[derive(SystemParam)]` facades that bundle the right queries/resources behind verb-style methods, and an `EntityCommands` extension trait for spawn/grant verbs (using the `queue(|entity, world| ...)` command form for component mutation). A new `Cooldowns` resource is ticked each `FixedUpdate` and gates casting. Everything composes with the existing sim; the firebolt slice and the spatial-targeting tests stay green.

**Tech Stack:** Rust 2021, Bevy 0.17.3 (`SystemParam` derive, `EntityCommand` closures, `World::trigger`), Avian3d 0.4.1 (`SpatialQuery`), obelisk path-deps.

---

## Verified obelisk facts (ground truth)

- `StatBlock`: `id: String`, `current_life: f64`, `current_mana: f64`, `cooldown_reduction: f64` (public field), `computed_max_life() -> f64` (mod.rs:903), `computed_max_mana() -> f64` (908), `is_alive() -> bool` (898), `effects: Vec<Effect>`, `can_use_skill(&Skill) -> bool`, `apply_barrier(&mut self, f64)` (1039), `grant_elude_stacks(&mut self, u32)` (1045), `add_effect(&mut self, Effect) -> Vec<TriggeredEffect>` (1692).
- `Skill::effective_cooldown(cooldown_reduction: f64) -> f64` = `cooldown / (1 + cdr)`.
- `stat_core::effect_registry() -> &'static EffectRegistry`; `EffectRegistry::get(&self, id: &str) -> Option<&EffectConfig>` (effects.rs:398); `EffectConfig::to_effect(&self, source_id: &str) -> Effect` (effects.rs:233). (`EffectConfig` type path: confirm — likely `stat_core::config::EffectConfig` or `stat_core::config::effects::EffectConfig`; the implementer resolves the public path, see Task 3.)
- `combat::resolve_one_hit(caster: &mut StatBlock, target: &mut StatBlock, skill, registry, rng) -> Result<HitOutcome, SkillUseError>` already exists and is the deterministic funnel. `HitOutcome { total_damage, is_killing_blow, effects_applied, mana_spent }`.

## Current consumer surface (from `src/prelude.rs`)

Exports: `Attributes, Combatant, Faction, SkillSlots`; `ObeliskConfigExt, SkillRegistry, SkillSource, CombatRng`; `ObeliskId, ObeliskEntityIndex`; `CastTimeline, CastTimelineHandles`; `CastAim, CastSkillExt`; `ActiveCast, SkillPhase`; `Hitbox, Hurtbox, insert_hurtbox`; `events::*`; `ObeliskPlugins, ObeliskSimPlugin, ObeliskSet`.

Tests run with `cargo test --features test-support --lib --tests`. Baseline at the start of this batch: **31 passing**. The `ObeliskTestApp` harness (`obelisk_bevy::testkit`) drives the headless sim deterministically.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/core/cooldown.rs` | create | `Cooldowns` resource + methods; `tick_cooldowns` system. |
| `src/events.rs` | modify | `CooldownStarted`, `CooldownReady` events; `CastRejectReason::OnCooldown`. |
| `src/timeline/advance.rs` | modify | `validate_casts` gates on cooldown + starts it on a successful cast. |
| `src/core/mod.rs` | modify | register `Cooldowns` + `tick_cooldowns` in `ObeliskCorePlugin`. |
| `src/facade/read.rs` | create | `ObeliskRead` SystemParam (HUD/AI reads + `can_cast`). |
| `src/facade/spatial.rs` | create | `ObeliskSpatial` SystemParam (target acquisition). |
| `src/facade/combat.rs` | create | `ObeliskCombat` SystemParam (`resolve_skill_hit`, `resolve_aoe`). |
| `src/facade/mod.rs` | create | `pub mod read/spatial/combat;` + re-exports. |
| `src/verbs.rs` | create | `ObeliskCommandsExt` (make_combatant, apply_obelisk_effect, grant_skill, grant_barrier, grant_elude). |
| `src/lib.rs` | modify | `pub mod facade; pub mod verbs;`. |
| `src/prelude.rs` | modify | export facades, verbs, `Cooldowns`. |
| `tests/facades.rs` | create | Integration: AI acquires a target + resolves a hit; cooldown gating; barrier grant. |

---

## Task 1: `Cooldowns` resource + tick system + events

**Files:** Create `src/core/cooldown.rs`; Modify `src/events.rs`, `src/core/mod.rs`.

- [ ] **Step 1: Add events + reject reason in `src/events.rs`**

Add the `OnCooldown` variant to `CastRejectReason` (keep all existing variants) and two events:
```rust
#[derive(Event, Clone, Debug)]
pub struct CooldownStarted { pub caster: Entity, pub skill_id: String, pub duration: f32 }

#[derive(Event, Clone, Debug)]
pub struct CooldownReady { pub caster: Entity, pub skill_id: String }
```
Add `OnCooldown` to the enum:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastRejectReason {
    UnknownSkill,
    TimelineMissing,
    InsufficientMana,
    ConditionNotMet,
    OutOfRange,
    NoTarget,
    NoLineOfSight,
    OnCooldown,
}
```

- [ ] **Step 2: Write the failing test in `src/core/cooldown.rs`**

```rust
use bevy::prelude::*;
use std::collections::HashMap;

/// Per-(entity, skill) cooldown timers (seconds remaining). Empty/absent = ready.
#[derive(Resource, Default)]
pub struct Cooldowns {
    remaining: HashMap<(Entity, String), f32>,
}

impl Cooldowns {
    pub fn is_ready(&self, e: Entity, skill: &str) -> bool {
        self.remaining.get(&(e, skill.to_string())).map_or(true, |&r| r <= 0.0)
    }
    pub fn remaining(&self, e: Entity, skill: &str) -> f32 {
        self.remaining.get(&(e, skill.to_string())).copied().unwrap_or(0.0)
    }
    pub fn start(&mut self, e: Entity, skill: &str, duration: f32) {
        if duration > 0.0 {
            self.remaining.insert((e, skill.to_string()), duration);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_when_absent_then_busy_after_start_then_ready_after_zero() {
        let e = Entity::from_raw_u32(1).unwrap();
        let mut cd = Cooldowns::default();
        assert!(cd.is_ready(e, "firebolt"), "absent = ready");
        cd.start(e, "firebolt", 2.0);
        assert!(!cd.is_ready(e, "firebolt"), "busy after start");
        assert!((cd.remaining(e, "firebolt") - 2.0).abs() < 1e-6);
        cd.start(e, "other", 0.0); // zero duration is a no-op
        assert!(cd.is_ready(e, "other"));
    }
}
```

- [ ] **Step 3: Run — expect FAIL then implement the tick system + PASS**

Run: `cargo test --features test-support --lib cooldown` → FAIL only if the struct/methods are missing; if they compile, this test passes immediately. Then add the tick system to `src/core/cooldown.rs`:
```rust
use crate::events::CooldownReady;

/// Decrement cooldowns each fixed step; emit CooldownReady + remove when they hit zero.
pub fn tick_cooldowns(time: Res<Time<Fixed>>, mut cooldowns: ResMut<Cooldowns>, mut commands: Commands) {
    let dt = time.delta_secs();
    let mut ready: Vec<(Entity, String)> = Vec::new();
    for (key, rem) in cooldowns.remaining.iter_mut() {
        *rem -= dt;
        if *rem <= 0.0 {
            ready.push(key.clone());
        }
    }
    for key in ready {
        cooldowns.remaining.remove(&key);
        commands.trigger(CooldownReady { caster: key.0, skill_id: key.1 });
    }
}
```
(The `remaining` field is private; the tick system is in the same module so it can access it. `Entity::from_raw_u32(n).unwrap()` is the bevy 0.17.3 constructor for tests — `Entity::from_raw` was removed.)

Run: `cargo test --features test-support --lib cooldown` → PASS.

- [ ] **Step 4: Register in `ObeliskCorePlugin` (`src/core/mod.rs`)**

Add `pub mod cooldown;` to the module list at the top. In `ObeliskCorePlugin::build`, init the resource and schedule the tick (it belongs in `ObeliskSet::TickEffects` alongside the effect tick):
```rust
        app.init_resource::<crate::core::cooldown::Cooldowns>()
            .add_systems(
                FixedUpdate,
                crate::core::cooldown::tick_cooldowns.in_set(crate::ObeliskSet::TickEffects),
            );
```
(Add this to the existing `app...` builder chain in `build`.)

- [ ] **Step 5: Build + suite**

Run: `cargo test --features test-support --lib --tests` → all green (31 + the cooldown unit test).

- [ ] **Step 6: Commit**

```bash
git add src/core/cooldown.rs src/events.rs src/core/mod.rs
git commit -m "feat(cooldown): Cooldowns resource + tick + CooldownStarted/Ready events"
```

---

## Task 2: Cooldown gate + start in `validate_casts`

**Files:** Modify `src/timeline/advance.rs`.

- [ ] **Step 1: Add the cooldown gate + start to `validate_casts`**

Add imports + the `Cooldowns` param. Add to the imports:
```rust
use crate::core::cooldown::Cooldowns;
use crate::events::CooldownStarted;
```
Add a param to `validate_casts`:
```rust
    mut cooldowns: ResMut<Cooldowns>,
```
After the `can_use_skill` mana check and BEFORE the aim resolution, gate on cooldown:
```rust
        if !cooldowns.is_ready(caster, &req.skill_id) {
            commands.trigger(CastRejected {
                caster,
                skill_id: req.skill_id.clone(),
                reason: CastRejectReason::OnCooldown,
            });
            continue;
        }
```
After the `ActiveCast` is inserted and `CastBegan` triggered (the cast is committed), start the cooldown using the caster's CDR:
```rust
        let cd = skill.effective_cooldown(attrs.0.cooldown_reduction) as f32;
        if cd > 0.0 {
            cooldowns.start(caster, &req.skill_id, cd);
            commands.trigger(CooldownStarted { caster, skill_id: req.skill_id.clone(), duration: cd });
        }
```
(`attrs` is the `&Attributes` already fetched for the mana check; `attrs.0.cooldown_reduction` is a public f64 field. `skill` is the `&Skill` already in scope.)

- [ ] **Step 2: Build + suite**

Run: `cargo test --features test-support --lib --tests` → all green. The firebolt slice has `cooldown` unset (defaults to 0 → `effective_cooldown` returns 0 → no cooldown started), so casting twice still works; the slice stays green.

- [ ] **Step 3: Commit**

```bash
git add src/timeline/advance.rs
git commit -m "feat(cooldown): gate casts on cooldown + start it on a committed cast"
```

---

## Task 3: `ObeliskRead` facade (HUD/AI reads + can_cast)

**Files:** Create `src/facade/read.rs`, `src/facade/mod.rs`; Modify `src/lib.rs`.

- [ ] **Step 1: Write `src/facade/read.rs`**

```rust
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crate::core::components::Attributes;
use crate::core::config::SkillRegistry;
use crate::core::cooldown::Cooldowns;
use crate::events::CastRejectReason;

/// Read-only consumer facade for HUD / UI / AI. Holds no `&mut` — safe to use anywhere.
#[derive(SystemParam)]
pub struct ObeliskRead<'w, 's> {
    attrs: Query<'w, 's, &'static Attributes>,
    registry: Res<'w, SkillRegistry>,
    cooldowns: Res<'w, Cooldowns>,
}

impl ObeliskRead<'_, '_> {
    pub fn life_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.current_life)
    }
    pub fn max_life_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.computed_max_life())
    }
    pub fn mana_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.current_mana)
    }
    pub fn max_mana_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.computed_max_mana())
    }
    pub fn is_alive(&self, e: Entity) -> bool {
        self.attrs.get(e).map(|a| a.0.is_alive()).unwrap_or(false)
    }
    pub fn effect_count(&self, e: Entity) -> usize {
        self.attrs.get(e).map(|a| a.0.effects.len()).unwrap_or(0)
    }
    pub fn has_effect(&self, e: Entity, effect_id: &str) -> bool {
        self.attrs.get(e).map(|a| a.0.effects.iter().any(|ef| ef.id == effect_id)).unwrap_or(false)
    }
    pub fn cooldown_remaining(&self, e: Entity, skill_id: &str) -> f32 {
        self.cooldowns.remaining(e, skill_id)
    }
    /// Can `e` begin casting `skill_id` right now? Checks the skill exists, mana
    /// (obelisk `can_use_skill`), and cooldown. Range/LOS are validated at cast time
    /// (they need a target). Returns the reason it can't, for UI gray-out + tooltips.
    pub fn can_cast(&self, e: Entity, skill_id: &str) -> Result<(), CastRejectReason> {
        let Some(skill) = self.registry.0.get(skill_id) else {
            return Err(CastRejectReason::UnknownSkill);
        };
        let Ok(attrs) = self.attrs.get(e) else {
            return Err(CastRejectReason::NoTarget);
        };
        if !attrs.0.can_use_skill(skill) {
            return Err(CastRejectReason::InsufficientMana);
        }
        if !self.cooldowns.is_ready(e, skill_id) {
            return Err(CastRejectReason::OnCooldown);
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Write `src/facade/mod.rs`** (other facades added in later tasks)

```rust
pub mod read;
pub use read::ObeliskRead;
```

- [ ] **Step 3: Wire `pub mod facade;` in `src/lib.rs`, write a test in `src/facade/read.rs`**

Add the test module (uses the testkit harness to spawn a combatant and read it):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use bevy::ecs::system::RunSystemOnce;
    use stat_core::StatBlock;

    #[test]
    fn reads_life_and_can_cast() {
        let mut t = ObeliskTestApp::new(1);
        let mut block = StatBlock::with_id("hero");
        block.max_life.base = 80.0;
        block.current_life = 80.0;
        block.max_mana.base = 50.0;
        block.current_mana = 50.0;
        let hero = t.app.world_mut().spawn((
            Combatant, Attributes(block), Faction::Player, ObeliskId("hero".into()), Transform::default(),
        )).id();
        t.app.update();

        let life = t.app.world_mut().run_system_once(move |r: ObeliskRead| r.life_of(hero)).unwrap();
        assert_eq!(life, Some(80.0));
        // "firebolt" is loaded by the harness from tests/fixtures/skills.
        let can = t.app.world_mut().run_system_once(move |r: ObeliskRead| r.can_cast(hero, "firebolt")).unwrap();
        assert!(can.is_ok(), "hero with mana + no cooldown can cast firebolt: {can:?}");
        let bad = t.app.world_mut().run_system_once(move |r: ObeliskRead| r.can_cast(hero, "nope")).unwrap();
        assert_eq!(bad, Err(CastRejectReason::UnknownSkill));
    }
}
```
(`RunSystemOnce::run_system_once` returns the system's output; if its signature differs in 0.17.3, write the result into a `Resource` and read it back instead — report the working form.)

- [ ] **Step 4: Run + commit**

Run: `cargo test --features test-support --lib reads_life_and_can_cast` → PASS. Then full suite.
```bash
git add src/facade src/lib.rs
git commit -m "feat(facade): ObeliskRead (HUD/AI reads + can_cast predicate)"
```

---

## Task 4: EntityCommands verbs — make_combatant / grant_skill / grant_barrier / grant_elude

**Files:** Create `src/verbs.rs`; Modify `src/lib.rs`.

- [ ] **Step 1: Write `src/verbs.rs`**

```rust
use bevy::prelude::*;
use crate::core::components::{Attributes, Combatant, SkillSlots};
use crate::ids::ObeliskId;
use stat_core::StatBlock;

/// Verb-style EntityCommands extensions for spawning + granting.
pub trait ObeliskCommandsExt {
    /// Turn this entity into a combatant with a REAL StatBlock (sets Attributes + ObeliskId from `block.id`).
    fn make_combatant(&mut self, block: StatBlock) -> &mut Self;
    /// Add a skill id to this entity's SkillSlots.
    fn grant_skill(&mut self, skill_id: impl Into<String>) -> &mut Self;
    /// Grant barrier (energy shield) to this entity's StatBlock.
    fn grant_barrier(&mut self, amount: f64) -> &mut Self;
    /// Grant elude stacks to this entity's StatBlock.
    fn grant_elude(&mut self, stacks: u32) -> &mut Self;
}

impl ObeliskCommandsExt for EntityCommands<'_> {
    fn make_combatant(&mut self, block: StatBlock) -> &mut Self {
        let id = block.id.clone();
        self.insert((Combatant, Attributes(block), ObeliskId(id)));
        self
    }
    fn grant_skill(&mut self, skill_id: impl Into<String>) -> &mut Self {
        let skill_id = skill_id.into();
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut slots) = entity.get_mut::<SkillSlots>() {
                slots.0.push(skill_id);
            } else {
                entity.insert(SkillSlots(vec![skill_id]));
            }
        });
        self
    }
    fn grant_barrier(&mut self, amount: f64) -> &mut Self {
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut attrs) = entity.get_mut::<Attributes>() {
                attrs.0.apply_barrier(amount);
            }
        });
        self
    }
    fn grant_elude(&mut self, stacks: u32) -> &mut Self {
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut attrs) = entity.get_mut::<Attributes>() {
                attrs.0.grant_elude_stacks(stacks);
            }
        });
        self
    }
}
```

- [ ] **Step 2: Wire `pub mod verbs;` in `src/lib.rs`, write a test in `src/verbs.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::ObeliskTestApp;

    #[test]
    fn make_combatant_and_grants_apply() {
        let mut t = ObeliskTestApp::new(1);
        let mut block = StatBlock::with_id("orc");
        block.max_life.base = 60.0;
        block.current_life = 60.0;
        let e = t.app.world_mut().spawn_empty().id();
        t.app.world_mut().commands().entity(e).make_combatant(block).grant_skill("firebolt").grant_barrier(25.0);
        t.app.update();

        let attrs = t.app.world().entity(e).get::<Attributes>().expect("Attributes inserted");
        assert_eq!(attrs.0.id, "orc");
        assert!(attrs.0.current_barrier >= 25.0, "barrier granted (got {})", attrs.0.current_barrier);
        let slots = t.app.world().entity(e).get::<SkillSlots>().expect("SkillSlots");
        assert!(slots.0.contains(&"firebolt".to_string()));
    }
}
```
(`StatBlock.current_barrier` is a public f64 field. If `apply_barrier` clamps to a max barrier of 0, set `block.set_max_barrier(...)` first — verify what `apply_barrier` does; if it needs a max, the test should set `block.max_barrier` or call `set_max_barrier`. Report what was needed.)

- [ ] **Step 3: Run + commit**

Run: `cargo test --features test-support --lib make_combatant_and_grants_apply` → PASS. Then full suite.
- **If `self.queue(|mut entity: EntityWorldMut| ...)` doesn't match the bevy 0.17.3 `EntityCommand` API**, find the form that compiles (it may be `FnOnce(EntityWorldMut)` or `FnOnce(Entity, &mut World)`; `EntityWorldMut::get_mut::<T>()` and `.insert(...)` are the accessors). Report the working closure form — Task 5 reuses it.
```bash
git add src/verbs.rs src/lib.rs
git commit -m "feat(verbs): make_combatant + grant_skill/barrier/elude"
```

---

## Task 5: `apply_obelisk_effect` verb

**Files:** Modify `src/verbs.rs`.

- [ ] **Step 1: Add `apply_obelisk_effect` to the trait + impl in `src/verbs.rs`**

Add to the `ObeliskCommandsExt` trait:
```rust
    /// Apply an obelisk effect (by config id) to this entity, sourced from itself.
    /// Looks up the global EffectRegistry, builds the Effect, and adds it; emits EffectApplied.
    fn apply_obelisk_effect(&mut self, effect_id: impl Into<String>) -> &mut Self;
```
Add to the impl (uses the `World` form so it can read the global registry + trigger an event):
```rust
    fn apply_obelisk_effect(&mut self, effect_id: impl Into<String>) -> &mut Self {
        let effect_id = effect_id.into();
        self.queue(move |mut entity: EntityWorldMut| {
            // Build the effect from the global registry, sourced from this entity's id.
            let source_id = entity.get::<Attributes>().map(|a| a.0.id.clone()).unwrap_or_default();
            if !stat_core::config::effect_registry_initialized() {
                return;
            }
            let Some(config) = stat_core::effect_registry().get(&effect_id) else { return };
            let effect = config.to_effect(&source_id);
            let target = entity.id();
            let (total_duration, stacks, triggered) = {
                let Some(mut attrs) = entity.get_mut::<Attributes>() else { return };
                let triggered = attrs.0.add_effect(effect);
                // re-read the applied effect's display fields for the event
                let applied = attrs.0.effects.iter().find(|e| e.id == effect_id);
                (
                    applied.map(|e| e.total_duration).unwrap_or(0.0),
                    applied.map(|e| e.stacks).unwrap_or(1),
                    triggered,
                )
            };
            // OnApply / OnMaxStacks trigger CASCADE (firing the triggered skills) is a later
            // batch; for now we surface that they occurred via a debug log so they aren't lost.
            if !triggered.is_empty() {
                bevy::log::debug!("apply_obelisk_effect: {} triggered effects (cascade deferred)", triggered.len());
            }
            entity.world_scope(|world| {
                world.trigger(crate::events::EffectApplied {
                    target,
                    effect_id: effect_id.clone(),
                    total_duration,
                    stacks,
                });
            });
        });
        self
    }
```

- [ ] **Step 2: Write a test in `src/verbs.rs`** (add to the existing tests module)

```rust
    #[test]
    fn apply_obelisk_effect_adds_a_status() {
        let mut t = ObeliskTestApp::new(1); // harness loads the burn effect registry from fixtures
        let mut block = StatBlock::with_id("victim");
        block.max_life.base = 100.0;
        block.current_life = 100.0;
        let e = t.app.world_mut().spawn((
            crate::prelude::Combatant,
            Attributes(block),
            crate::prelude::Faction::Enemy,
            ObeliskId("victim".into()),
            Transform::default(),
        )).id();
        t.app.update();
        t.app.world_mut().commands().entity(e).apply_obelisk_effect("burn");
        t.app.update();
        let attrs = t.app.world().entity(e).get::<Attributes>().unwrap();
        assert!(attrs.0.effects.iter().any(|ef| ef.id == "burn"), "burn effect should be applied");
    }
```

- [ ] **Step 3: Run + commit**

Run: `cargo test --features test-support --lib apply_obelisk_effect_adds_a_status` → PASS. Then full suite.
- **If `EntityWorldMut::world_scope` / `world.trigger` differ in 0.17.3**, find the working way to fire an observer event from within an `EntityCommand` (you may need to capture the `Entity` and use `FnOnce(Entity, &mut World)` form instead so you directly have `&mut World` for both `get_mut` and `trigger`). Report the working form.
```bash
git add src/verbs.rs
git commit -m "feat(verbs): apply_obelisk_effect (registry -> add_effect -> EffectApplied)"
```

---

## Task 6: `ObeliskSpatial` facade (target acquisition)

**Files:** Create `src/facade/spatial.rs`; Modify `src/facade/mod.rs`.

- [ ] **Step 1: Write `src/facade/spatial.rs`**

```rust
use avian3d::prelude::{Collider, SpatialQuery, SpatialQueryFilter};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*; // Dir3, Vec3, Quat, Entity, Query come from here
use crate::core::components::Faction;
use crate::spatial::boxes::Hurtbox;
use crate::spatial::cone::point_in_cone;

/// Target-acquisition facade. Wraps Avian's SpatialQuery + hurtbox/faction lookups.
#[derive(SystemParam)]
pub struct ObeliskSpatial<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
    hurtboxes: Query<'w, 's, (&'static Hurtbox, &'static Transform)>,
    factions: Query<'w, 's, &'static Faction>,
}

impl ObeliskSpatial<'_, '_> {
    /// All hurtbox owners within `range` of `origin` whose faction differs from `caster_faction`.
    pub fn enemies_in_range(&self, origin: Vec3, range: f32, caster_faction: Faction) -> Vec<Entity> {
        let shape = Collider::sphere(range);
        self.spatial
            .shape_intersections(&shape, origin, Quat::IDENTITY, &SpatialQueryFilter::default())
            .into_iter()
            .filter_map(|hit_e| self.hurtboxes.get(hit_e).ok().map(|(h, _)| h.owner))
            .filter(|&owner| self.factions.get(owner).copied().unwrap_or(Faction::Neutral) != caster_faction)
            .collect()
    }

    /// The single nearest enemy within `range`, or None.
    pub fn nearest_enemy(&self, origin: Vec3, range: f32, caster_faction: Faction) -> Option<Entity> {
        self.enemies_in_range(origin, range, caster_faction)
            .into_iter()
            .filter_map(|e| self.hurtboxes.get(e).ok().map(|(_, tf)| (e, tf.translation.distance_squared(origin))))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(e, _)| e)
    }

    /// Enemies within a cone (apex `origin`, axis `dir`, full `angle` degrees, slant `range`).
    pub fn cone_targets(&self, origin: Vec3, dir: Vec3, angle_deg: f32, range: f32, caster_faction: Faction) -> Vec<Entity> {
        let half = angle_deg.to_radians() * 0.5;
        self.enemies_in_range(origin, range, caster_faction)
            .into_iter()
            .filter(|&e| {
                self.hurtboxes
                    .get(e)
                    .map(|(_, tf)| point_in_cone(origin, dir, half, range, tf.translation))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// First hurtbox owner struck by a ray from `origin` along `dir` within `range`.
    pub fn raycast_target(&self, origin: Vec3, dir: Dir3, range: f32) -> Option<Entity> {
        let hit = self.spatial.cast_ray(origin, dir, range, true, &SpatialQueryFilter::default())?;
        self.hurtboxes.get(hit.entity).ok().map(|(h, _)| h.owner)
    }

    /// Whether the straight segment a->b is unobstructed (no collider strictly between).
    pub fn los_clear(&self, a: Vec3, b: Vec3) -> bool {
        let delta = b - a;
        let dist = delta.length();
        if dist <= f32::EPSILON {
            return true;
        }
        let dir = Dir3::new(delta).unwrap_or(Dir3::Z);
        // A hit shorter than the full distance (minus a small skin) = blocked.
        match self.spatial.cast_ray(a, dir, dist, true, &SpatialQueryFilter::default()) {
            Some(hit) => hit.distance >= dist - 0.01,
            None => true,
        }
    }
}
```

- [ ] **Step 2: Add `pub mod spatial; pub use spatial::ObeliskSpatial;` to `src/facade/mod.rs`.**

- [ ] **Step 3: Write a test in `src/facade/spatial.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use bevy::ecs::system::RunSystemOnce;
    use stat_core::StatBlock;

    fn spawn(t: &mut ObeliskTestApp, id: &str, faction: Faction, pos: Vec3) -> Entity {
        let mut b = StatBlock::with_id(id);
        b.max_life.base = 50.0; b.current_life = 50.0;
        let e = t.app.world_mut().spawn((Combatant, Attributes(b), faction, ObeliskId(id.into()), Transform::from_translation(pos))).id();
        let mut c = t.app.world_mut().commands();
        insert_hurtbox(&mut c, e, 0.5, pos);
        e
    }

    #[test]
    fn nearest_enemy_picks_the_closest_other_faction() {
        let mut t = ObeliskTestApp::new(1);
        let _player = spawn(&mut t, "player", Faction::Player, Vec3::ZERO);
        let near = spawn(&mut t, "near", Faction::Enemy, Vec3::new(0.0, 0.0, 2.0));
        let _far = spawn(&mut t, "far", Faction::Enemy, Vec3::new(0.0, 0.0, 8.0));
        let _ally = spawn(&mut t, "ally", Faction::Player, Vec3::new(0.0, 0.0, 1.0));
        // physics needs a couple of ticks to register the static colliders
        t.app.update();
        t.app.update();
        t.app.update();

        let got = t.app.world_mut().run_system_once(move |s: ObeliskSpatial| {
            s.nearest_enemy(Vec3::ZERO, 10.0, Faction::Player)
        }).unwrap();
        assert_eq!(got, Some(near), "nearest enemy is `near`, not the ally or the far enemy");
    }
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test --features test-support --lib nearest_enemy_picks_the_closest` → PASS. (Static hurtboxes are visible from the 2nd tick — the test updates 3×.) **Confirm `cast_ray`'s `RayHitData.distance` field name** (used in `los_clear`); adjust if different. Then full suite.
```bash
git add src/facade src/lib.rs
git commit -m "feat(facade): ObeliskSpatial target acquisition (nearest/cone/ray/los)"
```

---

## Task 7: `ObeliskCombat` facade (programmatic resolution)

**Files:** Create `src/facade/combat.rs`; Modify `src/facade/mod.rs`.

- [ ] **Step 1: Write `src/facade/combat.rs`**

```rust
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crate::combat::resolve::resolve_one_hit;
use crate::core::components::Attributes;
use crate::core::config::{CombatRng, SkillRegistry};
use crate::events::{DamageResolved, EffectApplied, EntityDied};

/// Authoritative programmatic combat entry. Lets a consumer resolve a skill hit WITHOUT the
/// spatial pipeline (scripted damage, AI that picked a target via ObeliskSpatial, etc.).
/// Routes through the deterministic `resolve_one_hit` (never `thread_rng`) and emits events.
#[derive(SystemParam)]
pub struct ObeliskCombat<'w, 's> {
    attrs: Query<'w, 's, &'static mut Attributes>,
    registry: Res<'w, SkillRegistry>,
    rng: ResMut<'w, CombatRng>,
    commands: Commands<'w, 's>,
}

impl ObeliskCombat<'_, '_> {
    /// Resolve one hit of `skill_id` from `caster` onto `target`. Returns the total damage dealt,
    /// or None if the skill/entities are missing or the same entity. Emits DamageResolved /
    /// EffectApplied / EntityDied.
    pub fn resolve_skill_hit(&mut self, caster: Entity, target: Entity, skill_id: &str) -> Option<f64> {
        let skill = self.registry.0.get(skill_id)?.clone();
        let [mut caster_a, mut target_a] = self.attrs.get_many_mut([caster, target]).ok()?;
        let outcome = resolve_one_hit(&mut caster_a.0, &mut target_a.0, &skill, &self.registry.0, &mut self.rng.0).ok()?;
        let life_after = target_a.0.current_life;
        let alive = target_a.0.is_alive();
        self.commands.trigger(DamageResolved {
            caster, target, skill_id: skill_id.to_string(),
            total_damage: outcome.total_damage, is_killing_blow: outcome.is_killing_blow,
            life_after, mana_spent: outcome.mana_spent,
        });
        for ef in &outcome.effects_applied {
            self.commands.trigger(EffectApplied { target, effect_id: ef.id.clone(), total_duration: ef.total_duration, stacks: ef.stacks });
        }
        if outcome.is_killing_blow || !alive {
            self.commands.trigger(EntityDied { target, killer: Some(caster) });
        }
        Some(outcome.total_damage)
    }

    /// Fan one cast over many targets. Targets are sorted by a STABLE key (the StatBlock id)
    /// before drawing from the seeded RNG, so HashMap/iteration order can't perturb determinism.
    pub fn resolve_aoe(&mut self, caster: Entity, targets: &[Entity], skill_id: &str) -> usize {
        let mut ordered: Vec<Entity> = targets.to_vec();
        ordered.sort_by_key(|&e| self.attrs.get(e).map(|a| a.0.id.clone()).unwrap_or_default());
        let mut hits = 0;
        for target in ordered {
            if target == caster {
                continue;
            }
            if self.resolve_skill_hit(caster, target, skill_id).is_some() {
                hits += 1;
            }
        }
        hits
    }
}
```

- [ ] **Step 2: Add `pub mod combat; pub use combat::ObeliskCombat;` to `src/facade/mod.rs`.**

- [ ] **Step 3: Write a test in `src/facade/combat.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use bevy::ecs::system::RunSystemOnce;
    use stat_core::StatBlock;

    fn spawn(t: &mut ObeliskTestApp, id: &str, faction: Faction, life: f64) -> Entity {
        let mut b = StatBlock::with_id(id);
        b.max_life.base = life; b.current_life = life; b.max_mana.base = 100.0; b.current_mana = 100.0;
        t.app.world_mut().spawn((Combatant, Attributes(b), faction, ObeliskId(id.into()), Transform::default())).id()
    }

    #[test]
    fn resolve_skill_hit_deals_damage_programmatically() {
        let mut t = ObeliskTestApp::new(5);
        let caster = spawn(&mut t, "caster", Faction::Player, 100.0);
        let target = spawn(&mut t, "target", Faction::Enemy, 100.0);
        t.app.update();
        let dmg = t.app.world_mut().run_system_once(move |mut c: ObeliskCombat| {
            c.resolve_skill_hit(caster, target, "firebolt")
        }).unwrap();
        assert!(dmg.unwrap_or(0.0) > 0.0, "programmatic firebolt should deal damage");
        let remaining = t.app.world().entity(target).get::<Attributes>().unwrap().0.current_life;
        assert!(remaining < 100.0, "target took damage (life {remaining})");
    }
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test --features test-support --lib resolve_skill_hit_deals_damage` → PASS. Then full suite.
```bash
git add src/facade src/lib.rs
git commit -m "feat(facade): ObeliskCombat programmatic resolve_skill_hit + resolve_aoe"
```

---

## Task 8: Integration + prelude exports + quality gates

**Files:** Create `tests/facades.rs`; Modify `src/prelude.rs`.

- [ ] **Step 1: Export the new surface in `src/prelude.rs`**

Add:
```rust
pub use crate::core::cooldown::Cooldowns;
pub use crate::facade::{ObeliskCombat, ObeliskRead, ObeliskSpatial};
pub use crate::verbs::ObeliskCommandsExt;
```

- [ ] **Step 2: Write the integration test `tests/facades.rs`**

```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use bevy::ecs::system::RunSystemOnce;
use stat_core::StatBlock;

fn spawn(t: &mut ObeliskTestApp, id: &str, faction: Faction, pos: Vec3, life: f64) -> Entity {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life; b.current_life = life; b.max_mana.base = 100.0; b.current_mana = 100.0;
    let e = t.app.world_mut().spawn((Combatant, Attributes(b), faction, ObeliskId(id.into()), Transform::from_translation(pos))).id();
    let mut c = t.app.world_mut().commands();
    insert_hurtbox(&mut c, e, 0.5, pos);
    e
}

/// An "AI turn": acquire the nearest enemy with ObeliskSpatial, then resolve a hit with ObeliskCombat.
#[test]
fn ai_acquires_target_and_resolves_a_hit() {
    let mut t = ObeliskTestApp::new(9);
    let ai = spawn(&mut t, "ai", Faction::Enemy, Vec3::ZERO, 100.0);
    let hero = spawn(&mut t, "hero", Faction::Player, Vec3::new(0.0, 0.0, 3.0), 100.0);
    t.app.update();
    t.app.update();
    t.app.update();

    let acquired = t.app.world_mut().run_system_once(move |s: ObeliskSpatial| {
        s.nearest_enemy(Vec3::ZERO, 10.0, Faction::Enemy)
    }).unwrap();
    assert_eq!(acquired, Some(hero), "AI (Enemy) acquires the hero (Player) as nearest enemy");

    let dmg = t.app.world_mut().run_system_once(move |mut c: ObeliskCombat| {
        c.resolve_skill_hit(ai, hero, "firebolt")
    }).unwrap();
    assert!(dmg.unwrap_or(0.0) > 0.0);
    assert!(t.app.world().entity(hero).get::<Attributes>().unwrap().0.current_life < 100.0);
}

/// Cooldown gating: a skill with a cooldown can't be re-cast until it elapses.
#[test]
fn can_cast_reflects_cooldown() {
    let mut t = ObeliskTestApp::new(9);
    let hero = spawn(&mut t, "hero", Faction::Player, Vec3::ZERO, 100.0);
    t.app.update();
    // Manually start a cooldown on firebolt, then can_cast must report OnCooldown.
    t.app.world_mut().resource_mut::<Cooldowns>().start(hero, "firebolt", 5.0);
    let res = t.app.world_mut().run_system_once(move |r: ObeliskRead| r.can_cast(hero, "firebolt")).unwrap();
    assert_eq!(res, Err(CastRejectReason::OnCooldown));
}
```

- [ ] **Step 3: Run the integration + full suite + quality gates**

Run: `cargo test --features test-support --test facades -- --nocapture` → 2 passed.
Run: `cargo test --features test-support --lib --tests` → all green (the slice, spatial-targeting, and all new facade/verb/cooldown tests).
Run: `cargo clippy --features test-support --lib --tests -- -D warnings` → clean for `obelisk-bevy`.
Run: `cargo fmt` then `cargo fmt --check` → clean.

**Debug guidance:** if `ai_acquires_target_and_resolves_a_hit` finds no target, the hurtboxes need ≥2 ticks to register (the test updates 3×) — confirm. If `can_cast_reflects_cooldown` fails, confirm `Cooldowns::start` + `ObeliskRead::can_cast`'s cooldown branch use the same `(Entity, skill_id)` key.

- [ ] **Step 4: Commit**

```bash
git add tests/facades.rs src/prelude.rs
git commit -m "test(facade): AI acquire+resolve + cooldown gating integration; export consumer surface"
```

---

## Self-review notes (coverage vs spec §4)

- `Cooldowns` resource + `effective_cooldown` + `CooldownStarted/Ready`: Tasks 1,2 ✅
- `ObeliskRead` (life/mana/stat reads, `can_cast`, effect presence): Task 3 ✅
- EntityCommands verbs (`make_combatant`, `grant_skill`, `grant_barrier`, `grant_elude`): Task 4 ✅
- `apply_obelisk_effect` (registry → `to_effect` → `add_effect`, captures triggers): Task 5 ✅ (trigger-cascade *firing* deferred — logged, not dropped silently)
- `ObeliskSpatial` (`nearest_enemy`/`enemies_in_range`/`cone_targets`/`raycast_target`/`los_clear`): Task 6 ✅
- `ObeliskCombat` (`resolve_skill_hit`, `resolve_aoe` with stable ordering): Task 7 ✅
- Prelude exports + integration: Task 8 ✅
- Backward compat (firebolt slice + spatial-targeting green): asserted each task ✅

## Deferred (out of this batch)

Trigger-cascade *firing* of `apply_obelisk_effect`'s returned `TriggeredEffect`s (needs the TriggerFired event + cascade infra from the netcode/trigger batch); `ObeliskRead::computed_stat` for arbitrary `StatType` (needs a stat-accessor mapping); cooldown UI events wired to a default HUD (presentation batch); LOS collision-layer filtering (uses simple ray-distance here).
