//! Task 5 — free sub-cast resolution (spec §3.2 billing rule): only hits from the cast's
//! scheduled windows bill mana. Chain re-strikes (`hop > 0`) and triggered sub-casts
//! (`depth > 0`) resolve mana-free — they never pass through cast validation, so a caster at
//! zero mana must NOT fizzle on them, and NOTHING gets billed or put on cooldown.
#![cfg(feature = "test-support")]

use bevy::prelude::*;
use obelisk_bevy::assets::{
    CastDelivery, CastTargeting, CollisionShape, CollisionWindow, HitFilter, HitMode, OnEnd,
    PhaseDurations, VolumeMotion, WindowPhase,
};
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use obelisk_bevy::timeline::triggered::TriggeredExec;
use stat_core::StatBlock;
use std::collections::HashMap;

fn make_block(id: &str, life: f64, mana: f64) -> StatBlock {
    let mut b = StatBlock::with_id(id);
    b.max_life.base = life;
    b.current_life = life;
    b.max_mana.base = mana;
    b.current_mana = mana;
    b
}

/// Spawns a player + dummy pair (mirrors `tests/end_events.rs`'s `setup`), no cast — this suite
/// drives `HitConfirmed` directly since it's testing the observer's billing logic, not the
/// timeline/spatial machinery that produces the event in real play.
fn setup(seed: u64) -> (ObeliskTestApp, Entity, Entity) {
    let mut t = ObeliskTestApp::new(seed);
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
    t.app.update();
    (t, player, dummy)
}

fn drain_caster_mana(t: &mut ObeliskTestApp, caster: Entity) {
    t.app
        .world_mut()
        .entity_mut(caster)
        .get_mut::<Attributes>()
        .expect("caster has Attributes")
        .0
        .current_mana = 0.0;
}

fn caster_mana(t: &ObeliskTestApp, caster: Entity) -> f64 {
    t.app
        .world()
        .entity(caster)
        .get::<Attributes>()
        .unwrap()
        .0
        .current_mana
}

/// Manually fires a `HitConfirmed` for "firebolt" (fixture skill, `mana_cost = 5.0`) against the
/// dummy, bypassing cast validation entirely — exactly the shape a triggered sub-cast or a
/// chain-hop re-strike takes in real play (both skip `advance.rs`'s cast pipeline).
fn fire_hit(t: &mut ObeliskTestApp, caster: Entity, target: Entity, depth: u8, hop: u8) {
    t.app.world_mut().trigger(HitConfirmed {
        caster,
        target,
        skill_id: "firebolt".to_string(),
        window_id: "bolt".to_string(),
        charge: None,
        position: Vec3::ZERO,
        depth,
        hop,
    });
    // Flush the observer's queued `commands.trigger(DamageResolved ...)` so the recorder sees it.
    t.app.world_mut().flush();
}

#[test]
fn depth_gt_zero_hits_do_not_bill_or_fizzle_on_mana() {
    let (mut t, player, dummy) = setup(1);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 1, 0);

    assert!(
        !t.rec().damage_resolved.is_empty(),
        "zero mana must not fizzle a depth>0 sub-cast hit"
    );
    assert_eq!(
        caster_mana(&t, player),
        0.0,
        "a free hit must not bill any mana"
    );
}

#[test]
fn hop_gt_zero_hits_do_not_bill_or_fizzle_on_mana() {
    let (mut t, player, dummy) = setup(2);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 0, 1);

    assert!(
        !t.rec().damage_resolved.is_empty(),
        "zero mana must not fizzle a hop>0 chain re-strike hit"
    );
    assert_eq!(
        caster_mana(&t, player),
        0.0,
        "a free hit must not bill any mana"
    );
}

/// Paired control: today's (unchanged) behavior. A depth-0, hop-0 hit is a normal scheduled-
/// window hit — it bills mana per-hit, and at zero mana it fizzles (no `DamageResolved`).
#[test]
fn depth_zero_hop_zero_hits_still_fizzle_on_zero_mana() {
    let (mut t, player, dummy) = setup(3);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 0, 0);

    assert!(
        t.rec().damage_resolved.is_empty(),
        "a paid hit at zero mana must still fizzle (today's preserved behavior)"
    );
}

/// Sub-casts never pass through `advance.rs`'s cast validation (the only place that calls
/// `Cooldowns::start`), so a free hit must leave the skill's cooldown untouched.
#[test]
fn free_sub_cast_hit_does_not_start_a_cooldown() {
    let (mut t, player, dummy) = setup(4);
    drain_caster_mana(&mut t, player);

    fire_hit(&mut t, player, dummy, 1, 0);

    let cooldowns = t.app.world().resource::<Cooldowns>();
    assert!(
        cooldowns.is_ready(player, "firebolt"),
        "a free sub-cast hit must never start a cooldown"
    );
}

// ---------------------------------------------------------------------------------------------
// Task 6 — the public triggered-timeline executor (spec §3.2). `execute_skill_timeline` spawns
// a skill's collision windows at an arbitrary world position on a virtual clock, detached from
// any caster's cast-state. These tests drive it directly (no trigger wiring exists yet — that's
// later tasks).
// ---------------------------------------------------------------------------------------------

/// One authored collision window: `Active` phase, `Static` motion (no projectile drift — makes
/// spawn position assertions exact), a short `active_duration` so window A can fully expire
/// (fuse out) before window B's later offset fires — the executor's despawn/skip bookkeeping
/// must not depend on A's hitbox still existing when B spawns.
fn window(id: &str, offset: f32) -> CollisionWindow {
    CollisionWindow {
        id: id.into(),
        spawn_phase: WindowPhase::Active,
        spawn_offset: offset,
        active_duration: 0.15,
        shape: CollisionShape::Sphere { radius: 0.5 },
        motion: VolumeMotion::Static,
        hit_filter: HitFilter::Enemies,
        hit_mode: HitMode::OncePerTarget,
        rehit_interval: None,
        on_end: OnEnd::default(),
    }
}

/// A "two-window skill": window A at offset 0.0, window B at offset 0.5 — both `Active` phase
/// off a zero-windup/zero-recovery timeline, so their spawn-eligible times ARE their offsets.
fn two_window_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "test_skill".into(),
        phase_durations: PhaseDurations {
            windup: 0.0,
            active: 1.0,
            recovery: 0.0,
        },
        collision_windows: vec![window("a", 0.0), window("b", 0.5)],
        targeting: CastTargeting::SelfCast,
        delivery: CastDelivery::Instant,
        vfx_cues: HashMap::new(),
    }
}

/// A bare test app with `test_skill`'s two-window timeline registered in `CastTimelineHandles` /
/// `Assets<CastTimeline>` (mirrors how `tests/beam_retarget.rs::setup` inserts timelines
/// programmatically) — no `SkillRegistry` entry needed since `execute_skill_timeline` never
/// passes through `validate_casts`.
fn harness_with_two_window_skill() -> ObeliskTestApp {
    let mut t = ObeliskTestApp::new(1);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(two_window_timeline());
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("test_skill".into(), handle);
    t.app.update();
    t
}

/// Wraps `execute_skill_timeline` through world commands (a fresh, component-less caster entity
/// — attribution only, unused by these assertions), then flushes so the `TriggeredExec` entity
/// exists before the next tick.
fn exec_timeline(t: &mut ObeliskTestApp, skill_id: &str, position: Vec3, depth: u8) -> Entity {
    let caster = t.app.world_mut().spawn_empty().id();
    let mut commands = t.app.world_mut().commands();
    execute_skill_timeline(
        &mut commands,
        caster,
        skill_id,
        ExecPayload {
            position,
            direction: Vec3::Z,
            target: None,
            charge: None,
            depth,
        },
    );
    t.app.world_mut().flush();
    caster
}

fn app_step(t: &mut ObeliskTestApp, n: usize) {
    t.advance_ticks(n);
}

/// Advances by (at least) `secs` of fixed-tick time — the harness ticks at a fixed 1/60s.
fn app_step_secs(t: &mut ObeliskTestApp, secs: f32) {
    let ticks = ((secs * 60.0).ceil() as usize).max(1);
    t.advance_ticks(ticks);
}

/// Live hitbox entity count (a window may have already fused out by the time a later window
/// spawns — use `hitbox_count_total_seen` for a cumulative count).
fn hitbox_count(t: &ObeliskTestApp) -> usize {
    let mut q = t
        .app
        .world()
        .try_query_filtered::<Entity, With<Hitbox>>()
        .expect("Hitbox query builds");
    q.iter(t.app.world()).count()
}

/// Total distinct hitbox windows ever opened, via the `HitWindowOpened` event recorder — robust
/// to a window having already expired (and its hitbox despawned) by the time a later window
/// spawns.
fn hitbox_count_total_seen(t: &ObeliskTestApp) -> usize {
    t.rec().hit_window_opened.len()
}

/// World position of the `idx`-th live hitbox (by entity iteration order).
fn hitbox_pos(t: &ObeliskTestApp, idx: usize) -> Vec3 {
    let mut q = t
        .app
        .world()
        .try_query_filtered::<&Transform, With<Hitbox>>()
        .expect("Hitbox query builds");
    q.iter(t.app.world())
        .nth(idx)
        .expect("a live hitbox at this index")
        .translation
}

#[test]
fn executor_spawns_windows_at_payload_honoring_offsets() {
    let mut t = harness_with_two_window_skill();
    exec_timeline(&mut t, "test_skill", Vec3::new(3.0, 1.0, 0.0), 1);

    app_step(&mut t, 1); // one fixed tick: window A's offset (0.0) is already elapsed
    assert_eq!(hitbox_count(&t), 1, "window A immediate");
    let pos = hitbox_pos(&t, 0);
    assert!(
        (pos - Vec3::new(3.0, 1.0, 0.0)).length() < 0.1,
        "spawned AT the payload, got {pos:?}"
    );

    app_step_secs(&mut t, 0.6); // past window B's 0.5s offset (and past A's 0.15s fuse)
    assert_eq!(
        hitbox_count_total_seen(&t),
        2,
        "window B opens after its offset, even though A has since expired"
    );
}

#[test]
fn executor_drops_at_depth_cap_with_warning() {
    let mut t = harness_with_two_window_skill();
    exec_timeline(&mut t, "test_skill", Vec3::ZERO, MAX_TRIGGER_DEPTH);

    app_step(&mut t, 5);
    assert_eq!(hitbox_count(&t), 0, "cap drops, never spawns");
    assert_eq!(
        hitbox_count_total_seen(&t),
        0,
        "cap drops before any window ever opens"
    );
}

/// The single live `TriggeredExec` entity, if any — used to assert the exec survives (or is
/// despawned) across ticks without depending on the caster entity (which never carries the
/// component itself).
fn triggered_exec_entity(t: &mut ObeliskTestApp) -> Option<Entity> {
    t.app
        .world_mut()
        .query::<(Entity, &TriggeredExec)>()
        .iter(t.app.world())
        .map(|(e, _)| e)
        .next()
}

/// A handle registered in `CastTimelineHandles` (skill id known) whose `CastTimeline` asset has
/// NOT been inserted into `Assets<CastTimeline>` yet — mirrors the real async-load window between
/// a trigger firing and the timeline asset streaming in. Returns the harness plus the reserved
/// handle so the caller can insert the asset later.
fn harness_with_pending_skill() -> (ObeliskTestApp, Handle<CastTimeline>) {
    let mut t = ObeliskTestApp::new(5);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .reserve_handle();
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("pending_skill".into(), handle.clone());
    t.app.update();
    (t, handle)
}

/// Finding 1 (review): a trigger can fire before its skill's `CastTimeline` asset has streamed
/// in. The exec must retry (not despawn) while the handle is registered but the asset isn't
/// loaded yet, and must not tick `elapsed` while pending — once the asset arrives, the offset-0
/// window must still fire (elapsed didn't secretly run out the clock while waiting).
#[test]
fn executor_retries_while_timeline_asset_is_still_loading() {
    let (mut t, handle) = harness_with_pending_skill();
    exec_timeline(&mut t, "pending_skill", Vec3::ZERO, 0);

    let exec = triggered_exec_entity(&mut t).expect("TriggeredExec spawned");

    // Several ticks with the asset still unloaded: exec must survive, no windows open.
    app_step(&mut t, 10);
    assert!(
        t.app.world_mut().get_entity(exec).is_ok(),
        "exec must not despawn while its timeline asset is still loading"
    );
    assert_eq!(
        hitbox_count_total_seen(&t),
        0,
        "no window can spawn before the timeline asset resolves"
    );

    // Now the asset streams in.
    let tl = CastTimeline {
        skill_id: "pending_skill".into(),
        phase_durations: PhaseDurations {
            windup: 0.0,
            active: 1.0,
            recovery: 0.0,
        },
        collision_windows: vec![window("a", 0.0)],
        targeting: CastTargeting::SelfCast,
        delivery: CastDelivery::Instant,
        vfx_cues: HashMap::new(),
    };
    let _ = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .insert(handle.id(), tl);

    app_step(&mut t, 1);
    assert_eq!(
        hitbox_count_total_seen(&t),
        1,
        "offset-0 window fires once the asset resolves, elapsed didn't run out while pending"
    );
}

/// Finding 1 (review), unknown-skill half: no handle registered at all (not even a pending one)
/// means the skill id is genuinely unknown — the exec must despawn (give up) rather than retry
/// forever, and nothing ever spawns.
#[test]
fn executor_despawns_on_unknown_skill_id() {
    let mut t = harness_with_two_window_skill();
    exec_timeline(&mut t, "no_such_skill", Vec3::ZERO, 0);

    let exec = triggered_exec_entity(&mut t).expect("TriggeredExec spawned");

    app_step(&mut t, 5);
    assert!(
        t.app.world_mut().get_entity(exec).is_err(),
        "an unknown skill id must despawn the exec (no handle ever exists to retry against)"
    );
    assert_eq!(
        hitbox_count_total_seen(&t),
        0,
        "an unknown skill must never spawn any window"
    );
}

/// Finding 3 (review, minor): a `Chained` window on the executor's timeline must never spawn on
/// its own schedule (only via a parent window's `on_end`, which this fixture never triggers) —
/// total spawned count stays pinned to the two `Active` windows.
#[test]
fn executor_never_spawns_chained_window_on_its_own_schedule() {
    let mut t = ObeliskTestApp::new(6);
    let mut tl = two_window_timeline();
    let mut chained = window("c", 0.0);
    chained.spawn_phase = WindowPhase::Chained;
    tl.collision_windows.push(chained);
    let handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(tl);
    t.app
        .world_mut()
        .resource_mut::<CastTimelineHandles>()
        .0
        .insert("test_skill".into(), handle);
    t.app.update();

    exec_timeline(&mut t, "test_skill", Vec3::ZERO, 0);
    app_step_secs(&mut t, 1.0); // past both A (0.0) and B (0.5) offsets

    assert_eq!(
        hitbox_count_total_seen(&t),
        2,
        "the Chained window never spawns on the phase schedule — only A and B open"
    );
}
