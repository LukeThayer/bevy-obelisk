//! Integration suite for the surfaces (ground effects) core — spec
//! `obelisk-arena/docs/superpowers/specs/2026-07-09-surfaces-ground-effects-design.md`.
use obelisk_bevy::prelude::*;
use obelisk_bevy::surfaces::{load_surfaces_dir, SurfaceRegistry};
use std::path::Path;

#[test]
fn surfaces_dir_loads_and_validates() {
    obelisk_bevy::testkit::init_test_obelisk(); // effect registry from fixtures (burn/empower)
    let skills =
        stat_core::config::load_skills_dir(Path::new("tests/fixtures/skills")).expect("skills");
    let reg = obelisk_bevy::core::config::SkillRegistry(skills);
    let map = load_surfaces_dir(Path::new("tests/fixtures/surfaces"), Some(&reg)).expect("load");
    assert!(map.contains_key("frost"));
    assert!(map.contains_key("oil"));
    let burning = &map["burning"];
    let standing = burning.standing.as_ref().expect("burning has standing");
    assert_eq!(standing.tick_skill.as_deref(), Some("burning_tick"));
    assert_eq!(standing.rehit_interval, 0.2);
    let oil = &map["oil"];
    assert_eq!(oil.on_skill_contact.len(), 1);
    assert!(oil.on_skill_contact[0].consume);
    // defaults
    assert_eq!(map["frost"].merge_radius, 0.25);
    assert_eq!(map["frost"].max_patches, 64);
    assert_eq!(map["frost"].patch_radius, 0.45);
}

#[test]
fn surfaces_loader_rejects_bad_refs() {
    obelisk_bevy::testkit::init_test_obelisk();
    let skills =
        stat_core::config::load_skills_dir(Path::new("tests/fixtures/skills")).expect("skills");
    let reg = obelisk_bevy::core::config::SkillRegistry(skills);
    // unknown tick_skill
    let dir = std::env::temp_dir().join("surf_bad_skill");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("bad.toml"),
        "id = \"bad\"\n[standing]\ntick_skill = \"no_such_skill\"\n",
    )
    .unwrap();
    let err = load_surfaces_dir(&dir, Some(&reg)).unwrap_err();
    assert!(err.contains("no_such_skill"), "error names the bad ref: {err}");
    // unknown effect
    let dir2 = std::env::temp_dir().join("surf_bad_effect");
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(
        dir2.join("bad.toml"),
        "id = \"bad\"\n[standing]\neffect = \"no_such_effect\"\n",
    )
    .unwrap();
    let err2 = load_surfaces_dir(&dir2, Some(&reg)).unwrap_err();
    assert!(err2.contains("no_such_effect"), "error names the bad effect: {err2}");
    // unknown on_skill_contact tag (would silently never match at runtime)
    let dir3 = std::env::temp_dir().join("surf_bad_tag");
    std::fs::create_dir_all(&dir3).unwrap();
    std::fs::write(
        dir3.join("bad.toml"),
        "id = \"bad\"\n[[on_skill_contact]]\ntags_any = [\"fira\"]\ntrigger_skill = \"test_ignite\"\n",
    )
    .unwrap();
    let err3 = load_surfaces_dir(&dir3, Some(&reg)).unwrap_err();
    assert!(err3.contains("bad"), "error names the surface id: {err3}");
    assert!(err3.contains("fira"), "error names the bad tag: {err3}");
}

#[test]
fn add_obelisk_surfaces_inserts_the_registry() {
    let mut t = obelisk_bevy::testkit::ObeliskTestApp::new(1);
    t.app.add_obelisk_surfaces(Path::new("tests/fixtures/surfaces"));
    let reg = t.app.world().resource::<SurfaceRegistry>();
    assert!(reg.0.contains_key("frost"));
}

use bevy::prelude::*;
use obelisk_bevy::surfaces::{
    PaintSurface, SurfacePatch, SurfaceRemoveReason,
};
use obelisk_bevy::testkit::ObeliskTestApp;
use stat_core::StatBlock;

/// Test app with the surface fixtures registered.
fn surf_app(seed: u64) -> ObeliskTestApp {
    let mut t = ObeliskTestApp::new(seed);
    t.app
        .add_obelisk_surfaces(Path::new("tests/fixtures/surfaces"));
    t
}

fn spawn_combatant(
    t: &mut ObeliskTestApp,
    id: &str,
    pos: Vec3,
    faction: obelisk_bevy::prelude::Faction,
) -> Entity {
    let mut block = StatBlock::with_id(id);
    block.max_life.base = 200.0;
    block.current_life = 200.0;
    block.max_mana.base = 100.0;
    block.current_mana = 100.0;
    t.app
        .world_mut()
        .spawn((
            obelisk_bevy::prelude::Combatant,
            obelisk_bevy::prelude::Attributes(block),
            faction,
            obelisk_bevy::prelude::ObeliskId(id.into()),
            Transform::from_translation(pos),
        ))
        .id()
}

fn patch_count(t: &mut ObeliskTestApp, surface: &str) -> usize {
    let mut q = t.app.world_mut().query::<&SurfacePatch>();
    q.iter(t.app.world()).filter(|p| p.surface == surface).count()
}

#[test]
fn paint_request_spawns_a_patch_and_dedups() {
    let mut t = surf_app(1);
    let owner = spawn_combatant(&mut t, "painter", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    t.app.update();
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(2.0, 0.0, 0.0),
        owner,
    });
    t.app.world_mut().flush();
    t.app.update();
    assert_eq!(patch_count(&mut t, "frost"), 1);
    assert_eq!(t.rec().surfaces_painted.len(), 1);
    assert_eq!(t.rec().surfaces_painted[0].surface, "frost");
    // A second paint within merge_radius (0.25 default) dedups — still one patch.
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(2.1, 0.0, 0.0),
        owner,
    });
    t.app.world_mut().flush();
    t.app.update();
    assert_eq!(patch_count(&mut t, "frost"), 1, "merge_radius dedup");
    // But a paint farther away spawns a second patch.
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner,
    });
    t.app.world_mut().flush();
    t.app.update();
    assert_eq!(patch_count(&mut t, "frost"), 2);
}

#[test]
fn patches_expire_and_evict_oldest_at_cap() {
    let mut t = surf_app(1);
    let owner = spawn_combatant(&mut t, "painter", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    t.app.update();
    // dew lifetime = 0.3s -> gone within ~30 ticks, with an Expired removal event.
    t.app.world_mut().trigger(PaintSurface {
        surface: "dew".into(),
        position: Vec3::new(1.0, 0.0, 1.0),
        owner,
    });
    t.app.world_mut().flush();
    t.advance_ticks(30);
    assert_eq!(patch_count(&mut t, "dew"), 0, "dew expired");
    assert!(t
        .rec()
        .surfaces_removed
        .iter()
        .any(|r| r.surface == "dew" && r.reason == SurfaceRemoveReason::Expired));
    // capped max_patches = 3: painting 5 distinct spots keeps the NEWEST 3 (oldest evicted).
    for i in 0..5 {
        t.app.world_mut().trigger(PaintSurface {
            surface: "capped".into(),
            position: Vec3::new(i as f32 * 2.0, 0.0, 5.0),
            owner,
        });
        t.app.world_mut().flush();
        t.app.update();
    }
    assert_eq!(patch_count(&mut t, "capped"), 3);
    let evicted: Vec<_> = t
        .rec()
        .surfaces_removed
        .iter()
        .filter(|r| r.surface == "capped" && r.reason == SurfaceRemoveReason::Evicted)
        .collect();
    assert_eq!(evicted.len(), 2, "two oldest evicted");
    // The SURVIVING patches are the three newest (x = 4, 6, 8).
    let mut q = t.app.world_mut().query::<(&SurfacePatch, &Transform)>();
    let mut xs: Vec<f32> = q
        .iter(t.app.world())
        .filter(|(p, _)| p.surface == "capped")
        .map(|(_, tf)| tf.translation.x)
        .collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs, vec![4.0, 6.0, 8.0]);
}

#[test]
fn unknown_surface_paint_is_a_warn_not_a_panic() {
    let mut t = surf_app(1);
    let owner = spawn_combatant(&mut t, "painter", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    t.app.update();
    t.app.world_mut().trigger(PaintSurface {
        surface: "no_such_surface".into(),
        position: Vec3::ZERO,
        owner,
    });
    t.app.world_mut().flush();
    t.app.update(); // must not panic
    assert_eq!(t.rec().surfaces_painted.len(), 0);
}

use obelisk_bevy::prelude::CastTimeline;

/// Load a `.cast.ron` fixture and register its handle (the run_scenario pattern).
fn load_timeline(t: &mut ObeliskTestApp, skill_id: &str, path: &str) {
    let h: bevy::asset::Handle<CastTimeline> = t
        .app
        .world()
        .resource::<bevy::asset::AssetServer>()
        .load(path.to_string());
    let mut loaded = false;
    for _ in 0..2000 {
        t.app.update();
        if t.app
            .world()
            .resource::<bevy::asset::Assets<CastTimeline>>()
            .get(&h)
            .is_some()
        {
            loaded = true;
            break;
        }
    }
    assert!(loaded, "timeline asset {path} loaded");
    t.app
        .world_mut()
        .resource_mut::<obelisk_bevy::prelude::CastTimelineHandles>()
        .0
        .insert(skill_id.to_string(), h);
}

fn grant_and_cast_dir(t: &mut ObeliskTestApp, caster: Entity, skill: &str, dir: Vec3) {
    use obelisk_bevy::timeline::cast::CastSkillExt;
    use obelisk_bevy::verbs::ObeliskCommandsExt;
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .grant_skill(skill)
        .cast_skill_dir(skill, bevy::math::Dir3::new(dir).unwrap());
    t.app.world_mut().flush();
}

#[test]
fn trail_window_paints_spaced_patches_along_its_flight() {
    let mut t = surf_app(2);
    let caster = spawn_combatant(&mut t, "roller", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "paint_roller", "tests/fixtures/cast/paint_roller.cast.ron");
    grant_and_cast_dir(&mut t, caster, "paint_roller", Vec3::X);
    // active_duration 1.0s at 8 m/s = ~8 m of travel; step 0.5 -> ~16 patches; merge_radius
    // 0.25 < step so dedup never blocks the trail. Run 90 ticks (cast + full flight + end).
    t.advance_ticks(90);
    let painted = t.rec().surfaces_painted.len();
    assert!(
        (12..=20).contains(&painted),
        "trail paints ~16 spaced patches, got {painted}"
    );
    // Spacing: consecutive paint positions are >= 0.5 - epsilon apart in X.
    let xs: Vec<f32> = t.rec().surfaces_painted.iter().map(|p| p.position.x).collect();
    for w in xs.windows(2) {
        assert!(w[1] - w[0] >= 0.45, "trail spacing ~step: {:?}", xs);
    }
}

#[test]
fn on_end_window_paints_once_at_the_end_position_with_lifetime_override() {
    let mut t = surf_app(3);
    let caster = spawn_combatant(&mut t, "blaster", Vec3::new(5.0, 0.0, 5.0), obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "paint_blast", "tests/fixtures/cast/paint_blast.cast.ron");
    grant_and_cast_dir(&mut t, caster, "paint_blast", Vec3::X);
    t.advance_ticks(30); // windup 0 + active window fuse 0.2s -> ends, paints once
    assert_eq!(t.rec().surfaces_painted.len(), 1, "OnEnd paints exactly once");
    let p = &t.rec().surfaces_painted[0];
    assert_eq!(p.surface, "burning");
    assert!(
        (p.position - Vec3::new(5.0, 0.0, 5.0)).length() < 0.01,
        "static window ends at its spawn position, got {:?}",
        p.position
    );
    // lifetime override 3.0 (not burning's 8.0): patch persists at 2.5s, gone by 3.5s.
    t.advance_ticks(120); // ~2.0s more (total ~2.5s since paint)
    assert_eq!(patch_count(&mut t, "burning"), 1);
    t.advance_ticks(90); // ~1.5s more (total ~4.0s)
    assert_eq!(patch_count(&mut t, "burning"), 0, "override lifetime expired");
}

fn damage_events_for(t: &ObeliskTestApp, skill: &str) -> usize {
    t.rec().damage_resolved.iter().filter(|d| d.skill_id == skill).count()
}

#[test]
fn standing_in_burning_ticks_damage_attributed_to_the_painter() {
    let mut t = surf_app(4);
    let painter = spawn_combatant(&mut t, "painter", Vec3::new(-5.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
    let victim = spawn_combatant(&mut t, "victim", Vec3::new(3.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Enemy);
    load_timeline(&mut t, "burning_tick", "tests/fixtures/cast/burning_tick.cast.ron");
    // Hurtbox so the tick skill's blast can actually hit the victim.
    obelisk_bevy::spatial::boxes::insert_hurtbox(
        &mut t.app.world_mut().commands(),
        victim,
        0.5,
        Vec3::new(3.0, 0.0, 0.0),
    );
    t.app.world_mut().flush();
    t.advance_ticks(3); // spatial pipeline sees the fresh static collider (probe note)
    t.app.world_mut().trigger(PaintSurface {
        surface: "burning".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    // rehit_interval 0.2s -> over 1.0s expect ~5 tick executions.
    t.advance_ticks(60);
    let ticks = damage_events_for(&t, "burning_tick");
    assert!(
        (4..=6).contains(&ticks),
        "burning ticks ~5x in 1s, got {ticks}"
    );
    let d = t
        .rec()
        .damage_resolved
        .iter()
        .find(|d| d.skill_id == "burning_tick")
        .unwrap();
    assert_eq!(d.caster, painter, "standing damage attributed to the painter");
    assert_eq!(d.target, victim);
    // Painter (same faction as... no: painter is Player, filter enemies) never self-ticks: no
    // damage against the painter.
    assert!(
        !t.rec().damage_resolved.iter().any(|d| d.target == painter),
        "filter=enemies never ticks the painter's own faction"
    );
}

#[test]
fn standing_effect_applies_to_allies_and_stops_on_exit() {
    let mut t = surf_app(5);
    let painter = spawn_combatant(&mut t, "priest", Vec3::new(0.0, 0.0, -4.0), obelisk_bevy::prelude::Faction::Player);
    let ally = spawn_combatant(&mut t, "ally", Vec3::new(0.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
    t.app.world_mut().trigger(PaintSurface {
        surface: "blessed".into(),
        position: Vec3::ZERO,
        owner: painter,
    });
    t.app.world_mut().flush();
    t.advance_ticks(20);
    let applied = t
        .rec()
        .effect_applied
        .iter()
        .filter(|e| e.effect_id == "empower" && e.target == ally)
        .count();
    assert!(applied >= 1, "ally standing in blessed gets empower");
    // Exit: move the ally away; effect application stops (count freezes).
    t.app
        .world_mut()
        .entity_mut(ally)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(50.0, 0.0, 50.0);
    let before = t.rec().effect_applied.len();
    t.advance_ticks(30);
    assert_eq!(
        t.rec().effect_applied.len(),
        before,
        "no further applications after leaving the surface"
    );
}

#[test]
fn on_enter_only_fires_once_per_visit() {
    let mut t = surf_app(6);
    let painter = spawn_combatant(&mut t, "painter", Vec3::new(-5.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
    let victim = spawn_combatant(&mut t, "victim", Vec3::new(2.0, 0.0, 2.0), obelisk_bevy::prelude::Faction::Enemy);
    load_timeline(&mut t, "burning_tick", "tests/fixtures/cast/burning_tick.cast.ron");
    obelisk_bevy::spatial::boxes::insert_hurtbox(
        &mut t.app.world_mut().commands(),
        victim,
        0.5,
        Vec3::new(2.0, 0.0, 2.0),
    );
    t.app.world_mut().flush();
    t.advance_ticks(3);
    t.app.world_mut().trigger(PaintSurface {
        surface: "mud".into(),
        position: Vec3::new(2.0, 0.0, 2.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    t.advance_ticks(40);
    assert_eq!(
        damage_events_for(&t, "burning_tick"),
        1,
        "enter-only fires once while standing"
    );
    // Leave and re-enter -> exactly one more.
    t.app
        .world_mut()
        .entity_mut(victim)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(30.0, 0.0, 30.0);
    t.advance_ticks(10);
    t.app
        .world_mut()
        .entity_mut(victim)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(2.0, 0.0, 2.0);
    t.advance_ticks(40);
    assert_eq!(
        damage_events_for(&t, "burning_tick"),
        2,
        "re-entering fires the enter edge again"
    );
}

#[test]
fn dead_victims_stop_receiving_standing_ticks() {
    let mut t = surf_app(11);
    let painter = spawn_combatant(&mut t, "painter", Vec3::new(-5.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Player);
    let victim = spawn_combatant(&mut t, "victim", Vec3::new(3.0, 0.0, 0.0), obelisk_bevy::prelude::Faction::Enemy);
    load_timeline(&mut t, "burning_tick", "tests/fixtures/cast/burning_tick.cast.ron");
    obelisk_bevy::spatial::boxes::insert_hurtbox(
        &mut t.app.world_mut().commands(),
        victim,
        0.5,
        Vec3::new(3.0, 0.0, 0.0),
    );
    t.app.world_mut().flush();
    t.advance_ticks(3);
    t.app.world_mut().trigger(PaintSurface {
        surface: "burning".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    t.advance_ticks(30); // ~2 ticks land while alive
    let ticks_while_alive = damage_events_for(&t, "burning_tick");
    assert!(ticks_while_alive >= 1, "sanity: standing ticks land while alive");
    // Kill the victim in place (death does not despawn).
    {
        let mut entity_mut = t.app.world_mut().entity_mut(victim);
        let mut attrs = entity_mut.get_mut::<obelisk_bevy::prelude::Attributes>().unwrap();
        attrs.0.current_life = 0.0;
    }
    t.advance_ticks(60);
    assert_eq!(
        damage_events_for(&t, "burning_tick"),
        ticks_while_alive,
        "no further standing ticks against a corpse"
    );
}

#[test]
fn fire_hitbox_through_oil_ignites_once_and_consumes_the_patch() {
    let mut t = surf_app(7);
    let painter = spawn_combatant(&mut t, "oiler", Vec3::new(0.0, 0.0, -6.0), obelisk_bevy::prelude::Faction::Enemy);
    let caster = spawn_combatant(&mut t, "pyro", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "fire_probe", "tests/fixtures/cast/fire_probe.cast.ron");
    load_timeline(&mut t, "test_ignite", "tests/fixtures/cast/test_ignite.cast.ron");
    // One oil patch 3m down the +X flight path.
    t.app.world_mut().trigger(PaintSurface {
        surface: "oil".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    grant_and_cast_dir(&mut t, caster, "fire_probe", Vec3::X);
    t.advance_ticks(90);
    // The ignite timeline executed at (about) the contact point...
    let ignites: Vec<_> = t
        .rec()
        .hit_window_opened
        .iter()
        .filter(|w| w.skill_id == "test_ignite")
        .collect();
    assert_eq!(ignites.len(), 1, "ignite executes exactly once");
    // ...and the oil patch was consumed.
    assert_eq!(patch_count(&mut t, "oil"), 0, "oil consumed");
    assert!(t
        .rec()
        .surfaces_removed
        .iter()
        .any(|r| r.surface == "oil" && r.reason == SurfaceRemoveReason::Consumed));
}

#[test]
fn non_matching_tags_do_not_ignite_oil() {
    let mut t = surf_app(8);
    let painter = spawn_combatant(&mut t, "oiler", Vec3::new(0.0, 0.0, -6.0), obelisk_bevy::prelude::Faction::Enemy);
    let caster = spawn_combatant(&mut t, "frosty", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "cold_probe", "tests/fixtures/cast/cold_probe.cast.ron");
    load_timeline(&mut t, "test_ignite", "tests/fixtures/cast/test_ignite.cast.ron");
    t.app.world_mut().trigger(PaintSurface {
        surface: "oil".into(),
        position: Vec3::new(3.0, 0.0, 0.0),
        owner: painter,
    });
    t.app.world_mut().flush();
    grant_and_cast_dir(&mut t, caster, "cold_probe", Vec3::X);
    t.advance_ticks(90);
    assert!(
        !t.rec().hit_window_opened.iter().any(|w| w.skill_id == "test_ignite"),
        "cold tags don't match tags_any=[fire]"
    );
    assert_eq!(patch_count(&mut t, "oil"), 1, "oil untouched");
}

use obelisk_bevy::prelude::CastSkillExt;

#[test]
fn on_surface_gates_snaps_and_consumes_on_accept() {
    let mut t = surf_app(9);
    let caster = spawn_combatant(&mut t, "spirer", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "spire_probe", "tests/fixtures/cast/spire_probe.cast.ron");
    // Patch at (4, 0, 0); aim 0.5m off-center (within radius 0.45 + slack 0.3).
    t.app.world_mut().trigger(PaintSurface {
        surface: "frost".into(),
        position: Vec3::new(4.0, 0.0, 0.0),
        owner: caster,
    });
    t.app.world_mut().flush();
    {
        use obelisk_bevy::verbs::ObeliskCommandsExt;
        t.app.world_mut().commands().entity(caster).grant_skill("spire_probe");
        t.app
            .world_mut()
            .commands()
            .entity(caster)
            .cast_skill_at_point("spire_probe", Vec3::new(4.5, 0.0, 0.0));
    }
    t.app.world_mut().flush();
    t.advance_ticks(20);
    assert_eq!(t.rec().cast_began.len(), 1, "gated cast accepted on a frost patch");
    // Consume-on-accept: the patch is gone.
    assert_eq!(patch_count(&mut t, "frost"), 0, "patch consumed at cast-accept");
    assert!(t
        .rec()
        .surfaces_removed
        .iter()
        .any(|r| r.surface == "frost" && r.reason == SurfaceRemoveReason::Consumed));
    // Snap: the CastPoint-anchored window spawned at the PATCH CENTER (4,0,0), not (4.5,..).
    let w = t
        .rec()
        .hit_window_opened
        .iter()
        .find(|w| w.skill_id == "spire_probe")
        .expect("window opened");
    let hb_pos = t.rec().hitbox_ended.iter().find(|e| e.skill_id == "spire_probe").unwrap().position;
    let _ = w;
    assert!(
        (hb_pos - Vec3::new(4.0, 0.0, 0.0)).length() < 0.01,
        "snapped to patch center, got {hb_pos:?}"
    );
}

#[test]
fn on_surface_miss_fizzles_or_falls_back() {
    let mut t = surf_app(10);
    let caster = spawn_combatant(&mut t, "spirer", Vec3::ZERO, obelisk_bevy::prelude::Faction::Player);
    load_timeline(&mut t, "spire_probe", "tests/fixtures/cast/spire_probe.cast.ron");
    load_timeline(&mut t, "spire_fallback", "tests/fixtures/cast/spire_fallback.cast.ron");
    {
        use obelisk_bevy::verbs::ObeliskCommandsExt;
        t.app.world_mut().commands().entity(caster).grant_skill("spire_probe");
        t.app.world_mut().commands().entity(caster).grant_skill("spire_fallback");
        // No frost anywhere: Fizzle variant rejects...
        t.app
            .world_mut()
            .commands()
            .entity(caster)
            .cast_skill_at_point("spire_probe", Vec3::new(4.0, 0.0, 0.0));
    }
    t.app.world_mut().flush();
    t.advance_ticks(10);
    assert_eq!(t.rec().cast_began.len(), 0);
    assert_eq!(t.rec().cast_rejected.len(), 1, "no frost -> paid fizzle (CastRejected)");
    // ...while the Then(SelfPoint) variant falls back and casts at the caster.
    t.app
        .world_mut()
        .commands()
        .entity(caster)
        .cast_skill_at_point("spire_fallback", Vec3::new(4.0, 0.0, 0.0));
    t.app.world_mut().flush();
    t.advance_ticks(20);
    assert_eq!(t.rec().cast_began.len(), 1, "fallback chain rescued the cast");
    let hb_pos = t
        .rec()
        .hitbox_ended
        .iter()
        .find(|e| e.skill_id == "spire_fallback")
        .unwrap()
        .position;
    assert!(
        (hb_pos - Vec3::ZERO).length() < 0.01,
        "SelfPoint fallback anchors at the caster, got {hb_pos:?}"
    );
}
