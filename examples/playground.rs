//! Windowed playground — a 3D demo driven by the shared scenario library.
//!
//! Run from the repo root so Bevy's AssetServer can find `assets/` (and the fixtures the
//! scenarios reference):
//!   cargo run --example playground --features debug-gizmos
//!
//! Controls:
//!   1-9 0 -  jump to one of the first 11 `feature_matrix()` scenarios (keys `1`-`9`, then `0`/`-`).
//!   [  ]     cycle prev/next through ALL scenarios (so the whole matrix is reachable as it grows).
//!            Selecting (re)spawns the scenario's actors and plays its script via a fixed-tick runner.
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

    app.init_resource::<ActiveScenarioInfo>();

    app.add_systems(Startup, (setup_scene, setup_info_panel, load_cast_assets));
    app.add_systems(
        Update,
        (
            poll_cast_assets,
            handle_input,
            free_cast_on_space,
            reset_on_key,
            update_active_scenario_text,
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

/// The selected scenario's name + description, written on select (and cleared on reset). Read by
/// `update_active_scenario_text` to refresh the info panel's dynamic line only when it changes.
#[derive(Resource, Default)]
struct ActiveScenarioInfo {
    name: Option<String>,
    description: String,
    /// Index of the currently-selected scenario, for `[`/`]` prev/next cycling. `None` until first select.
    current: Option<usize>,
}

/// Marker for the info panel's dynamic `Text` (the active scenario name + description).
#[derive(Component)]
struct ActiveScenarioText;

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

    info!(
        "playground ready — keys 1-9/0/- select the first 11 scenarios, [ ] cycle all {}; Space free-casts, R resets.",
        library.0.len()
    );
    for (i, s) in library.0.iter().enumerate() {
        let label = KEY_LABELS.get(i).copied().unwrap_or("·");
        info!("  [{label}] {}", s.name);
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

/// Spawn the playground info panel (bevy_ui): a CONTROLS legend, a key->scenario list, and an
/// (initially-hint) line for the ACTIVE scenario's name + description. Uses the same UI idioms as
/// `present::debug_viz` (Node flex layout + Text + TextFont + TextColor). The debug-viz HUD anchors
/// its roster top-LEFT and its event-log bottom-LEFT, so this panel anchors top-RIGHT to coexist
/// without overlap. This panel is PLAYGROUND-LOCAL (the debug-viz plugin is scenario-agnostic).
fn setup_info_panel(mut commands: Commands, library: Res<ScenarioLibrary>) {
    // Build the static CONTROLS + key->scenario legend once at startup.
    let mut legend = String::from(
        "CONTROLS\n  1-9 / 0 / - : first 11   |   [ ] : cycle prev/next (all)   |   Space: free-cast   |   R: reset\n\nSCENARIOS",
    );
    for (i, s) in library.0.iter().enumerate() {
        let label = KEY_LABELS.get(i).copied().unwrap_or("·");
        legend.push_str(&format!("\n  [{label}] {}", s.name));
    }

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(8.0),
                top: Val::Px(8.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(6.0)),
                max_width: Val::Px(440.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .with_children(|panel| {
            // Static controls + key->scenario legend.
            panel.spawn((
                Text::new(legend),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.92, 1.0)),
            ));
            // Dynamic active-scenario line (name + description); rebuilt on select via
            // `update_active_scenario_text`.
            panel.spawn((
                ActiveScenarioText,
                Text::new("\nACTIVE: press a scenario key to begin"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.9, 0.55)),
            ));
        });
}

/// Refresh the active-scenario line whenever the selection changes. Reads the name+description that
/// `handle_input` / `reset_on_key` wrote into `ActiveScenarioInfo`, guarded by `is_changed` + a
/// text-diff so the string is only rebuilt on an actual selection change.
fn update_active_scenario_text(
    info: Res<ActiveScenarioInfo>,
    mut q: Query<&mut Text, With<ActiveScenarioText>>,
) {
    if !info.is_changed() {
        return;
    }
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let body = match &info.name {
        Some(name) => format!("\nACTIVE: {name}\n  {}", info.description),
        None => "\nACTIVE: press a scenario key to begin".to_string(),
    };
    if text.as_str() != body {
        text.0 = body;
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

/// Number-key quick-access slots for the first 11 scenarios (`1`-`9`, then `0` and `-`); scenarios
/// beyond the 11th are reached with the `[`/`]` cycle keys. Parallel to `KEY_LABELS`.
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

/// Scenario picker: number keys (`1`-`9`/`0`/`-`) jump to the first 11 scenarios; `[`/`]` cycle
/// prev/next through ALL of `feature_matrix()`. Selecting (re)starts the matching scenario.
#[allow(clippy::too_many_arguments)]
fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut runner: ResMut<ActiveRunner>,
    mut info: ResMut<ActiveScenarioInfo>,
    library: Res<ScenarioLibrary>,
    existing: Query<Entity, With<ScenarioEntity>>,
) {
    let n = library.0.len();
    if n == 0 {
        return;
    }
    // Number keys (`1`-`9`/`0`/`-`) jump to the first 11 scenarios; `[` / `]` cycle prev/next through
    // ALL of them (so every scenario stays reachable as `feature_matrix()` grows past 11).
    let index = if let Some(i) = DIGIT_KEYS
        .iter()
        .position(|k| keys.just_pressed(*k))
        .filter(|&i| i < n)
    {
        i
    } else if keys.just_pressed(KeyCode::BracketRight) {
        info.current.map_or(0, |c| (c + 1) % n)
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        info.current.map_or(n - 1, |c| (c + n - 1) % n)
    } else {
        return;
    };
    let Some(scenario) = library.0.get(index) else {
        return;
    };

    // Despawn the prior scenario's actors/obstacles.
    despawn_scenario(&mut commands, &existing);

    // Surface the selected scenario's name + description in the info panel (the active scenario
    // carries `.description` from the library).
    info.name = Some(scenario.name.clone());
    info.description = scenario.description.clone();
    info.current = Some(index);

    let label = KEY_LABELS.get(index).copied().unwrap_or("·");
    info!(
        "playing scenario [{label}] ({}/{n}): {}",
        index + 1,
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

/// A `StatSource` for the playground's `Action::ApplyStatSources` (flows `(StatType, value)` mods
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
    mut info: ResMut<ActiveScenarioInfo>,
    existing: Query<Entity, With<ScenarioEntity>>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    despawn_scenario(&mut commands, &existing);
    runner.scenario = None;
    runner.elapsed = 0;
    // Clear the active-scenario line back to the hint (ResMut bumps change detection so
    // `update_active_scenario_text` rebuilds it).
    info.name = None;
    info.description.clear();
    info.current = None;
    info!("reset");
}

fn despawn_scenario(commands: &mut Commands, existing: &Query<Entity, With<ScenarioEntity>>) {
    for e in existing {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.despawn();
        }
    }
}
