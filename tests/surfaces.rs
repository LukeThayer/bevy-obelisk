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
