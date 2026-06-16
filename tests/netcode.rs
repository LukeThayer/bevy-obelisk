use bevy::prelude::*;
use obelisk_bevy::net::NetEvent;
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

#[derive(Resource, Default)]
struct Collected(Vec<NetEvent>);

/// A server runs the sim headless, casts firebolt, and drains the serializable NetEvent stream.
#[test]
fn server_drains_serializable_netevents() {
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

    t.app.init_resource::<Collected>();
    // Drain NetEvents every Update (after the FixedUpdate that emits them).
    t.app.add_systems(
        Update,
        |mut reader: MessageReader<NetEvent>, mut c: ResMut<Collected>| {
            for ev in reader.read() {
                c.0.push(ev.clone());
            }
        },
    );

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

    let collected = t.app.world().resource::<Collected>().0.clone();
    assert!(
        collected
            .iter()
            .any(|e| matches!(e, NetEvent::CastBegan { caster, .. } if caster == "player")),
        "stream should contain CastBegan for player"
    );
    assert!(collected.iter().any(|e| matches!(e, NetEvent::DamageResolved { target, caster, .. } if target == "dummy" && caster == "player")),
        "stream should contain DamageResolved player->dummy by stable id");
    assert!(
        collected
            .iter()
            .any(|e| matches!(e, NetEvent::EntityDied { target, .. } if target == "dummy")),
        "stream should contain EntityDied for dummy"
    );

    // Report actual drained counts for diagnostics.
    let count_cast_began = collected
        .iter()
        .filter(|e| matches!(e, NetEvent::CastBegan { .. }))
        .count();
    let count_damage = collected
        .iter()
        .filter(|e| matches!(e, NetEvent::DamageResolved { .. }))
        .count();
    let count_effect_applied = collected
        .iter()
        .filter(|e| matches!(e, NetEvent::EffectApplied { .. }))
        .count();
    let count_effect_expired = collected
        .iter()
        .filter(|e| matches!(e, NetEvent::EffectExpired { .. }))
        .count();
    let count_dot = collected
        .iter()
        .filter(|e| matches!(e, NetEvent::DotTicked { .. }))
        .count();
    let count_died = collected
        .iter()
        .filter(|e| matches!(e, NetEvent::EntityDied { .. }))
        .count();
    println!(
        "Drained {} NetEvents total: CastBegan={} DamageResolved={} EffectApplied={} EffectExpired={} DotTicked={} EntityDied={}",
        collected.len(), count_cast_began, count_damage, count_effect_applied, count_effect_expired, count_dot, count_died
    );

    // The whole drained stream is the serializable wire format.
    let json = serde_json::to_string(&collected).expect("serialize the drained stream");
    let back: Vec<NetEvent> = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(collected, back, "drained NetEvent stream round-trips");
    let _ = dummy;
}

#[test]
fn cast_rejected_is_mirrored_to_netevents() {
    use obelisk_bevy::net::NetEvent;
    let mut t = ObeliskTestApp::new(1);
    t.app.init_resource::<Collected>();
    t.app.add_systems(
        Update,
        |mut r: MessageReader<NetEvent>, mut c: ResMut<Collected>| {
            for ev in r.read() {
                c.0.push(ev.clone());
            }
        },
    );
    // A caster with no loaded timeline -> CastRejected{UnknownSkill}.
    let caster = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("player", 100.0, 100.0)),
            Faction::Player,
            ObeliskId("player".into()),
            Transform::default(),
        ))
        .id();
    let target = t
        .app
        .world_mut()
        .spawn((
            Combatant,
            Attributes(make_block("t", 50.0, 0.0)),
            Faction::Enemy,
            ObeliskId("t".into()),
            Transform::default(),
        ))
        .id();
    t.app.update();
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at("no_such_skill", target);
    t.advance_ticks(5);
    let collected = t.app.world().resource::<Collected>().0.clone();
    assert!(
        collected
            .iter()
            .any(|e| matches!(e, NetEvent::CastRejected { caster, .. } if caster == "player")),
        "a rejected cast should be mirrored to the NetEvent stream"
    );
}
