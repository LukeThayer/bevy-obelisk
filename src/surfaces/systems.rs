//! Surface behavior systems: the Trail painter, the OnEnd paint observer (Task 4), standing
//! payloads (Task 5), and skill-contact triggers (Task 6). DETERMINISM: every loop that fires
//! paints/triggers iterates in sorted order (hitboxes by `Entity::index()`, patches by `seq`);
//! nothing here draws RNG.
use bevy::prelude::*;

use crate::assets::{CastTimeline, CastTimelineHandles, PaintMode, PaintSpec};
use crate::core::components::{Attributes, Combatant, Faction};
use crate::core::config::SkillRegistry;
use crate::events::HitboxEnded;
use crate::spatial::boxes::Hitbox;
use crate::spatial::filter::passes_filter;
use crate::surfaces::patch::{
    patch_contains, try_paint, SurfacePatch, SurfaceRemoveReason, SurfaceRemoved, SurfaceSeq,
};
use crate::surfaces::types::SurfaceRegistry;
use crate::timeline::triggered::{execute_skill_timeline, ExecPayload};
use crate::verbs::ObeliskCommandsExt;
use stat_core::types::SkillTag;
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
    // ONE eviction ledger for the whole run: a burst of same-type paints at cap (two Trail
    // hitboxes in one `paint_surfaces` tick) shares it so they cannot both evict the same oldest
    // patch off the stale `existing` snapshot (I1 — see `try_paint`).
    let mut evicted: HashSet<Entity> = HashSet::new();
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
                    &mut evicted,
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
                        &mut evicted,
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
    // Fresh per invocation (same blindness as today): residual same-tick cross-OBSERVER bursts at
    // cap remain snapshot-blind; rare, deterministic, tracked for the arena increment.
    let mut evicted: HashSet<Entity> = HashSet::new();
    try_paint(
        &mut commands,
        &registry,
        &mut seq,
        &existing,
        &mut batch,
        &mut evicted,
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
    let mut overlaps: Vec<(u64, Entity, &SurfacePatch, Entity, Vec3, Faction)> = Vec::new();
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
                    ve,
                    vtf.translation,
                    *vf,
                ));
            }
        }
    }
    overlaps.sort_by_key(|(seq, _, _, ve, _, _)| (*seq, ve.index()));

    let mut fired_this_tick: HashSet<(Entity, String)> = HashSet::new();
    for (_, pe, patch, victim, victim_pos, victim_faction) in overlaps {
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

/// Surface types this hitbox has already reacted with — the once-per-(hitbox, surface-type)
/// guard (spec §5.2: first contact only; no fire-propagation in v1).
#[derive(Component, Debug, Default)]
pub struct SurfaceContacts(pub std::collections::HashSet<String>);

/// Resolve a surface reaction's authored tag string (snake_case, matching stat_core's `SkillTag`
/// serde form — e.g. `"fire"`) to a [`SkillTag`]. ADAPTATION vs the brief: skill rules carry
/// `Vec<SkillTag>` (an enum), whereas a surface's `on_skill_contact.tags_any` is authored as loose
/// strings, so the string is resolved to the enum for matching (the brief assumed `tags: Vec<String>`
/// and a direct `st == t`). An unknown tag string resolves to `None` and matches nothing.
pub(crate) fn parse_skill_tag(s: &str) -> Option<SkillTag> {
    use serde::de::{value::StrDeserializer, IntoDeserializer, Deserialize};
    let de: StrDeserializer<serde::de::value::Error> = s.into_deserializer();
    SkillTag::deserialize(de).ok()
}

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
                    .filter_map(|t| parse_skill_tag(t))
                    .any(|want| skill.tags.contains(&want));
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
