use crate::events::*;
use bevy::prelude::*;
use std::path::Path;
use std::sync::Once;
use std::time::Duration;

static INIT: Once = Once::new();

/// Idempotently init obelisk globals from test fixtures. Safe across parallel tests.
pub fn init_test_obelisk() {
    INIT.call_once(|| {
        stat_core::config::ensure_constants_initialized();
        if !stat_core::config::effect_registry_initialized() {
            let _ = stat_core::init_effect_registry(Path::new("tests/fixtures/effects"));
        }
    });
}

/// Records every gameplay event for assertions.
#[derive(Resource, Default)]
pub struct EventRecorder {
    pub cast_began: Vec<CastBegan>,
    pub cast_rejected: Vec<CastRejected>,
    pub phase_changed: Vec<CastPhaseChanged>,
    pub hit_window_opened: Vec<HitWindowOpened>,
    pub hit_confirmed: Vec<HitConfirmed>,
    pub damage_resolved: Vec<DamageResolved>,
    pub effect_applied: Vec<EffectApplied>,
    pub dot_ticked: Vec<DotTicked>,
    pub died: Vec<EntityDied>,
}

pub struct EventRecorderPlugin;
impl Plugin for EventRecorderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EventRecorder>();
        app.add_observer(|e: On<CastBegan>, mut r: ResMut<EventRecorder>| {
            r.cast_began.push(e.event().clone())
        });
        app.add_observer(|e: On<CastRejected>, mut r: ResMut<EventRecorder>| {
            r.cast_rejected.push(e.event().clone())
        });
        app.add_observer(|e: On<CastPhaseChanged>, mut r: ResMut<EventRecorder>| {
            r.phase_changed.push(e.event().clone())
        });
        app.add_observer(|e: On<HitWindowOpened>, mut r: ResMut<EventRecorder>| {
            r.hit_window_opened.push(e.event().clone())
        });
        app.add_observer(|e: On<HitConfirmed>, mut r: ResMut<EventRecorder>| {
            r.hit_confirmed.push(e.event().clone())
        });
        app.add_observer(|e: On<DamageResolved>, mut r: ResMut<EventRecorder>| {
            r.damage_resolved.push(e.event().clone())
        });
        app.add_observer(|e: On<EffectApplied>, mut r: ResMut<EventRecorder>| {
            r.effect_applied.push(e.event().clone())
        });
        app.add_observer(|e: On<DotTicked>, mut r: ResMut<EventRecorder>| {
            r.dot_ticked.push(e.event().clone())
        });
        app.add_observer(|e: On<EntityDied>, mut r: ResMut<EventRecorder>| {
            r.died.push(e.event().clone())
        });
    }
}

/// A headless app preconfigured for deterministic integration tests.
pub struct ObeliskTestApp {
    pub app: App,
}

impl ObeliskTestApp {
    pub fn new(seed: u64) -> Self {
        init_test_obelisk();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin {
                file_path: ".".into(),
                ..default()
            })
            .add_plugins(bevy::mesh::MeshPlugin)
            .add_plugins(bevy::scene::ScenePlugin)
            .add_plugins(crate::ObeliskSimPlugin)
            .add_plugins(EventRecorderPlugin)
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_secs_f64(1.0 / 60.0),
            ))
            .insert_resource(Time::<Fixed>::from_hz(60.0));
        use crate::core::config::SkillSource;
        use crate::prelude::ObeliskConfigExt;
        app.add_obelisk_skills(SkillSource::Dir(std::path::PathBuf::from(
            "tests/fixtures/skills",
        )));
        app.seed_combat_rng(seed);
        app.finish();
        app.cleanup();
        Self { app }
    }
    /// Step `n` fixed ticks (TimeUpdateStrategy advances ~1 fixed tick per update).
    pub fn advance_ticks(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }
    pub fn rec(&self) -> &EventRecorder {
        self.app.world().resource::<EventRecorder>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_app_builds_and_ticks() {
        let mut t = ObeliskTestApp::new(1);
        t.advance_ticks(5); // must not panic; Avian + FixedUpdate run headless
        assert!(
            t.rec().damage_resolved.is_empty(),
            "no combat without a cast"
        );
    }
}
