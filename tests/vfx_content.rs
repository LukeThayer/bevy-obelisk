use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use obelisk_bevy::vfx::ObeliskCueExt;
use stat_core::StatBlock;
use std::sync::{Arc, Mutex};

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = mana;
    b.current_mana = mana;
    b
}

#[test]
fn firebolt_fires_cast_and_hit_cues() {
    let mut t = ObeliskTestApp::new(42);
    let handle: Handle<CastTimeline> = t
        .app
        .world()
        .resource::<AssetServer>()
        .load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 {
        t.app.update();
        if t.app
            .world()
            .resource::<Assets<CastTimeline>>()
            .get(&handle)
            .is_some()
        {
            break;
        }
    }
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("firebolt".into(), handle);

    let fired: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let f1 = fired.clone();
    t.app.observe_cue("firebolt_cast", move |_cue, _cmds| {
        f1.lock().unwrap().push("firebolt_cast".into())
    });
    let f2 = fired.clone();
    t.app.observe_cue("firebolt_impact", move |_cue, _cmds| {
        f2.lock().unwrap().push("firebolt_impact".into())
    });

    let player = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("player", 100.0, 100.0)),
            Faction::Player,
            ObeliskId("player".into()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    let dummy = t
        .app
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
        let mut c = t.app.world_mut().commands();
        insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_at("firebolt", dummy);
    t.advance_ticks(600);

    let fired = fired.lock().unwrap();
    println!("Cues fired: {:?}", *fired);
    assert!(
        fired.contains(&"firebolt_cast".to_string()),
        "on_cast cue should fire"
    );
    assert!(
        fired.contains(&"firebolt_impact".to_string()),
        "on_hit cue should fire"
    );
    let _ = dummy;
}

#[test]
fn dead_enemy_with_a_drop_table_drops_loot() {
    use obelisk_bevy::loot::{DropTableId, DropTables};
    let mut t = ObeliskTestApp::new(7);

    let table_toml = r#"
[table]
id = "goblin"

[[table.rolls]]
count = 1
weight = 1

[[entries]]
type = "currency"
weight = 1
id = "gold"
"#;
    let registry =
        tables_core::DropTableRegistry::load_from_strings(&[("goblin.toml", table_toml)])
            .expect("load drop table");
    t.app.insert_resource(DropTables(registry));

    #[derive(Resource, Default)]
    struct Loot(Vec<tables_core::Drop>);
    t.app.init_resource::<Loot>();
    t.app.add_observer(
        |e: On<obelisk_bevy::events::LootDropped>, mut l: ResMut<Loot>| {
            l.0.extend(e.event().drops.iter().cloned());
        },
    );

    let goblin = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("goblin", 10.0, 0.0)),
            Faction::Enemy,
            ObeliskId("goblin".into()),
            Transform::default(),
            DropTableId("goblin".into()),
        ))
        .id();
    t.app.update();
    t.app
        .world_mut()
        .commands()
        .trigger(obelisk_bevy::events::EntityDied {
            target: goblin,
            killer: None,
        });
    t.app.update();

    assert!(
        !t.app.world().resource::<Loot>().0.is_empty(),
        "a dead enemy with a drop table should drop loot"
    );
}
