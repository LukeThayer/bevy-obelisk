# Surfaces (Ground Effects) — Obelisk Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persistent, typed, paintable ground state ("surfaces") as a first-class deterministic
obelisk concept: surface types as content, patch entities in the sim, windows that paint,
acquisition that requires/consumes, standing payloads (effects + tick skills), and
skill-contact reactions (fire ignites oil).

**Architecture:** A new `src/surfaces/` module (types/registry, patch entity + paint/decay,
systems) + additive schema fields in `src/assets/mod.rs` (`paints` on `CollisionWindow`,
`on_surface` on `Acquisition::GroundPoint`) + a threading change in `validate_casts`. All
cross-skill causality reuses the existing triggered-execution machinery
(`execute_skill_timeline`); a surface never deals damage itself. Spec:
`obelisk-arena/docs/superpowers/specs/2026-07-09-surfaces-ground-effects-design.md` (§4–§5 are
the normative schema; this plan implements spec increments 1–3 ONLY — arena wiring and editor
are follow-up plans).

**Tech Stack:** Rust, Bevy 0.18 (observer API: `#[derive(Event)]`, `commands.trigger(...)`,
`app.add_observer(fn)`, param `On<E>` + `ev.event()`), avian3d 0.5 (not needed by new code —
patch overlap is analytic), serde + `toml` (NEW dep), existing `ObeliskTestApp` harness.

## Global Constraints

- Repo: `~/src/obelisk-bevy`, branch **`surfaces-core`** off `main` (create in Task 1).
- **Determinism is law:** no wall clock, no new RNG draws (neither `CombatRng` nor `SpawnRng`),
  every multi-entity iteration that can fire triggers/paints must iterate in a **sorted order**
  (`Entity::index()` or patch `seq`) — never raw `HashMap`/`HashSet`/query order.
- **Existing goldens must stay byte-identical** (`cargo test --test golden`). Every new authored
  field is `#[serde(default)]`; every new system is a no-op when no content uses surfaces.
- New authored structs use `#[serde(deny_unknown_fields)]` (established crate convention — typos
  fail loud).
- The sim schedule is single-threaded (`ObeliskSimPlugin` pins it); systems may still not rely
  on unordered iteration (see determinism above).
- Run tests from the repo root: `cargo test --test surfaces` (new suite), `cargo test` (full).
- Every commit message ends with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
- Surfaces are 2.5D: XZ discs with `SURFACE_Y_TOLERANCE = 1.5` vertical tolerance. Constants:
  `SURFACE_MATCH_SLACK = 0.3` (acquisition match slack, mirrors arena `SPIRE_MATCH_RANGE`).

## File Structure

- Create: `src/surfaces/mod.rs` (plugin + re-exports), `src/surfaces/types.rs` (TOML schema +
  registry + loader), `src/surfaces/patch.rs` (patch component, paint helper, decay, events),
  `src/surfaces/systems.rs` (trail painter, OnEnd observer, standing payloads, contact triggers).
- Modify: `src/lib.rs` (module + plugin), `src/assets/mod.rs` (schema), `src/core/config.rs`
  (`add_obelisk_surfaces`), `src/timeline/advance.rs` (acquisition), `src/prelude.rs`,
  `src/testkit.rs` (recorder), `Cargo.toml` (toml dep).
- Test: `tests/surfaces.rs` (one integration suite for the whole feature) + fixtures under
  `tests/fixtures/surfaces/*.toml` and `tests/fixtures/cast/*.cast.ron` and one new
  `tests/fixtures/skills/surfaces.toml`.

---

### Task 1: Surface-type content schema, registry, loader, `add_obelisk_surfaces`

**Files:**
- Modify: `Cargo.toml` (add `toml = "0.8"` to `[dependencies]`)
- Create: `src/surfaces/mod.rs`, `src/surfaces/types.rs`
- Modify: `src/lib.rs` (add `pub mod surfaces;` to the module list)
- Modify: `src/core/config.rs` (trait method `add_obelisk_surfaces`)
- Create: `tests/fixtures/surfaces/frost.toml`, `burning.toml`, `oil.toml`, `dew.toml`,
  `capped.toml`, `mud.toml`, `blessed.toml`
- Create: `tests/fixtures/skills/surfaces.toml`
- Test: `tests/surfaces.rs`

**Interfaces:**
- Consumes: `crate::core::config::SkillRegistry` (`pub struct SkillRegistry(pub HashMap<String, Skill>)`),
  `stat_core::config::effect_registry_initialized()`, `stat_core::effect_registry().get(&id)`,
  `crate::assets::HitFilter`.
- Produces (later tasks rely on these EXACT names):
  - `surfaces::types::{SurfaceType, StandingPayload, StandingFilter, ContactReaction, SurfaceVisuals}`
  - `surfaces::types::SurfaceRegistry` (`#[derive(Resource, Default)] pub struct SurfaceRegistry(pub HashMap<String, SurfaceType>)`)
  - `surfaces::types::{load_surfaces_dir, SURFACE_Y_TOLERANCE, SURFACE_MATCH_SLACK}`
  - `ObeliskConfigExt::add_obelisk_surfaces(&mut self, dir: &Path) -> &mut Self`

- [ ] **Step 0: Branch**

```bash
cd ~/src/obelisk-bevy && git checkout -b surfaces-core
```

- [ ] **Step 1: Write the failing tests** (create `tests/surfaces.rs`)

```rust
//! Integration suite for the surfaces (ground effects) core — spec
//! `obelisk-arena/docs/superpowers/specs/2026-07-09-surfaces-ground-effects-design.md`.
use obelisk_bevy::prelude::*;
use obelisk_bevy::surfaces::{load_surfaces_dir, SurfaceRegistry};
use std::path::Path;

#[test]
fn surfaces_dir_loads_and_validates() {
    obelisk_bevy::testkit::init_test_obelisk(); // effect registry from fixtures (burn/empower)
    let skills =
        stat_core::config::load_skills_dir(Path::new("tests/fixtures/skills")).expect("skills");
    let reg = obelisk_bevy::core::config::SkillRegistry(skills);
    let map = load_surfaces_dir(Path::new("tests/fixtures/surfaces"), Some(&reg)).expect("load");
    assert!(map.contains_key("frost"));
    assert!(map.contains_key("oil"));
    let burning = &map["burning"];
    let standing = burning.standing.as_ref().expect("burning has standing");
    assert_eq!(standing.tick_skill.as_deref(), Some("burning_tick"));
    assert_eq!(standing.rehit_interval, 0.2);
    let oil = &map["oil"];
    assert_eq!(oil.on_skill_contact.len(), 1);
    assert!(oil.on_skill_contact[0].consume);
    // defaults
    assert_eq!(map["frost"].merge_radius, 0.25);
    assert_eq!(map["frost"].max_patches, 64);
    assert_eq!(map["frost"].patch_radius, 0.45);
}

#[test]
fn surfaces_loader_rejects_bad_refs() {
    obelisk_bevy::testkit::init_test_obelisk();
    let skills =
        stat_core::config::load_skills_dir(Path::new("tests/fixtures/skills")).expect("skills");
    let reg = obelisk_bevy::core::config::SkillRegistry(skills);
    // unknown tick_skill
    let dir = std::env::temp_dir().join("surf_bad_skill");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("bad.toml"),
        "id = \"bad\"\n[standing]\ntick_skill = \"no_such_skill\"\n",
    )
    .unwrap();
    let err = load_surfaces_dir(&dir, Some(&reg)).unwrap_err();
    assert!(err.contains("no_such_skill"), "error names the bad ref: {err}");
    // unknown effect
    let dir2 = std::env::temp_dir().join("surf_bad_effect");
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(
        dir2.join("bad.toml"),
        "id = \"bad\"\n[standing]\neffect = \"no_such_effect\"\n",
    )
    .unwrap();
    let err2 = load_surfaces_dir(&dir2, Some(&reg)).unwrap_err();
    assert!(err2.contains("no_such_effect"), "error names the bad effect: {err2}");
}

#[test]
fn add_obelisk_surfaces_inserts_the_registry() {
    let mut t = obelisk_bevy::testkit::ObeliskTestApp::new(1);
    t.app.add_obelisk_surfaces(Path::new("tests/fixtures/surfaces"));
    let reg = t.app.world().resource::<SurfaceRegistry>();
    assert!(reg.0.contains_key("frost"));
}
```

- [ ] **Step 2: Write the fixtures**

`tests/fixtures/surfaces/frost.toml`:
```toml
id = "frost"
lifetime = 180.0
```
`tests/fixtures/surfaces/burning.toml`:
```toml
id = "burning"
lifetime = 8.0

[standing]
filter = "enemies"
tick_skill = "burning_tick"
rehit_interval = 0.2
```
`tests/fixtures/surfaces/oil.toml`:
```toml
id = "oil"
lifetime = 30.0

[[on_skill_contact]]
tags_any = ["fire"]
trigger_skill = "test_ignite"
consume = true
```
`tests/fixtures/surfaces/dew.toml`:
```toml
id = "dew"
lifetime = 0.3
```
`tests/fixtures/surfaces/capped.toml`:
```toml
id = "capped"
lifetime = 60.0
merge_radius = 0.05
max_patches = 3
```
`tests/fixtures/surfaces/mud.toml`:
```toml
id = "mud"
lifetime = 60.0

[standing]
filter = "enemies"
tick_skill = "burning_tick"
on_enter_only = true
```
`tests/fixtures/surfaces/blessed.toml`:
```toml
id = "blessed"
lifetime = 60.0

[standing]
filter = "allies"
effect = "empower"
rehit_interval = 0.2
```

`tests/fixtures/skills/surfaces.toml` (loaded automatically by the harness's
`add_obelisk_skills(Dir)` — ids must not collide with existing fixtures):
```toml
[[skills]]
id = "paint_roller"
name = "Paint Roller"
tags = ["spell", "cold"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "cold", min = 1.0, max = 1.0 }]

[[skills]]
id = "paint_blast"
name = "Paint Blast"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "fire", min = 1.0, max = 1.0 }]

[[skills]]
id = "fire_probe"
name = "Fire Probe"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "fire", min = 1.0, max = 1.0 }]

[[skills]]
id = "cold_probe"
name = "Cold Probe"
tags = ["spell", "cold"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "cold", min = 1.0, max = 1.0 }]

[[skills]]
id = "test_ignite"
name = "Test Ignite"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "fire", min = 10.0, max = 10.0 }]

[[skills]]
id = "burning_tick"
name = "Burning Tick"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "fire", min = 5.0, max = 5.0 }]

[[skills]]
id = "spire_probe"
name = "Spire Probe"
tags = ["spell", "cold"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "cold", min = 8.0, max = 8.0 }]

[[skills]]
id = "spire_fallback"
name = "Spire Fallback"
tags = ["spell", "cold"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "cold", min = 8.0, max = 8.0 }]
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --test surfaces 2>&1 | tail -20`
Expected: COMPILE FAIL — `could not find `surfaces` in `obelisk_bevy``.

- [ ] **Step 4: Implement**

`Cargo.toml` — add to `[dependencies]` (after `thiserror = "1"`):
```toml
toml = "0.8"
```

`src/lib.rs` — in the module list (alphabetical, after `pub mod spatial;`):
```rust
pub mod surfaces;
```

`src/surfaces/mod.rs`:
```rust
//! Surfaces (ground effects): persistent, typed, paintable ground state — spec
//! `obelisk-arena/docs/superpowers/specs/2026-07-09-surfaces-ground-effects-design.md`.
//! A surface TYPE is content (`config/surfaces/<id>.toml` → [`SurfaceRegistry`], loaded via
//! `ObeliskConfigExt::add_obelisk_surfaces`); a surface PATCH is a sim entity (Task 2). Skills
//! interact through paint (window `paints`), require/consume (acquisition `on_surface`), and
//! react (the surface's own `standing` payload + `on_skill_contact` reactions). A surface never
//! deals damage itself — it applies effects or executes a triggered-only skill, so everything
//! rides the existing combat path.
pub mod types;

pub use types::{
    load_surfaces_dir, ContactReaction, StandingFilter, StandingPayload, SurfaceRegistry,
    SurfaceType, SurfaceVisuals, SURFACE_MATCH_SLACK, SURFACE_Y_TOLERANCE,
};
```

`src/surfaces/types.rs`:
```rust
//! Surface-type content schema + registry + directory loader. One TOML per surface, loaded by
//! `ObeliskConfigExt::add_obelisk_surfaces` (mirrors `add_obelisk_effects`, but registry-as-
//! Resource like `SkillRegistry` — no stat_core global, no double-init pain). Loader validation
//! fails LOUD (crate convention): unknown effect / skill refs reject the whole directory.
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Vertical tolerance of the 2.5D overlap test: a point is "in" a patch when its XZ distance is
/// within the patch radius AND its Y is within this many units of the patch (spec §5.2).
pub const SURFACE_Y_TOLERANCE: f32 = 1.5;
/// Acquisition match slack: `on_surface` matches a patch when the aimed point lies within
/// `patch.radius + SURFACE_MATCH_SLACK` in XZ (today's arena `SPIRE_MATCH_RANGE` feel).
pub const SURFACE_MATCH_SLACK: f32 = 0.3;

/// One authored surface type (`config/surfaces/<id>.toml`). The `[visuals]` block is sim-inert
/// data for the host/editor renderer (like `CastTimeline::cues`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceType {
    pub id: String,
    /// Default patch lifetime (secs); a window's `PaintSpec.lifetime` may override per-paint.
    #[serde(default = "default_lifetime")]
    pub lifetime: f32,
    /// Dedup radius: a paint this close (XZ) to an existing same-type patch is skipped.
    #[serde(default = "default_merge_radius")]
    pub merge_radius: f32,
    /// Per-type cap; exceeding it despawns the OLDEST patch (replace-oldest, arena tile semantics).
    #[serde(default = "default_max_patches")]
    pub max_patches: usize,
    /// Default patch radius for paints that don't author one (the `PaintSurface` request path).
    #[serde(default = "default_patch_radius")]
    pub patch_radius: f32,
    /// Payload for entities standing INSIDE the surface (attributed to the painter).
    #[serde(default)]
    pub standing: Option<StandingPayload>,
    /// Reactions to a HITBOX touching this surface ("fire ignites oil").
    #[serde(default)]
    pub on_skill_contact: Vec<ContactReaction>,
    /// Host-rendered visuals (decal texture key, tint, looping vfx preset). Sim never reads it.
    #[serde(default)]
    pub visuals: Option<SurfaceVisuals>,
}

fn default_lifetime() -> f32 {
    180.0
}
fn default_merge_radius() -> f32 {
    0.25
}
fn default_max_patches() -> usize {
    64
}
fn default_patch_radius() -> f32 {
    0.45
}

/// Standing-payload faction filter, relative to the PAINTER. Maps onto [`crate::assets::HitFilter`]
/// (so `passes_filter` semantics apply exactly): `all` = everyone EXCEPT the painter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StandingFilter {
    Enemies,
    Allies,
    All,
}

impl StandingFilter {
    pub fn to_hit_filter(self) -> crate::assets::HitFilter {
        match self {
            StandingFilter::Enemies => crate::assets::HitFilter::Enemies,
            StandingFilter::Allies => crate::assets::HitFilter::Allies,
            StandingFilter::All => crate::assets::HitFilter::All,
        }
    }
}

/// What happens to entities standing in the surface. One clock (per victim × surface-type,
/// `rehit_interval`) drives BOTH `effect` application/refresh and `tick_skill` execution;
/// `on_enter_only = true` replaces the clock with an enter-edge firing (once per patch × victim
/// visit). NOTE (spec §5.2): the `effect` path applies via `apply_obelisk_effect`, which sources
/// the effect from the VICTIM itself — painter attribution (kill credit, damage-driven scaling)
/// only flows through `tick_skill`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingPayload {
    #[serde(default = "default_standing_filter")]
    pub filter: StandingFilter,
    /// Obelisk effect id applied/refreshed while standing (statuses — chill, burn).
    #[serde(default)]
    pub effect: Option<String>,
    /// Triggered-only skill executed AT the victim on the clock (periodic damage via the full
    /// combat path — the `firebolt_explosion` pattern).
    #[serde(default)]
    pub tick_skill: Option<String>,
    #[serde(default = "default_rehit_interval")]
    pub rehit_interval: f32,
    #[serde(default)]
    pub on_enter_only: bool,
}

fn default_standing_filter() -> StandingFilter {
    StandingFilter::Enemies
}
fn default_rehit_interval() -> f32 {
    0.5
}

/// A reaction to a hitbox contacting this surface: if the contacting skill's rules `tags`
/// intersect `tags_any`, execute `trigger_skill` at the contact point (attributed to the
/// CONTACTING caster, spec D8), optionally consuming the touched patch.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactReaction {
    pub tags_any: Vec<String>,
    pub trigger_skill: String,
    #[serde(default)]
    pub consume: bool,
}

/// Sim-inert visual hints for the host renderer (decal + looping vfx per patch, spec §6/D10).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceVisuals {
    #[serde(default)]
    pub decal: Option<String>,
    #[serde(default)]
    pub color: Option<[f32; 4]>,
    #[serde(default)]
    pub vfx: Option<String>,
}

/// Registry of loaded surface types, keyed by id. Inserted by
/// `ObeliskConfigExt::add_obelisk_surfaces`; defaulted (empty) by `ObeliskSurfacesPlugin` so
/// surface systems are safe no-ops in apps that never load surface content.
#[derive(Resource, Default)]
pub struct SurfaceRegistry(pub HashMap<String, SurfaceType>);

/// Load every `*.toml` in `dir` into a surface-type map, validating:
/// - unique ids (and non-empty),
/// - numeric sanity (`lifetime > 0`, `merge_radius >= 0`, `max_patches >= 1`,
///   `patch_radius > 0`, `rehit_interval > 0`, non-empty `tags_any`),
/// - `standing.effect` exists in the stat_core effect registry (only checkable when the global
///   registry is initialized — callers should `add_obelisk_effects` first),
/// - `standing.tick_skill` + every `on_skill_contact.trigger_skill` exist in `skills` (only
///   checkable when `Some` — callers should `add_obelisk_skills` first). Timeline HANDLES are
///   deliberately NOT validated here (they load async after skills — same load-order reasoning
///   as `combat::system::partition_conditions`); `execute_skill_timeline` warns at runtime.
pub fn load_surfaces_dir(
    dir: &Path,
    skills: Option<&crate::core::config::SkillRegistry>,
) -> Result<HashMap<String, SurfaceType>, String> {
    let mut map: HashMap<String, SurfaceType> = HashMap::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir:?}: {e}"))?;
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort(); // deterministic load + error order
    for path in paths {
        let src =
            std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
        let st: SurfaceType =
            toml::from_str(&src).map_err(|e| format!("parse {path:?}: {e}"))?;
        if st.id.is_empty() {
            return Err(format!("{path:?}: surface id must be non-empty"));
        }
        if st.lifetime <= 0.0 {
            return Err(format!("surface '{}': lifetime must be > 0", st.id));
        }
        if st.merge_radius < 0.0 {
            return Err(format!("surface '{}': merge_radius must be >= 0", st.id));
        }
        if st.max_patches == 0 {
            return Err(format!("surface '{}': max_patches must be >= 1", st.id));
        }
        if st.patch_radius <= 0.0 {
            return Err(format!("surface '{}': patch_radius must be > 0", st.id));
        }
        if let Some(standing) = &st.standing {
            if standing.rehit_interval <= 0.0 {
                return Err(format!("surface '{}': rehit_interval must be > 0", st.id));
            }
            if let Some(eff) = &standing.effect {
                if stat_core::config::effect_registry_initialized()
                    && stat_core::effect_registry().get(eff).is_none()
                {
                    return Err(format!(
                        "surface '{}': standing.effect '{eff}' is not a registered effect \
                         (no_such_effect check)",
                        st.id
                    ));
                }
            }
            if let (Some(ts), Some(reg)) = (&standing.tick_skill, skills) {
                if !reg.0.contains_key(ts) {
                    return Err(format!(
                        "surface '{}': standing.tick_skill '{ts}' is not a registered skill",
                        st.id
                    ));
                }
            }
        }
        for r in &st.on_skill_contact {
            if r.tags_any.is_empty() {
                return Err(format!(
                    "surface '{}': on_skill_contact.tags_any must be non-empty",
                    st.id
                ));
            }
            if let Some(reg) = skills {
                if !reg.0.contains_key(&r.trigger_skill) {
                    return Err(format!(
                        "surface '{}': on_skill_contact.trigger_skill '{}' is not a registered \
                         skill",
                        st.id, r.trigger_skill
                    ));
                }
            }
        }
        if map.insert(st.id.clone(), st).is_some() {
            return Err(format!("duplicate surface id in {dir:?}"));
        }
    }
    Ok(map)
}
```

`src/core/config.rs` — add to the `ObeliskConfigExt` trait declaration (after
`add_obelisk_skills`):
```rust
    /// Load `config/surfaces/*.toml` into a [`crate::surfaces::SurfaceRegistry`] resource.
    /// Call AFTER `add_obelisk_effects` + `add_obelisk_skills` so effect/skill refs validate;
    /// without them the refs are accepted with a warn (registry/skills not yet available).
    fn add_obelisk_surfaces(&mut self, dir: &Path) -> &mut Self;
```
and to the `impl ObeliskConfigExt for App` block:
```rust
    fn add_obelisk_surfaces(&mut self, dir: &Path) -> &mut Self {
        let map = {
            let skills = self.world().get_resource::<crate::core::config::SkillRegistry>();
            if skills.is_none() {
                warn!(
                    "add_obelisk_surfaces: SkillRegistry not present — tick_skill/trigger_skill \
                     refs will not be validated (call add_obelisk_skills first)"
                );
            }
            crate::surfaces::load_surfaces_dir(dir, skills).expect("load surfaces dir")
        };
        self.insert_resource(crate::surfaces::SurfaceRegistry(map));
        self
    }
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --test surfaces 2>&1 | tail -8`
Expected: `3 passed`.

- [ ] **Step 6: Full-suite sanity + commit**

```bash
cargo test 2>&1 | tail -5
git add -A && git commit -m "feat(surfaces): surface-type content schema, registry, loader, add_obelisk_surfaces

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `SurfacePatch` entity, paint helper (dedup/evict), decay, events, plugin skeleton

**Files:**
- Create: `src/surfaces/patch.rs`
- Modify: `src/surfaces/mod.rs` (plugin + exports), `src/lib.rs` (add plugin to `ObeliskSimPlugin`),
  `src/prelude.rs`, `src/testkit.rs` (recorder)
- Test: `tests/surfaces.rs` (append)

**Interfaces:**
- Consumes: Task 1's `SurfaceRegistry`/`SurfaceType`; `crate::core::components::Faction`;
  `crate::ObeliskSet`.
- Produces (later tasks rely on these EXACT names):
  - `SurfacePatch { pub surface: String, pub owner: Entity, pub owner_faction: Faction, pub skill_id: String, pub radius: f32, pub remaining: f32, pub seq: u64 }` (Component; pos rides its `Transform`)
  - `SurfaceSeq(pub u64)` (Resource — deterministic spawn-order counter, the evict key)
  - Events: `SurfacePainted { patch, surface, position, owner }`,
    `SurfaceRemoved { patch, surface, position, reason }`,
    `SurfaceRemoveReason { Expired, Consumed, Evicted }`,
    `PaintSurface { surface, position, owner }` (public paint REQUEST — tests now, editor stage tool later)
  - `pub fn patch_contains(patch_pos: Vec3, radius: f32, p: Vec3) -> bool`
  - `pub(crate) fn xz_dist(a: Vec3, b: Vec3) -> f32`
  - `pub(crate) fn try_paint(commands, registry, seq, existing, painted_this_tick, surface_id, position, radius_override, lifetime_override, owner, owner_faction, skill_id) -> Option<Entity>` (exact signature in Step 3)
  - `pub fn decay_surfaces(...)` system; `pub fn on_paint_surface(...)` observer
  - `ObeliskSurfacesPlugin` (added inside `ObeliskSimPlugin`)

- [ ] **Step 1: Write the failing tests** (append to `tests/surfaces.rs`)

```rust
use bevy::prelude::*;
use obelisk_bevy::surfaces::{
    PaintSurface, SurfacePatch, SurfaceRemoveReason,
};
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

/// Test app with the surface fixtures registered.
fn surf_app(seed: u64) -> ObeliskTestApp {
    let mut t = ObeliskTestApp::new(seed);
    t.app
        .add_obelisk_surfaces(Path::new("tests/fixtures/surfaces"));
    t
}

fn spawn_combatant(
    t: &mut ObeliskTestApp,
    id: &str,
    pos: Vec3,
    faction: obelisk_bevy::prelude::Faction,
) -> Entity {
    let mut block = StatBlock::with_id(id);
    block.max_life.base = 200.0;
    block.current_life = 200.0;
    block.max_mana.base = 100.0;
    block.current_mana = 100.0;
    t.app
        .world_mut()
        .spawn((
            obelisk_bevy::prelude::Combatant,
            obelisk_bevy::prelude::Attributes(block),
            faction,
            obelisk_bevy::prelude::ObeliskId(id.into()),
            Transform::from_translation(pos),
        ))
        .id()
}

fn patch_count(t: &mut ObeliskTestApp, surface: &str) -> usize {
    let mut q = t.app.world_mut().query::<&SurfacePatch>();
    q.iter(t.app.world()).filter(|p| p.surface == surface).count()
}

#[test]
fn paint_request_spawns_a_patch_and_dedups() {
    let mut t = surf_app(1);
    let owner = spawn_combatant(&mut t, "painter", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    t.app.update();
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(2.0, 0.0, 0.0),
        owner,
    });
    t.app.world_mut().flush();
    t.app.update();
    assert_eq!(patch_count(&mut t, "frost"), 1);
    assert_eq!(t.rec().surfaces_painted.len(), 1);
    assert_eq!(t.rec().surfaces_painted[0].surface, "frost");
    // A second paint within merge_radius (0.25 default) dedups — still one patch.
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(2.1, 0.0, 0.0),
        owner,
    });
    t.app.world_mut().flush();
    t.app.update();
    assert_eq!(patch_count(&mut t, "frost"), 1, "merge_radius dedup");
    // But a paint farther away spawns a second patch.
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner,
    });
    t.app.world_mut().flush();
    t.app.update();
    assert_eq!(patch_count(&mut t, "frost"), 2);
}

#[test]
fn patches_expire_and_evict_oldest_at_cap() {
    let mut t = surf_app(1);
    let owner = spawn_combatant(&mut t, "painter", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    t.app.update();
    // dew lifetime = 0.3s -> gone within ~30 ticks, with an Expired removal event.
    t.app.world_mut().trigger(PaintSurface {
        surface: "dew".into(),
        position: Vec3::new(1.0, 0.0, 1.0),
        owner,
    });
    t.app.world_mut().flush();
    t.advance_ticks(30);
    assert_eq!(patch_count(&mut t, "dew"), 0, "dew expired");
    assert!(t
        .rec()
        .surfaces_removed
        .iter()
        .any(|r| r.surface == "dew" && r.reason == SurfaceRemoveReason::Expired));
    // capped max_patches = 3: painting 5 distinct spots keeps the NEWEST 3 (oldest evicted).
    for i in 0..5 {
        t.app.world_mut().trigger(PaintSurface {
            surface: "capped".into(),
            position: Vec3::new(i as f32 * 2.0, 0.0, 5.0),
            owner,
        });
        t.app.world_mut().flush();
        t.app.update();
    }
    assert_eq!(patch_count(&mut t, "capped"), 3);
    let evicted: Vec<_> = t
        .rec()
        .surfaces_removed
        .iter()
        .filter(|r| r.surface == "capped" && r.reason == SurfaceRemoveReason::Evicted)
        .collect();
    assert_eq!(evicted.len(), 2, "two oldest evicted");
    // The SURVIVING patches are the three newest (x = 4, 6, 8).
    let mut q = t.app.world_mut().query::<(&SurfacePatch, &Transform)>();
    let mut xs: Vec<f32> = q
        .iter(t.app.world())
        .filter(|(p, _)| p.surface == "capped")
        .map(|(_, tf)| tf.translation.x)
        .collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs, vec![4.0, 6.0, 8.0]);
}

#[test]
fn unknown_surface_paint_is_a_warn_not_a_panic() {
    let mut t = surf_app(1);
    let owner = spawn_combatant(&mut t, "painter", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    t.app.update();
    t.app.world_mut().trigger(PaintSurface {
        surface: "no_such_surface".into(),
        position: Vec3::ZERO,
        owner,
    });
    t.app.world_mut().flush();
    t.app.update(); // must not panic
    assert_eq!(t.rec().surfaces_painted.len(), 0);
}
```

Also add `use std::path::Path;` is already imported at the top of the file (Task 1).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test surfaces 2>&1 | tail -10`
Expected: COMPILE FAIL — `PaintSurface`/`SurfacePatch` unresolved; `surfaces_painted` missing on
`EventRecorder`.

- [ ] **Step 3: Implement**

`src/surfaces/patch.rs`:
```rust
//! Surface PATCH: one painted circle splat as a plain sim entity (spec D6 — entities so the
//! editor previews them natively and the arena replicates them like skill objects). Positions
//! ride `Transform`; expiry is a fixed-tick countdown (`remaining`, the hitbox pattern);
//! `seq` (from [`SurfaceSeq`]) is the deterministic replace-oldest eviction key.
use bevy::prelude::*;

use crate::core::components::Faction;
use crate::surfaces::types::{SurfaceRegistry, SURFACE_Y_TOLERANCE};

#[derive(Component, Debug, Clone)]
pub struct SurfacePatch {
    pub surface: String,
    pub owner: Entity,
    /// Snapshot of the painter's faction at paint time (the standing filter's reference frame;
    /// survives the painter despawning; patches are cleared on round reset host-side anyway).
    pub owner_faction: Faction,
    /// The skill whose window painted this (empty for direct `PaintSurface` requests).
    pub skill_id: String,
    pub radius: f32,
    /// Seconds until expiry (fixed-tick countdown).
    pub remaining: f32,
    /// Deterministic spawn ordinal — the eviction ("oldest") key and iteration sort key.
    pub seq: u64,
}

/// Monotonic patch spawn counter (deterministic — never wall clock).
#[derive(Resource, Default)]
pub struct SurfaceSeq(pub u64);

#[derive(Event, Clone, Debug)]
pub struct SurfacePainted {
    pub patch: Entity,
    pub surface: String,
    pub position: Vec3,
    pub owner: Entity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceRemoveReason {
    Expired,
    Consumed,
    Evicted,
}

#[derive(Event, Clone, Debug)]
pub struct SurfaceRemoved {
    pub patch: Entity,
    pub surface: String,
    pub position: Vec3,
    pub reason: SurfaceRemoveReason,
}

/// PUBLIC paint request: spawn a patch of `surface` at `position` owned by `owner`, using the
/// type's default radius/lifetime. The seam tests and the editor's stage paint tool use;
/// windows paint through their authored `PaintSpec` instead (Task 4).
#[derive(Event, Clone, Debug)]
pub struct PaintSurface {
    pub surface: String,
    pub position: Vec3,
    pub owner: Entity,
}

pub(crate) fn xz_dist(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// The 2.5D membership test: XZ within `radius`, Y within [`SURFACE_Y_TOLERANCE`].
pub fn patch_contains(patch_pos: Vec3, radius: f32, p: Vec3) -> bool {
    xz_dist(patch_pos, p) <= radius && (p.y - patch_pos.y).abs() <= SURFACE_Y_TOLERANCE
}

/// Spawn one patch, enforcing the type's `merge_radius` dedup and `max_patches` replace-oldest
/// cap. `painted_this_tick` self-dedups paints queued in the SAME tick (deferred spawns aren't
/// visible in `existing` yet); the cap is enforced against COMMITTED patches, so a burst tick
/// can transiently overshoot by its own paint count — it converges on the next paint (documented
/// v1 behavior). Returns the spawned entity, or `None` when deduped/unknown.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_paint(
    commands: &mut Commands,
    registry: &SurfaceRegistry,
    seq: &mut SurfaceSeq,
    existing: &Query<(Entity, &SurfacePatch, &Transform)>,
    painted_this_tick: &mut Vec<(String, Vec3)>,
    surface_id: &str,
    position: Vec3,
    radius_override: Option<f32>,
    lifetime_override: Option<f32>,
    owner: Entity,
    owner_faction: Faction,
    skill_id: &str,
) -> Option<Entity> {
    let Some(st) = registry.0.get(surface_id) else {
        warn!("try_paint: unknown surface '{surface_id}' — skipping (check config/surfaces)");
        return None;
    };
    // Dedup: an existing same-type patch (or one queued this tick) within merge_radius.
    let near_existing = existing
        .iter()
        .any(|(_, p, tf)| p.surface == surface_id && xz_dist(tf.translation, position) < st.merge_radius);
    let near_batch = painted_this_tick
        .iter()
        .any(|(s, pos)| s == surface_id && xz_dist(*pos, position) < st.merge_radius);
    if near_existing || near_batch {
        return None;
    }
    // Replace-oldest cap (committed patches only — see fn doc).
    let mut same: Vec<(Entity, u64, Vec3, String)> = existing
        .iter()
        .filter(|(_, p, _)| p.surface == surface_id)
        .map(|(e, p, tf)| (e, p.seq, tf.translation, p.surface.clone()))
        .collect();
    if same.len() + 1 > st.max_patches {
        same.sort_by_key(|(_, s, _, _)| *s);
        for (e, _, pos, surf) in same.iter().take(same.len() + 1 - st.max_patches) {
            commands.trigger(SurfaceRemoved {
                patch: *e,
                surface: surf.clone(),
                position: *pos,
                reason: SurfaceRemoveReason::Evicted,
            });
            commands.entity(*e).despawn();
        }
    }
    seq.0 += 1;
    let patch = commands
        .spawn((
            SurfacePatch {
                surface: surface_id.to_string(),
                owner,
                owner_faction,
                skill_id: skill_id.to_string(),
                radius: radius_override.unwrap_or(st.patch_radius),
                remaining: lifetime_override.unwrap_or(st.lifetime),
                seq: seq.0,
            },
            Transform::from_translation(position),
        ))
        .id();
    commands.trigger(SurfacePainted {
        patch,
        surface: surface_id.to_string(),
        position,
        owner,
    });
    painted_this_tick.push((surface_id.to_string(), position));
    Some(patch)
}

/// Observer for the public [`PaintSurface`] request.
pub fn on_paint_surface(
    ev: On<PaintSurface>,
    registry: Res<SurfaceRegistry>,
    mut seq: ResMut<SurfaceSeq>,
    existing: Query<(Entity, &SurfacePatch, &Transform)>,
    factions: Query<&Faction>,
    mut commands: Commands,
) {
    let e = ev.event();
    let owner_faction = factions.get(e.owner).copied().unwrap_or_default();
    let mut batch = Vec::new();
    try_paint(
        &mut commands,
        &registry,
        &mut seq,
        &existing,
        &mut batch,
        &e.surface,
        e.position,
        None,
        None,
        e.owner,
        owner_faction,
        "",
    );
}

/// Fixed-tick patch expiry.
pub fn decay_surfaces(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut SurfacePatch, &Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut p, tf) in &mut q {
        p.remaining -= dt;
        if p.remaining <= 0.0 {
            commands.trigger(SurfaceRemoved {
                patch: e,
                surface: p.surface.clone(),
                position: tf.translation,
                reason: SurfaceRemoveReason::Expired,
            });
            commands.entity(e).despawn();
        }
    }
}
```

`src/surfaces/mod.rs` — replace the file with:
```rust
//! Surfaces (ground effects): persistent, typed, paintable ground state — spec
//! `obelisk-arena/docs/superpowers/specs/2026-07-09-surfaces-ground-effects-design.md`.
//! A surface TYPE is content (`config/surfaces/<id>.toml` → [`SurfaceRegistry`], loaded via
//! `ObeliskConfigExt::add_obelisk_surfaces`); a surface PATCH is a sim entity. Skills interact
//! through paint (window `paints`), require/consume (acquisition `on_surface`), and react (the
//! surface's own `standing` payload + `on_skill_contact` reactions). A surface never deals
//! damage itself — it applies effects or executes a triggered-only skill, so everything rides
//! the existing combat path.
//!
//! DATA vs BEHAVIOR split: `add_obelisk_surfaces` loads the registry (any host — including a
//! render-only client that just needs `[visuals]`); [`ObeliskSurfacesPlugin`] adds the systems
//! (sim hosts: server, editor preview) and is included in `ObeliskSimPlugin`.
use bevy::prelude::*;

pub mod patch;
pub mod types;

pub use patch::{
    decay_surfaces, on_paint_surface, patch_contains, PaintSurface, SurfacePainted, SurfacePatch,
    SurfaceRemoveReason, SurfaceRemoved, SurfaceSeq,
};
pub use types::{
    load_surfaces_dir, ContactReaction, StandingFilter, StandingPayload, SurfaceRegistry,
    SurfaceType, SurfaceVisuals, SURFACE_MATCH_SLACK, SURFACE_Y_TOLERANCE,
};

pub struct ObeliskSurfacesPlugin;
impl Plugin for ObeliskSurfacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfaceRegistry>()
            .init_resource::<SurfaceSeq>();
        app.add_systems(
            bevy::app::FixedUpdate,
            decay_surfaces.in_set(crate::ObeliskSet::Advance),
        );
        app.add_observer(on_paint_surface);
    }
}
```

`src/lib.rs` — inside `ObeliskSimPlugin::build`, after `.add_plugins(loot::ObeliskLootPlugin);`:
```rust
        app.add_plugins(surfaces::ObeliskSurfacesPlugin);
```

`src/prelude.rs` — add:
```rust
pub use crate::surfaces::{
    ObeliskSurfacesPlugin, PaintSurface, SurfacePainted, SurfacePatch, SurfaceRegistry,
    SurfaceRemoveReason, SurfaceRemoved,
};
```
(If `ObeliskSurfacesPlugin` is not re-exported from `crate::surfaces` root, export it — it is
defined in `surfaces/mod.rs` so `crate::surfaces::ObeliskSurfacesPlugin` resolves.)

`src/testkit.rs` — add to `EventRecorder`:
```rust
    pub surfaces_painted: Vec<crate::surfaces::SurfacePainted>,
    pub surfaces_removed: Vec<crate::surfaces::SurfaceRemoved>,
```
and to `EventRecorderPlugin::build`:
```rust
        app.add_observer(
            |e: On<crate::surfaces::SurfacePainted>, mut r: ResMut<EventRecorder>| {
                r.surfaces_painted.push(e.event().clone())
            },
        );
        app.add_observer(
            |e: On<crate::surfaces::SurfaceRemoved>, mut r: ResMut<EventRecorder>| {
                r.surfaces_removed.push(e.event().clone())
            },
        );
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --test surfaces 2>&1 | tail -8`
Expected: `6 passed`.

- [ ] **Step 5: Full suite + commit**

```bash
cargo test 2>&1 | tail -5
git add -A && git commit -m "feat(surfaces): SurfacePatch entity, paint/dedup/evict/decay, events, plugin

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: `paints` window schema field (`PaintSpec` / `PaintMode`)

**Files:**
- Modify: `src/assets/mod.rs`
- Test: `src/assets/mod.rs` `#[cfg(test)]` module (unit tests live beside the schema, crate
  convention)

**Interfaces:**
- Produces (Task 4 relies on):
  - `assets::PaintSpec { pub surface: String, pub radius: f32, pub mode: PaintMode, pub lifetime: Option<f32> }`
  - `assets::PaintMode::{ Trail { step: f32 }, OnEnd }`
  - `CollisionWindow.paints: Option<PaintSpec>` (`#[serde(default)]`)
  - `validate_timeline` rejects `radius <= 0`, `Trail { step <= 0 }`, empty `surface`.

- [ ] **Step 1: Write the failing tests** (append inside `src/assets/mod.rs` `mod tests`)

```rust
    /// (Surfaces) `paints` round-trips, defaults to None, and validates numeric sanity.
    #[test]
    fn paints_field_round_trips_and_defaults() {
        let src = r#"(
            skill_id: "painter",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            collision_windows: [
                ( id: "roll", spawn: Scheduled( phase: Active ), active_duration: 1.0,
                  shape: Sphere( radius: 0.3 ), motion: Linear( speed: 8.0 ),
                  hit_filter: Enemies, hit_mode: OncePerTarget,
                  paints: Some(( surface: "frost", radius: 0.45, mode: Trail( step: 0.8 ) )) ),
            ],
        )"#;
        let tl: CastTimeline = ron::from_str(src).expect("paints content parses");
        let paints = tl.collision_windows[0].paints.as_ref().expect("paints present");
        assert_eq!(paints.surface, "frost");
        assert_eq!(paints.radius, 0.45);
        assert!(matches!(paints.mode, PaintMode::Trail { step } if step == 0.8));
        assert!(paints.lifetime.is_none());
        validate_timeline(&tl).expect("valid paints validates");
        // Round-trip.
        let s = ron::ser::to_string_pretty(&tl, Default::default()).unwrap();
        let back: CastTimeline = ron::from_str(&s).unwrap();
        assert!(back.collision_windows[0].paints.is_some());
        // Omitted -> None (every existing .cast.ron parses unchanged).
        let plain = timeline_with(basic_window("w"));
        assert!(plain.collision_windows[0].paints.is_none());
    }

    /// (Surfaces) invalid paints fail validation loudly.
    #[test]
    fn paints_validation_rejects_bad_values() {
        let mut win = basic_window("w");
        win.paints = Some(PaintSpec {
            surface: "frost".into(),
            radius: 0.0,
            mode: PaintMode::OnEnd,
            lifetime: None,
        });
        assert!(validate_timeline(&timeline_with(win)).is_err(), "radius must be > 0");

        let mut win = basic_window("w");
        win.paints = Some(PaintSpec {
            surface: "frost".into(),
            radius: 0.4,
            mode: PaintMode::Trail { step: 0.0 },
            lifetime: None,
        });
        assert!(validate_timeline(&timeline_with(win)).is_err(), "trail step must be > 0");

        let mut win = basic_window("w");
        win.paints = Some(PaintSpec {
            surface: "".into(),
            radius: 0.4,
            mode: PaintMode::OnEnd,
            lifetime: None,
        });
        assert!(validate_timeline(&timeline_with(win)).is_err(), "surface must be non-empty");
    }
```
Note: `basic_window` builds a full struct literal — it gains a `paints: None` field in Step 3
(compile guides you).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p obelisk-bevy --lib assets 2>&1 | tail -5`
Expected: COMPILE FAIL — no `paints` field / `PaintSpec` unresolved.

- [ ] **Step 3: Implement** (in `src/assets/mod.rs`)

After the `Emitter` struct definition, add:
```rust
/// Authored surface painting for a window (spec §5.1): while this window's hitbox is alive
/// (`Trail`) or when it ends (`OnEnd`), paint patches of `surface`. Painting is a window
/// PROPERTY (not a child window), so it composes with emitters without inheriting the parent
/// skill's lifecycle triggers — the exact trap that forced the arena's tile-drop poller.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintSpec {
    /// Surface-type id (resolved against the host's `SurfaceRegistry` at runtime; unknown ids
    /// warn-and-skip — editor validation is the blocking check, same split as cue effects).
    pub surface: String,
    /// Painted patch radius (world units).
    pub radius: f32,
    pub mode: PaintMode,
    /// Per-paint lifetime override (secs); `None` = the surface type's default.
    #[serde(default)]
    pub lifetime: Option<f32>,
}

/// When a painting window actually paints.
#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq)]
pub enum PaintMode {
    /// Paint every `step` meters of hitbox travel (the glacier-trail shape). Also paints once
    /// immediately at spawn.
    Trail { step: f32 },
    /// Paint once at the hitbox's end position, whatever the end reason (shards, blasts).
    OnEnd,
}
```

In `CollisionWindow`, after the `emitter` field:
```rust
    /// Paint surface patches while alive / on end (spec §5.1) — see [`PaintSpec`]. `None` (the
    /// default) = no painting; every pre-surfaces window omits this and is unaffected.
    #[serde(default)]
    pub paints: Option<PaintSpec>,
```

In `validate_timeline`, inside the existing `for w in &tl.collision_windows` loop that checks
anchors (the second loop), add:
```rust
        if let Some(paints) = &w.paints {
            if paints.surface.is_empty() {
                return Err(format!("window '{}' paints an empty surface id", w.id));
            }
            if paints.radius <= 0.0 {
                return Err(format!(
                    "window '{}' paints radius must be > 0, got {}",
                    w.id, paints.radius
                ));
            }
            if let PaintMode::Trail { step } = paints.mode {
                if step <= 0.0 {
                    return Err(format!(
                        "window '{}' paints Trail step must be > 0, got {step}",
                        w.id
                    ));
                }
            }
            if let Some(lt) = paints.lifetime {
                if lt <= 0.0 {
                    return Err(format!(
                        "window '{}' paints lifetime override must be > 0, got {lt}",
                        w.id
                    ));
                }
            }
        }
```

In `ObeliskAssetsPlugin::build`, extend the `register_type` chain:
```rust
            .register_type::<PaintSpec>()
            .register_type::<PaintMode>()
```

Fix the two struct literals that construct `CollisionWindow` in tests (`basic_window`) and in
`src/facade/combat.rs` (~line 359 block) by adding `paints: None,`. Compile errors will point at
every construction site — add `paints: None,` to each.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p obelisk-bevy --lib 2>&1 | tail -5` then `cargo test 2>&1 | tail -5`
Expected: PASS everywhere; **goldens untouched** (`cargo test --test golden` green).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(surfaces): authored paints field on CollisionWindow (Trail/OnEnd) + validation

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Painting systems — Trail painter + OnEnd observer

**Files:**
- Create: `src/surfaces/systems.rs`
- Modify: `src/surfaces/mod.rs` (module + exports + plugin registration)
- Create: `tests/fixtures/cast/paint_roller.cast.ron`, `tests/fixtures/cast/paint_blast.cast.ron`
- Test: `tests/surfaces.rs` (append)

**Interfaces:**
- Consumes: Task 2's `try_paint`/`SurfacePatch`/`SurfaceSeq`; Task 3's `PaintSpec`/`PaintMode`;
  `crate::assets::{CastTimeline, CastTimelineHandles}`; `crate::spatial::boxes::Hitbox`;
  `crate::events::HitboxEnded`; `crate::core::components::Faction`.
- Produces:
  - `systems::TrailPainted { pub last: Vec3 }` (Component, on hitbox entities)
  - `pub fn paint_surfaces(...)` system (registered in `ResolveHits`, before `detect_overlaps`)
  - `pub fn on_hitbox_ended_paint(...)` observer

- [ ] **Step 1: Write the fixtures**

`tests/fixtures/cast/paint_roller.cast.ron`:
```ron
( skill_id: "paint_roller",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "roll", spawn: Scheduled( phase: Active, offset: 0.0 ), active_duration: 1.0,
      shape: Sphere( radius: 0.3 ), motion: Linear( speed: 8.0 ),
      hit_filter: Enemies, hit_mode: OncePerTarget,
      paints: Some(( surface: "frost", radius: 0.45, mode: Trail( step: 0.5 ) )) ),
  ],
)
```
`tests/fixtures/cast/paint_blast.cast.ron`:
```ron
( skill_id: "paint_blast",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "blast", spawn: Scheduled( phase: Active, offset: 0.0 ), active_duration: 0.2,
      shape: Sphere( radius: 1.0 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget,
      paints: Some(( surface: "burning", radius: 1.0, mode: OnEnd, lifetime: Some(3.0) )) ),
  ],
)
```

- [ ] **Step 2: Write the failing tests** (append to `tests/surfaces.rs`)

```rust
use obelisk_bevy::prelude::CastTimeline;

/// Load a `.cast.ron` fixture and register its handle (the run_scenario pattern).
fn load_timeline(t: &mut ObeliskTestApp, skill_id: &str, path: &str) {
    let h: bevy::asset::Handle<CastTimeline> = t
        .app
        .world()
        .resource::<bevy::asset::AssetServer>()
        .load(path.to_string());
    let mut loaded = false;
    for _ in 0..2000 {
        t.app.update();
        if t.app
            .world()
            .resource::<bevy::asset::Assets<CastTimeline>>()
            .get(&h)
            .is_some()
        {
            loaded = true;
            break;
        }
    }
    assert!(loaded, "timeline asset {path} loaded");
    t.app
        .world_mut()
        .resource_mut::<obelisk_bevy::prelude::CastTimelineHandles>()
        .0
        .insert(skill_id.to_string(), h);
}

fn grant_and_cast_dir(t: &mut ObeliskTestApp, caster: Entity, skill: &str, dir: Vec3) {
    use obelisk_bevy::timeline::cast::CastSkillExt;
    use obelisk_bevy::verbs::ObeliskCommandsExt;
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .grant_skill(skill)
        .cast_skill_dir(skill, bevy::math::Dir3::new(dir).unwrap());
    t.app.world_mut().flush();
}

#[test]
fn trail_window_paints_spaced_patches_along_its_flight() {
    let mut t = surf_app(2);
    let caster = spawn_combatant(&mut t, "roller", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "paint_roller", "tests/fixtures/cast/paint_roller.cast.ron");
    grant_and_cast_dir(&mut t, caster, "paint_roller", Vec3::X);
    // active_duration 1.0s at 8 m/s = ~8 m of travel; step 0.5 -> ~16 patches; merge_radius
    // 0.25 < step so dedup never blocks the trail. Run 90 ticks (cast + full flight + end).
    t.advance_ticks(90);
    let painted = t.rec().surfaces_painted.len();
    assert!(
        (12..=20).contains(&painted),
        "trail paints ~16 spaced patches, got {painted}"
    );
    // Spacing: consecutive paint positions are >= 0.5 - epsilon apart in X.
    let xs: Vec<f32> = t.rec().surfaces_painted.iter().map(|p| p.position.x).collect();
    for w in xs.windows(2) {
        assert!(w[1] - w[0] >= 0.45, "trail spacing ~step: {:?}", xs);
    }
}

#[test]
fn on_end_window_paints_once_at_the_end_position_with_lifetime_override() {
    let mut t = surf_app(3);
    let caster = spawn_combatant(&mut t, "blaster", Vec3::new(5.0, 0.0, 5.0), obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "paint_blast", "tests/fixtures/cast/paint_blast.cast.ron");
    grant_and_cast_dir(&mut t, caster, "paint_blast", Vec3::X);
    t.advance_ticks(30); // windup 0 + active window fuse 0.2s -> ends, paints once
    assert_eq!(t.rec().surfaces_painted.len(), 1, "OnEnd paints exactly once");
    let p = &t.rec().surfaces_painted[0];
    assert_eq!(p.surface, "burning");
    assert!(
        (p.position - Vec3::new(5.0, 0.0, 5.0)).length() < 0.01,
        "static window ends at its spawn position, got {:?}",
        p.position
    );
    // lifetime override 3.0 (not burning's 8.0): patch persists at 2.5s, gone by 3.5s.
    t.advance_ticks(120); // ~2.0s more (total ~2.5s since paint)
    assert_eq!(patch_count(&mut t, "burning"), 1);
    t.advance_ticks(90); // ~1.5s more (total ~4.0s)
    assert_eq!(patch_count(&mut t, "burning"), 0, "override lifetime expired");
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --test surfaces trail_window 2>&1 | tail -8`
Expected: FAIL — 0 patches painted (systems don't exist yet; compile succeeds since Tasks 1–3
exported everything these tests import — if not, fix exports first).

- [ ] **Step 4: Implement**

`src/surfaces/systems.rs`:
```rust
//! Surface behavior systems: the Trail painter, the OnEnd paint observer (Task 4), standing
//! payloads (Task 5), and skill-contact triggers (Task 6). DETERMINISM: every loop that fires
//! paints/triggers iterates in sorted order (hitboxes by `Entity::index()`, patches by `seq`);
//! nothing here draws RNG.
use bevy::prelude::*;

use crate::assets::{CastTimeline, CastTimelineHandles, PaintMode, PaintSpec};
use crate::core::components::Faction;
use crate::events::HitboxEnded;
use crate::spatial::boxes::Hitbox;
use crate::surfaces::patch::{try_paint, SurfacePatch, SurfaceSeq};
use crate::surfaces::types::SurfaceRegistry;

/// Trail bookkeeping on a painting hitbox: where its last splat landed. Inserted (deferred) on
/// the first paint; the first paint itself happens the first tick the hitbox is seen.
#[derive(Component, Debug)]
pub struct TrailPainted {
    pub last: Vec3,
}

/// Resolve a live hitbox's authored `PaintSpec` (the `tick_emitters` lookup pattern).
fn window_paints<'a>(
    handles: &CastTimelineHandles,
    timelines: &'a Assets<CastTimeline>,
    skill_id: &str,
    window_id: &str,
) -> Option<&'a PaintSpec> {
    let h = handles.0.get(skill_id)?;
    let tl = timelines.get(h)?;
    tl.collision_windows
        .iter()
        .find(|w| w.id == window_id)?
        .paints
        .as_ref()
}

/// Trail-mode painting: every live hitbox whose window authors `paints: Trail(step)` paints a
/// patch at spawn and then every `step` meters of actual travel (full 3D distance — matches the
/// arena poller it replaces). Runs in `ResolveHits` BEFORE `detect_overlaps` (position is
/// post-`move_projectiles` for this tick).
pub fn paint_surfaces(
    mut commands: Commands,
    registry: Res<SurfaceRegistry>,
    mut seq: ResMut<SurfaceSeq>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    factions: Query<&Faction>,
    mut hitboxes: Query<(Entity, &Hitbox, &Transform, Option<&mut TrailPainted>)>,
    existing: Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>,
) {
    let mut batch: Vec<(String, Vec3)> = Vec::new();
    let mut sorted: Vec<_> = hitboxes.iter_mut().collect();
    sorted.sort_by_key(|(e, _, _, _)| e.index());
    for (e, hb, tf, trail) in sorted {
        let Some(paints) = window_paints(&handles, &timelines, &hb.skill_id, &hb.window_id)
        else {
            continue;
        };
        let PaintMode::Trail { step } = paints.mode else {
            continue;
        };
        let pos = tf.translation;
        let owner_faction = factions.get(hb.caster).copied().unwrap_or_default();
        match trail {
            None => {
                try_paint(
                    &mut commands,
                    &registry,
                    &mut seq,
                    &existing,
                    &mut batch,
                    &paints.surface,
                    pos,
                    Some(paints.radius),
                    paints.lifetime,
                    hb.caster,
                    owner_faction,
                    &hb.skill_id,
                );
                commands.entity(e).insert(TrailPainted { last: pos });
            }
            Some(mut t) => {
                if (pos - t.last).length() >= step {
                    try_paint(
                        &mut commands,
                        &registry,
                        &mut seq,
                        &existing,
                        &mut batch,
                        &paints.surface,
                        pos,
                        Some(paints.radius),
                        paints.lifetime,
                        hb.caster,
                        owner_faction,
                        &hb.skill_id,
                    );
                    t.last = pos;
                }
            }
        }
    }
}

/// OnEnd painting: hooks the existing termination funnel's event — paints once at the end
/// position, whatever the reason (enemy / world / fuse).
pub fn on_hitbox_ended_paint(
    ev: On<HitboxEnded>,
    registry: Res<SurfaceRegistry>,
    mut seq: ResMut<SurfaceSeq>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    factions: Query<&Faction>,
    existing: Query<(Entity, &SurfacePatch, &Transform)>,
    mut commands: Commands,
) {
    let e = ev.event();
    let Some(paints) = window_paints(&handles, &timelines, &e.skill_id, &e.window_id) else {
        return;
    };
    if paints.mode != PaintMode::OnEnd {
        return;
    }
    let owner_faction = factions.get(e.caster).copied().unwrap_or_default();
    let mut batch = Vec::new();
    try_paint(
        &mut commands,
        &registry,
        &mut seq,
        &existing,
        &mut batch,
        &paints.surface,
        e.position,
        Some(paints.radius),
        paints.lifetime,
        e.caster,
        owner_faction,
        &e.skill_id,
    );
}
```

NOTE on the `existing` query in `paint_surfaces`: the `Without<Hitbox>` is required for
disjointness with the `&mut`-bearing hitbox query (patches never carry `Hitbox`, so it filters
nothing). `try_paint` takes `&Query<(Entity, &SurfacePatch, &Transform)>` — adjust `try_paint`'s
signature to be generic-free by giving it the same `Without<Hitbox>` filter type:
change `try_paint`'s `existing` parameter type in `src/surfaces/patch.rs` to
`&Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>` (import
`crate::spatial::boxes::Hitbox`, add `use bevy::ecs::query::Without;` via prelude) and update
`on_paint_surface` + `on_hitbox_ended_paint` to use the same filtered query type (harmless
there).

`src/surfaces/mod.rs` — add module + exports + registration:
```rust
pub mod systems;

pub use systems::{on_hitbox_ended_paint, paint_surfaces, TrailPainted};
```
and in `ObeliskSurfacesPlugin::build`, extend:
```rust
        app.add_systems(
            bevy::app::FixedUpdate,
            paint_surfaces
                .in_set(crate::ObeliskSet::ResolveHits)
                .before(crate::spatial::detect::detect_overlaps),
        );
        app.add_observer(on_hitbox_ended_paint);
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --test surfaces 2>&1 | tail -8`
Expected: `8 passed`.

- [ ] **Step 6: Full suite (goldens!) + commit**

```bash
cargo test 2>&1 | tail -5
git add -A && git commit -m "feat(surfaces): Trail painter + OnEnd paint observer

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Standing payloads (effects + tick skills, enter/exit, per-(victim,type) clocks)

**Files:**
- Modify: `src/surfaces/systems.rs`, `src/surfaces/mod.rs`
- Create: `tests/fixtures/cast/burning_tick.cast.ron`
- Test: `tests/surfaces.rs` (append)

**Interfaces:**
- Consumes: `crate::timeline::triggered::{execute_skill_timeline, ExecPayload}`;
  `crate::spatial::filter::passes_filter`; `crate::verbs::ObeliskCommandsExt`
  (`apply_obelisk_effect`); Task 2's `patch_contains`.
- Produces:
  - `systems::StandingState { pub next_due: HashMap<(Entity, String), f32>, pub inside_prev: HashSet<(Entity, Entity)> }` (Resource)
  - `pub fn apply_standing_payloads(...)` system (chained after `paint_surfaces`, still before
    `detect_overlaps`)

- [ ] **Step 1: Write the fixture**

`tests/fixtures/cast/burning_tick.cast.ron` (the `firebolt_explosion` shape — a triggered-only
instant blast at the payload position):
```ron
( skill_id: "burning_tick",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "tick", spawn: Scheduled( phase: Active, offset: 0.0 ), anchor: CastPoint,
      active_duration: 0.05, shape: Sphere( radius: 0.6 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
  acquisition: SelfPoint,
)
```

- [ ] **Step 2: Write the failing tests** (append to `tests/surfaces.rs`)

```rust
fn damage_events_for(t: &ObeliskTestApp, skill: &str) -> usize {
    t.rec().damage_resolved.iter().filter(|d| d.skill_id == skill).count()
}

#[test]
fn standing_in_burning_ticks_damage_attributed_to_the_painter() {
    let mut t = surf_app(4);
    let painter = spawn_combatant(&mut t, "painter", Vec3::new(-5.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
    let victim = spawn_combatant(&mut t, "victim", Vec3::new(3.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Enemy);
    load_timeline(&mut t, "burning_tick", "tests/fixtures/cast/burning_tick.cast.ron");
    // Hurtbox so the tick skill's blast can actually hit the victim.
    obelisk_bevy::spatial::boxes::insert_hurtbox(
        &mut t.app.world_mut().commands(),
        victim,
        0.5,
        Vec3::new(3.0, 0.0, 0.0),
    );
    t.app.world_mut().flush();
    t.advance_ticks(3); // spatial pipeline sees the fresh static collider (probe note)
    t.app.world_mut().trigger(PaintSurface {
        surface: "burning".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    // rehit_interval 0.2s -> over 1.0s expect ~5 tick executions.
    t.advance_ticks(60);
    let ticks = damage_events_for(&t, "burning_tick");
    assert!(
        (4..=6).contains(&ticks),
        "burning ticks ~5x in 1s, got {ticks}"
    );
    let d = t
        .rec()
        .damage_resolved
        .iter()
        .find(|d| d.skill_id == "burning_tick")
        .unwrap();
    assert_eq!(d.caster, painter, "standing damage attributed to the painter");
    assert_eq!(d.target, victim);
    // Painter (same faction as... no: painter is Player, filter enemies) never self-ticks: no
    // damage against the painter.
    assert!(
        !t.rec().damage_resolved.iter().any(|d| d.target == painter),
        "filter=enemies never ticks the painter's own faction"
    );
}

#[test]
fn standing_effect_applies_to_allies_and_stops_on_exit() {
    let mut t = surf_app(5);
    let painter = spawn_combatant(&mut t, "priest", Vec3::new(0.0, 0.0, -4.0), obelisk_bevy::prelude::Faction::Player);
    let ally = spawn_combatant(&mut t, "ally", Vec3::new(0.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
    t.app.world_mut().trigger(PaintSurface {
        surface: "blessed".into(),
        position: Vec3::ZERO,
        owner: painter,
    });
    t.app.world_mut().flush();
    t.advance_ticks(20);
    let applied = t
        .rec()
        .effect_applied
        .iter()
        .filter(|e| e.effect_id == "empower" && e.target == ally)
        .count();
    assert!(applied >= 1, "ally standing in blessed gets empower");
    // Exit: move the ally away; effect application stops (count freezes).
    t.app
        .world_mut()
        .entity_mut(ally)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(50.0, 0.0, 50.0);
    let before = t.rec().effect_applied.len();
    t.advance_ticks(30);
    assert_eq!(
        t.rec().effect_applied.len(),
        before,
        "no further applications after leaving the surface"
    );
}

#[test]
fn on_enter_only_fires_once_per_visit() {
    let mut t = surf_app(6);
    let painter = spawn_combatant(&mut t, "painter", Vec3::new(-5.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
    let victim = spawn_combatant(&mut t, "victim", Vec3::new(2.0, 0.0, 2.0), obelisk_bevy::prelude::Faction::Enemy);
    load_timeline(&mut t, "burning_tick", "tests/fixtures/cast/burning_tick.cast.ron");
    obelisk_bevy::spatial::boxes::insert_hurtbox(
        &mut t.app.world_mut().commands(),
        victim,
        0.5,
        Vec3::new(2.0, 0.0, 2.0),
    );
    t.app.world_mut().flush();
    t.advance_ticks(3);
    t.app.world_mut().trigger(PaintSurface {
        surface: "mud".into(),
        position: Vec3::new(2.0, 0.0, 2.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    t.advance_ticks(40);
    assert_eq!(
        damage_events_for(&t, "burning_tick"),
        1,
        "enter-only fires once while standing"
    );
    // Leave and re-enter -> exactly one more.
    t.app
        .world_mut()
        .entity_mut(victim)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(30.0, 0.0, 30.0);
    t.advance_ticks(10);
    t.app
        .world_mut()
        .entity_mut(victim)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(2.0, 0.0, 2.0);
    t.advance_ticks(40);
    assert_eq!(
        damage_events_for(&t, "burning_tick"),
        2,
        "re-entering fires the enter edge again"
    );
}
```

NOTE for the implementer: `insert_hurtbox` puts a STATIC collider at a fixed pos on the victim
entity itself; moving the victim's `Transform` moves the hurtbox too (same entity). The tick
skill's blast window is anchored at the victim's position at execution time, so hits land.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --test surfaces standing 2>&1 | tail -8`
Expected: FAIL — zero `burning_tick` damage events / zero empower applications.

- [ ] **Step 4: Implement** (append to `src/surfaces/systems.rs`)

```rust
use crate::core::components::Combatant;
use crate::spatial::filter::passes_filter;
use crate::surfaces::patch::patch_contains;
use crate::timeline::triggered::{execute_skill_timeline, ExecPayload};
use crate::verbs::ObeliskCommandsExt;
use std::collections::{HashMap, HashSet};

/// Standing-payload state: per-(victim, surface-type) rehit clocks (`next_due`, sim-elapsed
/// seconds — standing in 3 overlapping burning patches ticks ONCE, spec §5.2) and the previous
/// tick's (patch, victim) inside-set for enter-edge detection.
#[derive(Resource, Default)]
pub struct StandingState {
    pub next_due: HashMap<(Entity, String), f32>,
    pub inside_prev: HashSet<(Entity, Entity)>,
}

/// Apply each surface's `standing` payload to combatants inside it. Effects apply/refresh via
/// `apply_obelisk_effect` (victim-sourced — see `StandingPayload` doc); `tick_skill` executes
/// as a triggered-only timeline AT the victim, attributed to the PAINTER, depth 1 (free-hit
/// billing). DETERMINISM: overlaps iterate sorted by (patch.seq, victim.index()).
pub fn apply_standing_payloads(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    registry: Res<SurfaceRegistry>,
    mut state: ResMut<StandingState>,
    patches: Query<(Entity, &SurfacePatch, &Transform)>,
    combatants: Query<(Entity, &Transform, &Faction), With<Combatant>>,
) {
    let now = time.elapsed_secs();
    // Collect + sort overlaps deterministically.
    let mut overlaps: Vec<(u64, Entity, &SurfacePatch, Vec3, Entity, Vec3, Faction)> = Vec::new();
    let mut inside_now: HashSet<(Entity, Entity)> = HashSet::new();
    for (pe, patch, ptf) in &patches {
        for (ve, vtf, vf) in &combatants {
            if patch_contains(ptf.translation, patch.radius, vtf.translation) {
                inside_now.insert((pe, ve));
                overlaps.push((
                    patch.seq,
                    pe,
                    patch,
                    ptf.translation,
                    ve,
                    vtf.translation,
                    *vf,
                ));
            }
        }
    }
    overlaps.sort_by_key(|(seq, _, _, _, ve, _, _)| (*seq, ve.index()));

    let mut fired_this_tick: HashSet<(Entity, String)> = HashSet::new();
    for (_, pe, patch, _ppos, victim, victim_pos, victim_faction) in overlaps {
        let Some(st) = registry.0.get(&patch.surface) else {
            continue;
        };
        let Some(standing) = &st.standing else {
            continue;
        };
        if !passes_filter(
            standing.filter.to_hit_filter(),
            patch.owner_faction,
            victim_faction,
            victim == patch.owner,
        ) {
            continue;
        }
        let due_key = (victim, patch.surface.clone());
        if standing.on_enter_only {
            if state.inside_prev.contains(&(pe, victim)) {
                continue; // still inside from last tick — no new edge
            }
        } else {
            if fired_this_tick.contains(&due_key) {
                continue; // overlapping same-type patch already ticked this victim
            }
            let due = state.next_due.get(&due_key).copied().unwrap_or(0.0);
            if now < due {
                continue;
            }
            state.next_due.insert(due_key.clone(), now + standing.rehit_interval);
            fired_this_tick.insert(due_key);
        }
        if let Some(eff) = &standing.effect {
            commands.entity(victim).apply_obelisk_effect(eff.clone());
        }
        if let Some(ts) = &standing.tick_skill {
            execute_skill_timeline(
                &mut commands,
                patch.owner,
                ts,
                ExecPayload {
                    position: victim_pos,
                    // Direction is irrelevant for the CastPoint-anchored instant blasts this
                    // path is for (same reasoning as on_hit_confirmed's Vec3::X).
                    direction: Vec3::X,
                    target: Some(victim),
                    charge: None,
                    depth: 1, // free-hit billing (is_free_hit: depth > 0)
                },
            );
        }
    }
    // Housekeeping: drop clocks for despawned victims; swap the inside set.
    state
        .next_due
        .retain(|(v, _), _| combatants.get(*v).is_ok());
    state.inside_prev = inside_now;
}
```

`src/surfaces/mod.rs` — export `StandingState` + `apply_standing_payloads`; register:
init `StandingState` resource, and change the paint registration to the chained triple (the
contact system arrives in Task 6 — for THIS task register the pair):
```rust
        app.init_resource::<systems::StandingState>();
        app.add_systems(
            bevy::app::FixedUpdate,
            (paint_surfaces, systems::apply_standing_payloads)
                .chain()
                .in_set(crate::ObeliskSet::ResolveHits)
                .before(crate::spatial::detect::detect_overlaps),
        );
```
(Remove the Task-4 single-system registration — one registration site.)

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --test surfaces 2>&1 | tail -8`
Expected: `11 passed`.

- [ ] **Step 6: Full suite + commit**

```bash
cargo test 2>&1 | tail -5
git add -A && git commit -m "feat(surfaces): standing payloads — effects + tick skills, enter edges, per-(victim,type) clocks

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Skill-contact triggers (fire ignites oil)

**Files:**
- Modify: `src/surfaces/systems.rs`, `src/surfaces/mod.rs`
- Create: `tests/fixtures/cast/fire_probe.cast.ron`, `tests/fixtures/cast/cold_probe.cast.ron`,
  `tests/fixtures/cast/test_ignite.cast.ron`
- Test: `tests/surfaces.rs` (append)

**Interfaces:**
- Consumes: `crate::core::config::SkillRegistry` (tags lookup), Task 2/5 helpers.
- Produces:
  - `systems::SurfaceContacts(pub HashSet<String>)` (Component on hitbox entities — surface
    types already reacted-to)
  - `pub fn surface_contact_triggers(...)` (chained between `paint_surfaces` and
    `apply_standing_payloads`)

- [ ] **Step 1: Write the fixtures**

`tests/fixtures/cast/fire_probe.cast.ron`:
```ron
( skill_id: "fire_probe",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "bolt", spawn: Scheduled( phase: Active, offset: 0.0 ), active_duration: 1.0,
      shape: Sphere( radius: 0.3 ), motion: Linear( speed: 8.0 ),
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
)
```
`tests/fixtures/cast/cold_probe.cast.ron` — identical but `skill_id: "cold_probe"` and window id
`"bolt"` unchanged.
`tests/fixtures/cast/test_ignite.cast.ron`:
```ron
( skill_id: "test_ignite",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "boom", spawn: Scheduled( phase: Active, offset: 0.0 ), anchor: CastPoint,
      active_duration: 0.05, shape: Sphere( radius: 1.5 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
  acquisition: SelfPoint,
)
```

- [ ] **Step 2: Write the failing tests** (append to `tests/surfaces.rs`)

```rust
#[test]
fn fire_hitbox_through_oil_ignites_once_and_consumes_the_patch() {
    let mut t = surf_app(7);
    let painter = spawn_combatant(&mut t, "oiler", Vec3::new(0.0, 0.0, -6.0), obelisk_bevy::prelude::Faction::Enemy);
    let caster = spawn_combatant(&mut t, "pyro", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "fire_probe", "tests/fixtures/cast/fire_probe.cast.ron");
    load_timeline(&mut t, "test_ignite", "tests/fixtures/cast/test_ignite.cast.ron");
    // One oil patch 3m down the +X flight path.
    t.app.world_mut().trigger(PaintSurface {
        surface: "oil".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    grant_and_cast_dir(&mut t, caster, "fire_probe", Vec3::X);
    t.advance_ticks(90);
    // The ignite timeline executed at (about) the contact point...
    let ignites: Vec<_> = t
        .rec()
        .hit_window_opened
        .iter()
        .filter(|w| w.skill_id == "test_ignite")
        .collect();
    assert_eq!(ignites.len(), 1, "ignite executes exactly once");
    // ...and the oil patch was consumed.
    assert_eq!(patch_count(&mut t, "oil"), 0, "oil consumed");
    assert!(t
        .rec()
        .surfaces_removed
        .iter()
        .any(|r| r.surface == "oil" && r.reason == SurfaceRemoveReason::Consumed));
}

#[test]
fn non_matching_tags_do_not_ignite_oil() {
    let mut t = surf_app(8);
    let painter = spawn_combatant(&mut t, "oiler", Vec3::new(0.0, 0.0, -6.0), obelisk_bevy::prelude::Faction::Enemy);
    let caster = spawn_combatant(&mut t, "frosty", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "cold_probe", "tests/fixtures/cast/cold_probe.cast.ron");
    load_timeline(&mut t, "test_ignite", "tests/fixtures/cast/test_ignite.cast.ron");
    t.app.world_mut().trigger(PaintSurface {
        surface: "oil".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    grant_and_cast_dir(&mut t, caster, "cold_probe", Vec3::X);
    t.advance_ticks(90);
    assert!(
        !t.rec().hit_window_opened.iter().any(|w| w.skill_id == "test_ignite"),
        "cold tags don't match tags_any=[fire]"
    );
    assert_eq!(patch_count(&mut t, "oil"), 1, "oil untouched");
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --test surfaces oil 2>&1 | tail -6`
Expected: FAIL — zero ignite executions.

- [ ] **Step 4: Implement** (append to `src/surfaces/systems.rs`)

```rust
use crate::core::config::SkillRegistry;
use crate::surfaces::patch::{SurfaceRemoveReason, SurfaceRemoved};

/// Surface types this hitbox has already reacted with — the once-per-(hitbox, surface-type)
/// guard (spec §5.2: first contact only; no fire-propagation in v1).
#[derive(Component, Debug, Default)]
pub struct SurfaceContacts(pub std::collections::HashSet<String>);

/// Hitbox-vs-surface contact reactions: when a live hitbox overlaps a patch whose surface
/// authors `on_skill_contact` matching the contacting skill's rules tags, execute the reaction's
/// `trigger_skill` at the CONTACT POINT (the hitbox position), attributed to the CONTACTING
/// caster (spec D8), one trigger-depth deeper; optionally consume the touched patch. Fires at
/// most once per (hitbox, surface-type). DETERMINISM: hitboxes by entity index, patches by seq.
pub fn surface_contact_triggers(
    mut commands: Commands,
    registry: Res<SurfaceRegistry>,
    skills: Res<SkillRegistry>,
    patches: Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>,
    mut hitboxes: Query<(Entity, &Hitbox, &Transform, Option<&mut SurfaceContacts>)>,
) {
    let mut sorted_patches: Vec<_> = patches.iter().collect();
    sorted_patches.sort_by_key(|(_, p, _)| p.seq);
    let mut consumed: std::collections::HashSet<Entity> = std::collections::HashSet::new();

    let mut sorted_hb: Vec<_> = hitboxes.iter_mut().collect();
    sorted_hb.sort_by_key(|(e, _, _, _)| e.index());
    for (he, hb, htf, contacts) in sorted_hb {
        let Some(skill) = skills.0.get(&hb.skill_id) else {
            continue;
        };
        // Local view of this hitbox's already-contacted types (component may not exist yet).
        let mut local: std::collections::HashSet<String> = match &contacts {
            Some(c) => c.0.clone(),
            None => Default::default(),
        };
        let mut dirty = false;
        for (pe, patch, ptf) in &sorted_patches {
            if consumed.contains(pe) || local.contains(&patch.surface) {
                continue;
            }
            if !patch_contains(ptf.translation, patch.radius, htf.translation) {
                continue;
            }
            let Some(st) = registry.0.get(&patch.surface) else {
                continue;
            };
            for reaction in &st.on_skill_contact {
                let tag_match = reaction
                    .tags_any
                    .iter()
                    .any(|t| skill.tags.iter().any(|st| st == t));
                if !tag_match {
                    continue;
                }
                local.insert(patch.surface.clone());
                dirty = true;
                execute_skill_timeline(
                    &mut commands,
                    hb.caster,
                    &reaction.trigger_skill,
                    ExecPayload {
                        position: htf.translation,
                        direction: hb.aim,
                        target: None,
                        charge: hb.charge,
                        depth: hb.depth.saturating_add(1),
                    },
                );
                if reaction.consume {
                    consumed.insert(*pe);
                    commands.trigger(SurfaceRemoved {
                        patch: *pe,
                        surface: patch.surface.clone(),
                        position: ptf.translation,
                        reason: SurfaceRemoveReason::Consumed,
                    });
                    commands.entity(*pe).despawn();
                }
                break; // first matching reaction per surface type
            }
        }
        if dirty {
            match contacts {
                Some(mut c) => c.0 = local,
                None => {
                    commands.entity(he).insert(SurfaceContacts(local));
                }
            }
        }
    }
}
```

`src/surfaces/mod.rs` — export `SurfaceContacts` + `surface_contact_triggers`, and update the
chained registration to the final triple:
```rust
        app.add_systems(
            bevy::app::FixedUpdate,
            (
                paint_surfaces,
                systems::surface_contact_triggers,
                systems::apply_standing_payloads,
            )
                .chain()
                .in_set(crate::ObeliskSet::ResolveHits)
                .before(crate::spatial::detect::detect_overlaps),
        );
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --test surfaces 2>&1 | tail -8`
Expected: `13 passed`.

- [ ] **Step 6: Full suite + commit**

```bash
cargo test 2>&1 | tail -5
git add -A && git commit -m "feat(surfaces): skill-contact triggers — tag-matched, once per (hitbox,type), consuming

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Acquisition `on_surface` — require / snap / consume-on-accept

**Files:**
- Modify: `src/assets/mod.rs` (`SurfaceRequirement`, `GroundPoint.on_surface`)
- Modify: `src/timeline/advance.rs` (`resolve_acquisition` threading + `validate_casts`)
- Create: `tests/fixtures/cast/spire_probe.cast.ron`, `tests/fixtures/cast/spire_fallback.cast.ron`
- Test: `tests/surfaces.rs` (append) + `src/assets/mod.rs` unit round-trip

**Interfaces:**
- Consumes: Task 2's `SurfacePatch`/`SurfaceRemoved`/`SURFACE_MATCH_SLACK`/`SURFACE_Y_TOLERANCE`,
  `xz_dist` (make it `pub` in `patch.rs` — change `pub(crate) fn xz_dist` to `pub fn xz_dist`).
- Produces:
  - `assets::SurfaceRequirement { pub surface: String, pub snap: bool (default true), pub consume: bool (default false) }`
  - `Acquisition::GroundPoint { range, fallback, on_surface: Option<SurfaceRequirement> }`
  - `resolve_acquisition(...) -> Result<(Option<Vec3>, Option<Entity>), CastRejectReason>`
    (second element = the matched patch to consume, threaded out to `validate_casts`).

- [ ] **Step 1: Write the fixtures**

`tests/fixtures/cast/spire_probe.cast.ron`:
```ron
( skill_id: "spire_probe",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "spike", spawn: Scheduled( phase: Active, offset: 0.0 ), anchor: CastPoint,
      active_duration: 0.1, shape: Sphere( radius: 0.5 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
  acquisition: GroundPoint( range: 60.0, fallback: Fizzle,
    on_surface: Some(( surface: "frost", snap: true, consume: true )) ),
)
```
`tests/fixtures/cast/spire_fallback.cast.ron`:
```ron
( skill_id: "spire_fallback",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "spike", spawn: Scheduled( phase: Active, offset: 0.0 ), anchor: CastPoint,
      active_duration: 0.1, shape: Sphere( radius: 0.5 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
  acquisition: GroundPoint( range: 60.0, fallback: Then(SelfPoint),
    on_surface: Some(( surface: "frost", snap: true, consume: true )) ),
)
```

- [ ] **Step 2: Write the failing tests** (append to `tests/surfaces.rs`)

```rust
use obelisk_bevy::prelude::CastSkillExt;

#[test]
fn on_surface_gates_snaps_and_consumes_on_accept() {
    let mut t = surf_app(9);
    let caster = spawn_combatant(&mut t, "spirer", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "spire_probe", "tests/fixtures/cast/spire_probe.cast.ron");
    // Patch at (4, 0, 0); aim 0.5m off-center (within radius 0.45 + slack 0.3).
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(4.0, 0.0, 0.0),
        owner: caster,
    });
    t.app.world_mut().flush();
    {
        use obelisk_bevy::verbs::ObeliskCommandsExt;
        t.app.world_mut().commands().entity(caster).grant_skill("spire_probe");
        t.app
            .world_mut()
            .commands()
            .entity(caster)
            .cast_skill_at_point("spire_probe", Vec3::new(4.5, 0.0, 0.0));
    }
    t.app.world_mut().flush();
    t.advance_ticks(20);
    assert_eq!(t.rec().cast_began.len(), 1, "gated cast accepted on a frost patch");
    // Consume-on-accept: the patch is gone.
    assert_eq!(patch_count(&mut t, "frost"), 0, "patch consumed at cast-accept");
    assert!(t
        .rec()
        .surfaces_removed
        .iter()
        .any(|r| r.surface == "frost" && r.reason == SurfaceRemoveReason::Consumed));
    // Snap: the CastPoint-anchored window spawned at the PATCH CENTER (4,0,0), not (4.5,..).
    let w = t
        .rec()
        .hit_window_opened
        .iter()
        .find(|w| w.skill_id == "spire_probe")
        .expect("window opened");
    let hb_pos = t.rec().hitbox_ended.iter().find(|e| e.skill_id == "spire_probe").unwrap().position;
    let _ = w;
    assert!(
        (hb_pos - Vec3::new(4.0, 0.0, 0.0)).length() < 0.01,
        "snapped to patch center, got {hb_pos:?}"
    );
}

#[test]
fn on_surface_miss_fizzles_or_falls_back() {
    let mut t = surf_app(10);
    let caster = spawn_combatant(&mut t, "spirer", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "spire_probe", "tests/fixtures/cast/spire_probe.cast.ron");
    load_timeline(&mut t, "spire_fallback", "tests/fixtures/cast/spire_fallback.cast.ron");
    {
        use obelisk_bevy::verbs::ObeliskCommandsExt;
        t.app.world_mut().commands().entity(caster).grant_skill("spire_probe");
        t.app.world_mut().commands().entity(caster).grant_skill("spire_fallback");
        // No frost anywhere: Fizzle variant rejects...
        t.app
            .world_mut()
            .commands()
            .entity(caster)
            .cast_skill_at_point("spire_probe", Vec3::new(4.0, 0.0, 0.0));
    }
    t.app.world_mut().flush();
    t.advance_ticks(10);
    assert_eq!(t.rec().cast_began.len(), 0);
    assert_eq!(t.rec().cast_rejected.len(), 1, "no frost -> paid fizzle (CastRejected)");
    // ...while the Then(SelfPoint) variant falls back and casts at the caster.
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at_point("spire_fallback", Vec3::new(4.0, 0.0, 0.0));
    t.app.world_mut().flush();
    t.advance_ticks(20);
    assert_eq!(t.rec().cast_began.len(), 1, "fallback chain rescued the cast");
    let hb_pos = t
        .rec()
        .hitbox_ended
        .iter()
        .find(|e| e.skill_id == "spire_fallback")
        .unwrap()
        .position;
    assert!(
        (hb_pos - Vec3::ZERO).length() < 0.01,
        "SelfPoint fallback anchors at the caster, got {hb_pos:?}"
    );
}
```

Also append to `src/assets/mod.rs` `mod tests`:
```rust
    /// (Surfaces) `on_surface` on GroundPoint round-trips and defaults.
    #[test]
    fn on_surface_round_trips() {
        let src = r#"(
            skill_id: "gated",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            acquisition: GroundPoint( range: 60.0, fallback: Fizzle,
                on_surface: Some(( surface: "frost", consume: true )) ),
        )"#;
        let tl: CastTimeline = ron::from_str(src).expect("on_surface parses");
        let Acquisition::GroundPoint { on_surface, .. } = &tl.acquisition else {
            panic!("GroundPoint expected");
        };
        let req = on_surface.as_ref().expect("requirement present");
        assert_eq!(req.surface, "frost");
        assert!(req.snap, "snap defaults to true");
        assert!(req.consume);
        // Old content (no on_surface) still parses.
        let old = r#"(
            skill_id: "plain",
            phase_durations: ( windup: 0.1, active: 0.1, recovery: 0.1 ),
            acquisition: GroundPoint( range: 30.0, fallback: Fizzle ),
        )"#;
        let tl: CastTimeline = ron::from_str(old).expect("pre-surfaces GroundPoint parses");
        let Acquisition::GroundPoint { on_surface, .. } = &tl.acquisition else {
            panic!()
        };
        assert!(on_surface.is_none());
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p obelisk-bevy --lib assets::tests::on_surface_round_trips 2>&1 | tail -5`
Expected: COMPILE FAIL (no `on_surface` variant field).

- [ ] **Step 4: Implement**

`src/assets/mod.rs` — after `AcqFallback`, add:
```rust
/// Surface gate on a point acquisition (spec §5.1): the aimed point must land ON a patch of
/// `surface` (XZ within `patch.radius + SURFACE_MATCH_SLACK`, Y within tolerance). `snap`
/// recenters the cast point on the matched patch; `consume` removes the patch at CAST-ACCEPT
/// (with mana — spec D7: an interrupted cast still spends the tile). Failure runs the normal
/// fallback chain (paid fizzle at `Fizzle`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRequirement {
    pub surface: String,
    #[serde(default = "default_true")]
    pub snap: bool,
    #[serde(default)]
    pub consume: bool,
}
```
(`default_true` already exists in this file for `CollisionWindow::strikes`.)

Extend the `GroundPoint` variant:
```rust
    GroundPoint {
        range: f32,
        fallback: AcqFallback,
        /// Optional surface gate (spec §5.1) — see [`SurfaceRequirement`].
        #[serde(default)]
        on_surface: Option<SurfaceRequirement>,
    },
```
Compile errors point at every `GroundPoint { .. }` construction/match site — add
`on_surface: None` to constructions and `on_surface, ..` or `..` to matches as appropriate
(`acquisition_can_produce_point` just needs `GroundPoint { .. } => true` unchanged).

`src/timeline/advance.rs` — thread patches through acquisition:

1. Add imports:
```rust
use crate::assets::SurfaceRequirement;
use crate::surfaces::patch::{xz_dist, SurfacePatch, SurfaceRemoveReason, SurfaceRemoved};
use crate::surfaces::types::{SURFACE_MATCH_SLACK, SURFACE_Y_TOLERANCE};
```

2. Define the pre-collected patch view (above `resolve_acquisition`):
```rust
/// Pre-collected, seq-sorted view of the live patches for acquisition checks (deterministic
/// candidate order): (patch entity, position, surface id, radius, seq).
type PatchView = (Entity, Vec3, String, f32, u64);
```

3. Change `resolve_acquisition`'s signature and `GroundPoint` arm (and `resolve_fallback`'s the
   same way — it recurses):
```rust
fn resolve_acquisition(
    acq: &Acquisition,
    aim: CastAim,
    caster_pos: Vec3,
    transforms: &Query<&Transform>,
    factions: &Query<&Faction>,
    caster: Entity,
    patches: &[PatchView],
) -> Result<(Option<Vec3>, Option<Entity>), CastRejectReason> {
    match acq {
        Acquisition::Aim => Ok((None, None)),
        Acquisition::SelfPoint => Ok((Some(caster_pos), None)),
        Acquisition::HitscanEntity { range, filter, fallback } => match check_hitscan_entity(
            aim, *range, *filter, caster_pos, transforms, factions, caster,
        ) {
            Ok(()) => Ok((None, None)),
            Err(reason) => resolve_fallback(
                fallback, aim, caster_pos, transforms, factions, caster, patches, reason,
            ),
        },
        Acquisition::GroundPoint { range, fallback, on_surface } => {
            match check_ground_point(aim, *range, caster_pos, on_surface.as_ref(), patches) {
                Ok(ok) => Ok(ok),
                Err(reason) => resolve_fallback(
                    fallback, aim, caster_pos, transforms, factions, caster, patches, reason,
                ),
            }
        }
    }
}
```
`resolve_fallback` gains the `patches: &[PatchView]` parameter and passes it through to the
recursive `resolve_acquisition` call; its `Fizzle` arm is unchanged.

4. Extend `check_ground_point`:
```rust
/// `GroundPoint`'s own requirement: `aim` must be `CastAim::Point`, within `range` of the
/// caster — and, when `on_surface` is authored, ON a matching patch (nearest by XZ distance,
/// ties broken by patch seq — deterministic). Returns (cast point, patch-to-consume).
fn check_ground_point(
    aim: CastAim,
    range: f32,
    caster_pos: Vec3,
    on_surface: Option<&SurfaceRequirement>,
    patches: &[PatchView],
) -> Result<(Option<Vec3>, Option<Entity>), CastRejectReason> {
    let CastAim::Point(p) = aim else {
        return Err(CastRejectReason::NoTarget); // wrong aim kind
    };
    if p.distance(caster_pos) > range {
        return Err(CastRejectReason::OutOfRange);
    }
    let Some(req) = on_surface else {
        return Ok((Some(p), None));
    };
    let best = patches
        .iter()
        .filter(|(_, pos, surface, radius, _)| {
            surface == &req.surface
                && xz_dist(*pos, p) <= *radius + SURFACE_MATCH_SLACK
                && (p.y - pos.y).abs() <= SURFACE_Y_TOLERANCE
        })
        .min_by(|a, b| xz_dist(a.1, p).total_cmp(&xz_dist(b.1, p)).then(a.4.cmp(&b.4)));
    let Some(best) = best else {
        // No matching surface under the aim: this branch fails; the fallback chain decides.
        return Err(CastRejectReason::NoTarget);
    };
    let point = if req.snap { best.1 } else { p };
    Ok((Some(point), req.consume.then_some(best.0)))
}
```

5. In `validate_casts`: add the query parameter
```rust
    surface_patches: Query<(Entity, &SurfacePatch, &Transform)>,
```
build the sorted view once at the top of the function body:
```rust
    let mut patch_view: Vec<PatchView> = surface_patches
        .iter()
        .map(|(e, p, tf)| (e, tf.translation, p.surface.clone(), p.radius, p.seq))
        .collect();
    patch_view.sort_by_key(|(_, _, _, _, seq)| *seq);
```
replace the existing `resolve_acquisition(...)` call site (it currently binds the cast point,
e.g. `let cast_point = match resolve_acquisition(&timeline.acquisition, aim, ...)`) with the
tuple form:
```rust
        let (cast_point, consume_patch) = match resolve_acquisition(
            &timeline.acquisition,
            aim,
            caster_pos,
            &transforms,
            &factions,
            caster,
            &patch_view,
        ) {
            Ok(pair) => pair,
            Err(reason) => {
                commands.trigger(CastRejected { caster, skill_id: req.skill_id.clone(), reason });
                continue;
            }
        };
```
(keep the surrounding rejection plumbing exactly as it is today — only the binding changes),
and IMMEDIATELY after the cast is accepted (where `ActiveCast` is inserted, before or after —
choose after `commands.trigger(CastBegan {...})` for readability), consume:
```rust
        if let Some(patch) = consume_patch {
            if let Ok((pe, p, ptf)) = surface_patches.get(patch) {
                commands.trigger(SurfaceRemoved {
                    patch: pe,
                    surface: p.surface.clone(),
                    position: ptf.translation,
                    reason: SurfaceRemoveReason::Consumed,
                });
            }
            commands.entity(patch).despawn();
        }
```
Note: there may be several `resolve_acquisition` calls if the function loops per pending cast —
the same edit applies inside the loop. Also update `patch.rs`: `pub(crate) fn xz_dist` →
`pub fn xz_dist`.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --test surfaces 2>&1 | tail -8` and `cargo test -p obelisk-bevy --lib 2>&1 | tail -5`
Expected: `15 passed` (integration) + lib green.

- [ ] **Step 6: Full suite (acquisition regression!) + commit**

```bash
cargo test 2>&1 | tail -6   # tests/acquisition.rs must stay green
git add -A && git commit -m "feat(surfaces): acquisition on_surface — require/snap/consume-on-accept

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: Determinism lock, prelude/docs polish, full verification

**Files:**
- Test: `tests/surfaces.rs` (append)
- Modify: `src/prelude.rs` (any missing exports), `src/surfaces/mod.rs` (doc completeness)

**Interfaces:** none new — this task locks behavior.

- [ ] **Step 1: Write the determinism test** (append to `tests/surfaces.rs`)

```rust
/// The surfaces golden: an identical script (trail paint + standing burn + oil ignite) run
/// twice from the same seed must produce IDENTICAL event streams — positions, counts, damage.
#[test]
fn surfaces_pipeline_is_deterministic_across_runs() {
    fn run(seed: u64) -> (Vec<(String, [i64; 3])>, Vec<i64>) {
        let mut t = surf_app(seed);
        let painter =
            spawn_combatant(&mut t, "painter", Vec3::new(-5.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
        let victim =
            spawn_combatant(&mut t, "victim", Vec3::new(3.0, 0.0, 0.5), obelisk_bevy::prelude::Faction::Enemy);
        load_timeline(&mut t, "paint_roller", "tests/fixtures/cast/paint_roller.cast.ron");
        load_timeline(&mut t, "burning_tick", "tests/fixtures/cast/burning_tick.cast.ron");
        load_timeline(&mut t, "fire_probe", "tests/fixtures/cast/fire_probe.cast.ron");
        load_timeline(&mut t, "test_ignite", "tests/fixtures/cast/test_ignite.cast.ron");
        obelisk_bevy::spatial::boxes::insert_hurtbox(
            &mut t.app.world_mut().commands(),
            victim,
            0.5,
            Vec3::new(3.0, 0.0, 0.5),
        );
        t.app.world_mut().flush();
        t.advance_ticks(3);
        // Script: burning under the victim, oil on the fire_probe's path, roller across.
        t.app.world_mut().trigger(PaintSurface {
            surface: "burning".into(),
            position: Vec3::new(3.0, 0.0, 0.5),
            owner: painter,
        });
        t.app.world_mut().trigger(PaintSurface {
            surface: "oil".into(),
            position: Vec3::new(4.0, 0.0, 0.0),
            owner: painter,
        });
        t.app.world_mut().flush();
        grant_and_cast_dir(&mut t, painter, "paint_roller", Vec3::X);
        t.advance_ticks(30);
        grant_and_cast_dir(&mut t, painter, "fire_probe", Vec3::X);
        t.advance_ticks(120);
        // Quantize to bit-stable integers (f32 determinism on one machine is exact; the
        // quantization just makes failures readable).
        let paints = t
            .rec()
            .surfaces_painted
            .iter()
            .map(|p| {
                (
                    p.surface.clone(),
                    [
                        (p.position.x * 1000.0) as i64,
                        (p.position.y * 1000.0) as i64,
                        (p.position.z * 1000.0) as i64,
                    ],
                )
            })
            .collect();
        let damage = t
            .rec()
            .damage_resolved
            .iter()
            .map(|d| (d.total_damage * 1000.0) as i64)
            .collect();
        (paints, damage)
    }
    let a = run(42);
    let b = run(42);
    assert_eq!(a.0, b.0, "paint stream identical across runs");
    assert_eq!(a.1, b.1, "damage stream identical across runs");
    assert!(!a.0.is_empty() && !a.1.is_empty(), "the script actually exercised the pipeline");
}
```

- [ ] **Step 2: Run to verify it passes** (it should — if it FAILS, a determinism bug from
  Tasks 4–7 must be fixed NOW: look for unsorted iteration)

Run: `cargo test --test surfaces 2>&1 | tail -8`
Expected: `16 passed`.

- [ ] **Step 3: Full-crate verification**

```bash
cargo test 2>&1 | tail -10          # EVERYTHING green, including tests/golden.rs byte-identical
cargo clippy --all-targets 2>&1 | grep -E "^(warning|error)" | head -20   # no NEW warnings in surfaces code
```

- [ ] **Step 4: Polish + commit**

Confirm `src/prelude.rs` exports (add any missing): `ObeliskSurfacesPlugin`, `PaintSurface`,
`SurfacePainted`, `SurfacePatch`, `SurfaceRegistry`, `SurfaceRemoveReason`, `SurfaceRemoved`,
and from `assets`: `PaintSpec`, `PaintMode`, `SurfaceRequirement` (extend the existing
`pub use crate::assets::{...}` line).

```bash
git add -A && git commit -m "test(surfaces): determinism lock across runs + prelude exports

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Post-plan notes (for the coordinating session, not a task)

- Increments 4–6 (arena replication/rendering/migration; editor authoring/preview/stage tool)
  are SEPARATE follow-up plans per the spec — do not start them from this plan.
- Do NOT push or merge `surfaces-core`; leave integration to the coordinating session.
- If any golden diff appears at any step, the change is wrong — new fields are defaulted and new
  systems must no-op without surface content. Fix the code, never the golden.
