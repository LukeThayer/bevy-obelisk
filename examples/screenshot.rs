//! Headless scenario screenshot renderer — render a `feature_matrix()` scenario at a chosen
//! fixed tick to an off-screen PNG, so an agent (or human) can SEE what the sim is doing without
//! a window. This is the visual companion to the golden-trace backbone: the goldens prove the
//! event stream, this proves the rendered scene (meshes, gizmos, projectile/flash reactions).
//!
//! Run from the repo root (so the AssetServer can find `assets/` + the fixtures the scenarios
//! reference):
//!   cargo run --example screenshot --features debug-gizmos -- --scenario firebolt_kill --tick 24
//!
//! Args (both optional; defaults shown):
//!   --scenario <name>   a scenario name from `feature_matrix()` (default: firebolt_kill)
//!   --tick <n>          the fixed tick to capture (default: 24)
//!
//! Output: `screenshots/<name>-<tick>.png`.
//!
//! ---------------------------------------------------------------------------------------------
//! HEADLESS RENDER-TO-PNG RECIPE (verified on macOS / Metal; was the throwaway
//! `examples/_screenshot_spike.rs`, recorded here so the recipe lives with its only consumer):
//!   - (1) Render a `Camera3d` to an off-screen GPU image (`RenderTarget::Image`); the texture
//!     needs `TextureUsages::COPY_SRC` so it can be read back.
//!   - (2) In the RenderApp, an `ImageCopyDriver` render-graph node (edge `CameraDriverLabel ->
//!     ImageCopy`) copies the GPU texture into a mappable GPU buffer each frame.
//!   - (3) `receive_image_from_buffer` (after `RenderSystems::Render`) does
//!     `map_async(MapMode::Read)` + `render_device.poll(PollType::Wait)` and ships the bytes to
//!     the main world over a `crossbeam_channel` (std `mpsc::Receiver` is `!Sync`, so it can't be
//!     a Bevy `Resource`).
//!   - (4) In the main world, `save_image` (PostUpdate) un-pads the GPU row alignment, builds an
//!     `Image`, and writes the PNG via `try_into_dynamic().to_rgba8().save(path)`.
//!
//! COORDINATION (the part beyond the spike): the sim must reach `--tick` BEFORE we capture, and we
//! capture only AFTER the GPU has actually drawn that state. So the flow is:
//!   BuildScene -> [run the scenario script tick-by-tick on FixedUpdate until elapsed == target]
//!   -> Render(pre_roll) -> [render a few frames of the final state] -> save PNG -> AppExit.
//! `TimeUpdateStrategy::ManualDuration(1/60s)` makes each `update()` advance exactly one fixed
//! tick, so the captured frame is deterministically tick `--tick`.

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    image::TextureFormatPixelInfo,
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_graph::{self, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel},
        render_resource::{
            Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, MapMode,
            PollType, TexelCopyBufferInfo, TexelCopyBufferLayout, TextureFormat, TextureUsages,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        Extract, Render, RenderApp, RenderSystems,
    },
    time::TimeUpdateStrategy,
    window::ExitCondition,
    winit::WinitPlugin,
};
use crossbeam_channel::{Receiver, Sender};
use obelisk_bevy::prelude::*;
use obelisk_bevy::scenario::library::feature_matrix;
use obelisk_bevy::scenario::{Action, ActorSpec, Aim, Scenario};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 640;
/// Frames rendered (after the sim reaches the target tick) before saving, so the GPU has actually
/// drawn the final state.
const PRE_ROLL_FRAMES: u32 = 8;

fn main() {
    let (scenario_name, target_tick) = parse_args();

    let Some(scenario) = feature_matrix()
        .into_iter()
        .find(|s| s.name == scenario_name)
    else {
        let names: Vec<String> = feature_matrix().into_iter().map(|s| s.name).collect();
        eprintln!(
            "unknown scenario {scenario_name:?}; available: {}",
            names.join(", ")
        );
        std::process::exit(2);
    };

    let out_path = PathBuf::from(format!("screenshots/{scenario_name}-{target_tick}.png"));

    let mut app = App::new();
    app.insert_resource(SceneController::new(WIDTH, HEIGHT, out_path))
        .insert_resource(ClearColor(Color::srgb_u8(26, 28, 40)))
        .insert_resource(ScenarioRunner {
            scenario: scenario.clone(),
            target_tick,
            elapsed: 0,
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        // The live sim + the debug-viz layer (gizmos under debug-gizmos). `ObeliskPlugins` would
        // add `ObeliskPresentPlugin` automatically under `present`, but we add the pieces
        // explicitly so it's clear the screenshot app composes the same sim + viz the playground
        // does — and so the off-screen camera (not a default window camera) drives rendering.
        .add_plugins(ObeliskSimPlugin)
        .add_plugins(obelisk_bevy::present::ObeliskPresentPlugin)
        .add_plugins(ImageCopyPlugin)
        .add_plugins(CaptureFramePlugin)
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        // Deterministic fixed timestep: each `update()` advances exactly one fixed tick.
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(Time::<Fixed>::from_hz(60.0));

    // Obelisk global config: constants + the fixture effect/skill registries + the scenario's
    // seed (so the render matches what the golden trace recorded).
    app.add_obelisk_config_constants_default();
    if !stat_core::config::effect_registry_initialized() {
        stat_core::init_effect_registry(Path::new("tests/fixtures/effects")).unwrap();
    }
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    app.seed_combat_rng(scenario.seed);

    app.add_systems(Startup, (setup_scene, load_cast_assets));
    // poll the cast assets every frame; run the scenario on the fixed timestep.
    app.add_systems(Update, poll_cast_assets);
    app.add_systems(FixedUpdate, run_scenario_to_target);

    app.run();
}

/// Parse `--scenario <name>` / `--tick <n>` (order-independent; sensible defaults).
fn parse_args() -> (String, usize) {
    let mut scenario = "firebolt_kill".to_string();
    let mut tick = 24usize;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--scenario" => {
                if let Some(v) = args.get(i + 1) {
                    scenario = v.clone();
                    i += 1;
                }
            }
            "--tick" => {
                if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    tick = v;
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (scenario, tick)
}

// ----------------------------------------------------------------------------------------------
// Scenario driving (mirrors examples/playground.rs — live Commands-based spawn + tick runner)
// ----------------------------------------------------------------------------------------------

/// The scenario being rendered + how far its script has advanced. When `elapsed` reaches
/// `target_tick`, the capture flips from `BuildScene` to `Render` (in `run_scenario_to_target`).
#[derive(Resource)]
struct ScenarioRunner {
    scenario: Scenario,
    target_tick: usize,
    elapsed: usize,
}

/// The cast-timeline handles being polled to load (skill id -> handle). Drained into
/// `CastTimelineHandles` once each asset finishes loading.
#[derive(Resource, Default)]
struct PendingCastAssets(Vec<(String, Handle<CastTimeline>)>);

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scene_controller: ResMut<SceneController>,
    render_device: Res<RenderDevice>,
    runner: Res<ScenarioRunner>,
) {
    let render_target = setup_render_target(
        &mut commands,
        &mut images,
        &render_device,
        &mut scene_controller,
    );

    // A ground plane so the scene reads as a 3D space (not actors floating in void).
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(8.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(60, 64, 80))),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));

    // Directional light (matches the playground's framing of the action near origin/+Z).
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Camera framing the action: the scenarios place actors around origin and out along +Z, so
    // look from +X/+Y back toward a point a couple units down +Z (same framing the playground
    // window uses). It renders to the off-screen image, not a window.
    commands.spawn((
        Camera3d::default(),
        render_target,
        Tonemapping::None,
        Transform::from_xyz(7.0, 7.0, -3.0).looking_at(Vec3::new(0.0, 0.5, 2.0), Vec3::Y),
    ));

    // Loot scenarios need a drop-table registry to roll on death (mirrors `scenario::run` +
    // the playground). Harmless if the chosen scenario drops nothing.
    if let Ok(toml) = std::fs::read_to_string("tests/fixtures/loot/goblin.toml") {
        if let Ok(registry) =
            tables_core::DropTableRegistry::load_from_strings(&[("goblin.toml", &toml)])
        {
            commands.insert_resource(DropTables(registry));
        }
    }

    // Spawn the scenario's actors with a visible mesh/material (the debug-viz reactions mutate
    // each combatant's own material, so they need one).
    let scenario = runner.scenario.clone();
    info!(
        "rendering scenario {:?} at tick {} -> {} actors",
        scenario.name,
        runner.target_tick,
        scenario.actors.len()
    );
    for actor in &scenario.actors {
        spawn_actor(&mut commands, &mut meshes, &mut materials, actor);
    }
}

/// Spawn one scenario actor via the public verbs (mirrors `scenario::spawn_actor`, but through
/// `Commands` so it runs inside a live Bevy system), plus a mesh/material so it's visible.
fn spawn_actor(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    actor: &ActorSpec,
) {
    let color = match actor.faction {
        Faction::Player => Color::srgb(0.2, 0.5, 1.0),
        Faction::Enemy => Color::srgb(1.0, 0.3, 0.2),
        Faction::Neutral => Color::srgb(0.7, 0.7, 0.7),
    };

    let e = commands
        .spawn_empty()
        .make_combatant(actor.stat_block())
        .insert((
            actor.faction,
            Transform::from_translation(actor.pos),
            Mesh3d(meshes.add(Capsule3d::new(0.3, 1.0))),
            MeshMaterial3d(mats.add(color)),
        ))
        .id();

    for skill in &actor.skills {
        commands.entity(e).grant_skill(skill.clone());
    }
    if let Some(table) = &actor.drop_table {
        commands.entity(e).insert(DropTableId(table.clone()));
    }
    insert_hurtbox(commands, e, actor.hurtbox_radius, actor.pos);
}

/// Kick off loading every `.cast.ron` the scenario references. `DefaultPlugins` sets
/// `AssetPlugin::file_path = "assets"`, so paths are relative to that folder.
fn load_cast_assets(mut commands: Commands, assets: Res<AssetServer>, runner: Res<ScenarioRunner>) {
    let mut pending = PendingCastAssets::default();
    for skill in &runner.scenario.cast_assets {
        let handle: Handle<CastTimeline> = assets.load(format!("skills/{skill}.cast.ron"));
        pending.0.push((skill.clone(), handle));
    }
    commands.insert_resource(pending);
}

/// Move loaded cast assets into `CastTimelineHandles` each frame.
fn poll_cast_assets(
    pending: Option<ResMut<PendingCastAssets>>,
    timelines: Res<Assets<CastTimeline>>,
    mut registry: ResMut<CastTimelineHandles>,
) {
    let Some(mut pending) = pending else {
        return;
    };
    pending.0.retain(|(skill, handle)| {
        if timelines.get(handle).is_some() {
            registry.0.insert(skill.clone(), handle.clone());
            false
        } else {
            true
        }
    });
}

/// Drive the scenario script on the fixed timestep, applying each action whose `at_tick` matches
/// the elapsed tick, then advancing. Once the sim reaches `target_tick`, flip the capture state to
/// `Render` so `save_image` records that frame. Don't start the script until the cast assets have
/// loaded (otherwise a tick-1 cast fires before its timeline exists).
fn run_scenario_to_target(
    mut runner: ResMut<ScenarioRunner>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    index: Res<ObeliskEntityIndex>,
    pending: Option<Res<PendingCastAssets>>,
    mut scene_controller: ResMut<SceneController>,
) {
    // Wait for cast timelines to finish loading before running the script (keeps tick alignment
    // identical to run_scenario, where assets are pre-loaded before the tick loop).
    if pending.map(|p| !p.0.is_empty()).unwrap_or(false) {
        return;
    }

    if runner.elapsed >= runner.target_tick {
        // Reached the target tick: capture this state (idempotent — only flips once).
        if matches!(scene_controller.state, SceneState::BuildScene) {
            info!("reached tick {} — capturing", runner.target_tick);
            scene_controller.state = SceneState::Render(PRE_ROLL_FRAMES);
        }
        return;
    }

    let tick = runner.elapsed;
    let scenario = runner.scenario.clone();
    for step in scenario.script.iter().filter(|s| s.at_tick == tick) {
        apply_action(&mut commands, &mut meshes, &mut mats, &index, &step.action);
    }
    runner.elapsed += 1;
}

/// Apply one scripted action through the public verbs (mirrors `scenario::apply_action`, but via
/// `Commands` for a live app). Ids are resolved through `ObeliskEntityIndex`.
fn apply_action(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    index: &ObeliskEntityIndex,
    action: &Action,
) {
    match action {
        Action::Cast { caster, skill, aim } => {
            let Some(c) = index.entity(caster) else {
                return;
            };
            match aim {
                Aim::Entity(target) => {
                    if let Some(t) = index.entity(target) {
                        commands.entity(c).cast_skill_at(skill.clone(), t);
                    }
                }
                Aim::Point(p) => {
                    commands.entity(c).cast_skill_at_point(skill.clone(), *p);
                }
                Aim::Dir(d) => {
                    if let Ok(dir) = Dir3::new(*d) {
                        commands.entity(c).cast_skill_dir(skill.clone(), dir);
                    }
                }
            }
        }
        Action::ApplyEffect { target, effect } => {
            if let Some(t) = index.entity(target) {
                commands.entity(t).apply_obelisk_effect(effect.clone());
            }
        }
        Action::SetMana { id, mana } => {
            if let Some(e) = index.entity(id) {
                let mana = *mana;
                commands.entity(e).queue(move |mut entity: EntityWorldMut| {
                    if let Some(mut attrs) = entity.get_mut::<Attributes>() {
                        attrs.0.current_mana = mana;
                    }
                });
            }
        }
        Action::Move { id, to } => {
            if let Some(e) = index.entity(id) {
                let to = *to;
                commands.entity(e).queue(move |mut entity: EntityWorldMut| {
                    if let Some(mut tf) = entity.get_mut::<Transform>() {
                        tf.translation = to;
                    }
                });
            }
        }
        Action::Despawn { id } => {
            if let Some(e) = index.entity(id) {
                commands.entity(e).despawn();
            }
        }
        Action::Obstacle { pos, radius } => {
            commands.spawn((
                avian3d::prelude::RigidBody::Static,
                avian3d::prelude::Collider::sphere(*radius),
                Transform::from_translation(*pos),
                Mesh3d(meshes.add(Sphere::new(*radius))),
                MeshMaterial3d(mats.add(Color::srgb(0.5, 0.5, 0.55))),
            ));
        }
        Action::Interrupt { id } => {
            if let Some(e) = index.entity(id) {
                commands.entity(e).interrupt_cast();
            }
        }
        Action::ApplyStatSources { id, stats } => {
            if let Some(e) = index.entity(id) {
                commands
                    .entity(e)
                    .apply_stat_sources(vec![Box::new(DemoStatSource(stats.clone()))]);
            }
        }
    }
}

/// A `StatSource` for the example's `Action::ApplyStatSources` (flows `(StatType, value)` mods
/// through obelisk's real rebuild path — mirrors the scenario harness's private source).
struct DemoStatSource(Vec<(stat_core::StatType, f64)>);
impl stat_core::source::StatSource for DemoStatSource {
    fn id(&self) -> &str {
        "demo_stats"
    }
    fn apply(&self, stats: &mut stat_core::stat_block::StatAccumulator) {
        for (stat, value) in &self.0 {
            stats.apply_stat_type(stat.clone(), *value);
        }
    }
}

// ----------------------------------------------------------------------------------------------
// Headless render-to-PNG (the verified _screenshot_spike recipe — see the module doc comment).
// ----------------------------------------------------------------------------------------------

/// Receives image bytes asynchronously from the render world (lives in the main world).
#[derive(Resource, Deref)]
struct MainWorldReceiver(Receiver<Vec<u8>>);

/// Sends image bytes asynchronously to the main world (lives in the render world).
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<Vec<u8>>);

/// Capture image settings + state.
#[derive(Resource)]
struct SceneController {
    state: SceneState,
    width: u32,
    height: u32,
    out_path: PathBuf,
}

impl SceneController {
    fn new(width: u32, height: u32, out_path: PathBuf) -> Self {
        Self {
            state: SceneState::BuildScene,
            width,
            height,
            out_path,
        }
    }
}

enum SceneState {
    /// Building the scene + running the scenario script up to the target tick.
    BuildScene,
    /// Frames remaining before saving the image (post-target-tick render warm-up).
    Render(u32),
}

/// Creates the off-screen render-target image + the CPU image to save into, and the buffer copier.
fn setup_render_target(
    commands: &mut Commands,
    images: &mut ResMut<Assets<Image>>,
    render_device: &Res<RenderDevice>,
    scene_controller: &mut ResMut<SceneController>,
) -> RenderTarget {
    let size = Extent3d {
        width: scene_controller.width,
        height: scene_controller.height,
        ..Default::default()
    };

    // Texture rendered to (needs COPY_SRC so we can read it back).
    let mut render_target_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default(), None);
    render_target_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let render_target_image_handle = images.add(render_target_image);

    // Texture copied to on the CPU side.
    let cpu_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default(), None);
    let cpu_image_handle = images.add(cpu_image);

    commands.spawn(ImageCopier::new(
        render_target_image_handle.clone(),
        size,
        render_device,
    ));
    commands.spawn(ImageToSave(cpu_image_handle));

    RenderTarget::Image(render_target_image_handle.into())
}

/// Render-world part: the copy-to-buffer graph node + the readback system.
struct ImageCopyPlugin;
impl Plugin for ImageCopyPlugin {
    fn build(&self, app: &mut App) {
        let (s, r) = crossbeam_channel::unbounded();

        let render_app = app
            .insert_resource(MainWorldReceiver(r))
            .sub_app_mut(RenderApp);

        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();
        graph.add_node(ImageCopy, ImageCopyDriver);
        graph.add_node_edge(bevy::render::graph::CameraDriverLabel, ImageCopy);

        render_app
            .insert_resource(RenderWorldSender(s))
            .add_systems(ExtractSchedule, image_copy_extract)
            .add_systems(
                Render,
                receive_image_from_buffer.after(RenderSystems::Render),
            );
    }
}

/// Saver plugin: runs in the main world, writes the PNG, exits.
struct CaptureFramePlugin;
impl Plugin for CaptureFramePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, save_image);
    }
}

/// `ImageCopier`s collected into the render world.
#[derive(Clone, Default, Resource, Deref, bevy::prelude::DerefMut)]
struct ImageCopiers(pub Vec<ImageCopier>);

/// Drives the copy from render target to a mappable buffer.
#[derive(Clone, Component)]
struct ImageCopier {
    buffer: Buffer,
    enabled: Arc<AtomicBool>,
    src_image: Handle<Image>,
}

impl ImageCopier {
    fn new(src_image: Handle<Image>, size: Extent3d, render_device: &RenderDevice) -> Self {
        let padded_bytes_per_row =
            RenderDevice::align_copy_bytes_per_row((size.width) as usize) * 4;

        let cpu_buffer = render_device.create_buffer(&BufferDescriptor {
            label: None,
            size: padded_bytes_per_row as u64 * size.height as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            buffer: cpu_buffer,
            src_image,
            enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

fn image_copy_extract(mut commands: Commands, image_copiers: Extract<Query<&ImageCopier>>) {
    commands.insert_resource(ImageCopiers(
        image_copiers.iter().cloned().collect::<Vec<ImageCopier>>(),
    ));
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, RenderLabel)]
struct ImageCopy;

#[derive(Default)]
struct ImageCopyDriver;

impl render_graph::Node for ImageCopyDriver {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let image_copiers = world.get_resource::<ImageCopiers>().unwrap();
        let gpu_images = world
            .get_resource::<RenderAssets<bevy::render::texture::GpuImage>>()
            .unwrap();

        for image_copier in image_copiers.iter() {
            if !image_copier.enabled() {
                continue;
            }

            let src_image = gpu_images.get(&image_copier.src_image).unwrap();

            let mut encoder = render_context
                .render_device()
                .create_command_encoder(&CommandEncoderDescriptor::default());

            let block_dimensions = src_image.texture_format.block_dimensions();
            let block_size = src_image.texture_format.block_copy_size(None).unwrap();

            let padded_bytes_per_row = RenderDevice::align_copy_bytes_per_row(
                (src_image.size.width as usize / block_dimensions.0 as usize) * block_size as usize,
            );

            encoder.copy_texture_to_buffer(
                src_image.texture.as_image_copy(),
                TexelCopyBufferInfo {
                    buffer: &image_copier.buffer,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(
                            std::num::NonZero::<u32>::new(padded_bytes_per_row as u32)
                                .unwrap()
                                .into(),
                        ),
                        rows_per_image: None,
                    },
                },
                src_image.size,
            );

            let render_queue = world.get_resource::<RenderQueue>().unwrap();
            render_queue.submit(std::iter::once(encoder.finish()));
        }

        Ok(())
    }
}

/// Reads the mapped buffer back and sends the bytes to the main world.
fn receive_image_from_buffer(
    image_copiers: Res<ImageCopiers>,
    render_device: Res<RenderDevice>,
    sender: Res<RenderWorldSender>,
) {
    for image_copier in image_copiers.0.iter() {
        if !image_copier.enabled() {
            continue;
        }

        let buffer_slice = image_copier.buffer.slice(..);

        // map_async hands ownership of the buffer to the CPU; we wait via a 1-shot channel.
        let (s, r) = crossbeam_channel::bounded(1);
        buffer_slice.map_async(MapMode::Read, move |r| match r {
            Ok(r) => s.send(r).expect("Failed to send map update"),
            Err(err) => panic!("Failed to map buffer {err}"),
        });

        // Natively we must poll the device for the map to complete.
        render_device
            .poll(PollType::wait_indefinitely())
            .expect("Failed to poll device for map async");

        r.recv().expect("Failed to receive the map_async message");

        let _ = sender.send(buffer_slice.get_mapped_range().to_vec());

        image_copier.buffer.unmap();
    }
}

/// CPU-side image handle to save.
#[derive(Component, Deref)]
struct ImageToSave(Handle<Image>);

/// Takes channel bytes, fills the CPU image, writes the PNG, then exits.
fn save_image(
    images_to_save: Query<&ImageToSave>,
    receiver: Res<MainWorldReceiver>,
    mut images: ResMut<Assets<Image>>,
    mut scene_controller: ResMut<SceneController>,
    mut app_exit_writer: MessageWriter<AppExit>,
) {
    if let SceneState::Render(n) = scene_controller.state {
        if n < 1 {
            // drain to the most recent frame
            let mut image_data = Vec::new();
            while let Ok(data) = receiver.try_recv() {
                image_data = data;
            }
            if !image_data.is_empty() {
                for image in images_to_save.iter() {
                    let img_bytes = images.get_mut(image.id()).unwrap();

                    // Undo the row padding the GPU copy may have introduced.
                    let row_bytes = img_bytes.width() as usize
                        * img_bytes.texture_descriptor.format.pixel_size().unwrap();
                    let aligned_row_bytes = RenderDevice::align_copy_bytes_per_row(row_bytes);
                    if row_bytes == aligned_row_bytes {
                        img_bytes.data.as_mut().unwrap().clone_from(&image_data);
                    } else {
                        img_bytes.data = Some(
                            image_data
                                .chunks(aligned_row_bytes)
                                .take(img_bytes.height() as usize)
                                .flat_map(|row| &row[..row_bytes.min(row.len())])
                                .cloned()
                                .collect(),
                        );
                    }

                    let img = match img_bytes.clone().try_into_dynamic() {
                        Ok(img) => img.to_rgba8(),
                        Err(e) => panic!("Failed to create image buffer {e:?}"),
                    };

                    let path = scene_controller.out_path.clone();
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    info!("Saving screenshot to: {path:?}");
                    if let Err(e) = img.save(&path) {
                        panic!("Failed to save image: {e}");
                    }
                }
                app_exit_writer.write(AppExit::Success);
            }
        } else {
            while receiver.try_recv().is_ok() {}
            scene_controller.state = SceneState::Render(n - 1);
        }
    }
}
