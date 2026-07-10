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
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
pub fn on_hitbox_ended_paint(
    ev: On<HitboxEnded>,
    registry: Res<SurfaceRegistry>,
    mut seq: ResMut<SurfaceSeq>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    factions: Query<&Faction>,
    existing: Query<(Entity, &SurfacePatch, &Transform), Without<Hitbox>>,
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
