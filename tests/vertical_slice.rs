use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = mana;
    b.current_mana = mana;
    b
}

#[test]
fn firebolt_casts_hits_damages_and_burns_to_death() {
    let mut t = ObeliskTestApp::new(42);

    // Load the cast timeline asset and register it under the skill id.
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

    // Spawn player + dummy.
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
        let mut commands = t.app.world_mut().commands();
        insert_hurtbox(&mut commands, dummy, 0.6, Vec3::new(0.0, 0.0, 2.0));
    }
    t.app.update();

    // Cast firebolt at the dummy.
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_at("firebolt", dummy);

    t.advance_ticks(600);

    let rec = t.rec();
    assert!(!rec.cast_began.is_empty(), "cast should begin");
    assert!(
        rec.phase_changed
            .iter()
            .any(|p| matches!(p.to, SkillPhase::Active)),
        "should reach Active phase"
    );
    assert!(!rec.hit_window_opened.is_empty(), "bolt window should open");
    assert!(!rec.hit_confirmed.is_empty(), "bolt should hit the dummy");
    assert!(!rec.damage_resolved.is_empty(), "damage should resolve");
    assert!(
        rec.effect_applied.iter().any(|e| e.effect_id == "burn"),
        "burn should be applied"
    );
    assert!(!rec.dot_ticked.is_empty(), "burn should tick");
    assert!(
        rec.died.iter().any(|d| d.target == dummy),
        "dummy should die from impact + burn"
    );
}

#[test]
fn second_cast_while_casting_is_rejected() {
    let mut t = ObeliskTestApp::new(1);
    let handle: Handle<CastTimeline> =
        t.app.world().resource::<AssetServer>().load("assets/skills/firebolt.cast.ron");
    for _ in 0..2000 {
        t.app.update();
        if t.app.world().resource::<Assets<CastTimeline>>().get(&handle).is_some() {
            break;
        }
    }
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("firebolt".into(), handle);

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
            Attributes(make_block("dummy", 200.0, 0.0)),
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

    // First cast: starts an ActiveCast (firebolt windup is 0.3 s ~ 18 ticks).
    t.app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);
    t.advance_ticks(3); // still in windup -> ActiveCast present

    // Second cast while the first is in flight.
    t.app.world_mut().commands().entity(player).cast_skill_at("firebolt", dummy);
    t.advance_ticks(2);

    assert!(
        t.rec().cast_rejected.iter().any(|r| r.reason == CastRejectReason::AlreadyCasting),
        "a second cast while one is in flight must be rejected AlreadyCasting"
    );
    let _ = dummy;
}

#[test]
fn slice_is_deterministic_across_two_runs() {
    let run = || {
        let mut t = ObeliskTestApp::new(99);
        let h: Handle<CastTimeline> = t
            .app
            .world()
            .resource::<AssetServer>()
            .load("assets/skills/firebolt.cast.ron");
        for _ in 0..2000 {
            t.app.update();
            if t.app
                .world()
                .resource::<Assets<CastTimeline>>()
                .get(&h)
                .is_some()
            {
                break;
            }
        }
        t.app
            .world_mut()
            .resource_mut::<CastTimelineHandles>()
            .0
            .insert("firebolt".into(), h);
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
        t.rec()
            .damage_resolved
            .iter()
            .map(|d| d.total_damage)
            .sum::<f64>()
    };
    assert_eq!(run(), run(), "same seed -> identical total damage");
}
