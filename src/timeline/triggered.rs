//! Task 6 — the public triggered-timeline executor (spec §3.2): the machinery later trigger
//! tasks call when a trigger names a skill with a timeline. Spawns that skill's collision
//! windows at an arbitrary world position, on a virtual clock, as a free sub-cast — WITHOUT a
//! caster cast-state (no `PendingCast`/`ActiveCast`, no validation gates, no phases on the
//! caster). Window origins resolve per the authored anchor (Task 9): `CastPoint` = the
//! execution's payload position, `Caster` = the caster entity's position at spawn time; the
//! window's `anchor_offset` applies on top in world axes.
use crate::assets::{CastTimeline, CastTimelineHandles, WindowAnchor, WindowSpawn};
use crate::timeline::advance::{spawn_window_hitbox, ChainPayload};
use bevy::prelude::*;

/// Trigger-generation recursion cap: a triggered timeline whose own windows chain to further
/// triggers (later tasks) must not recurse forever. `execute_skill_timeline` refuses to spawn
/// anything at or past this depth.
pub const MAX_TRIGGER_DEPTH: u8 = 8;

/// Everything `execute_skill_timeline` needs to place a skill's timeline in the world, detached
/// from any caster's cast-state.
#[derive(Clone, Copy, Debug)]
pub struct ExecPayload {
    /// The execution's CAST POINT: what `WindowAnchor::CastPoint` windows resolve against
    /// (the trigger's hit / impact / expiry position).
    pub position: Vec3,
    /// Facing direction handed to `spawn_window_hitbox` (projectile heading / cone axis).
    pub direction: Vec3,
    /// Designated beam target, if any. `Some` -> passed through as `ChainPayload::beam_target`
    /// for beam windows; `None` -> a beam window spawns windowless-target (paid-fizzle/no-op).
    pub target: Option<Entity>,
    /// Per-execution charge, forwarded to `spawn_window_hitbox` unchanged.
    pub charge: Option<u8>,
    /// Trigger-generation depth this execution runs at (0 = a player cast). Stamped onto every
    /// spawned `Hitbox` so downstream billing (Task 5) treats its hits as free.
    pub depth: u8,
}

/// A skill timeline running on its own virtual clock, detached from any caster's `ActiveCast`.
/// Ticked by [`advance_triggered_execs`], which spawns each `Scheduled` window's hitbox once
/// its (unscaled) start time elapses, then despawns itself once every window has spawned.
#[derive(Component, Debug)]
pub struct TriggeredExec {
    pub caster: Entity,
    pub skill_id: String,
    pub payload: ExecPayload,
    pub elapsed: f32,
    /// Parallel to the timeline's `collision_windows` (by index): whether that window has
    /// already spawned (or, for `Template` windows, been permanently skipped). Lazily sized to
    /// the timeline's window count on first tick (the timeline asset may not be loaded yet when
    /// `execute_skill_timeline` runs — it only has `&mut Commands`, no asset access).
    pub spawned: Vec<bool>,
}

/// Queue skill `skill_id`'s timeline to execute at `payload` as a free sub-cast, attributed to
/// `caster`. Spawns a data-only `TriggeredExec` entity; `advance_triggered_execs` does the
/// actual window spawning. At `payload.depth >= MAX_TRIGGER_DEPTH`, warns once and spawns
/// nothing (recursion cap).
pub fn execute_skill_timeline(
    commands: &mut Commands,
    caster: Entity,
    skill_id: &str,
    payload: ExecPayload,
) {
    if payload.depth >= MAX_TRIGGER_DEPTH {
        warn!(
            "execute_skill_timeline: depth {} >= MAX_TRIGGER_DEPTH ({MAX_TRIGGER_DEPTH}) for \
             skill '{skill_id}' — dropping triggered execution",
            payload.depth
        );
        return;
    }
    commands.spawn(TriggeredExec {
        caster,
        skill_id: skill_id.to_string(),
        payload,
        elapsed: 0.0,
        spawned: Vec::new(),
    });
}

/// Ticks every [`TriggeredExec`]'s virtual clock by the fixed delta and spawns each not-yet-
/// spawned window whose (unscaled) start time has elapsed, at its resolved anchor (`CastPoint`
/// = `payload.position`, `Caster` = the caster's position now — falling back to the payload
/// position if the caster has no `Transform` — plus the authored `anchor_offset`). `Template`
/// windows are never on the schedule (same rule as `advance_casts`) — permanently skipped.
/// Despawns the exec entity once every window has spawned (or been skipped). An unknown skill id
/// (no handle registered) `warn!`s and drops the exec rather than ticking forever; a registered
/// handle whose `CastTimeline` asset hasn't streamed in yet retries next tick without advancing
/// `elapsed` (the virtual clock starts once the asset resolves).
pub fn advance_triggered_execs(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut execs: Query<(Entity, &mut TriggeredExec)>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
) {
    let dt = time.delta_secs();
    for (entity, mut exec) in &mut execs {
        // Distinguish "skill genuinely unknown" (no handle registered at all — give up) from
        // "asset still streaming in" (handle registered, `CastTimeline` not loaded yet — retry
        // next tick). A trigger can fire before its skill's async-loaded timeline asset arrives,
        // so the pending case must NOT despawn (mirrors `advance_casts`'s analogous resolve,
        // which also tolerates a not-yet-loaded timeline rather than giving up).
        let Some(handle) = handles.0.get(&exec.skill_id) else {
            warn!(
                "advance_triggered_execs: unknown skill '{}' — dropping triggered execution",
                exec.skill_id
            );
            commands.entity(entity).despawn();
            continue;
        };
        let Some(timeline) = timelines.get(handle) else {
            // Asset not loaded yet — retry next tick. Elapsed must NOT accumulate while
            // pending: the execution's virtual clock starts when the asset arrives, not when
            // `execute_skill_timeline` was called.
            continue;
        };

        exec.elapsed += dt;

        if exec.spawned.len() < timeline.collision_windows.len() {
            exec.spawned.resize(timeline.collision_windows.len(), false);
        }

        for (i, win) in timeline.collision_windows.iter().enumerate() {
            if exec.spawned[i] {
                continue;
            }
            if win.spawn == WindowSpawn::Template {
                // Never on the phase schedule — only instantiated by an emitter (Task 11).
                exec.spawned[i] = true;
                continue;
            }
            let start = super::advance::window_start_time(&timeline.phase_durations, win);
            if exec.elapsed >= start {
                exec.spawned[i] = true;
                let anchor_base = match win.anchor {
                    WindowAnchor::CastPoint => exec.payload.position,
                    WindowAnchor::Caster => transforms
                        .get(exec.caster)
                        .map(|t| t.translation)
                        .unwrap_or(exec.payload.position),
                };
                spawn_window_hitbox(
                    &mut commands,
                    win,
                    exec.caster,
                    &exec.skill_id,
                    anchor_base + win.anchor_offset,
                    exec.payload.direction,
                    exec.payload.charge,
                    ChainPayload {
                        beam_target: exec.payload.target,
                        depth: exec.payload.depth,
                        ..Default::default()
                    },
                );
            }
        }

        if exec.spawned.iter().all(|&s| s) {
            commands.entity(entity).despawn();
        }
    }
}
