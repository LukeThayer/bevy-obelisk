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
    /// Whether any damage packet of this hit was a critical strike (from `DamagePacket`).
    pub is_critical: bool,
    /// Total damage prevented by all mitigation on the defender, summed across every
    /// `CombatResult` produced by this hit (see `combat_result_prevented`).
    pub damage_prevented: f64,
    /// Life the caster gained from this hit (life-on-hit + life-on-kill leech).
    pub life_gained: f64,
    /// Mana the caster gained from this hit (mana-on-hit + mana-on-kill leech).
    pub mana_gained: f64,
    /// Effect-condition triggers produced by this hit (OnApply/OnMaxStacks/OnConsume, etc.):
    /// both caster-side (from consuming self-effects in `use_skill_against`) and target-side
    /// (from status effects applied to the defender during damage resolution). Surfaced so the
    /// caller can fire / observe them — never silently dropped.
    pub triggered: Vec<stat_core::TriggeredEffect>,
    /// Skill-condition-triggered hits (e.g. an `on_crit` damage condition naming a `trigger_skill`)
    /// that obelisk already resolved INLINE against the defender. Each is a real `CombatResult` the
    /// defender already absorbed; `on_hit_confirmed` re-buckets these into their own secondary cast
    /// (a distinct `TriggerFired` plus `DamageResolved`), instead of summing them into the primary
    /// `total_damage`. These are NOT effect triggers (`triggered`) and are NEVER re-resolved.
    pub triggered_skill_hits: Vec<TriggeredSkillHit>,
}

/// A skill-condition-triggered secondary hit, projected straight off the already-resolved
/// `CombatResult` obelisk produced for the triggered `DamagePacket`. Carries the numbers needed to
/// emit a distinct `TriggerFired` + `DamageResolved` for the secondary cast — never re-resolved.
#[derive(Debug, Clone)]
pub struct TriggeredSkillHit {
    /// The secondary (triggered) skill id, e.g. "static_discharge" (`CombatResult.skill_id`).
    pub secondary_skill_id: String,
    /// The parent skill that fired the condition, e.g. "critzap" (`CombatResult.triggered_by`).
    pub triggered_by: String,
    pub total_damage: f64,
    pub is_killing_blow: bool,
    pub is_critical: bool,
    pub damage_prevented: f64,
    pub life_gained: f64,
    pub mana_gained: f64,
}

/// Total damage prevented by every mitigation channel obelisk records on a single `CombatResult`:
/// barrier absorption, armour, resistances, block, physical/generic damage reduction, evasion-cap
/// oneshot protection, and elude. Reads real fields off the result — never recomputes mitigation.
pub fn combat_result_prevented(r: &stat_core::combat::CombatResult) -> f64 {
    r.damage_blocked_by_barrier
        + r.damage_reduced_by_armour
        + r.damage_reduced_by_resists
        + r.damage_blocked
        + r.damage_reduced_by_physical_dr
        + r.damage_reduced_by_dr
        + r.damage_prevented_by_oneshot
        + r.damage_prevented_by_elude
}

/// Life gained by the caster from one `CombatResult` (on-hit + on-kill leech).
pub fn combat_result_life_gained(r: &stat_core::combat::CombatResult) -> f64 {
    r.life_gained_on_hit + r.life_gained_on_kill
}

/// Mana gained by the caster from one `CombatResult` (on-hit + on-kill leech).
pub fn combat_result_mana_gained(r: &stat_core::combat::CombatResult) -> f64 {
    r.mana_gained_on_hit + r.mana_gained_on_kill
}

/// The ONE true deterministic resolve path. Never calls `receive_damage`/`resolve_damage`.
///
/// Uncharged (`charge: None`) — identical to the pre-charge code path; see
/// [`resolve_one_hit_charged`].
pub fn resolve_one_hit(
    caster: &mut StatBlock,
    target: &mut StatBlock,
    skill: &Skill,
    registry: &HashMap<String, Skill>,
    rng: &mut ChaCha8Rng,
) -> Result<HitOutcome, SkillUseError> {
    resolve_one_hit_charged(caster, target, skill, registry, rng, None)
}

/// As [`resolve_one_hit`], with an optional per-cast `charge` that scales the damage dealt by
/// [`charge_mult`](crate::timeline::cast::charge_mult).
///
/// Golden-safety: when `charge` is `None` this takes EXACTLY the original code path — the resolved
/// packets are never touched, so the byte-for-byte output is unchanged. When `charge` is `Some`, the
/// packet `FinalDamage` magnitudes are scaled AFTER `use_skill_against` produces them (so the crit
/// roll already happened) and BEFORE `resolve_damage_with_triggers` consumes them (so resistances /
/// mitigation apply on the scaled "stronger bolt"). The scale is a plain float multiply that draws
/// NO RNG, preserving determinism.
pub fn resolve_one_hit_charged(
    caster: &mut StatBlock,
    target: &mut StatBlock,
    skill: &Skill,
    registry: &HashMap<String, Skill>,
    rng: &mut ChaCha8Rng,
    charge: Option<u8>,
) -> Result<HitOutcome, SkillUseError> {
    let source_id = caster.id.clone();
    // use_skill_against needs &mut caster + &target simultaneously: snapshot the target.
    let target_snapshot = target.clone();
    let mut skill_result =
        caster.use_skill_against(Some(&target_snapshot), skill, registry, source_id, rng)?;

    // Charge multiplier: a no-op for the default `None` path (packets untouched). Only when a charge
    // is present do we scale the produced packets' damage magnitudes — a stronger bolt — before the
    // resolve consumes them. No RNG is drawn here.
    if charge.is_some() {
        let mult = crate::timeline::cast::charge_mult(charge) as f64;
        for packet in &mut skill_result.packets {
            for d in &mut packet.damages {
                d.amount *= mult;
            }
        }
    }

    // Resolve the produced packets against the live target (deterministic, seeded).
    let tr =
        resolve_damage_with_triggers(caster, target, &skill_result.packets, skill, registry, rng);

    // Partition the resolved results into the PRIMARY hit (`!is_triggered`) and skill-condition
    // SECONDARY casts (`is_triggered`). obelisk already resolved every packet inline and mutated the
    // defender once, so the triggered results' damage is ALREADY reflected in `tr.defender`; we only
    // re-bucket the already-computed output. Triggered results are EXCLUDED from `total_damage` (the
    // double-count fix) and each becomes a `TriggeredSkillHit` read straight off the result — never
    // re-resolved (which would re-draw the seeded RNG / double-damage the defender).
    //
    // `is_critical` lives on the DamagePacket, not the CombatResult; pair each result with its source
    // packet by position. `resolve_damage_with_triggers` pushes results in packet order until it
    // (optionally) breaks early on a kill, so the resolved slice is a prefix of `packets` and
    // `packets.iter().zip(tr.results.iter())` aligns every result with its packet.
    let mut total_damage = 0.0;
    let mut is_killing_blow = false;
    let mut effects_applied: Vec<Effect> = Vec::new();
    let mut is_critical = false;
    let mut damage_prevented = 0.0;
    let mut life_gained = 0.0;
    let mut mana_gained = 0.0;
    let mut triggered_skill_hits: Vec<TriggeredSkillHit> = Vec::new();

    for (packet, r) in skill_result.packets.iter().zip(tr.results.iter()) {
        if r.is_triggered {
            // Secondary cast: surface as its own hit, do NOT fold into the primary aggregates.
            triggered_skill_hits.push(TriggeredSkillHit {
                secondary_skill_id: r.skill_id.clone(),
                triggered_by: r.triggered_by.clone().unwrap_or_default(),
                total_damage: r.total_damage,
                is_killing_blow: r.is_killing_blow,
                is_critical: packet.is_critical,
                damage_prevented: combat_result_prevented(r),
                life_gained: combat_result_life_gained(r),
                mana_gained: combat_result_mana_gained(r),
            });
        } else {
            // Primary hit: feed the existing aggregates (identical to the pre-partition behavior
            // when no triggered packets exist — the strict no-op for every untriggered scenario).
            total_damage += r.total_damage;
            is_killing_blow |= r.is_killing_blow;
            effects_applied.extend(r.effects_applied.iter().cloned());
            is_critical |= packet.is_critical;
            damage_prevented += combat_result_prevented(r);
            life_gained += combat_result_life_gained(r);
            mana_gained += combat_result_mana_gained(r);
        }
    }

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
        is_critical,
        damage_prevented,
        life_gained,
        mana_gained,
        triggered,
        triggered_skill_hits,
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

    /// A per-cast charge scales the damage dealt by `charge_mult`: `Some(255)` => 2.0x (40 from a
    /// base-20 firebolt), while `None` is the exact uncharged base (20). The multiply draws no RNG.
    #[test]
    fn charge_scales_firebolt_damage() {
        stat_core::config::ensure_constants_initialized();
        let registry = firebolt_registry();
        let skill = registry.get("firebolt").unwrap();

        let resolve = |charge: Option<u8>| {
            let mut caster = StatBlock::with_id("player");
            caster.max_mana.base = 100.0;
            caster.current_mana = 100.0;
            let mut target = StatBlock::with_id("dummy");
            target.max_life.base = 500.0;
            target.current_life = 500.0;
            let mut rng = ChaCha8Rng::seed_from_u64(1);
            resolve_one_hit_charged(&mut caster, &mut target, skill, &registry, &mut rng, charge)
                .unwrap()
                .total_damage
        };

        assert!(
            (resolve(None) - 20.0).abs() < 1e-6,
            "uncharged firebolt deals base 20, got {}",
            resolve(None)
        );
        assert!(
            (resolve(Some(255)) - 40.0).abs() < 1e-6,
            "fully charged firebolt deals 2.0x = 40, got {}",
            resolve(Some(255))
        );
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

    /// Skill-condition damage trigger: `critzap` carries an `on_crit` condition (`additional = true`)
    /// that triggers `static_discharge`. With a 100% crit caster, critzap crits, so obelisk produces
    /// a primary critzap packet (20 fire x 1.5 crit = 30) AND a triggered static_discharge packet.
    /// The 100% crit chance applies to EVERY packet, so the triggered packet also crits
    /// (25 lightning x 1.5 = 37.5) — faithful, deterministic obelisk behavior. The fix must surface
    /// the secondary cast SEPARATELY:
    ///   - the primary `outcome.total_damage` is 30 — it EXCLUDES the 37.5 triggered damage (the
    ///     double-count fix; previously this was a summed 67.5);
    ///   - exactly one `TriggeredSkillHit` is surfaced, carrying the secondary skill id
    ///     `static_discharge`, its parent `critzap`, and its own 37.5 damage.
    ///
    /// The defender (high life, survives) absorbs BOTH inline, so its life drops by 30 + 37.5 = 67.5.
    #[test]
    fn skill_trigger_excludes_triggered_from_primary() {
        crate::testkit::init_test_obelisk();
        let registry =
            stat_core::config::load_skills_dir(std::path::Path::new("tests/fixtures/skills"))
                .unwrap();
        let skill = registry.get("critzap").unwrap();

        let mut caster = StatBlock::with_id("player");
        caster.max_mana.base = 100.0;
        caster.current_mana = 100.0;
        // 100% flat crit chance => the seeded crit roll always passes (same path as `crit_strike`).
        caster.critical_chance.flat = 100.0;

        let mut target = StatBlock::with_id("dummy");
        target.max_life.base = 200.0;
        target.current_life = 200.0;

        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let outcome =
            resolve_one_hit(&mut caster, &mut target, skill, &registry, &mut rng).unwrap();

        // Primary critzap crit = 20 base * 1.5 = 30, and the triggered 25 is NOT folded in.
        assert!(
            (outcome.total_damage - 30.0).abs() < 1e-6,
            "primary total_damage must EXCLUDE the triggered 25 (expected 30, got {})",
            outcome.total_damage
        );
        assert!(outcome.is_critical, "critzap should have crit");

        // Exactly one triggered secondary cast, surfaced with its own numbers.
        assert_eq!(
            outcome.triggered_skill_hits.len(),
            1,
            "expected exactly one triggered skill hit, got {:?}",
            outcome.triggered_skill_hits
        );
        let th = &outcome.triggered_skill_hits[0];
        assert_eq!(th.secondary_skill_id, "static_discharge");
        assert_eq!(th.triggered_by, "critzap");
        assert!(
            th.is_critical,
            "the triggered packet also crits under 100% crit"
        );
        assert!(
            (th.total_damage - 37.5).abs() < 1e-6,
            "triggered static_discharge should deal 25 x 1.5 crit = 37.5, got {}",
            th.total_damage
        );

        // The defender absorbed BOTH inline: 200 - 30 - 37.5 = 132.5.
        assert!(
            (target.current_life - 132.5).abs() < 1e-6,
            "defender should have taken 30 + 37.5 = 67.5 (life 200 -> 132.5), got {}",
            target.current_life
        );
    }

    #[test]
    fn burn_is_applied_and_ticks_down_life() {
        crate::testkit::init_test_obelisk();
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
