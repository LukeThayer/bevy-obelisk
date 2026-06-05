# Spatial & Targeting Completeness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the vertical slice's hard-coded spatial simplifications with real, asset-driven behavior: per-hitbox shapes, true cone/sector geometry, faction-aware `HitFilter`, all `HitMode`s + re-hit intervals, aimed targeting (entity/point/direction), and range + line-of-sight validation.

**Architecture:** Extend the existing `obelisk-bevy` spatial + timeline modules. Detection logic is factored into **pure, unit-testable helper functions** (`passes_filter`, `point_in_cone`, `Hitbox::can_hit`) that the `detect_overlaps` system composes. Casting gains an aim model (`CastAim`) resolved to a facing direction at validation time; the hitbox stores its own shape + aim so detection honors the authored asset. All new asset fields use `#[serde(default)]` so existing `.cast.ron` files keep parsing. The firebolt slice scenario stays green throughout (its resolved aim is `+Z`, matching the old hard-coded direction).

**Tech Stack:** Rust 2021, Bevy 0.17.3, Avian3d 0.4.1 (`SpatialQuery::shape_intersections` + `cast_ray`), the existing obelisk path-deps.

---

## Current state (ground truth — verified by reading the code)

- `src/spatial/boxes.rs`: `Hitbox { caster, skill_id, window_id, filter: HitFilter, mode: HitMode, remaining: f32, already_hit: Vec<Entity> }`; `Hurtbox { owner }`; `insert_hurtbox(commands, owner, radius, pos)`.
- `src/spatial/detect.rs`: `detect_overlaps` hard-codes `Collider::sphere(0.5)` and an Enemies-only filter, dedupes via `already_hit`.
- `src/spatial/shapes.rs`: `to_collider(&CollisionShape) -> Collider` (Sphere/Capsule real; Cone→sphere(range) approximation).
- `src/timeline/cast.rs`: `PendingCast { skill_id, target: Entity }`; `CastSkillExt::cast_skill_at(skill, Entity)`.
- `src/timeline/state.rs`: `ActiveCast { skill_id, target: Entity, phase, elapsed, windup, active, recovery, fired_windows }`.
- `src/timeline/advance.rs`: `validate_casts` (range/LOS NOT checked); `advance_casts` spawns a `Hitbox` at the caster with hard-coded `dir = Vec3::Z`; `expire_hitboxes` decrements `remaining`.
- `src/assets/mod.rs`: `CollisionShape { Sphere{radius}, Capsule{radius,height}, Cone{angle,range} }` (`Clone`, not `Copy`); `CollisionWindow { id, spawn_phase, spawn_offset, active_duration, shape, motion, hit_filter, hit_mode }`; `HitFilter { Caster, Allies, Enemies, All }` (Copy); `HitMode { OncePerTarget, FirstOnly, EveryTick }` (Copy); `CastTargeting { SelfCast, SingleEntity{range}, Direction{range}, Cone{angle,range} }`; `CastDelivery { Melee, Instant, Projectile{speed} }`.
- `src/events.rs`: `CastRejectReason { UnknownSkill, TimelineMissing, InsufficientMana, ConditionNotMet, OutOfRange, NoTarget }`.
- Tests run with `cargo test --features test-support`. Current count: **12 passing** (10 lib + 2 integration). Use `--lib --tests` to skip the example (disk).
- Headless test harness: `obelisk_bevy::testkit::ObeliskTestApp::new(seed)` + `advance_ticks(n)` + `rec()` (EventRecorder). It loads skills from `tests/fixtures/skills` and the slice asset from `assets/`.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/assets/mod.rs` | modify | `CollisionShape` gains `Copy`; `CollisionWindow` gains `rehit_interval: Option<f32>` (`#[serde(default)]`). |
| `src/spatial/boxes.rs` | modify | `Hitbox` carries `shape`, `aim`, `age`, `rehit_interval`, `hit_log`, `done` (replaces `already_hit`); add `Hitbox::can_hit`/`register_hit`. |
| `src/spatial/filter.rs` | create | Pure `passes_filter(...)` helper + unit tests. |
| `src/spatial/cone.rs` | create | Pure `point_in_cone(...)` helper + unit tests. |
| `src/spatial/detect.rs` | modify | Use the hitbox's own shape; cone broadphase+angle; real `HitFilter`; `HitMode`/re-hit via `can_hit`. |
| `src/spatial/mod.rs` | modify | `pub mod filter; pub mod cone;`. |
| `src/timeline/cast.rs` | modify | `CastAim` enum; `PendingCast { skill_id, aim }`; verbs `cast_skill_at` / `cast_skill_at_point` / `cast_skill_dir`. |
| `src/timeline/state.rs` | modify | `ActiveCast` gains `aim_dir: Vec3` + `target: Option<Entity>` (replaces `target: Entity`). |
| `src/timeline/advance.rs` | modify | `validate_casts` resolves aim → `aim_dir`, checks range + LOS; `advance_casts` orients the hitbox/projectile along `aim_dir`; `expire_hitboxes` increments `age`. |
| `src/events.rs` | modify | `CastRejectReason` gains `NoLineOfSight`. |
| `src/prelude.rs` | modify | re-export the new `CastSkillExt` verbs (already via trait) — confirm `CastAim` is exported. |
| `tests/fixtures/skills/cleave.toml` | create | A melee skill for the integration test. |
| `assets/skills/cleave.cast.ron` | create | A cone melee timeline. |
| `tests/spatial_targeting.rs` | create | Integration tests: cone multi-hit + faction filter + aimed cast. |

---

## Task 1: Hitbox stores its own shape + aim (replaces hard-coded sphere)

**Files:** Modify `src/assets/mod.rs`, `src/spatial/boxes.rs`, `src/spatial/detect.rs`, `src/timeline/advance.rs`.

- [ ] **Step 1: Make `CollisionShape` `Copy`** in `src/assets/mod.rs`

Change its derive from `#[derive(Debug, Clone, Deserialize)]` to:
```rust
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum CollisionShape {
    Sphere { radius: f32 },
    Capsule { radius: f32, height: f32 },
    Cone { angle: f32, range: f32 },
}
```

- [ ] **Step 2: Rewrite the `Hitbox` struct** in `src/spatial/boxes.rs`

Replace the existing `Hitbox` struct (keep `Hurtbox` and `insert_hurtbox` unchanged) with:
```rust
use crate::assets::{CollisionShape, HitFilter, HitMode};
use std::collections::HashMap;

/// Offensive volume spawned during an active collision window.
#[derive(Component, Debug)]
pub struct Hitbox {
    pub caster: Entity,
    pub skill_id: String,
    pub window_id: String,
    pub filter: HitFilter,
    pub mode: HitMode,
    /// The authored shape (drives the SpatialQuery + cone test).
    pub shape: CollisionShape,
    /// Normalized facing direction (cone axis / projectile heading).
    pub aim: Vec3,
    /// Seconds this hitbox has existed (for re-hit interval timing).
    pub age: f32,
    /// If set, a target may be hit again this many seconds after its last hit.
    pub rehit_interval: Option<f32>,
    /// Seconds remaining before the window expires.
    pub remaining: f32,
    /// target -> `age` at which it was last hit.
    pub hit_log: HashMap<Entity, f32>,
    /// FirstOnly: set true after the single hit so the box stops hitting.
    pub done: bool,
}

impl Hitbox {
    /// Whether `target` may be hit right now given mode + re-hit interval.
    pub fn can_hit(&self, target: Entity) -> bool {
        if self.done {
            return false;
        }
        match self.mode {
            HitMode::EveryTick => true,
            HitMode::FirstOnly | HitMode::OncePerTarget => match self.hit_log.get(&target) {
                None => true,
                Some(&last) => self.rehit_interval.is_some_and(|i| self.age - last >= i),
            },
        }
    }

    /// Record a hit on `target` and apply FirstOnly stop semantics.
    pub fn register_hit(&mut self, target: Entity) {
        self.hit_log.insert(target, self.age);
        if matches!(self.mode, HitMode::FirstOnly) {
            self.done = true;
        }
    }
}
```

- [ ] **Step 3: Update `advance_casts` to populate the new fields** in `src/timeline/advance.rs`

In the window-spawn block, replace the `Hitbox { ... }` construction (currently using `already_hit: Vec::new()`) with the new fields. Keep `dir = Vec3::Z` for now (Task 5 replaces it with the resolved aim). Add `use crate::assets::HitMode;` is not needed; the window already has `hit_filter`/`hit_mode`. Replace the spawn:
```rust
                cast.fired_windows.push(win.id.clone());
                let dir = Vec3::Z; // replaced by resolved aim in Task 5
                let mut ent = commands.spawn((
                    Hitbox {
                        caster,
                        skill_id: cast.skill_id.clone(),
                        window_id: win.id.clone(),
                        filter: win.hit_filter,
                        mode: win.hit_mode,
                        shape: win.shape,
                        aim: dir,
                        age: 0.0,
                        rehit_interval: win.rehit_interval,
                        remaining: win.active_duration,
                        hit_log: std::collections::HashMap::new(),
                        done: false,
                    },
                    Transform::from_translation(caster_tf.translation),
                ));
```
(The rest of the block — the `Projectile` insert and `HitWindowOpened` trigger — stays the same.)

- [ ] **Step 4: Add `rehit_interval` to `CollisionWindow`** in `src/assets/mod.rs`

Add the field (with a serde default so existing `.cast.ron` files keep parsing):
```rust
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
    #[serde(default)]
    pub rehit_interval: Option<f32>,
}
```

- [ ] **Step 5: Use the hitbox's own shape in `detect_overlaps`** in `src/spatial/detect.rs`

Replace the hard-coded `let collider = Collider::sphere(0.5);` with the converted authored shape, and switch the dedupe from `already_hit` to `can_hit`/`register_hit`. Replace the body of the `for (mut hitbox, hb_tf) in &mut hitboxes` loop's collider construction and the hit-bookkeeping (leave the faction check as the existing Enemies hard-code for now — Task 3 replaces it). Concretely, change these lines:
```rust
        let collider = crate::spatial::shapes::to_collider(&hitbox.shape);
        let hits = spatial.shape_intersections(
            &collider,
            hb_tf.translation,
            hb_tf.rotation,
            &SpatialQueryFilter::default(),
        );
```
and replace the `if hitbox.already_hit.contains(&target) { continue; }` + `hitbox.already_hit.push(target);` with:
```rust
            if !hitbox.can_hit(target) {
                continue;
            }
```
(keep the existing faction `is_enemy` check for now) and, right before `commands.trigger(HitConfirmed { ... })`, add:
```rust
            hitbox.register_hit(target);
```

- [ ] **Step 6: Build + run the suite**

Run: `cargo test --features test-support --lib --tests`
Expected: PASS, still **12 passed**. (Firebolt uses `Sphere{0.5}` + `FirstOnly`; `to_collider` yields `sphere(0.5)` and `can_hit`/`register_hit` reproduce the one-hit behavior — identical to before.)

- [ ] **Step 7: Commit**

```bash
git add src/assets/mod.rs src/spatial/boxes.rs src/spatial/detect.rs src/timeline/advance.rs
git commit -m "feat(spatial): hitbox stores its own shape + re-hit state"
```

---

## Task 2: Cone / sector geometry (pure `point_in_cone` + cone detection)

**Files:** Create `src/spatial/cone.rs`; Modify `src/spatial/mod.rs`, `src/spatial/detect.rs`.

- [ ] **Step 1: Write the failing pure-helper test** in `src/spatial/cone.rs`

```rust
use bevy::prelude::*;

/// True if `point` lies within a cone/sector with apex `apex`, central axis `axis`
/// (need not be normalized), half-angle `half_angle_rad`, and slant `range`.
pub fn point_in_cone(apex: Vec3, axis: Vec3, half_angle_rad: f32, range: f32, point: Vec3) -> bool {
    let to_point = point - apex;
    let dist = to_point.length();
    if dist > range || dist <= f32::EPSILON {
        return dist <= range; // apex itself counts as inside
    }
    let axis_n = axis.normalize_or_zero();
    if axis_n == Vec3::ZERO {
        return true; // degenerate axis -> treat as a sphere
    }
    let cos = to_point.normalize().dot(axis_n).clamp(-1.0, 1.0);
    cos >= half_angle_rad.cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_ahead_is_inside() {
        // 90-degree total cone (45 deg half-angle), range 3, axis +Z
        assert!(point_in_cone(Vec3::ZERO, Vec3::Z, 45f32.to_radians(), 3.0, Vec3::new(0.0, 0.0, 2.0)));
    }
    #[test]
    fn behind_is_outside() {
        assert!(!point_in_cone(Vec3::ZERO, Vec3::Z, 45f32.to_radians(), 3.0, Vec3::new(0.0, 0.0, -2.0)));
    }
    #[test]
    fn beyond_range_is_outside() {
        assert!(!point_in_cone(Vec3::ZERO, Vec3::Z, 45f32.to_radians(), 3.0, Vec3::new(0.0, 0.0, 5.0)));
    }
    #[test]
    fn just_outside_the_angle_is_outside() {
        // point at 60 deg off-axis, half-angle 45 deg -> outside
        let p = Vec3::new(0.0, 2.0, 1.0); // ~63 deg from +Z
        assert!(!point_in_cone(Vec3::ZERO, Vec3::Z, 45f32.to_radians(), 3.0, p));
    }
    #[test]
    fn within_the_angle_is_inside() {
        let p = Vec3::new(0.0, 0.5, 2.0); // ~14 deg from +Z
        assert!(point_in_cone(Vec3::ZERO, Vec3::Z, 45f32.to_radians(), 3.0, p));
    }
}
```

- [ ] **Step 2: Wire the module** — add `pub mod cone;` to `src/spatial/mod.rs`.

- [ ] **Step 3: Run the helper tests**

Run: `cargo test --features test-support --lib point_in_cone`
Expected: PASS (5 new tests). If `f32::to_radians`/`normalize_or_zero` differ, adjust; these are stable in bevy's glam.

- [ ] **Step 4: Apply the cone filter in `detect_overlaps`** in `src/spatial/detect.rs`

The cone uses a sphere(`range`) broadphase (already what `to_collider` returns for `Cone`), then an angle filter against each hurtbox's world position. Add `&Transform` to the hurtbox query and, for cone shapes, reject hits outside the sector.

Change the hurtbox query type:
```rust
    hurtboxes: Query<(Entity, &Hurtbox, &Transform)>,
```
Update the destructure inside the hits loop:
```rust
        for hurt_e in hits {
            let Ok((owner_e, hurt, hurt_tf)) = hurtboxes.get(hurt_e) else {
                continue;
            };
            let target = hurt.owner;
            if target == hitbox.caster {
                continue;
            }
            // Cone/sector narrow-phase: the broadphase was a sphere(range); reject hits
            // outside the authored cone angle.
            if let crate::assets::CollisionShape::Cone { angle, range } = hitbox.shape {
                let half = angle.to_radians() * 0.5;
                if !crate::spatial::cone::point_in_cone(
                    hb_tf.translation,
                    hitbox.aim,
                    half,
                    range,
                    hurt_tf.translation,
                ) {
                    continue;
                }
            }
```
(Leave the rest of the loop — faction check, `can_hit`, `register_hit`, trigger — unchanged.)

- [ ] **Step 5: Build + run the suite**

Run: `cargo test --features test-support --lib --tests`
Expected: PASS, still **12 passed** (firebolt is a Sphere, so the cone branch is skipped). Plus the 5 cone unit tests.

- [ ] **Step 6: Commit**

```bash
git add src/spatial/cone.rs src/spatial/mod.rs src/spatial/detect.rs
git commit -m "feat(spatial): cone/sector geometry (broadphase sphere + angle filter)"
```

---

## Task 3: Real `HitFilter` (faction-aware target selection)

**Files:** Create `src/spatial/filter.rs`; Modify `src/spatial/mod.rs`, `src/spatial/detect.rs`.

- [ ] **Step 1: Write the failing pure-helper test** in `src/spatial/filter.rs`

```rust
use crate::assets::HitFilter;
use crate::core::components::Faction;

/// Whether a hitbox with `filter` (owned by `caster_faction`) may hit a target.
/// `is_self` is true when the target entity IS the caster.
pub fn passes_filter(
    filter: HitFilter,
    caster_faction: Faction,
    target_faction: Faction,
    is_self: bool,
) -> bool {
    match filter {
        HitFilter::Caster => is_self,
        HitFilter::All => !is_self,
        HitFilter::Enemies => !is_self && target_faction != caster_faction,
        HitFilter::Allies => !is_self && target_faction == caster_faction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemies_hits_other_faction_only() {
        assert!(passes_filter(HitFilter::Enemies, Faction::Player, Faction::Enemy, false));
        assert!(!passes_filter(HitFilter::Enemies, Faction::Player, Faction::Player, false));
        assert!(!passes_filter(HitFilter::Enemies, Faction::Player, Faction::Enemy, true));
    }
    #[test]
    fn allies_hits_same_faction_only() {
        assert!(passes_filter(HitFilter::Allies, Faction::Player, Faction::Player, false));
        assert!(!passes_filter(HitFilter::Allies, Faction::Player, Faction::Enemy, false));
    }
    #[test]
    fn all_hits_anyone_but_self() {
        assert!(passes_filter(HitFilter::All, Faction::Player, Faction::Enemy, false));
        assert!(passes_filter(HitFilter::All, Faction::Player, Faction::Player, false));
        assert!(!passes_filter(HitFilter::All, Faction::Player, Faction::Player, true));
    }
    #[test]
    fn caster_hits_only_self() {
        assert!(passes_filter(HitFilter::Caster, Faction::Player, Faction::Player, true));
        assert!(!passes_filter(HitFilter::Caster, Faction::Player, Faction::Enemy, false));
    }
}
```

- [ ] **Step 2: Wire the module** — add `pub mod filter;` to `src/spatial/mod.rs`.

- [ ] **Step 3: Run the helper tests**

Run: `cargo test --features test-support --lib passes_filter`
Expected: PASS (4 tests).

- [ ] **Step 4: Use `passes_filter` in `detect_overlaps`** in `src/spatial/detect.rs`

Remove the `if target == hitbox.caster { continue; }` early-skip (the filter now handles self via `is_self`) and replace the Enemies hard-code. Concretely, delete:
```rust
            if target == hitbox.caster {
                continue;
            }
```
and replace the faction block:
```rust
            // Faction filter (HitFilter::Enemies for the slice).
            let target_faction = factions.get(target).copied().unwrap_or(Faction::Neutral);
            let is_enemy = target_faction != caster_faction;
            if !is_enemy {
                continue;
            }
```
with:
```rust
            let target_faction = factions.get(target).copied().unwrap_or(Faction::Neutral);
            let is_self = target == hitbox.caster;
            if !crate::spatial::filter::passes_filter(
                hitbox.filter,
                caster_faction,
                target_faction,
                is_self,
            ) {
                continue;
            }
```
**IMPORTANT ordering:** the cone narrow-phase check (Task 2) skips self via `target == hitbox.caster` implicitly only for non-Caster filters now; ensure the cone check still runs after the self-handling. Keep the cone block where it is (after the destructure, before the filter) — it does not reference self. The filter block must come before `can_hit`/`register_hit`.

- [ ] **Step 5: Build + run the suite**

Run: `cargo test --features test-support --lib --tests`
Expected: PASS, **12 passed** (firebolt: filter Enemies, player→enemy still hits; player not hit since it has no hurtbox anyway). Plus filter unit tests.

- [ ] **Step 6: Commit**

```bash
git add src/spatial/filter.rs src/spatial/mod.rs src/spatial/detect.rs
git commit -m "feat(spatial): faction-aware HitFilter (Caster/Allies/Enemies/All)"
```

---

## Task 4: `HitMode` + re-hit timing wired through `age`

**Files:** Modify `src/timeline/advance.rs`.

`can_hit`/`register_hit` already implement the modes (Task 1). This task ensures `age` advances so `rehit_interval` works, and adds a focused test.

- [ ] **Step 1: Increment `age` in `expire_hitboxes`** in `src/timeline/advance.rs`

Change `expire_hitboxes` to advance `age` alongside `remaining`:
```rust
pub fn expire_hitboxes(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Hitbox)>,
) {
    let dt = time.delta_secs();
    for (e, mut hb) in &mut q {
        hb.age += dt;
        hb.remaining -= dt;
        if hb.remaining <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
```

- [ ] **Step 2: Write a pure re-hit test** in `src/spatial/boxes.rs` (add a `#[cfg(test)] mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{CollisionShape, HitFilter, HitMode};
    use bevy::prelude::*;

    fn hitbox(mode: HitMode, rehit: Option<f32>) -> Hitbox {
        Hitbox {
            caster: Entity::PLACEHOLDER,
            skill_id: "s".into(),
            window_id: "w".into(),
            filter: HitFilter::Enemies,
            mode,
            shape: CollisionShape::Sphere { radius: 1.0 },
            aim: Vec3::Z,
            age: 0.0,
            rehit_interval: rehit,
            remaining: 5.0,
            hit_log: HashMap::new(),
            done: false,
        }
    }

    #[test]
    fn once_per_target_hits_once() {
        let t = Entity::from_raw(7);
        let mut hb = hitbox(HitMode::OncePerTarget, None);
        assert!(hb.can_hit(t));
        hb.register_hit(t);
        assert!(!hb.can_hit(t), "no second hit without a re-hit interval");
    }

    #[test]
    fn first_only_stops_after_one_target() {
        let mut hb = hitbox(HitMode::FirstOnly, None);
        let a = Entity::from_raw(1);
        let b = Entity::from_raw(2);
        assert!(hb.can_hit(a));
        hb.register_hit(a);
        assert!(!hb.can_hit(b), "FirstOnly stops after the first target");
    }

    #[test]
    fn every_tick_always_hits() {
        let t = Entity::from_raw(3);
        let mut hb = hitbox(HitMode::EveryTick, None);
        hb.register_hit(t);
        assert!(hb.can_hit(t), "EveryTick re-hits the same target");
    }

    #[test]
    fn rehit_interval_allows_re_hit_after_delay() {
        let t = Entity::from_raw(4);
        let mut hb = hitbox(HitMode::OncePerTarget, Some(0.5));
        hb.register_hit(t); // logged at age 0
        assert!(!hb.can_hit(t), "too soon");
        hb.age = 0.6;
        assert!(hb.can_hit(t), "interval elapsed -> re-hit allowed");
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --features test-support --lib boxes::tests`
Expected: PASS (4 tests). Then `cargo test --features test-support --lib --tests` → still **12 passed** overall + the new units.

- [ ] **Step 4: Commit**

```bash
git add src/timeline/advance.rs src/spatial/boxes.rs
git commit -m "feat(spatial): HitMode + re-hit interval timing via hitbox age"
```

---

## Task 5: Aimed targeting — entity / point / direction

**Files:** Modify `src/timeline/cast.rs`, `src/timeline/state.rs`, `src/timeline/advance.rs`, `src/prelude.rs`.

- [ ] **Step 1: Add the `CastAim` model + verbs** in `src/timeline/cast.rs`

Replace the whole file with:
```rust
use bevy::prelude::*;

/// How a cast is aimed. Resolved to a facing direction at validation time.
#[derive(Clone, Copy, Debug)]
pub enum CastAim {
    /// Aim at an entity (direction = toward its transform; range = distance to it).
    Entity(Entity),
    /// Aim at a ground point.
    Point(Vec3),
    /// Aim along an explicit direction (no target entity / range gate).
    Direction(Dir3),
}

/// Pending cast request, consumed by the Validate system.
#[derive(Component, Debug)]
pub struct PendingCast {
    pub skill_id: String,
    pub aim: CastAim,
}

/// EntityCommands verbs to request a cast.
pub trait CastSkillExt {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self;
    fn cast_skill_at_point(&mut self, skill_id: impl Into<String>, point: Vec3) -> &mut Self;
    fn cast_skill_dir(&mut self, skill_id: impl Into<String>, dir: Dir3) -> &mut Self;
}

impl CastSkillExt for EntityCommands<'_> {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self {
        self.insert(PendingCast { skill_id: skill_id.into(), aim: CastAim::Entity(target) });
        self
    }
    fn cast_skill_at_point(&mut self, skill_id: impl Into<String>, point: Vec3) -> &mut Self {
        self.insert(PendingCast { skill_id: skill_id.into(), aim: CastAim::Point(point) });
        self
    }
    fn cast_skill_dir(&mut self, skill_id: impl Into<String>, dir: Dir3) -> &mut Self {
        self.insert(PendingCast { skill_id: skill_id.into(), aim: CastAim::Direction(dir) });
        self
    }
}
```

- [ ] **Step 2: Update `ActiveCast`** in `src/timeline/state.rs`

Change the `target: Entity` field to a resolved aim + optional target:
```rust
#[derive(Component, Debug)]
pub struct ActiveCast {
    pub skill_id: String,
    /// The aimed target entity, if any (for reference / future single-target rules).
    pub target: Option<Entity>,
    /// Resolved, normalized facing direction (projectile heading / cone axis).
    pub aim_dir: Vec3,
    pub phase: SkillPhase,
    pub elapsed: f32,
    pub windup: f32,
    pub active: f32,
    pub recovery: f32,
    pub fired_windows: Vec<String>,
}
```
(`total_duration` / `phase_at` impls are unchanged.)

- [ ] **Step 3: Resolve the aim in `validate_casts`** in `src/timeline/advance.rs`

Add a `Transform` query and resolve `CastAim` → `(aim_dir, target)`. Update imports:
```rust
use crate::timeline::cast::{CastAim, PendingCast};
```
Add the query param to `validate_casts`:
```rust
    transforms: Query<&Transform>,
```
Then, after the `can_use_skill` check and before building `ActiveCast`, resolve the aim:
```rust
        let caster_pos = transforms.get(caster).map(|t| t.translation).unwrap_or(Vec3::ZERO);
        let (aim_dir, target) = match req.aim {
            CastAim::Entity(e) => {
                let Ok(tf) = transforms.get(e) else {
                    commands.trigger(CastRejected {
                        caster,
                        skill_id: req.skill_id.clone(),
                        reason: CastRejectReason::NoTarget,
                    });
                    continue;
                };
                let delta = tf.translation - caster_pos;
                (delta.normalize_or_zero(), Some(e))
            }
            CastAim::Point(p) => ((p - caster_pos).normalize_or_zero(), None),
            CastAim::Direction(d) => (d.as_vec3(), None),
        };
        let aim_dir = if aim_dir == Vec3::ZERO { Vec3::Z } else { aim_dir };
```
Build `ActiveCast` with the new fields (replace the old `target: req.target`):
```rust
        commands.entity(caster).insert(ActiveCast {
            skill_id: req.skill_id.clone(),
            target,
            aim_dir,
            phase: SkillPhase::Windup,
            elapsed: 0.0,
            windup,
            active,
            recovery,
            fired_windows: Vec::new(),
        });
```

- [ ] **Step 4: Orient the hitbox along the aim in `advance_casts`** in `src/timeline/advance.rs`

Replace `let dir = Vec3::Z;` (in the window-spawn block) with the resolved aim, and orient the hitbox transform to face it:
```rust
                let dir = cast.aim_dir;
                let rot = Quat::from_rotation_arc(Vec3::Z, dir);
                let mut ent = commands.spawn((
                    Hitbox {
                        caster,
                        skill_id: cast.skill_id.clone(),
                        window_id: win.id.clone(),
                        filter: win.hit_filter,
                        mode: win.hit_mode,
                        shape: win.shape,
                        aim: dir,
                        age: 0.0,
                        rehit_interval: win.rehit_interval,
                        remaining: win.active_duration,
                        hit_log: std::collections::HashMap::new(),
                        done: false,
                    },
                    Transform::from_translation(caster_tf.translation).with_rotation(rot),
                ));
```
(The `Projectile { velocity: dir * speed }` and `HitWindowOpened` parts stay; `dir` is now the resolved aim.)

- [ ] **Step 5: Export `CastAim`** in `src/prelude.rs`

Change the cast re-export line to also export `CastAim`:
```rust
pub use crate::timeline::cast::{CastAim, CastSkillExt};
```

- [ ] **Step 6: Build + run the suite**

Run: `cargo test --features test-support --lib --tests`
Expected: PASS, **12 passed**. The firebolt slice test calls `cast_skill_at("firebolt", dummy)` with the dummy at `(0,0,2)` and the player at `(0,0,0)` → resolved `aim_dir = +Z`, identical to the old hard-coded direction, so the bolt still hits.

- [ ] **Step 7: Commit**

```bash
git add src/timeline/cast.rs src/timeline/state.rs src/timeline/advance.rs src/prelude.rs
git commit -m "feat(targeting): aimed casts (entity/point/direction) resolve to a facing dir"
```

---

## Task 6: Range validation

**Files:** Modify `src/timeline/advance.rs`.

- [ ] **Step 1: Add a pure range-check helper + test** in `src/timeline/advance.rs`

Add near the top of the file (module scope):
```rust
use crate::assets::CastTargeting;

/// The max cast range for a targeting mode, if it gates on range. `None` = no range gate.
pub fn targeting_range(targeting: &CastTargeting) -> Option<f32> {
    match targeting {
        CastTargeting::SelfCast => None,
        CastTargeting::SingleEntity { range }
        | CastTargeting::Direction { range }
        | CastTargeting::Cone { range, .. } => Some(*range),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::CastTargeting;

    #[test]
    fn self_cast_has_no_range_gate() {
        assert_eq!(targeting_range(&CastTargeting::SelfCast), None);
    }
    #[test]
    fn single_entity_range_is_extracted() {
        assert_eq!(targeting_range(&CastTargeting::SingleEntity { range: 12.0 }), Some(12.0));
    }
    #[test]
    fn cone_range_is_extracted() {
        assert_eq!(targeting_range(&CastTargeting::Cone { angle: 90.0, range: 4.0 }), Some(4.0));
    }
}
```

- [ ] **Step 2: Run the helper tests**

Run: `cargo test --features test-support --lib targeting_range`
Expected: PASS (3 tests).

- [ ] **Step 3: Enforce range in `validate_casts`** in `src/timeline/advance.rs`

After resolving `(aim_dir, target)` and BEFORE inserting `ActiveCast`, add a range gate. Range only applies to entity/point aim (a bare Direction has no target distance):
```rust
        if let Some(max_range) = targeting_range(&timeline.targeting) {
            let aim_point = match req.aim {
                CastAim::Entity(e) => transforms.get(e).ok().map(|t| t.translation),
                CastAim::Point(p) => Some(p),
                CastAim::Direction(_) => None, // no distance to gate on
            };
            if let Some(point) = aim_point {
                if point.distance(caster_pos) > max_range {
                    commands.trigger(CastRejected {
                        caster,
                        skill_id: req.skill_id.clone(),
                        reason: CastRejectReason::OutOfRange,
                    });
                    continue;
                }
            }
        }
```

- [ ] **Step 4: Build + run the suite**

Run: `cargo test --features test-support --lib --tests`
Expected: PASS, **12 passed**. (Firebolt targeting is `SingleEntity { range: 15.0 }`; the dummy is 2 units away → within range.)

- [ ] **Step 5: Commit**

```bash
git add src/timeline/advance.rs
git commit -m "feat(targeting): range validation (OutOfRange rejection)"
```

---

## Task 7: Line-of-sight validation (raycast)

**Files:** Modify `src/events.rs`, `src/timeline/advance.rs`.

LOS is checked only for entity-targeted casts (we have a concrete target entity to ray toward). A ray is cast from the caster to the target; if it strikes a collider that is NOT the target's hurtbox before reaching the target, the cast is blocked. This works without dedicated collision layers by inspecting *what* the ray hit.

- [ ] **Step 1: Add the `NoLineOfSight` reject reason** in `src/events.rs`

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
}
```

- [ ] **Step 2: Add `SpatialQuery` to `validate_casts` and check LOS** in `src/timeline/advance.rs`

Add the imports + param:
```rust
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use crate::spatial::boxes::Hurtbox;
```
Add params to `validate_casts`:
```rust
    spatial: SpatialQuery,
    hurtboxes: Query<&Hurtbox>,
```
After the range gate and before inserting `ActiveCast`, for entity aim only:
```rust
        if let CastAim::Entity(e) = req.aim {
            if let Ok(tf) = transforms.get(e) {
                let to_target = tf.translation - caster_pos;
                let dist = to_target.length();
                if dist > f32::EPSILON {
                    let dir = Dir3::new(to_target).unwrap_or(Dir3::Z);
                    // Exclude the caster's own collider from the ray.
                    let filter = SpatialQueryFilter::default().with_excluded_entities([caster]);
                    if let Some(hit) = spatial.cast_ray(caster_pos, dir, dist, true, &filter) {
                        // Blocked if the first thing hit is not the target's own hurtbox.
                        let hit_is_target = hurtboxes.get(hit.entity).map(|h| h.owner) == Ok(e);
                        if !hit_is_target {
                            commands.trigger(CastRejected {
                                caster,
                                skill_id: req.skill_id.clone(),
                                reason: CastRejectReason::NoLineOfSight,
                            });
                            continue;
                        }
                    }
                }
            }
        }
```

- [ ] **Step 3: Build + run the suite**

Run: `cargo test --features test-support --lib --tests`
Expected: PASS, **12 passed**. (Firebolt's dummy has the only collider on the ray path, and it IS the target's hurtbox → LOS clear.) **If `cast_ray`'s signature differs in avian3d 0.4** (arg order/types of `origin, direction, max_distance, solid, filter`, or `with_excluded_entities` vs a different exclusion API, or `RayHitData.entity`), consult docs.rs/avian3d/0.4 and adjust; report the working form. If the exclusion API is awkward, an acceptable alternative is to skip the hit when `hit.entity`'s collider belongs to the caster.

- [ ] **Step 4: Commit**

```bash
git add src/events.rs src/timeline/advance.rs
git commit -m "feat(targeting): line-of-sight validation via raycast"
```

---

## Task 8: Integration — cone cleave + faction filter + aimed casts

**Files:** Create `tests/fixtures/skills/cleave.toml`, `assets/skills/cleave.cast.ron`, `tests/spatial_targeting.rs`.

- [ ] **Step 1: Author a melee cleave skill** — `tests/fixtures/skills/cleave.toml`

```toml
id = "cleave"
name = "Cleave"
tags = ["attack", "physical", "melee"]
targeting = "single_enemy"
delivery = "melee"
mana_cost = 0.0

[damage]
base_damages = [{ type = "physical", min = 100.0, max = 100.0 }]
```

- [ ] **Step 2: Author its cone timeline** — `assets/skills/cleave.cast.ron`

```ron
(
  skill_id: "cleave",
  phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
  collision_windows: [
    ( id: "arc", spawn_phase: Active, spawn_offset: 0.0, active_duration: 0.1,
      shape: Cone( angle: 120.0, range: 3.0 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
  targeting: Cone( angle: 120.0, range: 3.0 ),
  delivery: Melee,
)
```

- [ ] **Step 3: Write the integration tests** — `tests/spatial_targeting.rs`

```rust
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

fn make_block(id: &str, life: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = 100.0;
    b.current_mana = 100.0;
    b
}

fn load_cast(t: &mut ObeliskTestApp, skill: &str, file: &str) {
    let handle: Handle<CastTimeline> =
        t.app.world().resource::<AssetServer>().load(format!("assets/skills/{file}"));
    for _ in 0..2000 {
        t.app.update();
        if t.app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() {
            break;
        }
    }
    t.app.world_mut().resource_mut::<CastTimelineHandles>().0.insert(skill.into(), handle);
}

fn spawn_combatant(t: &mut ObeliskTestApp, id: &str, faction: Faction, pos: Vec3, life: f64) -> Entity {
    let e = t.app.world_mut().spawn((
        Combatant,
        Attributes(make_block(id, life)),
        faction,
        ObeliskId(id.into()),
        Transform::from_translation(pos),
    )).id();
    let mut c = t.app.world_mut().commands();
    insert_hurtbox(&mut c, e, 0.5, pos);
    e
}

#[test]
fn cone_cleave_hits_multiple_enemies_in_arc_but_not_behind() {
    let mut t = ObeliskTestApp::new(7);
    load_cast(&mut t, "cleave", "cleave.cast.ron");

    // Player faces +Z. Two enemies in front (within the 120-degree arc), one behind.
    let player = spawn_combatant(&mut t, "player", Faction::Player, Vec3::ZERO, 100.0);
    let front_a = spawn_combatant(&mut t, "front_a", Faction::Enemy, Vec3::new(0.0, 0.0, 2.0), 50.0);
    let front_b = spawn_combatant(&mut t, "front_b", Faction::Enemy, Vec3::new(1.0, 0.0, 1.5), 50.0);
    let behind = spawn_combatant(&mut t, "behind", Faction::Enemy, Vec3::new(0.0, 0.0, -2.0), 50.0);
    t.app.update();

    // Aim the cleave forward (+Z) as a direction so the cone axis is deterministic.
    t.app.world_mut().commands().entity(player).cast_skill_dir("cleave", Dir3::Z);
    t.advance_ticks(60);

    let rec = t.rec();
    let hit: std::collections::HashSet<Entity> =
        rec.damage_resolved.iter().map(|d| d.target).collect();
    assert!(hit.contains(&front_a), "front_a (straight ahead) should be hit");
    assert!(hit.contains(&front_b), "front_b (within arc) should be hit");
    assert!(!hit.contains(&behind), "behind (outside the cone) must NOT be hit");
    let _ = player;
}

#[test]
fn cleave_does_not_hit_allies() {
    let mut t = ObeliskTestApp::new(7);
    load_cast(&mut t, "cleave", "cleave.cast.ron");
    let player = spawn_combatant(&mut t, "player", Faction::Player, Vec3::ZERO, 100.0);
    let ally = spawn_combatant(&mut t, "ally", Faction::Player, Vec3::new(0.0, 0.0, 2.0), 50.0);
    t.app.update();
    t.app.world_mut().commands().entity(player).cast_skill_dir("cleave", Dir3::Z);
    t.advance_ticks(60);
    assert!(t.rec().damage_resolved.iter().all(|d| d.target != ally),
        "Enemies filter must not hit a same-faction ally");
}

#[test]
fn out_of_range_cast_is_rejected() {
    let mut t = ObeliskTestApp::new(7);
    load_cast(&mut t, "cleave", "cleave.cast.ron");
    let player = spawn_combatant(&mut t, "player", Faction::Player, Vec3::ZERO, 100.0);
    // Enemy 10 units away; cleave range is 3.
    let far = spawn_combatant(&mut t, "far", Faction::Enemy, Vec3::new(0.0, 0.0, 10.0), 50.0);
    t.app.update();
    t.app.world_mut().commands().entity(player).cast_skill_at("cleave", far);
    t.advance_ticks(10);
    assert!(t.rec().cast_rejected.iter().any(|r| r.reason == CastRejectReason::OutOfRange),
        "a target beyond range should be rejected as OutOfRange");
    assert!(t.rec().damage_resolved.is_empty(), "no damage on a rejected cast");
}
```

- [ ] **Step 4: Run the integration tests**

Run: `cargo test --features test-support --test spatial_targeting -- --nocapture`
Expected: PASS (3 tests). **Debug guidance if a test fails:**
- If `front_b` isn't hit, its angle from +Z is `atan2(1.0, 1.5) ≈ 33.7°`, well inside the 60° half-angle — confirm the cone uses `angle * 0.5` as the half-angle and the hitbox `aim` is `+Z`. Print the hitbox `aim` and each hurtbox position.
- If `behind` IS hit, the cone narrow-phase isn't running — confirm `hitbox.shape` is `Cone` (the asset's window shape) and the `point_in_cone` branch executes.
- If nothing is hit at all, the cleave window may expire before detection — the window is `active_duration: 0.1` (~6 ticks); ensure `advance_ticks(60)` covers windup (0.1s) + active. If needed, widen `active_duration`.
- If `OutOfRange` doesn't fire, confirm `cast_skill_at` (entity aim) is used for that test and the distance (10) exceeds range (3).

- [ ] **Step 5: Full suite + clippy + fmt**

Run: `cargo test --features test-support --lib --tests` (expect **15 passed**: 12 prior + 3 new integration; plus all the new unit tests under `--lib`).
Run: `cargo clippy --features test-support --lib --tests -- -D warnings` (must be clean for `obelisk-bevy`).
Run: `cargo fmt` then `cargo fmt --check`.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/skills/cleave.toml assets/skills/cleave.cast.ron tests/spatial_targeting.rs
git commit -m "test(spatial): cone cleave multi-hit + ally filter + out-of-range integration"
```

---

## Self-review notes (coverage vs the batch scope)

- Per-hitbox stored collider (honor asset shape): Task 1 ✅
- True cone/sector geometry: Task 2 ✅
- Real `HitFilter` (Caster/Allies/Enemies/All): Task 3 ✅
- `HitMode::EveryTick` + re-hit intervals: Tasks 1 (logic) + 4 (age timing) ✅
- Ground/direction targeting: Task 5 (`cast_skill_at_point`, `cast_skill_dir`, `CastAim`) ✅
- Range validation: Task 6 ✅
- Line-of-sight validation: Task 7 ✅
- Backward compatibility (firebolt slice green): asserted at the end of Tasks 1,2,3,5,6,7 ✅
- Integration proof (cone multi-hit, ally filter, out-of-range): Task 8 ✅

## Deferred (out of this batch)

Projectile pierce/chain/consume-on-hit; collision-layer-based filtering (this batch filters at resolution time, which is sufficient and matches the slice's approach); per-hitbox `Box` shape (the asset enum has Sphere/Capsule/Cone only); cone as a true swept volume over time. These belong to later batches.
