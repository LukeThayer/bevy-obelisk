use bevy::prelude::*;
use crate::assets::{CastTimeline, CastTimelineHandles};
use crate::events::{CastBegan, CueEvent, CueKind, HitConfirmed, HitWindowOpened};

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
    }
}

fn cue_for(handles: &CastTimelineHandles, timelines: &Assets<CastTimeline>, skill_id: &str, slot: &str) -> Option<String> {
    let handle = handles.0.get(skill_id)?;
    let timeline = timelines.get(handle)?;
    timeline.vfx_cues.get(slot).cloned()
}

fn cue_on_cast(ev: On<CastBegan>, handles: Res<CastTimelineHandles>, timelines: Res<Assets<CastTimeline>>, transforms: Query<&Transform>, mut commands: Commands) {
    let e = ev.event();
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, "on_cast") {
        let position = transforms.get(e.caster).map(|t| t.translation).unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent { cue_id, source: e.caster, position, kind: CueKind::OnCast });
    }
}

fn cue_on_window(ev: On<HitWindowOpened>, handles: Res<CastTimelineHandles>, timelines: Res<Assets<CastTimeline>>, transforms: Query<&Transform>, mut commands: Commands) {
    let e = ev.event();
    let slot = format!("on_window_{}", e.window_id);
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, &slot) {
        let position = transforms.get(e.hitbox).map(|t| t.translation).unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent { cue_id, source: e.caster, position, kind: CueKind::OnWindow });
    }
}

fn cue_on_hit(ev: On<HitConfirmed>, handles: Res<CastTimelineHandles>, timelines: Res<Assets<CastTimeline>>, transforms: Query<&Transform>, mut commands: Commands) {
    let e = ev.event();
    if let Some(cue_id) = cue_for(&handles, &timelines, &e.skill_id, "on_hit") {
        let position = transforms.get(e.target).map(|t| t.translation).unwrap_or(Vec3::ZERO);
        commands.trigger(CueEvent { cue_id, source: e.target, position, kind: CueKind::OnHit });
    }
}
