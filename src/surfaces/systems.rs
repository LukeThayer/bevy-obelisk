//! Surface behavior systems: the Trail painter, the OnEnd paint observer (Task 4), standing
//! payloads (Task 5), and skill-contact triggers (Task 6). DETERMINISM: every loop that fires
//! paints/triggers iterates in sorted order (hitboxes by `Entity::index()`, patches by `seq`);
//! nothing here draws RNG.
use bevy::prelude::*;

use crate::assets::{CastTimeline, CastTimelineHandles, PaintMode, PaintSpec};
use crate::core::components::{Attributes, Combatant, Faction};
use crate::events::HitboxEnded;
use crate::spatial::boxes::Hitbox;
use crate::spatial::filter::passes_filter;
use crate::surfaces::patch::{patch_contains, try_paint, SurfacePatch, SurfaceSeq};
use crate::surfaces::types::SurfaceRegistry;
use crate::timeline::triggered::{execute_skill_timeline, ExecPayload};
use crate::verbs::ObeliskCommandsExt;
use std::collections::{HashMap, HashSet};

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
    combatants: Query<(Entity, &Transform, &Faction, &Attributes), With<Combatant>>,
) {
    let now = time.elapsed_secs();
    // Collect + sort overlaps deterministically.
    let mut overlaps: Vec<(u64, Entity, &SurfacePatch, Vec3, Entity, Vec3, Faction)> = Vec::new();
    let mut inside_now: HashSet<(Entity, Entity)> = HashSet::new();
    for (pe, patch, ptf) in &patches {
        for (ve, vtf, vf, attrs) in &combatants {
            // Precedent: tick_effects_system (core/tick.rs) gates on is_alive() because death
            // doesn't despawn. Without this gate a corpse standing in a persistent surface keeps
            // receiving tick_skill hits, which re-trigger EntityDied and the (non-idempotent)
            // loot observer every rehit_interval.
            if !attrs.0.is_alive() {
                continue;
            }
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
