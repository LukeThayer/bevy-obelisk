use crate::core::spawn_rng::SpawnRng;
use bevy::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use stat_core::Skill;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// XOR mask deriving `SpawnRng`'s seed from `CombatRng`'s (Task 11, spec §3.2) — an arbitrary,
/// fixed constant chosen only so the two streams never share a seed (and thus never coincide),
/// NOT for any cryptographic property. See `SpawnRng`'s doc for why the two streams must stay
/// independent.
const SPAWN_RNG_SEED_XOR: u64 = 0x5EED_5EED;

/// Registry of obelisk skills (the REAL Skill type, not DamagePacketGenerator).
#[derive(Resource, Default)]
pub struct SkillRegistry(pub HashMap<String, Skill>);

/// The only RNG threaded into combat. Seeded for determinism.
#[derive(Resource)]
pub struct CombatRng(pub ChaCha8Rng);

impl Default for CombatRng {
    /// Deterministic seed-0 default so combat never panics on a missing resource.
    /// Consumers should override it via `App::seed_combat_rng` for a real seed.
    fn default() -> Self {
        CombatRng(ChaCha8Rng::seed_from_u64(0))
    }
}

/// Where skill rules come from.
pub enum SkillSource {
    Dir(PathBuf),
    Toml(String),
}

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
        // ensure_constants_initialized() is idempotent (Once-guarded defaults) — no manual guard needed.
        stat_core::config::ensure_constants_initialized();
        self
    }
    fn add_obelisk_config_constants(&mut self, path: &Path) -> &mut Self {
        // No ensure_* variant for a custom path exists; guard manually so we never double-init.
        if !stat_core::config::constants_initialized() {
            stat_core::init_constants(path).expect("init constants from path");
        }
        self
    }
    fn add_obelisk_effects(&mut self, dir: &Path) -> &mut Self {
        // ensure_effect_registry_initialized() initialises an EMPTY registry — not what we want here;
        // guard manually so we load from `dir` on first call without risking a double-init panic.
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
        // Task 11: seed the dedicated emitter-jitter stream ALONGSIDE CombatRng, from a
        // different derived seed so the two streams never coincide. See `SpawnRng`'s doc — this
        // is the ONLY place emitter code may ever draw jitter from; combat resolution never
        // touches this resource.
        self.insert_resource(SpawnRng(ChaCha8Rng::seed_from_u64(
            seed ^ SPAWN_RNG_SEED_XOR,
        )));
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

        assert!(app
            .world()
            .resource::<SkillRegistry>()
            .0
            .contains_key("firebolt"));
        assert!(app.world().get_resource::<CombatRng>().is_some());
        assert!(
            app.world().get_resource::<SpawnRng>().is_some(),
            "seed_combat_rng must also seed SpawnRng (Task 11)"
        );
        assert!(stat_core::config::constants_initialized());
    }
}
