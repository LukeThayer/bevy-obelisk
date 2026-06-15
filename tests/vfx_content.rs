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
    let handle: Handle<CastTimeline> =
        t.app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 {
        t.app.update();
        if t.app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() { break; }
    }
    t.app.world_mut().resource_mut::<CastTimelineHandles>().0.insert("firebolt".into(), handle);

    let fired: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let f1 = fired.clone();
    t.app.observe_cue("firebolt_cast", move |_cue, _cmds| f1.lock().unwrap().push("firebolt_cast".into()));
    let f2 = fired.clone();
    t.app.observe_cue("firebolt_impact", move |_cue, _cmds| f2.lock().unwrap().push("firebolt_impact".into()));

    let player = t.app.world_mut().spawn((
        Combatant,
        Attributes(make_block("player", 100.0, 100.0)),
        Faction::Player,
        ObeliskId("player".into()),
        Transform::from_xyz(0.0, 0.0, 0.0),
    )).id();
    let dummy = t.app.world_mut().spawn((
        Combatant,
        Attributes(make_block("dummy", 25.0, 0.0)),
        Faction::Enemy,
        ObeliskId("dummy".into()),
        Transform::from_xyz(0.0, 0.0, 2.0),
    )).id();
    {
        let mut c = t.app.world_mut().commands();
        insert_hurtbox(&mut c, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();
    t.app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);
    t.advance_ticks(600);

    let fired = fired.lock().unwrap();
    println!("Cues fired: {:?}", *fired);
    assert!(fired.contains(&"firebolt_cast".to_string()), "on_cast cue should fire");
    assert!(fired.contains(&"firebolt_impact".to_string()), "on_hit cue should fire");
    let _ = dummy;
}
