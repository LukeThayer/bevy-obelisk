use rand_chacha::ChaCha8Rng;
use stat_core::combat::resolve_damage_with_triggers;
use stat_core::StatBlock;
use stat_core::{Effect, Skill, SkillUseError};
use std::collections::HashMap;

/// Outcome of resolving one hit, projected from obelisk's CombatResults.
#[derive(Debug, Clone)]
pub struct HitOutcome {
    pub total_damage: f64,
    pub is_killing_blow: bool,
    pub effects_applied: Vec<Effect>,
    pub mana_spent: f64,
    /// Effect-condition triggers produced by this hit (OnApply/OnMaxStacks/OnConsume, etc.):
    /// both caster-side (from consuming self-effects in `use_skill_against`) and target-side
    /// (from status effects applied to the defender during damage resolution). Surfaced so the
    /// caller can fire / observe them — never silently dropped.
    pub triggered: Vec<stat_core::TriggeredEffect>,
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
    let skill_result =
        caster.use_skill_against(Some(&target_snapshot), skill, registry, source_id, rng)?;

    // Resolve the produced packets against the live target (deterministic, seeded).
    let tr =
        resolve_damage_with_triggers(caster, target, &skill_result.packets, skill, registry, rng);

    let total_damage: f64 = tr.results.iter().map(|r| r.total_damage).sum();
    let is_killing_blow = tr.results.iter().any(|r| r.is_killing_blow);
    let effects_applied: Vec<Effect> = tr
        .results
        .iter()
        .flat_map(|r| r.effects_applied.clone())
        .collect();

    // Surface ALL effect-condition triggers so none are silently dropped:
    //  - caster-side: produced by `use_skill_against` consuming self-effects (OnConsume, etc.).
    //  - target-side: produced while resolving the packets against the defender, e.g. an OnApply
    //    condition firing when a status effect lands on the target this hit.
    let mut triggered: Vec<stat_core::TriggeredEffect> = skill_result.triggered_effects.clone();
    for r in &tr.results {
        triggered.extend(r.triggered_effects.iter().cloned());
    }

    // Write the mutated defender back into the caller's target.
    *target = tr.defender;

    Ok(HitOutcome {
        total_damage,
        is_killing_blow,
        effects_applied,
        mana_spent: skill_result.mana_spent,
        triggered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
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
        caster.max_mana.base = 100.0;
        caster.current_mana = 100.0;

        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 50.0;
        target.current_life = 50.0;

        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let outcome =
            resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();

        assert!(outcome.total_damage > 0.0, "should deal damage");
        assert!(
            target.current_life < 50.0,
            "target should have taken damage"
        );
        assert_eq!(caster.current_mana, 95.0, "5 mana spent");
        assert_eq!(outcome.mana_spent, 5.0);
    }

    #[test]
    fn resolution_is_deterministic_for_a_fixed_seed() {
        stat_core::config::ensure_constants_initialized();
        let registry = firebolt_registry();
        let skill = registry.get("firebolt").unwrap();
        let run = || {
            let mut c = StatBlock::with_id("p");
            c.max_mana.base = 100.0;
            c.current_mana = 100.0;
            let mut t = StatBlock::with_id("d");
            t.max_life.base = 50.0;
            t.current_life = 50.0;
            let mut rng = ChaCha8Rng::seed_from_u64(7);
            resolve_one_hit(&mut c, &mut t, skill, &registry, &mut rng)
                .unwrap()
                .total_damage
        };
        assert_eq!(run(), run(), "same seed must produce identical damage");
    }

    /// Firebolt has no skill/effect triggers, so `triggered` must come back empty (and the
    /// field must be accessible without panicking) — proving the additive surfacing is a no-op
    /// for the untriggered slice.
    #[test]
    fn firebolt_produces_no_triggers() {
        stat_core::config::ensure_constants_initialized();
        let registry = firebolt_registry();
        let skill = registry.get("firebolt").unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0;
        caster.current_mana = 100.0;
        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 50.0;
        target.current_life = 50.0;

        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let outcome =
            resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();
        assert!(
            outcome.triggered.is_empty(),
            "firebolt has no triggers; HitOutcome.triggered must be empty"
        );
    }

    /// Author a real triggered effect end-to-end at the resolve layer: a caster carries a
    /// self-effect with an `OnConsume` condition; the skill consumes it. The resulting
    /// `TriggeredEffect` MUST be surfaced in `HitOutcome.triggered` (previously it was dropped
    /// when `use_skill_against`'s `SkillResult.triggered_effects` was ignored).
    #[test]
    fn on_consume_trigger_is_surfaced_in_hit_outcome() {
        use stat_core::{EffectCondition, EffectTrigger};

        stat_core::config::ensure_constants_initialized();
        // A skill that consumes the caster's "charged" self-effect on use.
        let toml = r#"
[[skills]]
id = "discharge_strike"
name = "Discharge Strike"
tags = ["attack", "physical", "melee"]
targeting = "single_enemy"
delivery = "melee"
mana_cost = 0.0
consumes_self_effect = [{ id = "charged" }]
[skills.damage]
base_damages = [{ type = "physical", min = 10.0, max = 10.0 }]
"#;
        let registry = stat_core::config::parse_skills(toml).unwrap();
        let skill = registry.get("discharge_strike").unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0;
        caster.current_mana = 100.0;
        // Place a "charged" effect on the caster carrying an OnConsume -> "static_discharge" trigger.
        let mut charged = Effect::stat_buff("charged", "Charged", 999.0, vec![]);
        charged.conditions = vec![EffectCondition {
            trigger_skill: "static_discharge".to_string(),
            trigger: EffectTrigger::OnConsume,
        }];
        caster.add_effect(charged);

        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 100.0;
        target.current_life = 100.0;

        let mut rng = ChaCha8Rng::seed_from_u64(5);
        let outcome =
            resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();

        assert!(
            outcome
                .triggered
                .iter()
                .any(|t| t.skill_id == "static_discharge" && t.effect_id == "charged"),
            "OnConsume trigger for 'charged' must be surfaced in HitOutcome.triggered, got {:?}",
            outcome.triggered
        );
    }

    #[test]
    fn burn_is_applied_and_ticks_down_life() {
        stat_core::config::ensure_constants_initialized();
        if !stat_core::config::effect_registry_initialized() {
            let _ = stat_core::init_effect_registry(std::path::Path::new("tests/fixtures/effects"));
        }
        let registry =
            stat_core::config::load_skills_dir(std::path::Path::new("tests/fixtures/skills"))
                .unwrap();
        let skill = registry.get("firebolt").unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0;
        caster.current_mana = 100.0;
        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 500.0;
        target.current_life = 500.0;

        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let outcome =
            resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();
        assert!(
            outcome.effects_applied.iter().any(|e| e.id == "burn"),
            "burn should be applied"
        );

        // Tick 1 second of DoT (immutable API returns a new block).
        let (ticked, tick_result) = target.tick_effects(1.0);
        assert!(
            tick_result.dot_damage > 0.0,
            "burn should deal DoT this tick"
        );
        assert!(
            ticked.current_life < target.current_life,
            "DoT should reduce life"
        );
    }
}
