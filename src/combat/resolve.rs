use std::collections::HashMap;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;
use stat_core::{Skill, SkillUseError, Effect};
use stat_core::StatBlock;
use stat_core::combat::resolve_damage_with_triggers;

/// Outcome of resolving one hit, projected from obelisk's CombatResults.
#[derive(Debug, Clone)]
pub struct HitOutcome {
    pub total_damage: f64,
    pub is_killing_blow: bool,
    pub effects_applied: Vec<Effect>,
    pub mana_spent: f64,
}

/// The ONE true deterministic resolve path. Never calls `receive_damage`/`resolve_damage`.
pub fn resolve_one_hit(
    caster: &mut StatBlock,
    target: &mut StatBlock,
    skill: &Skill,
    registry: &HashMap<String, Skill>,
    rng: &mut ChaCha8Rng,
) -> Result<HitOutcome, SkillUseError> {
    let source_id = caster.id.clone();
    // use_skill_against needs &mut caster + &target simultaneously: snapshot the target.
    let target_snapshot = target.clone();
    let skill_result = caster.use_skill_against(Some(&target_snapshot), skill, registry, source_id, rng)?;

    // Resolve the produced packets against the live target (deterministic, seeded).
    let tr = resolve_damage_with_triggers(caster, target, &skill_result.packets, skill, registry, rng);

    let total_damage: f64 = tr.results.iter().map(|r| r.total_damage).sum();
    let is_killing_blow = tr.results.iter().any(|r| r.is_killing_blow);
    let effects_applied: Vec<Effect> = tr.results.iter().flat_map(|r| r.effects_applied.clone()).collect();

    // Write the mutated defender back into the caller's target.
    *target = tr.defender;

    Ok(HitOutcome { total_damage, is_killing_blow, effects_applied, mana_spent: skill_result.mana_spent })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::StatBlock;

    fn firebolt_registry() -> HashMap<String, Skill> {
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
        stat_core::config::parse_skills(toml).unwrap()
    }

    #[test]
    fn firebolt_deals_deterministic_damage_and_spends_mana() {
        stat_core::config::ensure_constants_initialized();
        let registry = firebolt_registry();
        let skill = registry.get("firebolt").unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0; caster.current_mana = 100.0;

        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 50.0; target.current_life = 50.0;

        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let outcome = resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();

        assert!(outcome.total_damage > 0.0, "should deal damage");
        assert!(target.current_life < 50.0, "target should have taken damage");
        assert_eq!(caster.current_mana, 95.0, "5 mana spent");
        assert_eq!(outcome.mana_spent, 5.0);
    }

    #[test]
    fn resolution_is_deterministic_for_a_fixed_seed() {
        stat_core::config::ensure_constants_initialized();
        let registry = firebolt_registry();
        let skill = registry.get("firebolt").unwrap();
        let run = || {
            let mut c = StatBlock::with_id("p"); c.max_mana.base = 100.0; c.current_mana = 100.0;
            let mut t = StatBlock::with_id("d"); t.max_life.base = 50.0; t.current_life = 50.0;
            let mut rng = ChaCha8Rng::seed_from_u64(7);
            resolve_one_hit(&mut c, &mut t, skill, &registry, &mut rng).unwrap().total_damage
        };
        assert_eq!(run(), run(), "same seed must produce identical damage");
    }
}
