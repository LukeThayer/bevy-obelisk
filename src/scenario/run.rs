use crate::prelude::*;
use crate::scenario::trace::{RecordNet, Trace, TraceRecorderPlugin};
use crate::scenario::{Action, Scenario};
use bevy::prelude::*;
use std::time::Duration;

/// Run a scenario headlessly through the public integration path; return its event trace.
pub fn run_scenario(scenario: &Scenario) -> Trace {
    // init obelisk globals from fixtures (idempotent / Once-guarded helper).
    crate::testkit::init_test_obelisk();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin {
            file_path: ".".into(),
            ..default()
        })
        .add_plugins(bevy::mesh::MeshPlugin)
        .add_plugins(bevy::scene::ScenePlugin)
        .add_plugins(crate::ObeliskSimPlugin)
        .add_plugins(TraceRecorderPlugin)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(RecordNet(scenario.record_net));
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    // Load the surface-type registry UNCONDITIONALLY (after skills, so tick_skill/trigger_skill
    // refs validate). Inert for surface-less scenarios: no paint requests ⇒ no patches ⇒ no
    // SurfacePainted/SurfaceRemoved events, so every pre-existing golden stays byte-identical
    // (verified). Only the surfaces scenario, which actually paints, produces surface trace lines.
    app.add_obelisk_surfaces(std::path::Path::new("tests/fixtures/surfaces"));
    app.seed_combat_rng(scenario.seed);

    // Loot: if any actor rolls a drop table on death, load the drop-table registry so the
    // loot system has something to roll. Gated on actors actually declaring a drop_table so
    // non-loot scenarios are unaffected. (`spawn_actor` already inserts the `DropTableId`.)
    if scenario.actors.iter().any(|a| a.drop_table.is_some()) {
        // Load the scenario's declared fixture MAP (table name -> fixture file). When the scenario
        // declares no fixtures, fall back to the legacy single `goblin.toml` load so existing loot
        // scenarios stay byte-identical. The `load_from_strings` key must be `.toml`-suffixed.
        let fixtures: Vec<(String, String)> = if scenario.drop_table_fixtures.is_empty() {
            vec![(
                "goblin.toml".to_string(),
                "tests/fixtures/loot/goblin.toml".to_string(),
            )]
        } else {
            scenario
                .drop_table_fixtures
                .iter()
                .map(|(name, path)| {
                    let key = if name.ends_with(".toml") {
                        name.clone()
                    } else {
                        format!("{name}.toml")
                    };
                    (key, path.clone())
                })
                .collect()
        };
        let contents: Vec<(String, String)> = fixtures
            .iter()
            .map(|(key, path)| {
                let toml = std::fs::read_to_string(path)
                    .unwrap_or_else(|e| panic!("read drop-table fixture {path}: {e}"));
                (key.clone(), toml)
            })
            .collect();
        let pairs: Vec<(&str, &str)> = contents
            .iter()
            .map(|(key, toml)| (key.as_str(), toml.as_str()))
            .collect();
        let registry =
            tables_core::DropTableRegistry::load_from_strings(&pairs).expect("load drop table(s)");
        app.insert_resource(crate::loot::DropTables(registry));
    }

    app.finish();
    app.cleanup();

    // load referenced cast timelines. Golden-scenario cast timelines live in `assets/skills/`; the
    // surfaces fixtures author theirs under `tests/fixtures/cast/` (where `tests/surfaces.rs` also
    // loads them — they are test-only and intentionally not shipped in `assets/skills/`). Prefer
    // `assets/skills/` (the resolved path is UNCHANGED for every pre-existing scenario, so their
    // goldens stay byte-identical) and fall back to `tests/fixtures/cast/`.
    let mut handles = vec![];
    for skill in &scenario.cast_assets {
        let shipped = format!("assets/skills/{skill}.cast.ron");
        let path = if std::path::Path::new(&shipped).exists() {
            shipped
        } else {
            format!("tests/fixtures/cast/{skill}.cast.ron")
        };
        let h: Handle<CastTimeline> = app.world().resource::<AssetServer>().load(path);
        handles.push((skill.clone(), h));
    }
    for _ in 0..3000 {
        app.update();
        if handles.iter().all(|(_, h)| {
            app.world()
                .resource::<Assets<CastTimeline>>()
                .get(h)
                .is_some()
        }) {
            break;
        }
    }
    {
        let mut reg = app.world_mut().resource_mut::<CastTimelineHandles>();
        for (skill, h) in handles {
            reg.0.insert(skill, h);
        }
    }

    // spawn actors via the shared helper (public make_combatant verb; ObeliskId == StatBlock.id).
    for a in &scenario.actors {
        crate::scenario::spawn_actor(&mut app, a);
    }
    app.update(); // flush spawns + register hurtboxes

    // The cast-asset poll loop above runs a variable number of `app.update()` calls (it
    // depends on asset-load timing), each of which advances the tick counter. Reset it to 0
    // here so recorded ticks are scenario-relative and the golden trace is deterministic.
    app.world_mut()
        .resource_mut::<crate::scenario::trace::TickCounter>()
        .0 = 0;

    // run the fixed-tick loop, applying script steps (keyed by scenario-relative tick) before advancing.
    for step_offset in 0..scenario.ticks {
        let actions: Vec<Action> = scenario
            .script
            .iter()
            .filter(|s| s.at_tick == step_offset)
            .map(|s| s.action.clone())
            .collect();
        for action in actions {
            crate::scenario::apply_action(&mut app, &action);
        }
        app.update();
    }

    app.world_mut()
        .remove_resource::<Trace>()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::components::Faction;
    use crate::scenario::Aim;
    #[test]
    fn runner_produces_a_trace() {
        let s = Scenario::new("smoke", 42, 600)
            .cast_asset("firebolt")
            .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
            .with_skill("firebolt")
            .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
            .at(
                1,
                Action::Cast {
                    caster: "player".into(),
                    skill: "firebolt".into(),
                    aim: Aim::Entity("dummy".into()),
                },
            );
        let trace = run_scenario(&s);
        let txt = trace.to_text();
        assert!(txt.contains("CastBegan"), "trace:\n{txt}");
        assert!(
            txt.contains("Damage\tcaster=player target=dummy"),
            "trace:\n{txt}"
        );
        assert!(txt.contains("Died\ttarget=dummy"), "trace:\n{txt}");
    }
}
