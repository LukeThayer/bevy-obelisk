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
    app.seed_combat_rng(scenario.seed);

    // Loot: if any actor rolls a drop table on death, load the drop-table registry so the
    // loot system has something to roll. Gated on actors actually declaring a drop_table so
    // non-loot scenarios are unaffected. (`spawn_actor` already inserts the `DropTableId`.)
    if scenario.actors.iter().any(|a| a.drop_table.is_some()) {
        let goblin_toml = std::fs::read_to_string("tests/fixtures/loot/goblin.toml")
            .expect("read goblin drop-table fixture");
        let registry =
            tables_core::DropTableRegistry::load_from_strings(&[("goblin.toml", &goblin_toml)])
                .expect("load drop table");
        app.insert_resource(crate::loot::DropTables(registry));
    }

    app.finish();
    app.cleanup();

    // load referenced cast timelines
    let mut handles = vec![];
    for skill in &scenario.cast_assets {
        let h: Handle<CastTimeline> = app
            .world()
            .resource::<AssetServer>()
            .load(format!("assets/skills/{skill}.cast.ron"));
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
