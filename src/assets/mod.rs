use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Asset, Reflect, Debug, Clone, Serialize, Deserialize)]
pub struct CastTimeline {
    pub skill_id: String,
    pub phase_durations: PhaseDurations,
    #[serde(default)]
    pub collision_windows: Vec<CollisionWindow>,
    pub targeting: CastTargeting,
    pub delivery: CastDelivery,
    #[serde(default)]
    pub vfx_cues: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct PhaseDurations {
    pub windup: f32,
    pub active: f32,
    pub recovery: f32,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct CollisionWindow {
    pub id: String,
    pub spawn_phase: WindowPhase,
    #[serde(default)]
    pub spawn_offset: f32,
    pub active_duration: f32,
    pub shape: CollisionShape,
    #[serde(default)]
    pub motion: VolumeMotion,
    pub hit_filter: HitFilter,
    pub hit_mode: HitMode,
    #[serde(default)]
    pub rehit_interval: Option<f32>,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq, Eq)]
pub enum WindowPhase {
    Windup,
    Active,
    Recovery,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum CollisionShape {
    Sphere { radius: f32 },
    Capsule { radius: f32, height: f32 },
    Cone { angle: f32, range: f32 },
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize, Default)]
pub enum VolumeMotion {
    #[default]
    Static,
    Linear {
        speed: f32,
    },
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq, Eq)]
pub enum HitFilter {
    Caster,
    Allies,
    Enemies,
    All,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize, PartialEq, Eq)]
pub enum HitMode {
    OncePerTarget,
    FirstOnly,
    EveryTick,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub enum CastTargeting {
    SelfCast,
    SingleEntity { range: f32 },
    Direction { range: f32 },
    Cone { angle: f32, range: f32 },
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub enum CastDelivery {
    Melee,
    Instant,
    Projectile { speed: f32 },
}

/// RON loader for `*.cast.ron`.
#[derive(Default, TypePath)]
pub struct CastTimelineLoader;

impl AssetLoader for CastTimelineLoader {
    type Asset = CastTimeline;
    type Settings = ();
    type Error = CastLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<CastTimeline, CastLoadError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| CastLoadError::Io(e.to_string()))?;
        ron::de::from_bytes::<CastTimeline>(&bytes).map_err(|e| CastLoadError::Ron(e.to_string()))
    }

    fn extensions(&self) -> &[&str] {
        &["cast.ron"]
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CastLoadError {
    #[error("io: {0}")]
    Io(String),
    #[error("ron: {0}")]
    Ron(String),
}

/// Maps skill_id -> loaded timeline handle.
#[derive(Resource, Default)]
pub struct CastTimelineHandles(pub std::collections::HashMap<String, Handle<CastTimeline>>);

pub struct ObeliskAssetsPlugin;
impl Plugin for ObeliskAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<CastTimeline>()
            .register_asset_loader(CastTimelineLoader)
            .init_resource::<CastTimelineHandles>();
        app.register_type::<CastTimeline>()
            .register_type::<PhaseDurations>()
            .register_type::<CollisionWindow>()
            .register_type::<WindowPhase>()
            .register_type::<CollisionShape>()
            .register_type::<VolumeMotion>()
            .register_type::<HitFilter>()
            .register_type::<HitMode>()
            .register_type::<CastTargeting>()
            .register_type::<CastDelivery>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_firebolt_cast_ron() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin {
                file_path: ".".into(),
                ..default()
            })
            .add_plugins(ObeliskAssetsPlugin);
        app.finish();
        app.cleanup();
        let handle: Handle<CastTimeline> = app
            .world()
            .resource::<AssetServer>()
            .load("assets/skills/firebolt.cast.ron");
        for _ in 0..1000 {
            app.update();
            if app
                .world()
                .resource::<Assets<CastTimeline>>()
                .get(&handle)
                .is_some()
            {
                break;
            }
        }
        let timeline = app
            .world()
            .resource::<Assets<CastTimeline>>()
            .get(&handle)
            .expect("loaded");
        assert_eq!(timeline.skill_id, "firebolt");
        assert_eq!(timeline.collision_windows.len(), 1);
    }

    #[test]
    fn cast_timeline_round_trips_through_ron() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/skills/firebolt.cast.ron"
        ))
        .expect("read firebolt.cast.ron");
        let parsed: CastTimeline = ron::de::from_str(&src).expect("parse original");
        let serialized = ron::ser::to_string(&parsed).expect("serialize");
        let reparsed: CastTimeline = ron::de::from_str(&serialized).expect("re-parse");
        assert_eq!(parsed.skill_id, reparsed.skill_id);
        assert_eq!(
            parsed.phase_durations.windup,
            reparsed.phase_durations.windup
        );
        assert_eq!(
            parsed.phase_durations.active,
            reparsed.phase_durations.active
        );
        assert_eq!(
            parsed.phase_durations.recovery,
            reparsed.phase_durations.recovery
        );
        assert_eq!(
            parsed.collision_windows.len(),
            reparsed.collision_windows.len()
        );
        assert_eq!(
            format!("{:?}", parsed.collision_windows[0].shape),
            format!("{:?}", reparsed.collision_windows[0].shape)
        );
        assert_eq!(
            format!("{:?}", parsed.targeting),
            format!("{:?}", reparsed.targeting)
        );
        assert_eq!(
            format!("{:?}", parsed.delivery),
            format!("{:?}", reparsed.delivery)
        );
        assert_eq!(parsed.vfx_cues, reparsed.vfx_cues);
    }

    #[test]
    fn cast_timeline_type_is_registered_for_reflection() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin {
                file_path: ".".into(),
                ..default()
            })
            .add_plugins(ObeliskAssetsPlugin);
        app.finish();
        app.cleanup();
        let registry = app.world().resource::<AppTypeRegistry>().read();
        assert!(
            registry
                .get(std::any::TypeId::of::<CastTimeline>())
                .is_some(),
            "CastTimeline must be registered"
        );
    }
}
