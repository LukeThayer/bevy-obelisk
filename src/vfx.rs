use crate::assets::{CastTimeline, CastTimelineHandles};
use crate::events::{CastBegan, CueEvent, CueKind, HitConfirmed, HitWindowOpened, HitboxEnded};
use bevy::prelude::*;

/// App-builder ergonomic: run `handler` whenever a `CueEvent` with `cue_id` fires.
pub trait ObeliskCueExt {
    fn observe_cue(
        &mut self,
        cue_id: impl Into<String>,
        handler: impl Fn(&CueEvent, &mut Commands) + Send + Sync + 'static,
    ) -> &mut Self;
}

impl ObeliskCueExt for App {
    fn observe_cue(
        &mut self,
        cue_id: impl Into<String>,
        handler: impl Fn(&CueEvent, &mut Commands) + Send + Sync + 'static,
    ) -> &mut Self {
        let cue_id = cue_id.into();
        self.add_observer(move |ev: On<CueEvent>, mut commands: Commands| {
            if ev.event().cue_id == cue_id {
                handler(ev.event(), &mut commands);
            }
        });
        self
    }
}

/// Emits `CueEvent`s from a skill's authored `vfx_cues` at cast/window/hit moments.
/// Part of the sim (cheap + headless-testable); servers simply don't observe CueEvent.
pub struct ObeliskCuePlugin;

impl Plugin for ObeliskCuePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(cue_on_cast);
        app.add_observer(cue_on_window);
        app.add_observer(cue_on_hit);
        app.add_observer(cue_on_end);
    }
}

fn cue_for(
    handles: &CastTimelineHandles,
    timelines: &Assets<CastTimeline>,
    skill_id: &str,
    slot: &str,
) -> Option<String> {
    let handle = handles.0.get(skill_id)?;
    let timeline = timelines.get(handle)?;
    timeline.vfx_cues.get(slot).cloned()
}

fn cue_on_cast(
    ev: On<CastBegan>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
    mut commands: Commands,
) {
    let e = ev.event();
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, "on_cast") {
        let position = transforms
            .get(e.caster)
            .map(|t| t.translation)
            .unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent {
            cue_id,
            source: e.caster,
            position,
            position_from: None,
            kind: CueKind::OnCast,
        });
    }
}

fn cue_on_window(
    ev: On<HitWindowOpened>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
    hitboxes: Query<&crate::spatial::boxes::Hitbox>,
    mut commands: Commands,
) {
    let e = ev.event();
    let slot = format!("on_window_{}", e.window_id);
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, &slot) {
        let origin = transforms
            .get(e.hitbox)
            .map(|t| t.translation)
            .unwrap_or(Vec3::ZERO);
        // A beam window's open cue is TWO-POINT: from the beam origin to its designated
        // victim, so a lightning-arc lane can render between them.
        let beam_to = hitboxes
            .get(e.hitbox)
            .ok()
            .filter(|hb| hb.is_beam)
            .and_then(|hb| hb.beam_target)
            .and_then(|t| transforms.get(t).ok())
            .map(|t| t.translation);
        let (position, position_from) = match beam_to {
            Some(to) => (to, Some(origin)),
            None => (origin, None),
        };
        commands.trigger(CueEvent {
            cue_id,
            source: e.caster,
            position,
            position_from,
            kind: CueKind::OnWindow,
        });
    }
}

/// A window ended: fire the `on_end_{window_id}` cue AT THE END POSITION (the event carries it
/// — never an entity anchor, so the explosion renders where the bolt actually stopped: enemy,
/// world, or mid-air fuse).
fn cue_on_end(
    ev: On<HitboxEnded>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    mut commands: Commands,
) {
    let e = ev.event();
    let slot = format!("on_end_{}", e.window_id);
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, &slot) {
        commands.trigger(CueEvent {
            cue_id,
            source: e.caster,
            position: e.position,
            position_from: None,
            kind: CueKind::OnEnd,
        });
    }
}

fn cue_on_hit(
    ev: On<HitConfirmed>,
    handles: Res<CastTimelineHandles>,
    timelines: Res<Assets<CastTimeline>>,
    transforms: Query<&Transform>,
    mut commands: Commands,
) {
    let e = ev.event();
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, "on_hit") {
        let position = transforms
            .get(e.target)
            .map(|t| t.translation)
            .unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent {
            cue_id,
            source: e.target,
            position,
            position_from: None,
            kind: CueKind::OnHit,
        });
    }
}
