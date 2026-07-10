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
    existing: &Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>,
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
    existing: Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>,
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
