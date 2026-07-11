//! Surface PATCH: one painted circle splat as a plain sim entity (spec D6 — entities so the
//! editor previews them natively and the arena replicates them like skill objects). Positions
//! ride `Transform`; expiry is a fixed-tick countdown (`remaining`, the hitbox pattern);
//! `seq` (from [`SurfaceSeq`]) is the deterministic replace-oldest eviction key.
use bevy::prelude::*;

use crate::core::components::Faction;
use crate::spatial::boxes::Hitbox;
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

/// Same-tick paint/evict dedup state shared by EVERY paint entry point (the trail system, the
/// OnEnd observer, and `PaintSurface` request observers — which each run as separate invocations
/// within one tick and were previously blind to each other's deferred spawns/despawns).
/// Reset ONCE per sim tick (see [`clear_surface_tick_scratch`]); never serialized; deterministic
/// (insertion-order-free: only membership queries, plus the paint batch which is scanned linearly).
#[derive(Resource, Default)]
pub struct SurfaceTickScratch {
    /// Patches despawn-queued by eviction this tick (the evict-once guard).
    pub evicted: std::collections::HashSet<Entity>,
    /// (surface id, position) pairs paint-queued this tick (the merge-dedup batch).
    pub painted: Vec<(String, Vec3)>,
}

/// Reset the shared [`SurfaceTickScratch`] for the next sim tick — called at the END of
/// `apply_standing_payloads` (the last surfaces system in the tick), AFTER every FixedUpdate paint
/// site (`on_hitbox_ended_paint` in Advance, `paint_surfaces` in ResolveHits) has populated it, so
/// a whole tick's paints share one ledger and the next tick starts empty.
///
/// WHY a folded helper, not a dedicated system: registering a standalone clear system in ANY
/// FixedUpdate set adds a node to the schedule graph, which perturbs Bevy's topological tie-break
/// between the deliberately-unordered `advance_casts` / `advance_triggered_execs` (see the Task-11
/// note in `lib.rs`) and shifts golden event ORDER (a scheduling artifact — same paints, damage,
/// counts). Folding the reset into an existing node changes no edges and keeps every golden
/// byte-identical. Clearing at tick END (vs the top) is equivalent — nothing paints after this —
/// and, as a bonus, leaves no cross-tick residue for the between-tick request observers.
///
/// CAVEAT (outside FixedUpdate): a `PaintSurface` triggered from the Update schedule — e.g. the
/// editor palette's instant paint — shares the scratch of the ENCLOSING FixedUpdate tick boundary
/// rather than getting its own reset. While the sim keeps ticking normally this is harmless: the
/// scratch still clears at that tick's end like any FixedUpdate-triggered paint, so it holds at
/// most one tick's worth of stale membership entries, and a stale `evicted` entry names an
/// already-despawned entity (distinct Entity generation — never matches a live patch).
///
/// CAVEAT (host-gated schedules): the clear runs WITH the surfaces systems — a host that gates
/// those sets off (the editor's frozen scrub) while still triggering `PaintSurface` from an
/// ungated schedule (e.g. the editor's Reset re-applying patches) accumulates stale `painted`
/// entries across the whole gated window instead of at most one tick's worth. A despawn-then-
/// repaint at the same position during that window is then wrongly suppressed — the committed
/// `existing` query no longer sees the despawned patch, but the stale batch entry does — though
/// it self-heals once the sim ticks again. Hosts doing gated-window paint cycles should clear
/// this resource themselves first (the editor's reset does).
pub fn clear_surface_tick_scratch(scratch: &mut SurfaceTickScratch) {
    scratch.evicted.clear();
    scratch.painted.clear();
}

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

pub fn xz_dist(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// The 2.5D membership test: XZ within `radius`, Y within [`SURFACE_Y_TOLERANCE`].
pub fn patch_contains(patch_pos: Vec3, radius: f32, p: Vec3) -> bool {
    xz_dist(patch_pos, p) <= radius && (p.y - patch_pos.y).abs() <= SURFACE_Y_TOLERANCE
}

/// Spawn one patch, enforcing the type's `merge_radius` dedup and `max_patches` replace-oldest
/// cap. `scratch.painted` self-dedups paints queued in the SAME tick (deferred spawns aren't
/// visible in `existing` yet) and `scratch.evicted` guards each patch against a double-evict —
/// both shared across every paint entry point this tick (see [`SurfaceTickScratch`]). The cap is
/// enforced against COMMITTED patches, so a burst tick can transiently overshoot by its own paint
/// count — it converges on the next paint (documented v1 behavior). Returns the spawned entity,
/// or `None` when deduped/unknown.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_paint(
    commands: &mut Commands,
    registry: &SurfaceRegistry,
    seq: &mut SurfaceSeq,
    existing: &Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>,
    scratch: &mut SurfaceTickScratch,
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
    let near_batch = scratch
        .painted
        .iter()
        .any(|(s, pos)| s == surface_id && xz_dist(*pos, position) < st.merge_radius);
    if near_existing || near_batch {
        return None;
    }
    // Replace-oldest cap (committed patches only — see fn doc). I1: the `existing` snapshot never
    // reflects THIS-tick deferred despawns, so a same-type patch already evicted by an earlier
    // paint in this run (or a separate invocation earlier this tick) still shows up here. Exclude
    // anything in `scratch.evicted` — it is as-good-as-gone, so it is neither re-selected as a
    // victim NOR counted toward the live total (without this, two burst paints at cap both pick the
    // same oldest patch → double-fire).
    let mut same: Vec<(Entity, u64, Vec3, String)> = existing
        .iter()
        .filter(|(e, p, _)| p.surface == surface_id && !scratch.evicted.contains(e))
        .map(|(e, p, tf)| (e, p.seq, tf.translation, p.surface.clone()))
        .collect();
    if same.len() + 1 > st.max_patches {
        same.sort_by_key(|(_, s, _, _)| *s);
        for (e, _, pos, surf) in same.iter().take(same.len() + 1 - st.max_patches) {
            // Insert-guarded: even if the filter above ever misses, a second attempt to evict the
            // same entity is a structural no-op — no duplicate `SurfaceRemoved{Evicted}`, no double
            // `despawn` warn. This is the last-line guarantee the removal-event stream stays clean.
            if scratch.evicted.insert(*e) {
                commands.trigger(SurfaceRemoved {
                    patch: *e,
                    surface: surf.clone(),
                    position: *pos,
                    reason: SurfaceRemoveReason::Evicted,
                });
                commands.entity(*e).despawn();
            }
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
    scratch.painted.push((surface_id.to_string(), position));
    Some(patch)
}

/// Observer for the public [`PaintSurface`] request.
pub fn on_paint_surface(
    ev: On<PaintSurface>,
    registry: Res<SurfaceRegistry>,
    mut seq: ResMut<SurfaceSeq>,
    // Shared across every paint entry point THIS tick (reset once per tick — see
    // `clear_surface_tick_scratch`): two same-tick `PaintSurface` triggers run as separate observer
    // invocations but dedup/evict against ONE ledger.
    mut scratch: ResMut<SurfaceTickScratch>,
    existing: Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>,
    factions: Query<&Faction>,
    mut commands: Commands,
) {
    let e = ev.event();
    let owner_faction = factions.get(e.owner).copied().unwrap_or_default();
    try_paint(
        &mut commands,
        &registry,
        &mut seq,
        &existing,
        &mut scratch,
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
