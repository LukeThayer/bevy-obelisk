//! SPIKE (throwaway): can a headless Bevy 0.17 app render an off-screen scene to a
//! NON-BLANK PNG in THIS environment (macOS / Metal)?
//!
//! Recipe (verified against bevy-0.17.3 `examples/app/headless_renderer.rs`):
//! 1. Render from a `Camera3d` to a GPU image render target (`RenderTarget::Image`).
//! 2. Copy GPU image -> mappable GPU buffer via an `ImageCopyDriver` render-graph node
//!    (edge: CameraDriverLabel -> ImageCopy).
//! 3. Copy buffer -> channel via `receive_image_from_buffer` (runs after `RenderSystems::Render`):
//!    `map_async(MapMode::Read)` + `render_device.poll(PollType::Wait)`.
//! 4. Save channel bytes -> `/tmp/spike.png` in `PostUpdate` (MainWorld) via
//!    `Image::try_into_dynamic().to_rgba8().save(path)` (png via bevy default features).
//! 5. Exit after `single_image` is written.
//!
//! Differences from the vendored example (recorded as deviations):
//! - Uses `crossbeam_channel` (the example's choice) added as a *dev-dependency*; `std::sync::mpsc`
//!   does NOT work because its `Receiver` is `!Sync` and a Bevy `Resource` must be `Send + Sync`.
//! - Scene = ground plane + directional light + a colored cube (the task's "trivial lit scene").
//! - Output path is `/tmp/spike.png`; size 640x480; pre_roll 40 frames.
//!
//! Run: `cargo run --example _screenshot_spike`  (headless: no primary window).

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
    window::ExitCondition,
    winit::WinitPlugin,
};
use crossbeam_channel::{Receiver, Sender};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

const OUT_PATH: &str = "/tmp/spike.png";
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const PRE_ROLL_FRAMES: u32 = 40;

/// Receives image bytes asynchronously from the render world (lives in the main world).
#[derive(Resource, Deref)]
struct MainWorldReceiver(Receiver<Vec<u8>>);

/// Sends image bytes asynchronously to the main world (lives in the render world).
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<Vec<u8>>);

fn main() {
    App::new()
        .insert_resource(SceneController::new(WIDTH, HEIGHT, true))
        .insert_resource(ClearColor(Color::srgb_u8(40, 44, 64)))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ImageCopyPlugin)
        .add_plugins(CaptureFramePlugin)
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_systems(Startup, setup)
        .run();
}

/// Capture image settings + state.
#[derive(Debug, Default, Resource)]
struct SceneController {
    state: SceneState,
    width: u32,
    height: u32,
    single_image: bool,
}

impl SceneController {
    fn new(width: u32, height: u32, single_image: bool) -> Self {
        Self {
            state: SceneState::BuildScene,
            width,
            height,
            single_image,
        }
    }
}

#[derive(Debug, Default)]
enum SceneState {
    #[default]
    BuildScene,
    /// Frames remaining before saving the image.
    Render(u32),
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scene_controller: ResMut<SceneController>,
    render_device: Res<RenderDevice>,
) {
    let render_target = setup_render_target(
        &mut commands,
        &mut images,
        &render_device,
        &mut scene_controller,
        PRE_ROLL_FRAMES,
    );

    // ground plane
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(4.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(180, 180, 180))),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));
    // colored cube
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(220, 90, 70))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
    // directional light
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // camera -> off-screen image
    commands.spawn((
        Camera3d::default(),
        Camera {
            target: render_target,
            ..default()
        },
        Tonemapping::None,
        Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Creates the off-screen render-target image + the CPU image to save into, and the buffer copier.
fn setup_render_target(
    commands: &mut Commands,
    images: &mut ResMut<Assets<Image>>,
    render_device: &Res<RenderDevice>,
    scene_controller: &mut ResMut<SceneController>,
    pre_roll_frames: u32,
) -> RenderTarget {
    let size = Extent3d {
        width: scene_controller.width,
        height: scene_controller.height,
        ..Default::default()
    };

    // Texture rendered to (needs COPY_SRC so we can read it back).
    let mut render_target_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default());
    render_target_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let render_target_image_handle = images.add(render_target_image);

    // Texture copied to on the CPU side.
    let cpu_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default());
    let cpu_image_handle = images.add(cpu_image);

    commands.spawn(ImageCopier::new(
        render_target_image_handle.clone(),
        size,
        render_device,
    ));
    commands.spawn(ImageToSave(cpu_image_handle));

    scene_controller.state = SceneState::Render(pre_roll_frames);
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
            .poll(PollType::Wait)
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

                    let path = PathBuf::from(OUT_PATH);
                    info!("Saving spike image to: {path:?}");
                    if let Err(e) = img.save(&path) {
                        panic!("Failed to save image: {e}");
                    }
                }
                if scene_controller.single_image {
                    app_exit_writer.write(AppExit::Success);
                }
            }
        } else {
            while receiver.try_recv().is_ok() {}
            scene_controller.state = SceneState::Render(n - 1);
        }
    }
}
