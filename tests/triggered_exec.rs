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
use obelisk_bevy::combat::system::{is_invalid_timeline_target, partition_conditions};
use obelisk_bevy::prelude::*;
use obelisk_bevy::testkit::ObeliskTestApp;
use obelisk_bevy::timeline::triggered::TriggeredExec;
use stat_core::{SkillCondition, StatBlock, TriggerCondition};
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

// ---------------------------------------------------------------------------------------------
// Task 7 — hit-phase trigger integration (spec §3.2, "the fireball moment"): a hit by skill A
// whose rules condition names skill B, where B has a REGISTERED TIMELINE, executes B's own
// timeline at the hit position instead of resolving B as an inline damage packet.
// ---------------------------------------------------------------------------------------------

/// `fireball` (a real projectile bolt) carries an `Always` condition (`additional = true`) naming
/// `fireball_explosion` — a real skill in the registry whose timeline is ALSO registered by
/// `harness_with_fireball_pair`, so the condition partitions into the timeline-target bucket
/// (Task 7), not the legacy inline-packet bucket. `bolt_damage` lets the zero-damage-carrier test
/// author a fireball that deals no damage at all while still triggering the explosion — the
/// `Always` condition (`PreCalculation` phase, evaluated against pre-hit snapshots) doesn't
/// depend on the damage actually dealt.
fn fireball_toml(bolt_damage: f64) -> String {
    format!(
        r#"
[[skills]]
id = "fireball"
name = "Fireball"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0
[[skills.conditions]]
trigger_skill = "fireball_explosion"
type = "always"
additional = true
[skills.damage]
base_damages = [{{ type = "fire", min = {bolt_damage}, max = {bolt_damage} }}]

[[skills]]
id = "fireball_explosion"
name = "Fireball Explosion"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{{ type = "fire", min = 15.0, max = 15.0 }}]
"#,
    )
}

/// The bolt: one `Active` window, a `Linear` projectile aimed at the dummy — a real spatial
/// travel + contact, exactly like `end_events.rs`'s chaining bolt (minus any `on_end` reaction;
/// Task 7's trigger is a rules-level `SkillCondition`, not a spatial chain).
fn fireball_bolt_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "fireball".into(),
        phase_durations: PhaseDurations {
            windup: 0.05,
            active: 1.0,
            recovery: 0.05,
        },
        collision_windows: vec![CollisionWindow {
            id: "bolt".into(),
            spawn_phase: WindowPhase::Active,
            spawn_offset: 0.0,
            active_duration: 1.0,
            shape: CollisionShape::Sphere { radius: 0.5 },
            motion: VolumeMotion::Linear { speed: 10.0 },
            hit_filter: HitFilter::Enemies,
            hit_mode: HitMode::FirstOnly,
            rehit_interval: None,
            on_end: OnEnd::default(),
        }],
        targeting: CastTargeting::SingleEntity { range: 15.0 },
        delivery: CastDelivery::Projectile { speed: 10.0 },
        vfx_cues: HashMap::new(),
    }
}

/// The explosion: one offset-0 `Active` `Static` sphere, wide enough (radius 1.5) to reach the
/// dummy from the bolt's contact point — mirrors `triggered_exec.rs`'s own `two_window_timeline`
/// pattern (`SelfCast`/`Instant` targeting/delivery are unused by `execute_skill_timeline`, which
/// spawns windows directly at `ExecPayload::position`).
fn fireball_explosion_timeline() -> CastTimeline {
    CastTimeline {
        skill_id: "fireball_explosion".into(),
        phase_durations: PhaseDurations {
            windup: 0.0,
            active: 1.0,
            recovery: 0.0,
        },
        collision_windows: vec![CollisionWindow {
            id: "blast".into(),
            spawn_phase: WindowPhase::Active,
            spawn_offset: 0.0,
            active_duration: 0.2,
            shape: CollisionShape::Sphere { radius: 1.5 },
            motion: VolumeMotion::Static,
            hit_filter: HitFilter::Enemies,
            hit_mode: HitMode::OncePerTarget,
            rehit_interval: None,
            on_end: OnEnd::default(),
        }],
        targeting: CastTargeting::SelfCast,
        delivery: CastDelivery::Instant,
        vfx_cues: HashMap::new(),
    }
}

/// Player + dummy pair with BOTH `fireball` and `fireball_explosion` registered — in
/// `SkillRegistry` (rules) AND `CastTimelineHandles`/`Assets<CastTimeline>` (spatial) — mirroring
/// `end_events.rs::setup`'s hurtbox wiring so the bolt (and later the explosion) can actually
/// register a hit against the dummy.
fn harness_with_fireball_pair(seed: u64, bolt_damage: f64) -> (ObeliskTestApp, Entity, Entity) {
    let mut t = ObeliskTestApp::new(seed);

    let skills = stat_core::config::parse_skills(&fireball_toml(bolt_damage)).unwrap();
    t.app
        .world_mut()
        .resource_mut::<SkillRegistry>()
        .0
        .extend(skills);

    let bolt_handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(fireball_bolt_timeline());
    let blast_handle = t
        .app
        .world_mut()
        .resource_mut::<Assets<CastTimeline>>()
        .add(fireball_explosion_timeline());
    {
        let mut handles = t.app.world_mut().resource_mut::<CastTimelineHandles>();
        handles.0.insert("fireball".into(), bolt_handle);
        handles.0.insert("fireball_explosion".into(), blast_handle);
    }

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
            Attributes(make_block("dummy", 500.0, 0.0)),
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
    (t, player, dummy)
}

/// The fireball moment: the bolt hits the dummy (its own `DamageResolved`), and the `Always`
/// condition executes `fireball_explosion`'s OWN timeline at the hit position — a SEPARATE
/// `DamageResolved` for `fireball_explosion`, exactly once (no double-fire from the now-stripped
/// inline packet path).
#[test]
fn hit_trigger_with_timeline_executes_spatially_not_as_packet() {
    let (mut t, player, _dummy) = harness_with_fireball_pair(20, 20.0);
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("fireball", Dir3::Z);
    t.advance_ticks(90); // generous margin: cast -> bolt travel -> hit -> explosion timeline

    let rec = t.rec();
    let ids: Vec<&str> = rec
        .damage_resolved
        .iter()
        .map(|d| d.skill_id.as_str())
        .collect();
    assert!(
        ids.contains(&"fireball"),
        "bolt contact damage, got {ids:?}"
    );
    assert!(
        ids.contains(&"fireball_explosion"),
        "explosion resolved via ITS OWN timeline, got {ids:?}"
    );
    assert_eq!(
        ids.iter().filter(|i| **i == "fireball_explosion").count(),
        1,
        "exactly once — no double-fire from the inline packet path, got {ids:?}"
    );
}

/// A carrier bolt with ZERO base damage still triggers the explosion: the `Always` condition is
/// `PreCalculation` phase, evaluated against pre-hit snapshots — it never looks at the damage
/// actually dealt, so a hit that connects but deals no damage must still spatially fire B.
#[test]
fn zero_damage_carrier_still_triggers() {
    let (mut t, player, _dummy) = harness_with_fireball_pair(21, 0.0);
    t.app
        .world_mut()
        .commands()
        .entity(player)
        .cast_skill_dir("fireball", Dir3::Z);
    t.advance_ticks(90);

    let rec = t.rec();
    assert!(
        rec.damage_resolved
            .iter()
            .any(|d| d.skill_id == "fireball_explosion"),
        "explosion must still fire even though the carrier bolt deals zero damage, got {:?}",
        rec.damage_resolved
            .iter()
            .map(|d| d.skill_id.as_str())
            .collect::<Vec<_>>()
    );
}

/// The `additional = true` requirement (v1), in its HONEST form: `CastTimelineHandles` isn't
/// populated until well after `add_obelisk_skills` runs (see `partition_conditions`'s doc for the
/// load-order finding), so this can't be a load-time validation error the way a plain
/// `trigger_skill` reference check is (`validate_skill_trigger_references`, stat_core-side).
/// Instead it's a runtime predicate at the partition site, exercised here directly against a REAL
/// `CastTimelineHandles` (from `harness_with_fireball_pair`, which genuinely registers
/// `fireball_explosion`'s timeline) — `additional = false` must be flagged, `additional = true`
/// must not, and `partition_conditions` must still bucket the invalid one as timeline-target
/// (never silently dropped — `on_hit_confirmed` warns and treats it as additional regardless).
#[test]
fn timeline_target_condition_requires_additional_true() {
    let (t, _player, _dummy) = harness_with_fireball_pair(22, 20.0);
    let handles = t.app.world().resource::<CastTimelineHandles>();

    let invalid = SkillCondition {
        trigger_skill: "fireball_explosion".into(),
        additional: false,
        condition: TriggerCondition::Always,
    };
    assert!(
        is_invalid_timeline_target(&invalid, handles),
        "a timeline-target condition with additional = false must be flagged invalid (v1)"
    );

    let valid = SkillCondition {
        trigger_skill: "fireball_explosion".into(),
        additional: true,
        condition: TriggerCondition::Always,
    };
    assert!(
        !is_invalid_timeline_target(&valid, handles),
        "additional = true is the required, valid form"
    );

    let (timeline_targets, packet_conditions) =
        partition_conditions(std::slice::from_ref(&invalid), handles);
    assert_eq!(
        timeline_targets.len(),
        1,
        "the invalid condition is still bucketed as timeline-target, never silently dropped"
    );
    assert!(packet_conditions.is_empty());
}
