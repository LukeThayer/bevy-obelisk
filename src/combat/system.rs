use crate::combat::resolve::resolve_one_hit;
use crate::core::components::Attributes;
use crate::core::config::{CombatRng, SkillRegistry};
use crate::events::{DamageResolved, EffectApplied, EntityDied, HitConfirmed, TriggerFired};
use bevy::prelude::*;
use stat_core::combat::resolve_damage_with_rng;

/// Hard cap on the number of triggered-effect resolutions processed for a single hit.
/// Bounds any pathological re-entrant cascade (defensive — damage resolution does not itself
/// produce new triggers, so the worklist cannot actually grow from auto-firing alone).
const MAX_TRIGGER_RESOLUTIONS: usize = 8;

/// Observer: when a hit is confirmed, run the deterministic resolve funnel and emit results.
pub fn on_hit_confirmed(
    hit: On<HitConfirmed>,
    mut attrs: Query<&mut Attributes>,
    registry: Res<SkillRegistry>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
) {
    let ev = hit.event().clone();
    let Some(skill) = registry.0.get(&ev.skill_id) else {
        return;
    };

    // Need &mut caster + &mut target disjointly.
    let [mut caster_attrs, mut target_attrs] = match attrs.get_many_mut([ev.caster, ev.target]) {
        Ok(pair) => pair,
        Err(_) => return, // same entity or missing
    };

    let outcome = match resolve_one_hit(
        &mut caster_attrs.0,
        &mut target_attrs.0,
        skill,
        &registry.0,
        &mut rng.0,
    ) {
        Ok(o) => o,
        Err(_) => return,
    };

    let life_after = target_attrs.0.current_life;
    commands.trigger(DamageResolved {
        caster: ev.caster,
        target: ev.target,
        skill_id: ev.skill_id.clone(),
        total_damage: outcome.total_damage,
        is_killing_blow: outcome.is_killing_blow,
        life_after,
        mana_spent: outcome.mana_spent,
    });
    for eff in &outcome.effects_applied {
        commands.trigger(EffectApplied {
            target: ev.target,
            effect_id: eff.id.clone(),
            total_duration: eff.total_duration,
            stacks: eff.stacks,
        });
    }
    if outcome.is_killing_blow || !target_attrs.0.is_alive() {
        commands.trigger(EntityDied {
            target: ev.target,
            killer: Some(ev.caster),
        });
    }

    // Process effect-condition triggers (OnApply/OnMaxStacks/OnConsume, etc.) produced by this
    // hit. Every trigger is made observable via `TriggerFired` (never silently dropped). On-hit
    // triggered SKILLS that exist in the registry are additionally auto-fired through the same
    // deterministic resolve path, against the HIT TARGET — the natural default routing for on-hit
    // triggers. Bespoke routing (on_kill / splash / target reselection) is intentionally left to
    // the game via `TriggerFired` observability.
    //
    // Determinism: every triggered resolution draws from the same seeded `rng.0`, in worklist
    // order. The worklist is depth-guarded by MAX_TRIGGER_RESOLUTIONS so a (theoretical)
    // re-entrant cascade can never run unbounded; `resolve_damage_with_rng` returns only a
    // CombatResult (no new triggers), so the list does not actually grow from auto-firing.
    let caster_source_id = caster_attrs.0.id.clone();
    let mut worklist: std::collections::VecDeque<stat_core::TriggeredEffect> =
        outcome.triggered.into_iter().collect();
    let mut resolved = 0usize;
    while let Some(te) = worklist.pop_front() {
        if resolved >= MAX_TRIGGER_RESOLUTIONS {
            break;
        }
        resolved += 1;

        // Always observable, even if the triggered skill isn't in the registry / isn't a damage
        // skill (self-buff, deferred routing, etc.).
        commands.trigger(TriggerFired {
            source: ev.caster,
            target: ev.target,
            skill_id: te.skill_id.clone(),
            effect_id: te.effect_id.clone(),
        });

        // Auto-fire on-hit triggered damage skills against the hit target.
        let Some(triggered_skill) = registry.0.get(&te.skill_id) else {
            continue;
        };
        let packet = te.to_damage_packet(triggered_skill, &mut rng.0, &caster_source_id);
        // Reuse the already-borrowed `target_attrs` (do NOT re-borrow the query).
        let (new_def, result) = resolve_damage_with_rng(&target_attrs.0, &packet, &mut rng.0);
        target_attrs.0 = new_def;

        commands.trigger(DamageResolved {
            caster: ev.caster,
            target: ev.target,
            skill_id: te.skill_id.clone(),
            total_damage: result.total_damage,
            is_killing_blow: result.is_killing_blow,
            life_after: target_attrs.0.current_life,
            mana_spent: 0.0,
        });
        if result.is_killing_blow {
            commands.trigger(EntityDied {
                target: ev.target,
                killer: Some(ev.caster),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CombatRng;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;
    use stat_core::{Effect, EffectCondition, EffectTrigger, StatBlock};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// End-to-end through the observer: a caster carries an `OnConsume` self-effect that the
    /// cast skill consumes; the triggered skill ("static_discharge") IS in the registry, so it
    /// must (a) emit `TriggerFired` and (b) auto-fire as a triggered damage hit against the
    /// target — without panicking or looping (the worklist is depth-guarded).
    #[test]
    fn on_hit_confirmed_emits_trigger_fired_and_fires_triggered_skill() {
        stat_core::config::ensure_constants_initialized();

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

[[skills]]
id = "static_discharge"
name = "Static Discharge"
tags = ["spell", "lightning"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "lightning", min = 25.0, max = 25.0 }]
"#;
        let registry: HashMap<String, stat_core::Skill> =
            stat_core::config::parse_skills(toml).unwrap();

        let mut app = App::new();
        app.add_observer(on_hit_confirmed);
        app.insert_resource(SkillRegistry(registry));
        app.insert_resource(CombatRng(ChaCha8Rng::seed_from_u64(5)));

        // Recorder for TriggerFired (the EventRecorder in testkit doesn't cover this event).
        let fired: Arc<Mutex<Vec<TriggerFired>>> = Arc::new(Mutex::new(Vec::new()));
        let fired_clone = fired.clone();
        app.add_observer(move |e: On<TriggerFired>| {
            fired_clone.lock().unwrap().push(e.event().clone());
        });

        // Caster carries a "charged" self-effect with an OnConsume -> static_discharge trigger.
        let mut caster_block = StatBlock::with_id("player");
        caster_block.max_mana.base = 100.0;
        caster_block.current_mana = 100.0;
        let mut charged = Effect::stat_buff("charged", "Charged", 999.0, vec![]);
        charged.conditions = vec![EffectCondition {
            trigger_skill: "static_discharge".to_string(),
            trigger: EffectTrigger::OnConsume,
        }];
        caster_block.add_effect(charged);

        let mut target_block = StatBlock::with_id("dummy");
        target_block.max_life.base = 200.0;
        target_block.current_life = 200.0;

        let caster = app.world_mut().spawn(Attributes(caster_block)).id();
        let target = app.world_mut().spawn(Attributes(target_block)).id();

        let life_before = app
            .world()
            .entity(target)
            .get::<Attributes>()
            .unwrap()
            .0
            .current_life;

        app.world_mut().trigger(HitConfirmed {
            caster,
            target,
            skill_id: "discharge_strike".to_string(),
            window_id: "w".to_string(),
        });
        // Flush queued commands so the observer's `commands.trigger(...)` events (TriggerFired /
        // DamageResolved) actually dispatch and the target's mutated Attributes are applied.
        app.world_mut().flush();

        let fired = fired.lock().unwrap();
        assert!(
            fired
                .iter()
                .any(|t| t.skill_id == "static_discharge" && t.effect_id == "charged"),
            "TriggerFired for the OnConsume trigger must be emitted, got {:?}",
            *fired
        );

        // The triggered skill is in the registry, so it auto-fired a lightning hit against the
        // target — life must have dropped further than the primary physical hit alone.
        let life_after = app
            .world()
            .entity(target)
            .get::<Attributes>()
            .unwrap()
            .0
            .current_life;
        assert!(
            life_after < life_before - 10.0,
            "triggered static_discharge should have dealt additional damage: {} -> {}",
            life_before,
            life_after
        );
    }
}
