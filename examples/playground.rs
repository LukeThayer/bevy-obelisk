//! Visual playground — a minimal 3D app that lets you press Space to cast Firebolt.
//!
//! Run from the repo root so Bevy's AssetServer can find `assets/`:
//!   cargo run --example playground
//!
//! `DefaultPlugins` configures AssetPlugin with `file_path = "assets"`, so
//! asset paths are relative to that folder (e.g. "skills/firebolt.cast.ron").

use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use stat_core::StatBlock;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
       .add_plugins(ObeliskPlugins)
       .insert_resource(Time::<Fixed>::from_hz(60.0));

    app.add_obelisk_config_constants_default();
    if !stat_core::config::effect_registry_initialized() {
        stat_core::init_effect_registry(std::path::Path::new("tests/fixtures/effects")).unwrap();
    }
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    app.seed_combat_rng(1);
    app.add_systems(Startup, setup);
    app.add_systems(Update, cast_on_space);
    app.run();
}

#[derive(Resource)]
struct Players {
    player: Entity,
    dummy: Entity,
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut handles: ResMut<CastTimelineHandles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    // DefaultPlugins sets AssetPlugin::file_path = "assets", so paths are relative to that.
    handles.0.insert(
        "firebolt".into(),
        assets.load("skills/firebolt.cast.ron"),
    );

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(6.0, 6.0, 6.0).looking_at(Vec3::new(0.0, 0.0, 1.0), Vec3::Y),
    ));

    // Light
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Player (blue capsule at origin)
    let mut pblock = StatBlock::with_id("player");
    pblock.max_mana.base = 100.0;
    pblock.current_mana = 100.0;
    pblock.max_life.base = 100.0;
    pblock.current_life = 100.0;
    let player = commands
        .spawn((
            Combatant,
            Attributes(pblock),
            Faction::Player,
            ObeliskId("player".into()),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Mesh3d(meshes.add(Capsule3d::new(0.3, 1.2))),
            MeshMaterial3d(mats.add(Color::srgb(0.2, 0.5, 1.0))),
        ))
        .id();

    // Dummy (red sphere at z=4)
    let mut dblock = StatBlock::with_id("dummy");
    dblock.max_life.base = 60.0;
    dblock.current_life = 60.0;
    let dummy = commands
        .spawn((
            Combatant,
            Attributes(dblock),
            Faction::Enemy,
            ObeliskId("dummy".into()),
            Transform::from_xyz(0.0, 0.0, 4.0),
            Mesh3d(meshes.add(Sphere::new(0.6))),
            MeshMaterial3d(mats.add(Color::srgb(1.0, 0.3, 0.2))),
        ))
        .id();
    insert_hurtbox(&mut commands, dummy, 0.6, Vec3::new(0.0, 0.0, 4.0));

    commands.insert_resource(Players { player, dummy });
}

fn cast_on_space(
    keys: Res<ButtonInput<KeyCode>>,
    players: Res<Players>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Space) {
        commands
            .entity(players.player)
            .cast_skill_at("firebolt", players.dummy);
    }
}
