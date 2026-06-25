# Obelisk-bevy 0.18 Migration Map (Bevy 0.17→0.18, Avian 0.4→0.5)

**Status:** Input artifact for a detailed implementation plan. Not the plan itself.
**Date:** 2026-06-25
**Scope:** `/Users/luke/src/obelisk-bevy` — the whole crate (`src/`, `examples/`, `Cargo.toml`).
**Verified against:** sibling 0.18 codebases `/Users/luke/src/wisp` (`bevy = "0.18"`,
`avian3d = "0.5"`) and `/Users/luke/src/bevy_modal_editor` (`bevy = "0.18"`).

> **⚠️ EXECUTION CORRECTION (2026-06-25, during the migration):** Two claims in this map were
> **falsified** when checked against the actually-resolved dependency versions (bevy **0.18.1**,
> avian3d **0.5.0**):
> 1. **Area 5b is VOID.** `ColliderAabb::size()` is **NOT removed** in avian 0.5.0 — it still exists
>    and still returns the full extent (`self.max - self.min`), identical to 0.4. `half_size()` does
>    **not exist** on `ColliderAabb` in 0.5.0. So the prescribed `size()`→`half_size()` edit (and the
>    `* 0.5` removal) is **wrong** — applying it would *introduce* a compile error. `present/debug_viz.rs`
>    needs **zero** Avian edits; avian 0.5 is a pure-compatibility release for this crate. **Risk #2
>    (the multiplier trap) is therefore moot** — there is no edit. The gizmo radius math
>    (`size().max_element() * 0.5`) is already correct.
> 2. **Area 0/2 missed one real Bevy edit:** Bevy 0.18 made `TypePath` a supertrait of `AssetLoader`,
>    so `src/assets/mod.rs`'s `CastTimelineLoader` needed `#[derive(Default, TypePath)]` (a marker
>    derive, no behavior change). This was the **only** source edit required to compile the sim.
>
> Net: the migration's real source-edit surface so far is the dep bump + the one `TypePath` derive.
> The verified-against-source truth below overrides any contradicting prose in Areas 5b, 7 (T6), 8 (#2).

## How to read this map

This is a migration where **the crate will not fully compile until most of the work is done** —
Bevy's `Event`/`Message` split and the observer rename touch nearly every module. So the plan is
organized as **ordered compile-areas**, not feature work: fix one coherent slice, move to the next,
and treat *compiles* as the intermediate signal. The **39-golden / 84-test suite is the final
runtime gate** — it is the only thing that proves behavior (especially determinism + SpatialQuery
detection results) is unchanged.

**Reality check from the survey + code audit:** the overwhelming majority of obelisk-bevy's 0.17
surface is *already* on the 0.18 idiom. The crate adopted `#[derive(Message)]` + `add_message::<T>()`
+ `MessageReader`/`MessageWriter` and `#[derive(Event)]` + `commands.trigger` + `On<E>` *early*
(see CLAUDE.md "Confirmed external facts": these are noted as the 0.17 way already). That means the
two named "big" Bevy renames (`EventReader`→`MessageReader`, `Trigger`→`On`) **were done ahead of
time and need only verification, not rewriting.** The genuinely load-bearing changes are narrow:

- **Avian `ColliderAabb::size()` → `half_size()`** in `present/debug_viz.rs` (one call, with a
  multiplier-removal subtlety — a silent-gizmo-size bug if done naively).
- **Version bumps** in `Cargo.toml` (+ the first real compile pull of 0.18/avian-0.5, not yet in
  this machine's cargo registry).
- **Verification** that the already-migrated observer/message/required-component patterns still
  compile and produce byte-identical goldens under 0.18.

The danger in this migration is therefore **silent behavior change**, not compile churn. The
SpatialQuery rewrite is a *non-event* on the API surface (signatures identical 0.4→0.5) but a
*maximum-attention* item on the behavior surface (detection results feed every combat golden).

---

## AREA 0 — Cargo.toml + deps (the bump)

**Files:** `/Users/luke/src/obelisk-bevy/Cargo.toml` (1 file).

**Current form:**
```toml
[dependencies]
bevy = "0.17"
avian3d = "0.4"
```

**New form:**
```toml
[dependencies]
bevy = "0.18"
avian3d = "0.5"
```

**Notes / decisions:**
- **No feature changes required for this crate's own `[features]` table.** `default = ["present"]`,
  `present`, `test-support`, `debug-gizmos = ["present"]` are all crate-local feature names and are
  untouched by the 0.18 rename of *Bevy's* features (`animation`→`gltf_animation`,
  `bevy_sprite_picking_backend`→`sprite_picking`). obelisk-bevy does not enable any of the renamed
  Bevy features explicitly (it uses `bevy = "0.18"` with defaults), so the silent feature-rename
  risk does **not** apply here. Confirm by grepping `Cargo.toml` for `features = [...]` on the
  `bevy` line — there is none today.
- **avian3d `0.5` is the correct target** (confirmed: wisp pins `avian3d = { version = "0.5", ... }`
  against `bevy = "0.18"`). Per the official guide, Avian 0.5 is a *compatibility-only* release —
  no SpatialQuery/collider/physics API breakage beyond the `ColliderAabb` extent change handled in
  Area 5.
- **dev-dependencies** (`crossbeam-channel`, `serde_json`, and the self-dep
  `obelisk-bevy = { path = ".", features = ["test-support"] }`) need no version change.
- **obelisk path deps** (`stat_core`, `loot_core`, `skill_tree`, `tables_core` at `../obelisk/*`)
  are pure-Rust, Bevy-agnostic — **no change**. They are not part of this migration.
- **The in-flight `feat/combat-result-trigger-metadata` branch:** there is **no such branch and no
  uncommitted work in `obelisk-bevy` today** (working tree clean, only `main`, HEAD `0466e19`). If
  that branch exists elsewhere / is created before this migration lands, the carry rule is: **do the
  0.18 migration on its own branch off `main`, then rebase the metadata branch on top** (or vice
  versa, whichever lands first). Their change-sets barely overlap — the metadata branch touches
  combat event *payloads*/`DamageResolved` fields; this migration touches *plumbing* (renames,
  version bumps, one Avian call). The one shared file to watch on rebase is `src/combat/system.rs`
  (event triggers) and `src/events.rs` (event structs). Resolve by taking the metadata branch's
  payload definitions and this branch's derive/trigger spelling.

**Compile signal:** after this edit the crate will pull 0.18 + avian-0.5 (first fetch) and surface
the *full* error list, which drives Areas 1–6. Expect a large but shallow error set dominated by
"already correct, recompiles clean" plus the one Avian AABB error.

**Cannot be independently test-verified.** This is the entry compile gate; tests run only after
Areas 1–5 land.

---

## AREA 1 — Bevy observers / `On<E>` (verify-only; the "Trigger→On" rename is already done)

**Files (already on the 0.18 idiom — verify each still compiles):**
`src/testkit.rs` (9 `add_observer`), `src/vfx.rs` (4), `src/loot.rs` (1),
`src/present/mod.rs` (3), `src/present/debug_viz.rs` (12 incl. generic + `On<Add>`-free),
`src/core/tick.rs` (3), `src/combat/system.rs` (`on_hit_confirmed` + `world_mut().trigger`),
`src/spatial/detect.rs` (`commands.trigger(HitConfirmed)`), `src/timeline/advance.rs` (10 triggers),
`src/facade/combat.rs` (5 triggers), `src/core/cooldown.rs` (1), `src/verbs.rs` (2 `world.trigger`),
`src/scenario/trace.rs` (15 observers). **~14 files.**

**Current == New form (no change expected):**
```rust
// observer registration + read via .event() (GLOBAL events, not entity-scoped)
app.add_observer(|e: On<CastBegan>, mut r: ResMut<EventRecorder>| { let d = e.event(); /* … */ });
// generic observer over a sealed trait
fn on_hit_flash<E: HitTarget>(ev: On<E>, mut commands: Commands) { /* ev.event().target() */ }
// emit
commands.trigger(HitConfirmed { caster, target, skill_id, window_id });
app.world_mut().trigger(HitConfirmed { /* … */ });
world.trigger(crate::events::EffectApplied { /* … */ });  // from EntityCommand closure
```

**0.18 idiom reference (sibling repos):**
- `On<E>` observer param + `.event()` read on a **global** event: matches obelisk-bevy verbatim.
  See `/Users/luke/src/wisp/src/player/controller.rs:35,173,196` (`On<Fire<Look>>`, `On<Start<Jump>>`)
  for the param spelling.
- Entity-scoped triggers read the entity via the **field** `trigger.entity`, not a method:
  `/Users/luke/src/wisp/src/net/replication.rs:633-664` (`On<Add, NetworkedPlayer>` → `trigger.entity`).
  **obelisk-bevy does not use entity-scoped `On<Add, T>` today** — all its events are global with an
  explicit `Entity`/`source` field read via `.event()`. This is the migration-relevant distinction:
  the 0.18 "entity events became immutable, mutation moved to `SetEntityEventTarget`" change in the
  official guide **does not apply** to obelisk-bevy because it uses *global* (non-entity-targeted)
  events. **Verify** `e.event()` still returns `&E` for global events under 0.18 (it does in wisp).

**The one 0.18 observer restriction to actively check:** *exclusive systems are no longer allowed as
observers.* Audit every `add_observer` closure/fn — none of obelisk-bevy's observers take
`&mut World` / `World` as a system param (they take `Commands`, `Res`/`ResMut`, `Query`,
`MessageWriter`), so this restriction is **not triggered**. `app.world_mut().trigger(...)` in
`combat/system.rs` is a *call site*, not an observer signature — it is unaffected.

**Compile signal:** observers compile. **Cannot hit the test gate alone** (crate still broken
elsewhere until Areas 2–5).

---

## AREA 2 — Bevy required components (`#[require(...)]`, verify-only)

**Files:** `src/core/components.rs` (the `Combatant` `#[require(...)]`). **1 file.**

**Current == New form (no change expected):**
```rust
#[derive(Component, Default)]
#[require(Attributes, Faction, SkillSlots, crate::ids::ObeliskId, Transform)]
pub struct Combatant;
```

**0.18 idiom reference:** `#[require(...)]` is stable in 0.18; the editor/wisp use it throughout.

**Watch (official-guide silent changes — confirm none apply):**
- *Required-component restructures in 0.18* (RenderTarget split off `Camera`, `LineHeight` now
  required by `Text`/`Text2d`/`TextSpan`, `AnimationTarget` split into `AnimationTargetId` +
  `AnimatedBy`). obelisk-bevy's *gameplay* components (`Attributes`, `Faction`, `SkillSlots`,
  `ObeliskId`, `Transform`) are **not** affected. **But:** `present/debug_viz.rs` spawns `Text` +
  `TextFont` HUD nodes — see Area 4 for the `Text`/`LineHeight` check.

**Compile signal:** `Combatant` and its required set compile. Tied to Area 4 for the `Text` path.

---

## AREA 3 — Bevy messages / `add_message` (verify-only; the "EventReader→MessageReader" rename is already done)

**Files:** `src/net.rs` (`#[derive(Message)]` `NetEvent`, `add_message::<NetEvent>()`, 7
`MessageWriter<NetEvent>` mirror fns), `src/scenario/trace.rs` (`MessageReader<NetEvent>` drain
inside an observer closure), `examples/headless_server.rs` (drains `MessageReader<NetEvent>`),
prelude re-export. **~4 files.**

**Current == New form (no change expected):**
```rust
#[derive(Message, serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum NetEvent { /* … */ }
app.add_message::<NetEvent>();
fn mirror_cast_began(ev: On<CastBegan>, /* … */ mut net: MessageWriter<NetEvent>) { net.write(/* … */); }
// drain:
for ev in reader.read() { /* … */ }   // MessageReader<NetEvent>
```

**0.18 idiom reference:**
- `add_message::<T>()` registration: `/Users/luke/src/wisp/src/spells/mod.rs`.
- `MessageWriter<T>` as a plain SystemParam and inside helper fns:
  `/Users/luke/src/bevy_modal_editor/src/ui/hierarchy.rs:135-136`,
  `/Users/luke/src/bevy_modal_editor/src/ui/marks.rs:31-33`.
- `MessageWriter<'w, T>` as a named field in a derived SystemParam struct:
  `/Users/luke/src/bevy_modal_editor/src/ui/command_palette/commands.rs:734-736`.

**Watch (the real risk here):** the official guide's *Message/Event split* is terminology obelisk-bevy
already adopted, so there is no rename to do. The behavioral watch is **buffered-message ordering**:
`net.rs` writes multiple `NetEvent`s per frame (one per mirror observer) and `scenario/trace.rs`
drains them into the golden `Trace`. If 0.18 changed `Messages<T>` double-buffer / drain ordering,
the `netcode_egress` golden (and any trace that records net lines) would shift. **This is a golden-gated
check, not a compile check** — see Area 6 risk list.

**Compile signal:** message plumbing compiles. **Cannot hit the test gate alone.**

---

## AREA 4 — Bevy App / plugin / schedule + present/UI/examples render

**Files:** `src/lib.rs` (`configure_sets().chain()`, `add_systems().in_set()`, `ObeliskSet`),
`examples/playground.rs`, `examples/screenshot.rs`, `examples/headless_server.rs` (App builders),
`src/present/mod.rs` + `src/present/debug_viz.rs` (Update systems, `bevy_ui` HUD, `Text`/`TextFont`,
`Camera::world_to_viewport`, gizmos). **~6 files.**

### 4a. App / plugin / schedule (verify-only)

**Current == New form (no change expected):**
```rust
app.configure_sets(FixedUpdate, (ObeliskSet::Validate, ObeliskSet::Advance, /* … */).chain());
app.add_systems(FixedUpdate, (timeline::advance::validate_casts.in_set(ObeliskSet::Validate), /* … */));
app.add_plugins(DefaultPlugins).add_plugins(ObeliskPlugins).insert_resource(Time::<Fixed>::from_hz(60.0));
```

**Watch (official-guide):**
- *`SimpleExecutor` removed* — obelisk-bevy never names an executor; default is fine. No change.
- *`ScheduleBuildError` / combinator changes* — obelisk-bevy uses `.chain()` + `.in_set()`, **not**
  the `and`/`or`/`xor` run-condition combinators, so the "combinators now return false instead of
  propagating errors" silent change **does not apply**.
- *State-transition behavior change* (`set()` always transitions; new `set_if_neq`) — obelisk-bevy
  uses no Bevy `States` in the sim path. Confirm by grep (`add_state`, `States`, `OnEnter`,
  `init_state`) → expect none. No change.

### 4b. present / debug_viz / examples render + UI

The render/UI layer is `present`-gated and depends most heavily on Bevy graphics internals — highest
"compiles-but-the-window-looks-wrong" surface.

**Watch (official-guide silent changes to actively check in `present/debug_viz.rs` + examples):**
- **`Text` / `LineHeight`:** 0.18 makes `LineHeight` a required component of `Text`/`Text2d`/`TextSpan`,
  and `TextFont` no longer carries a `line_height` field. `debug_viz.rs` spawns `Text::new(...)` +
  `TextFont { font_size, ..default() }` (roster, event-log, floating damage). `..default()` on
  `TextFont` should keep compiling; **verify** `TextFont` no longer exposes `line_height` (it isn't
  set here, so likely a no-op) and that spawning `Text` without an explicit `LineHeight` still works
  via required-component defaulting.
- **`RenderTarget` split off `Camera`:** `examples/screenshot.rs` uses `RenderTarget::Image` for
  off-screen capture. In 0.18 `RenderTarget` is a standalone required component rather than a `Camera`
  field. **Verify** the screenshot example's camera/render-target wiring against 0.18 (this is the
  single most likely *example* to need a structural touch). Cross-check Bevy's 0.18 `headless_renderer`
  example shape if it errors.
- **`BorderRadius` removed from `Node`:** `debug_viz.rs` HUD `Node`s do **not** set `border_radius`
  (grep confirms) — not affected. Noted because it is a *silent* runtime failure if it were used.
- **Mesh `try_insert_attribute`:** `give_projectiles_a_mesh` uses `meshes.add(Sphere::new(0.2))`
  (a `Mesh` *built by a primitive*, not manual `insert_attribute`), so the "mesh ops now return
  `Result`" change does not bite here. No manual attribute insertion anywhere in `present`.
- **Automatic AABB updates / `NoAutoAabb`:** 0.18 auto-updates AABBs on mesh/sprite changes. The
  projectile mesh is cosmetic and not queried for its AABB; only Avian's `ColliderAabb` (Area 5) is
  read. No action, but be aware if gizmo/camera framing shifts.

**Current == New (App-builder) form (no change expected):**
```rust
app.add_plugins(DefaultPlugins.set(WindowPlugin { /* … */ }).disable::<WinitPlugin>())
   .add_plugins(ObeliskSimPlugin)
   .add_plugins(obelisk_bevy::present::ObeliskPresentPlugin);   // screenshot.rs
app.add_plugins(MinimalPlugins).add_plugins(AssetPlugin { file_path: ".".into(), ..default() })
   .add_plugins(bevy::mesh::MeshPlugin).add_plugins(bevy::scene::ScenePlugin)
   .add_plugins(spatial::ObeliskSpatialPlugin);   // headless_server.rs
```

**Compile signal:** `cargo build` (default features) + `cargo build --no-default-features` both
compile; `cargo build --examples --features debug-gizmos` compiles. The examples are the **only**
place a render-API break (RenderTarget) will surface a compile error.

---

## AREA 5 — Avian colliders / rigidbodies / AABB (the small real change)

**Files:** `src/spatial/boxes.rs`, `src/spatial/shapes.rs`, `src/scenario/mod.rs`, `src/lib.rs`
(doc only), `src/present/debug_viz.rs` (**the one real edit**). **~5 files, 1 real change.**

### 5a. Colliders / RigidBody / SpatialQueryFilter (verify-only — API stable 0.4→0.5)

**Current == New form (no change):**
```rust
RigidBody::Static
Collider::sphere(radius)                 // boxes.rs, detect.rs tests
Collider::sphere(*radius)                // shapes.rs to_collider
Collider::capsule(*radius, *height)      // shapes.rs to_collider
SpatialQueryFilter::default()
PhysicsPlugins::new(FixedUpdate)         // spatial plugin + test apps
```
**Confirmed stable in 0.5** via wisp 0.5: `Collider::sphere`, `Collider::capsule`,
`SpatialQueryFilter::default()`, `SpatialQueryFilter::from_excluded_entities(...)`,
`PhysicsPlugins::new(...)` all present and unchanged
(`/Users/luke/src/wisp/src/spells/deliveries.rs:220-227`).

**Determinism watch (CLAUDE.md design note):** "Avian 0.4 `RigidBody::Static` DOES track Transform
changes (static ≠ frozen) so hurtboxes follow moving owners." This is *empirically verified* by
`spatial/detect.rs::hurtbox_tracks_a_moving_owner`. **If Avian 0.5 changed static-body Transform
re-read semantics, that test fails** — it is the canary for this exact behavior. Run it explicitly
(Area 6).

### 5b. `ColliderAabb::size()` → `half_size()` — THE ONE REAL AVIAN EDIT

**File:** `/Users/luke/src/obelisk-bevy/src/present/debug_viz.rs`, in `draw_combat_gizmos`
(`debug-gizmos` feature), lines ~275-279.

**Current form (0.4):**
```rust
for (tf, aabb) in &hurtboxes {
    let radius = aabb
        .map(|a| (a.size().max_element() * 0.5).max(0.05))   // size() = FULL extent in 0.4
        .unwrap_or(0.6);
    let center = aabb.map(|a| a.center()).unwrap_or(tf.translation);
    gizmos.sphere(center, radius, gizmo_colors::HURTBOX);
}
```

**New form (0.5) — REMOVE the `* 0.5`, switch to `half_size()`:**
```rust
for (tf, aabb) in &hurtboxes {
    let radius = aabb
        .map(|a| a.half_size().max_element().max(0.05))      // half_size() already half-extent in 0.5
        .unwrap_or(0.6);
    let center = aabb.map(|a| a.center()).unwrap_or(tf.translation);
    gizmos.sphere(center, radius, gizmo_colors::HURTBOX);
}
```

**Why the multiplier must go (silent-bug trap):** in 0.4 `size()` returns the **full** AABB extent,
and the code multiplies by `0.5` to get a radius. In 0.5 `half_size()` already returns the
**half-extent**. If you mechanically rename `size()`→`half_size()` but *keep* the `* 0.5`, the radius
becomes **half of correct** — hurtbox gizmos render at half the intended size with **no compile error
and no test failure** (gizmos aren't in any golden; goldens are headless event traces). The only
detector is the eyeball / screenshot. **Drop the `* 0.5`.** `center()` is unchanged.

**Compile-and-behavior caveat:** this is the **only** code path in the migration that can pass
`cargo build` + `cargo test` while being visually wrong. It is also `debug-gizmos`-gated, so a
default build won't even compile it. Verify with the playground/screenshot (Area 6 mitigation).

### 5c. SpatialQuery rewrite — `spatial/detect.rs` — **VERDICT: NOT A REWRITE.**

This was flagged as "THE BIG ONE." After auditing the actual call sites against wisp's 0.5 code,
**there is no rewrite.** The `SpatialQuery::shape_intersections` and `SpatialQuery::cast_ray`
signatures are **byte-identical** between Avian 0.4 and 0.5. The cone/hitbox detection logic does
**not** need to be re-expressed at all.

**Current form (`detect.rs::detect_overlaps`, line 19) — UNCHANGED in 0.5:**
```rust
let hits = spatial.shape_intersections(
    &collider,
    hb_tf.translation,
    hb_tf.rotation,
    &SpatialQueryFilter::default(),
);
```

**0.5 reference proving the signature is identical** (`/Users/luke/src/wisp/src/spells/deliveries.rs:227`):
```rust
spatial.shape_intersections(&collider, origin_pos, Quat::IDENTITY, &filter)
//                          ^Collider   ^Vec3       ^Quat          ^&SpatialQueryFilter → Vec<Entity>
```
Same 4-arg shape: `(&Collider, translation: Vec3, rotation: Quat, &SpatialQueryFilter) -> Vec<Entity>`.
The `detect.rs` tests (`detect_sys`, `move_probe_sys`) call it the same way and need **no change**.

**`cast_ray` (used by `facade/spatial.rs`, not `detect.rs`) — also identical in 0.5**
(`/Users/luke/src/wisp/src/net/server.rs:485,541,791`, `src/spells/convex_lens.rs:530`):
```rust
spatial.cast_ray(origin, Dir3::new(delta).unwrap_or(Dir3::Z), range, true, &SpatialQueryFilter::default())
//               ^Vec3   ^Dir3                                 ^f32   ^bool ^&filter → Option<RayHitData>
```

**The cone/hitbox detection stays exactly as written:** `detect_overlaps` does a broad-phase
`shape_intersections` to get candidate entities, then applies the **cone narrow-phase in pure Rust**
(`crate::spatial::cone::point_in_cone`, lines 35-46) — that cone math is obelisk-bevy's own code, not
Avian's, so Avian's version has zero bearing on it. Faction filter, `HitMode` dedupe
(`hitbox.can_hit` / `register_hit`), and the `HitConfirmed` trigger are likewise all crate-local.

**So the "rewrite" reduces to: confirm the broad-phase returns the same candidate set in 0.5.** That
is a *behavioral* confirmation (run the spatial + golden suites), not an API edit. **The entire risk
of this file is whether `shape_intersections` returns the identical entity set for the identical
inputs** — see Area 6, risk #1 (the top silent-behavior risk in the whole migration).

**Compile signal for Area 5:** `cargo build` + `cargo build --no-default-features` +
`cargo build --features debug-gizmos` all compile (the `half_size()` edit is the only thing that
*could* error if the method name is wrong). **This area's behavior is test-gated, not compile-gated.**

---

## AREA 6 — Verification gate (the runtime truth)

The crate compiles after Areas 0–5. Now prove behavior is unchanged. **Run in this order**; do not
declare done until every command below is green and the goldens match without regeneration.

### Gate commands (exact)

```bash
# 1. Both build configurations compile (client + headless/server).
cargo build                                   # default features (present on)
cargo build --no-default-features             # presentation compiled out (server build)

# 2. The full suite (84 tests across lib + integration tests).
cargo test --features test-support --lib --tests

# 3. THE BACKBONE — 39 golden traces, must match WITHOUT regeneration.
cargo test --features test-support --test golden

# 4. Lint + format (must be clean for obelisk-bevy's own crate; stat_core dead-code warnings are the dep's).
cargo clippy --features test-support --lib --tests -- -D warnings
cargo fmt --check

# 5. Examples must build (the only place a render-API break surfaces).
cargo build --example playground --features debug-gizmos
cargo build --example screenshot  --features debug-gizmos
cargo build --example headless_server --no-default-features

# 6. Visual confirmation of the ONE silent-risk edit (gizmo size) — agent-readable:
cargo run --example screenshot --features debug-gizmos -- --scenario firebolt_kill --tick 24
#   then Read screenshots/firebolt_kill-24.png and confirm hurtbox spheres ≈ collider radius (~0.5),
#   not half that. (Goldens do NOT cover gizmo size — this is the only detector.)
```

### Order to bring tests back green

1. **Make it compile** (Areas 0–5). Until `cargo build` + `cargo build --no-default-features` pass,
   no test runs. This is pure compile-area iteration.
2. **Spatial-determinism canaries first.** Run the `spatial/detect.rs` unit tests
   (`spatial_query_finds_an_overlapping_hurtbox`, `hurtbox_tracks_a_moving_owner`) — these prove
   Avian 0.5 `shape_intersections` + static-body Transform tracking still behave. If these fail, the
   goldens *will* fail; fix here first.
   ```bash
   cargo test --features test-support --lib spatial::detect
   ```
3. **`tests/spatial_targeting.rs`** (facade `cast_ray`/cone target acquisition) — confirms
   `facade/spatial.rs` ray + cone paths under 0.5.
4. **`tests/determinism.rs`** — cross-seed divergence + same-seed idempotence. Proves `CombatRng`
   ordering + observer/trigger dispatch order is preserved under 0.18. Must pass before goldens.
5. **The golden suite** (`--test golden`). If a golden diffs, **do NOT blind-regenerate.** A diff is
   a behavior change to justify. If (and only if) it's an *intended* Avian/Bevy numerical artifact,
   `UPDATE_GOLDEN=1 cargo test … --test golden`, `git diff tests/golden/` to review, and record the
   rationale. For a pure plumbing migration, the **expectation is zero golden diffs.**
6. **The remaining integration tests** (`facades.rs`, `netcode.rs`, `vertical_slice.rs`,
   `vfx_content.rs`) round out the 84.
7. **clippy + fmt**, then **examples build**, then the **screenshot eyeball** for gizmo size.

---

## AREA 7 — Suggested task decomposition (ordered, bite-sized)

Each task is a coherent compile-area or the final gate. Marked **[compile-only]** (progress is
"it compiles", no test feedback yet) vs **[test-gated]** (hits a runnable check).

1. **T1 — Bump deps.** `Cargo.toml`: `bevy 0.18`, `avian3d 0.5`. First 0.18/avian-0.5 fetch + full
   error list. **[compile-only]** (everything below depends on this).
2. **T2 — Observers/`On<E>` verify-sweep.** Confirm all ~14 observer files compile under 0.18; audit
   for exclusive-system-as-observer (expect none) and global-event `.event()` reads. **[compile-only]**
3. **T3 — Required components verify.** `core/components.rs` `Combatant` `#[require]`; grep for Bevy
   `States` usage (expect none). **[compile-only]**
4. **T4 — Messages/`add_message` verify-sweep.** `net.rs`, `scenario/trace.rs`, `headless_server.rs`
   message plumbing compiles. **[compile-only]**
5. **T5 — App/plugin/schedule verify.** `lib.rs` set config + `examples/headless_server.rs`. Confirm
   no executor/combinator/state-API breakage. **[compile-only]**
6. **T6 — Avian collider/AABB edit.** The `size()`→`half_size()` change **with the `* 0.5` removed**
   in `present/debug_viz.rs`; confirm `boxes.rs`/`shapes.rs`/`scenario/mod.rs` collider/RigidBody
   calls compile. **[compile-only]** (the visual correctness is verified later in T9).
7. **T7 — present/UI + examples render.** `present/{mod,debug_viz}.rs` Text/HUD/gizmos +
   `examples/{playground,screenshot}.rs`. Resolve any `RenderTarget`/`Text`/`LineHeight` breakage.
   Make `cargo build`, `cargo build --no-default-features`, and `--examples` all compile.
   **[compile-only]** (last compile-area; after this the whole crate builds).
8. **T8 — Spatial-determinism canary tests.** Run `spatial::detect` unit tests +
   `tests/spatial_targeting.rs` + `tests/determinism.rs`. First **[test-gated]** task — proves
   SpatialQuery + RNG + dispatch order survived the bump.
9. **T9 — Full suite + goldens + lint/fmt + screenshot eyeball.** `cargo test … --lib --tests`,
   `--test golden` (expect zero diffs), `clippy -D warnings`, `fmt --check`, build all examples, and
   `Read` the firebolt screenshot to confirm gizmo radius. **[test-gated — the final gate]**.

(Optional **T0** if the `feat/combat-result-trigger-metadata` branch is in play: rebase decision per
Area 0 — sequence it *before* T1 if that branch must carry, resolving `combat/system.rs` + `events.rs`
in favor of the metadata payloads + this migration's derive spelling.)

**Independence note:** T2–T7 are all *compile-only* and only become individually meaningful once
T1 lands and the crate is being driven to compile; they are best done as one continuous compile-fix
pass (the error list from T1 dictates order in practice). T8 and T9 are the only tasks with real
test feedback — they are where regressions actually surface.

---

## AREA 8 — Risks / silent-behavior watch (the real danger)

Ordered by how likely they are to **pass compile but change a golden or a visual** (the migration's
true hazard). The first three are the top-3 to call out.

1. **[TOP] SpatialQuery `shape_intersections` candidate-set identity.** The signature is identical
   0.4→0.5, so `detect.rs` compiles untouched — but the *detection result set* feeds **every combat
   golden** (`firebolt_kill`, `cone_cleave`, `faction_filter`, `everytick_hitbox`, …). If Avian 0.5's
   broad-phase returns even a slightly different candidate set (ordering, inclusivity at exact
   boundary contact, sensor/static handling) the cone narrow-phase still runs but on different
   candidates → different `HitConfirmed` → different `DamageResolved` → **golden diff with no compile
   error.** *Detector:* the golden suite + `spatial::detect` unit tests. *Mitigation:* run T8 before
   T9; treat any golden diff as a behavior regression to investigate, never auto-regenerate.

2. **[TOP] Avian `ColliderAabb::half_size()` multiplier trap (gizmo size).** Renaming `size()`→
   `half_size()` while keeping `* 0.5` halves the rendered hurtbox radius — **no compile error, no
   test failure** (gizmos are `debug-gizmos`-gated and absent from all goldens; goldens are headless
   event traces). *Detector:* only the screenshot/playground eyeball. *Mitigation:* drop the `* 0.5`
   (Area 5b) and `Read` the firebolt screenshot in T9 to confirm radius ≈ collider radius (~0.5).

3. **[TOP] Static-body Transform tracking under Avian 0.5.** CLAUDE.md's hurtbox design *depends* on
   `RigidBody::Static` re-reading the owner's `Transform` each step (static ≠ frozen) so hurtboxes
   follow moving owners. If 0.5 changed this, `hurtbox_tracks_a_moving_owner` fails and moving-target
   goldens shift. *Detector:* the `hurtbox_tracks_a_moving_owner` + `spatial_query_finds_an_overlapping_hurtbox`
   unit tests (run first in T8). *Mitigation:* they are the explicit canary — run them before anything else.

4. **Buffered `Messages<T>` drain ordering (netcode goldens).** `net.rs` writes multiple `NetEvent`s
   per frame; `scenario/trace.rs` drains them into the `netcode_egress` golden. If 0.18 changed
   `Messages<T>` double-buffer/drain order, the net trace lines reorder → golden diff, no compile
   error. *Detector:* `netcode_egress` golden + `tests/netcode.rs`. *Mitigation:* T8/T9 golden gate.

5. **Observer/trigger dispatch order vs determinism.** The sim resolves combat via observers
   (`commands.trigger` → `On<E>`) that flush within `update()`. Determinism (and the goldens) require
   the **exact same event order** as 0.17. The 0.18 observer rearchitecture (rename + immutable entity
   events) is claimed order-preserving for global events, but it is unproven here. *Detector:*
   `tests/determinism.rs` (same-seed idempotence) + the goldens. *Mitigation:* run `determinism.rs`
   in T8 before goldens.

6. **`Time<Fixed>` fixed-timestep rounding drift.** Phase/window durations are computed from
   `time.delta_secs()` on `Time<Fixed>`. If 0.18 changed fixed-timestep accumulation/rounding,
   `instant_cast` (~tick-2 window) vs `firebolt` (~tick-19 window) timing could shift a tick →
   golden diff. *Detector:* the timing-sensitive goldens. *Mitigation:* golden gate; the
   `{:.3}`-precision trace makes a 1-tick shift visible.

7. **5-set FixedUpdate ordering equivalence.** `Validate → Advance → Projectiles → ResolveHits →
   TickEffects` is `.chain()`-ed. 0.18 schedule-build internals changed (`DiGraphToposortError`,
   `SimpleExecutor` removed). Same-set ordering should be preserved, but a different intra-set tie-break
   would reorder same-tick events. *Detector:* goldens. *Mitigation:* golden gate; if a diff appears,
   check whether two systems in the same set lost their relative order.

8. **`screenshot.rs` `RenderTarget`/`Text`/headless capture on Metal.** The off-screen PNG path
   (`RenderTarget::Image`, render-graph composition) leans on Bevy graphics internals that 0.18
   restructured (RenderTarget split off `Camera`). Most likely place for an *example* compile break,
   and the headless-capture-on-Metal path could regress at runtime even if it compiles. *Detector:*
   `cargo build --example screenshot` + actually running the screenshot command in T9. *Mitigation:*
   cross-check against Bevy 0.18's `headless_renderer` example if it errors.

9. **Lower-likelihood official-guide silent changes — confirmed NOT applicable, listed so a reviewer
   need not re-derive:** `BorderRadius` removed from `Node` (not used in HUD), system run-condition
   combinators returning `false` instead of erroring (crate uses `.chain()`/`.in_set()`, not
   `and`/`or`/`xor`), `EntityRef`/`EntityWorldMut` `EntityNotSpawnedError` (crate's `EntityWorldMut`
   closures in `verbs.rs` operate on freshly-spawned/known-live entities), `Internal` component
   removal making engine entities visible in queries (crate's queries filter on gameplay components
   like `With<Hurtbox>`/`With<Projectile>`, not on hidden-state), state-transition double-fire (no
   Bevy `States` in the sim), `try_insert_attribute` Result (no manual mesh attribute insertion),
   `Reflect`-attribute paren-only syntax (crate uses no `#[reflect(...)]` attrs in the migrated path).
   Each is a 1-line grep to reconfirm during T2–T7.
