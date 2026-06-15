//! Minimal headless authoritative server. Run with the presentation layer compiled out:
//!   cargo run --example headless_server --no-default-features
use bevy::prelude::*;
use obelisk_bevy::net::NetEvent;
use obelisk_bevy::prelude::*;
use stat_core::StatBlock;
use std::time::Duration;

#[derive(Resource, Default)]
struct Egress(Vec<NetEvent>);

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = mana;
    b.current_mana = mana;
    b
}

fn main() {
    if !stat_core::config::constants_initialized() {
        stat_core::init_constants_default().unwrap();
    }
    if !stat_core::config::effect_registry_initialized() {
        stat_core::init_effect_registry(std::path::Path::new("tests/fixtures/effects")).unwrap();
    }

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin {
            file_path: ".".into(),
            ..default()
        })
        .add_plugins(bevy::mesh::MeshPlugin)
        .add_plugins(bevy::scene::ScenePlugin)
        .add_plugins(ObeliskSimPlugin)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .init_resource::<Egress>();
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    app.seed_combat_rng(42);
    app.add_systems(
        Update,
        |mut reader: MessageReader<NetEvent>, mut e: ResMut<Egress>| {
            for ev in reader.read() {
                e.0.push(ev.clone());
            }
        },
    );
    app.finish();
    app.cleanup();

    let handle: Handle<CastTimeline> = app
        .world()
        .resource::<AssetServer>()
        .load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 {
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
    app.world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("firebolt".into(), handle);

    let player = app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("player", 100.0, 100.0)),
            Faction::Player,
            ObeliskId("player".into()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    let dummy = app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("dummy", 25.0, 0.0)),
            Faction::Enemy,
            ObeliskId("dummy".into()),
            Transform::from_xyz(0.0, 0.0, 2.0),
        ))
        .id();
    {
        let mut c = app.world_mut().commands();
        insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    app.update();
    app.world_mut()
        .commands()
        .entity(player)
        .cast_skill_at("firebolt", dummy);
    for _ in 0..600 {
        app.update();
    }

    let egress = &app.world().resource::<Egress>().0;
    println!(
        "[headless server] authoritative NetEvent stream ({} events):",
        egress.len()
    );
    for ev in egress {
        println!("  {}", serde_json::to_string(ev).unwrap());
    }
    let _ = dummy;
}
