//! obelisk-bevy: a Bevy 0.17 plugin exposing the obelisk ARPG libraries.
//!
//! # API notes — confirmed against bevy 0.17.3 / avian3d 0.4.1 (probe 2026-06-05)
//!
//! ## (A) Required components with non-Default value
//! `#[require(Health)]` works as written: Bevy calls `Health::default()` automatically
//! when spawning a `Unit` without an explicit `Health`. No literal expression needed.
//! ```rust,ignore
//! #[derive(Component, Default)]
//! #[require(Health)]
//! struct Unit;
//! #[derive(Component)]
//! struct Health(f32);
//! impl Default for Health { fn default() -> Self { Health(100.0) } }
//! ```
//!
//! ## (B) Observer-triggered event
//! - Derive: `#[derive(Event, Clone)]`
//! - Trigger: `commands.trigger(MyEvent { .. })` — fires a global (entity-less) observer.
//! - System param: `On<MyEvent>` (NOT `Trigger<MyEvent>`). Access via `ev.event()`.
//! - Register: `app.add_observer(my_observer_fn)`.
//! - Observer fires during the `Startup` system's `app.update()` call (update 1).
//! ```rust,ignore
//! fn on_ping(ev: On<Ping>) { let _ = ev.event().who; }
//! app.add_observer(on_ping);
//! ```
//!
//! ## (C) Time<Fixed> fixed delta accessors
//! - `time.delta_secs() -> f32`  (= 1/60 ≈ 0.016666668 at 60 Hz)
//! - `time.delta_secs_f64() -> f64` (= 0.016666667)
//!   Both confirmed to compile and return correct values.
//! ```rust,ignore
//! fn fixed_sys(time: Res<Time<Fixed>>) {
//!     let _f32: f32 = time.delta_secs();
//!     let _f64: f64 = time.delta_secs_f64();
//! }
//! ```
//!
//! ## (D) SpatialQuery::shape_intersections
//! Exact signature (avian3d 0.4):
//! ```rust,ignore
//! fn detect(spatial: SpatialQuery) {
//!     let hits: Vec<Entity> = spatial.shape_intersections(
//!         &Collider::sphere(0.5),   // shape: &Collider
//!         Vec3::ZERO,               // origin: Vec3
//!         Quat::IDENTITY,           // rotation: Quat
//!         &SpatialQueryFilter::default(),
//!     );
//! }
//! ```
//! A `RigidBody::Static` + `Collider::sphere(0.5)` spawned before `finish()`/`update()` is
//! NOT visible in the first fixed-tick of the second `app.update()` call (hits=0), but IS
//! visible in every subsequent fixed tick (hits=1). Pattern: call `app.update()` twice
//! before asserting SpatialQuery finds a freshly-spawned static collider.
//!
//! ## Collider constructors confirmed
//! - `Collider::sphere(radius: f32) -> Collider`
//! - `Collider::capsule(radius: f32, length: f32) -> Collider`  ← radius first, length second
//!
//! ## PhysicsPlugins (avian3d 0.4) headless app setup
//! `PhysicsPlugins::new(FixedUpdate)` compiles and works as expected.
//! When driving `App` manually (not via `app.run()`), the following plugins are required
//! alongside `MinimalPlugins` + `AssetPlugin::default()` for avian to initialize cleanly:
//! - `bevy::mesh::MeshPlugin`   — registers `Mesh` as an asset (needed by `ColliderCachePlugin`)
//! - `bevy::scene::ScenePlugin` — registers `SceneSpawner` (needed by avian collider hierarchy)
//! - `bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(100))` as a resource
//!   to make `FixedUpdate` accumulate deterministic time in tests.
//! - `app.finish(); app.cleanup();` MUST be called before the first `app.update()` when
//!   constructing an `App` manually; `run()` does this internally but `update()` does not.

use bevy::app::{PluginGroup, PluginGroupBuilder};
use bevy::prelude::*;

pub mod assets;
pub mod combat;
pub mod core;
pub mod events;
pub mod facade;
pub mod ids;
pub mod loot;
pub mod net;
pub mod prelude;
#[cfg(feature = "present")]
pub mod present;
pub mod scenario;
pub mod spatial;
#[cfg(feature = "test-support")]
pub mod testkit;
pub mod timeline;
pub mod verbs;
pub mod vfx;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObeliskSet {
    Validate,
    Advance,
    SpawnVolumes,
    Projectiles,
    ResolveHits,
    TickEffects,
}

/// The headless authoritative simulation: core + assets + spatial + timeline + combat.
pub struct ObeliskSimPlugin;
impl Plugin for ObeliskSimPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(assets::ObeliskAssetsPlugin)
            .add_plugins(spatial::ObeliskSpatialPlugin)
            .add_plugins(core::ObeliskCorePlugin)
            .add_plugins(combat::ObeliskCombatPlugin)
            .add_plugins(net::ObeliskNetPlugin)
            .add_plugins(vfx::ObeliskCuePlugin)
            .add_plugins(loot::ObeliskLootPlugin);

        app.configure_sets(
            FixedUpdate,
            (
                ObeliskSet::Validate,
                ObeliskSet::Advance,
                ObeliskSet::Projectiles,
                ObeliskSet::ResolveHits,
                ObeliskSet::TickEffects,
            )
                .chain(),
        );

        app.add_systems(
            FixedUpdate,
            (
                timeline::advance::validate_casts.in_set(ObeliskSet::Validate),
                (
                    timeline::advance::advance_casts,
                    // Task 11: ticks every live hitbox's emitter (if any), spawning Template
                    // instances. `tick_emitters` and `end_hitboxes` both mutate `Hitbox` — a
                    // real query conflict Bevy must serialize — so ONLY this pair is `.chain()`d,
                    // end BEFORE tick (despawn commands are deferred, so a hitbox reaped this
                    // tick still gets one last emit chance before it's actually gone next sync
                    // point). `advance_casts`/`advance_triggered_execs` are deliberately left
                    // OUTSIDE the chain, unordered relative to the rest exactly as before this
                    // task — chaining `tick_emitters` BEFORE `end_hitboxes` (the seemingly more
                    // "natural" reading order) instead reordered `end_hitboxes` relative to
                    // `advance_casts`/the ResolveHits set's sync boundary and perturbed
                    // `everytick_hitbox`'s golden (its `EveryTick` hitbox's fuse-out tick lost its
                    // final overlap hit). No existing content authors an emitter, so
                    // `tick_emitters` itself is a no-op for every current golden either way —
                    // this ordering is scheduling-only, not a behavior change.
                    (
                        timeline::advance::end_hitboxes,
                        timeline::advance::tick_emitters,
                    )
                        .chain(),
                    timeline::triggered::advance_triggered_execs,
                )
                    .in_set(ObeliskSet::Advance),
                spatial::projectile::move_projectiles.in_set(ObeliskSet::Projectiles),
                (
                    spatial::detect::detect_overlaps,
                    spatial::detect::resolve_beam_hits,
                )
                    .in_set(ObeliskSet::ResolveHits),
            ),
        );
        // Host-fired world impacts feed the `end_hitboxes` funnel via a marker component.
        app.add_observer(timeline::advance::on_hitbox_world_hit);
    }
}

/// Umbrella plugin group.
pub struct ObeliskPlugins;
impl PluginGroup for ObeliskPlugins {
    fn build(self) -> PluginGroupBuilder {
        #[allow(unused_mut)]
        let mut b = PluginGroupBuilder::start::<Self>().add(ObeliskSimPlugin);
        #[cfg(feature = "present")]
        {
            b = b.add(present::ObeliskPresentPlugin);
        }
        b
    }
}
