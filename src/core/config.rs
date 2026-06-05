use bevy::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;
use stat_core::Skill;

/// Registry of obelisk skills (the REAL Skill type, not DamagePacketGenerator).
#[derive(Resource, Default)]
pub struct SkillRegistry(pub HashMap<String, Skill>);

/// The only RNG threaded into combat. Seeded for determinism.
#[derive(Resource)]
pub struct CombatRng(pub ChaCha8Rng);

/// Where skill rules come from.
pub enum SkillSource { Dir(PathBuf), Toml(String) }

/// App-builder verbs for obelisk setup. All global init is GUARDED (idempotent) because
/// obelisk's `init_*` panic on a second call — critical for tests and in-process client+server.
pub trait ObeliskConfigExt {
    fn add_obelisk_config_constants_default(&mut self) -> &mut Self;
    fn add_obelisk_config_constants(&mut self, path: &Path) -> &mut Self;
    fn add_obelisk_effects(&mut self, dir: &Path) -> &mut Self;
    fn add_obelisk_skills(&mut self, source: SkillSource) -> &mut Self;
    fn seed_combat_rng(&mut self, seed: u64) -> &mut Self;
}

impl ObeliskConfigExt for App {
    fn add_obelisk_config_constants_default(&mut self) -> &mut Self {
        if !stat_core::config::constants_initialized() {
            stat_core::init_constants_default().expect("init constants");
        }
        self
    }
    fn add_obelisk_config_constants(&mut self, path: &Path) -> &mut Self {
        if !stat_core::config::constants_initialized() {
            stat_core::init_constants(path).expect("init constants from path");
        }
        self
    }
    fn add_obelisk_effects(&mut self, dir: &Path) -> &mut Self {
        if !stat_core::config::effect_registry_initialized() {
            stat_core::init_effect_registry(dir).expect("init effect registry");
        }
        self
    }
    fn add_obelisk_skills(&mut self, source: SkillSource) -> &mut Self {
        let map = match source {
            SkillSource::Dir(d) => stat_core::config::load_skills_dir(&d).expect("load skills dir"),
            SkillSource::Toml(s) => stat_core::config::parse_skills(&s).expect("parse skills"),
        };
        self.insert_resource(SkillRegistry(map));
        self
    }
    fn seed_combat_rng(&mut self, seed: u64) -> &mut Self {
        self.insert_resource(CombatRng(ChaCha8Rng::seed_from_u64(seed)));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_loads_skills_and_seeds_rng() {
        let toml = r#"
[[skills]]
id = "firebolt"
name = "Firebolt"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 5.0
[skills.damage]
base_damages = [{ type = "fire", min = 20.0, max = 20.0 }]
"#;
        let mut app = App::new();
        app.add_obelisk_config_constants_default();
        app.add_obelisk_skills(SkillSource::Toml(toml.into()));
        app.seed_combat_rng(42);

        assert!(app.world().resource::<SkillRegistry>().0.contains_key("firebolt"));
        assert!(app.world().get_resource::<CombatRng>().is_some());
        assert!(stat_core::config::constants_initialized());
    }
}
