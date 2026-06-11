use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

fn spawn(t: &mut ObeliskTestApp, id: &str, faction: Faction, pos: Vec3, life: f64) -> Entity {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = 100.0;
    b.current_mana = 100.0;
    let e = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(b),
            faction,
            ObeliskId(id.into()),
            Transform::from_translation(pos),
        ))
        .id();
    let mut c = t.app.world_mut().commands();
    insert_hurtbox(&mut c, e, 0.5, pos);
    e
}

/// An "AI turn": acquire the nearest enemy with ObeliskSpatial, then resolve a hit with ObeliskCombat.
#[test]
fn ai_acquires_target_and_resolves_a_hit() {
    let mut t = ObeliskTestApp::new(9);
    let ai = spawn(&mut t, "ai", Faction::Enemy, Vec3::ZERO, 100.0);
    let hero = spawn(
        &mut t,
        "hero",
        Faction::Player,
        Vec3::new(0.0, 0.0, 3.0),
        100.0,
    );
    t.app.update();
    t.app.update();
    t.app.update();

    let acquired = t
        .app
        .world_mut()
        .run_system_once(move |s: ObeliskSpatial| s.nearest_enemy(Vec3::ZERO, 10.0, Faction::Enemy))
        .unwrap();
    assert_eq!(
        acquired,
        Some(hero),
        "AI (Enemy) acquires the hero (Player) as nearest enemy"
    );

    let dmg = t
        .app
        .world_mut()
        .run_system_once(move |mut c: ObeliskCombat| c.resolve_skill_hit(ai, hero, "firebolt"))
        .unwrap();
    assert!(dmg.unwrap_or(0.0) > 0.0);
    assert!(
        t.app
            .world()
            .entity(hero)
            .get::<Attributes>()
            .unwrap()
            .0
            .current_life
            < 100.0
    );
}

/// Cooldown gating: ObeliskRead::can_cast reflects an active cooldown.
#[test]
fn can_cast_reflects_cooldown() {
    let mut t = ObeliskTestApp::new(9);
    let hero = spawn(&mut t, "hero", Faction::Player, Vec3::ZERO, 100.0);
    t.app.update();
    t.app
        .world_mut()
        .resource_mut::<Cooldowns>()
        .start(hero, "firebolt", 5.0);
    let res = t
        .app
        .world_mut()
        .run_system_once(move |r: ObeliskRead| r.can_cast(hero, "firebolt"))
        .unwrap();
    assert_eq!(res, Err(CastRejectReason::OnCooldown));
}
