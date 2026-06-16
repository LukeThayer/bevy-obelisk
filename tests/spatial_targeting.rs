use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

fn make_block(id: &str, life: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = 100.0;
    b.current_mana = 100.0;
    b
}

fn load_cast(t: &mut ObeliskTestApp, skill: &str, file: &str) {
    let handle: Handle<CastTimeline> = t
        .app
        .world()
        .resource::<AssetServer>()
        .load(format!("assets/skills/{file}"));
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
        .insert(skill.into(), handle);
}

fn spawn_combatant(
    t: &mut ObeliskTestApp,
    id: &str,
    faction: Faction,
    pos: Vec3,
    life: f64,
) -> Entity {
    let e = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block(id, life)),
            faction,
            ObeliskId(id.into()),
            Transform::from_translation(pos),
        ))
        .id();
    let mut c = t.app.world_mut().commands();
    insert_hurtbox(&mut c, e, 0.5, pos);
    e
}

#[test]
fn cone_cleave_hits_multiple_enemies_in_arc_but_not_behind() {
    let mut t = ObeliskTestApp::new(7);
    load_cast(&mut t, "cleave", "cleave.cast.ron");

    let player = spawn_combatant(&mut t, "player", Faction::Player, Vec3::ZERO, 100.0);
    let front_a = spawn_combatant(
        &mut t,
        "front_a",
        Faction::Enemy,
        Vec3::new(0.0, 0.0, 2.0),
        50.0,
    );
    let front_b = spawn_combatant(
        &mut t,
        "front_b",
        Faction::Enemy,
        Vec3::new(1.0, 0.0, 1.5),
        50.0,
    );
    let behind = spawn_combatant(
        &mut t,
        "behind",
        Faction::Enemy,
        Vec3::new(0.0, 0.0, -2.0),
        50.0,
    );
    t.app.update();

    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("cleave", Dir3::Z);
    t.advance_ticks(60);

    let rec = t.rec();
    let hit: std::collections::HashSet<Entity> =
        rec.damage_resolved.iter().map(|d| d.target).collect();
    assert!(
        hit.contains(&front_a),
        "front_a (straight ahead) should be hit"
    );
    assert!(hit.contains(&front_b), "front_b (within arc) should be hit");
    assert!(
        !hit.contains(&behind),
        "behind (outside the cone) must NOT be hit"
    );
    let _ = player;
}

#[test]
fn cleave_does_not_hit_allies() {
    let mut t = ObeliskTestApp::new(7);
    load_cast(&mut t, "cleave", "cleave.cast.ron");
    let player = spawn_combatant(&mut t, "player", Faction::Player, Vec3::ZERO, 100.0);
    let ally = spawn_combatant(
        &mut t,
        "ally",
        Faction::Player,
        Vec3::new(0.0, 0.0, 2.0),
        50.0,
    );
    t.app.update();
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("cleave", Dir3::Z);
    t.advance_ticks(60);
    assert!(
        t.rec().damage_resolved.iter().all(|d| d.target != ally),
        "Enemies filter must not hit a same-faction ally"
    );
}

#[test]
fn out_of_range_cast_is_rejected() {
    let mut t = ObeliskTestApp::new(7);
    load_cast(&mut t, "cleave", "cleave.cast.ron");
    let player = spawn_combatant(&mut t, "player", Faction::Player, Vec3::ZERO, 100.0);
    let far = spawn_combatant(
        &mut t,
        "far",
        Faction::Enemy,
        Vec3::new(0.0, 0.0, 10.0),
        50.0,
    );
    t.app.update();
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_at("cleave", far);
    t.advance_ticks(10);
    assert!(
        t.rec()
            .cast_rejected
            .iter()
            .any(|r| r.reason == CastRejectReason::OutOfRange),
        "a target beyond range should be rejected as OutOfRange"
    );
    assert!(
        t.rec().damage_resolved.is_empty(),
        "no damage on a rejected cast"
    );
}

#[test]
fn cast_blocked_by_obstacle_is_rejected_then_clears() {
    use avian3d::prelude::*;
    let mut t = ObeliskTestApp::new(1);
    load_cast(&mut t, "firebolt", "firebolt.cast.ron");

    let player = spawn_combatant(&mut t, "player", Faction::Player, Vec3::ZERO, 100.0);
    let target = spawn_combatant(
        &mut t,
        "target",
        Faction::Enemy,
        Vec3::new(0.0, 0.0, 6.0),
        100.0,
    );
    // A wall (non-hurtbox static collider) between them at z=3.
    let wall = t
        .app
        .world_mut()
        .spawn((
            RigidBody::Static,
            Collider::sphere(1.0),
            Transform::from_xyz(0.0, 0.0, 3.0),
        ))
        .id();
    t.app.update();
    t.app.update();
    t.app.update(); // register colliders

    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_at("firebolt", target);
    t.advance_ticks(3);
    assert!(
        t.rec()
            .cast_rejected
            .iter()
            .any(|r| r.reason == CastRejectReason::NoLineOfSight),
        "a cast through a wall must be rejected NoLineOfSight"
    );

    // Remove the wall; now the cast should begin.
    t.app.world_mut().entity_mut(wall).despawn();
    t.app.update();
    t.app.update();
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_at("firebolt", target);
    t.advance_ticks(3);
    assert!(
        t.rec().cast_began.iter().any(|c| c.skill_id == "firebolt"),
        "with LOS clear, the cast should begin"
    );
    let _ = (player, target);
}
