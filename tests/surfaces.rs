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
