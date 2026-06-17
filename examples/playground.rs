//! Windowed playground — a 3D demo driven by the shared scenario library.
//!
//! Run from the repo root so Bevy's AssetServer can find `assets/` (and the fixtures the
//! scenarios reference):
//!   cargo run --example playground --features debug-gizmos
//!
//! Controls:
//!   1-9 0 -  spawn + replay the matching `feature_matrix()` scenario (keys `1`-`9`, then `0`
//!            and `-` for the 10th and 11th). Despawns the prior scenario's actors, spawns the
//!            selected scenario's actors, then plays its script via a fixed-tick runner.
//!   Space    free-cast the player's first skill at the nearest enemy (via `ObeliskSpatial`).
//!   R        reset (despawn all scenario actors + stop the active runner).
//!
//! The presentation is supplied entirely by `ObeliskDebugVizPlugin` (brought in by
//! `ObeliskPlugins` under the `present` feature): projectile mesh, hit/death material reactions,
//! a roster + event-log HUD, floating damage numbers, and (under `debug-gizmos`) hit/hurt/cone
//! gizmos. This example only sets up the app, spawns the scenario actors with a visible
//! mesh/material, and drives the scripts.
//!
//! `DefaultPlugins` configures `AssetPlugin` with `file_path = "assets"`, so cast-timeline asset
//! paths are relative to that folder (e.g. "skills/firebolt.cast.ron"). The scenario data the
//! picker uses lives in `obelisk_bevy::scenario` (always compiled).

use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::scenario::library::feature_matrix;
use obelisk_bevy::scenario::{Action, ActorSpec, Aim, Scenario};
use std::path::Path;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
        .add_plugins(ObeliskPlugins)
        .insert_resource(Time::<Fixed>::from_hz(60.0));

    // Obelisk global config: constants + the fixture effect/skill registries + a fixed RNG seed.
    app.add_obelisk_config_constants_default();
    if !stat_core::config::effect_registry_initialized() {
        // The `apply_effect` / `trigger_cascade` scenarios apply `burn` / `charged` effects.
        stat_core::init_effect_registry(Path::new("tests/fixtures/effects")).unwrap();
    }
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    app.seed_combat_rng(1);

    // The full scenario matrix is the picker's data source (always-compiled scenario library).
    app.insert_resource(ScenarioLibrary(feature_matrix()));
    app.init_resource::<ActiveRunner>();

    app.add_systems(Startup, (setup_scene, load_cast_assets));
    app.add_systems(
        Update,
        (
            poll_cast_assets,
            handle_input,
            free_cast_on_space,
            reset_on_key,
        ),
    );
    // The scripted runner advances on the fixed timestep so its tick alignment matches the sim.
    app.add_systems(FixedUpdate, run_active_scenario);

    app.run();
}

// ----------------------------------------------------------------------------------------------
// Resources / markers
// ----------------------------------------------------------------------------------------------

/// The picker's data source: the always-compiled `feature_matrix()` scenario library.
#[derive(Resource)]
struct ScenarioLibrary(Vec<Scenario>);

/// The currently-playing scenario (a clone of the selected library entry) + its elapsed tick.
/// `None` until a number key selects one.
#[derive(Resource, Default)]
struct ActiveRunner {
    scenario: Option<Scenario>,
    elapsed: usize,
}

/// Marker for everything a scenario spawns (actors + obstacles), so a re-select or `R` can
/// despawn the prior scenario without touching the camera / light / HUD.
#[derive(Component)]
struct ScenarioEntity;

/// The cast-timeline handles being polled to load (skill id -> handle). Drained into
/// `CastTimelineHandles` once each asset finishes loading.
#[derive(Resource, Default)]
struct PendingCastAssets(Vec<(String, Handle<CastTimeline>)>);

// ----------------------------------------------------------------------------------------------
// Startup
// ----------------------------------------------------------------------------------------------

fn setup_scene(mut commands: Commands, library: Res<ScenarioLibrary>) {
    // Camera looking down the +Z axis where the scenarios place their actors.
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(7.0, 7.0, -3.0).looking_at(Vec3::new(0.0, 0.0, 2.0), Vec3::Y),
    ));

    // Light.
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    info!("playground ready — press a scenario key (1-9, 0, -), Space to free-cast, R to reset.");
    for (i, s) in library.0.iter().enumerate() {
        let label = KEY_LABELS.get(i).copied().unwrap_or("?");
        info!("  [{}] {}", label, s.name);
    }

    // Loot scenarios need a drop-table registry to roll on death. Load the goblin fixture once
    // (mirrors `scenario::run`); harmless if no loot scenario is ever selected.
    if let Ok(toml) = std::fs::read_to_string("tests/fixtures/loot/goblin.toml") {
        if let Ok(registry) =
            tables_core::DropTableRegistry::load_from_strings(&[("goblin.toml", &toml)])
        {
            commands.insert_resource(DropTables(registry));
        }
    }
}

/// Kick off loading every `.cast.ron` referenced by any scenario in the library.
fn load_cast_assets(
    mut commands: Commands,
    assets: Res<AssetServer>,
    library: Res<ScenarioLibrary>,
) {
    let mut skills: Vec<String> = library
        .0
        .iter()
        .flat_map(|s| s.cast_assets.iter().cloned())
        .collect();
    skills.sort();
    skills.dedup();

    let mut pending = PendingCastAssets::default();
    for skill in skills {
        // DefaultPlugins sets AssetPlugin::file_path = "assets", so paths are relative to it.
        let handle: Handle<CastTimeline> = assets.load(format!("skills/{skill}.cast.ron"));
        pending.0.push((skill, handle));
    }
    commands.insert_resource(pending);
}

/// Poll the pending cast assets each frame; move loaded ones into `CastTimelineHandles`.
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
            false // loaded — drop from the pending list
        } else {
            true // still loading
        }
    });
}

// ----------------------------------------------------------------------------------------------
// Scenario picker
// ----------------------------------------------------------------------------------------------

/// Selection keys, one per scenario slot. `feature_matrix()` currently has 11 scenarios, so the
/// 10th and 11th map to `0` and `-` after `1`-`9`. Parallel to `KEY_LABELS`.
const DIGIT_KEYS: [KeyCode; 11] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
    KeyCode::Minus,
];

/// Human-readable label for each selection key, parallel to `DIGIT_KEYS`.
const KEY_LABELS: [&str; 11] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-"];

/// Selection keys (`1`-`9`, `0`, `-`) select + start the matching scenario.
fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut runner: ResMut<ActiveRunner>,
    library: Res<ScenarioLibrary>,
    existing: Query<Entity, With<ScenarioEntity>>,
) {
    let Some(index) = DIGIT_KEYS.iter().position(|k| keys.just_pressed(*k)) else {
        return;
    };
    let Some(scenario) = library.0.get(index) else {
        return;
    };

    // Despawn the prior scenario's actors/obstacles.
    despawn_scenario(&mut commands, &existing);

    info!(
        "playing scenario [{}]: {}",
        KEY_LABELS.get(index).copied().unwrap_or("?"),
        scenario.name
    );

    // Spawn this scenario's actors with a visible mesh + material (the debug-viz reactions mutate
    // each combatant's own material, so they need one).
    for actor in &scenario.actors {
        spawn_actor(&mut commands, &mut meshes, &mut mats, actor);
    }

    // Arm the fixed-tick runner with a fresh copy of the scenario.
    runner.scenario = Some(scenario.clone());
    runner.elapsed = 0;
}

/// Spawn one scenario actor via the public verbs (mirrors `scenario::spawn_actor`, but through
/// `Commands` so it runs inside a live Bevy system), plus a mesh/material so it's visible and the
/// debug-viz reactions have something to flash/fade. Tagged `ScenarioEntity` for despawn/reset.
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
            ScenarioEntity,
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

/// Drive the active scenario's script on the fixed timestep: apply every action whose `at_tick`
/// matches the elapsed tick, then advance. Stops once `elapsed` reaches the scenario's length.
fn run_active_scenario(
    mut runner: ResMut<ActiveRunner>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    index: Res<ObeliskEntityIndex>,
) {
    let Some(scenario) = runner.scenario.clone() else {
        return;
    };
    if runner.elapsed >= scenario.ticks {
        // Finished playing the script; leave the actors on screen and idle the runner.
        runner.scenario = None;
        return;
    }

    let tick = runner.elapsed;
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
            // A visible static collider (so LOS scenarios have something to block + show).
            commands.spawn((
                avian3d::prelude::RigidBody::Static,
                avian3d::prelude::Collider::sphere(*radius),
                Transform::from_translation(*pos),
                Mesh3d(meshes.add(Sphere::new(*radius))),
                MeshMaterial3d(mats.add(Color::srgb(0.5, 0.5, 0.55))),
                ScenarioEntity,
            ));
        }
    }
}

// ----------------------------------------------------------------------------------------------
// Free-cast (Space) + reset (R)
// ----------------------------------------------------------------------------------------------

/// Space free-casts the player's first granted skill at the nearest enemy (via `ObeliskSpatial`).
#[allow(clippy::type_complexity)]
fn free_cast_on_space(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    spatial: ObeliskSpatial,
    players: Query<(Entity, &Transform, &Faction, &SkillSlots), With<ScenarioEntity>>,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }
    // The first Player-faction scenario actor that has at least one skill is "the player".
    let Some((player, tf, faction, slots)) = players
        .iter()
        .filter(|(_, _, f, slots)| **f == Faction::Player && !slots.0.is_empty())
        .min_by(|a, b| a.0.cmp(&b.0))
    else {
        return;
    };
    let Some(skill) = slots.0.first().cloned() else {
        return;
    };
    if let Some(target) = spatial.nearest_enemy(tf.translation, 30.0, *faction) {
        info!("free-cast {skill} at nearest enemy");
        commands.entity(player).cast_skill_at(skill, target);
    } else {
        info!("free-cast {skill}: no enemy in range");
    }
}

/// R resets: despawn every scenario entity and idle the runner.
fn reset_on_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut runner: ResMut<ActiveRunner>,
    existing: Query<Entity, With<ScenarioEntity>>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    despawn_scenario(&mut commands, &existing);
    runner.scenario = None;
    runner.elapsed = 0;
    info!("reset");
}

fn despawn_scenario(commands: &mut Commands, existing: &Query<Entity, With<ScenarioEntity>>) {
    for e in existing {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.despawn();
        }
    }
}
