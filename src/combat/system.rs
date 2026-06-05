use bevy::prelude::*;
use crate::combat::resolve::resolve_one_hit;
use crate::core::components::Attributes;
use crate::core::config::{CombatRng, SkillRegistry};
use crate::events::{DamageResolved, EffectApplied, EntityDied, HitConfirmed};

/// Observer: when a hit is confirmed, run the deterministic resolve funnel and emit results.
pub fn on_hit_confirmed(
    hit: On<HitConfirmed>,
    mut attrs: Query<&mut Attributes>,
    registry: Res<SkillRegistry>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
) {
    let ev = hit.event().clone();
    let Some(skill) = registry.0.get(&ev.skill_id) else { return };

    // Need &mut caster + &mut target disjointly.
    let [mut caster_attrs, mut target_attrs] = match attrs.get_many_mut([ev.caster, ev.target]) {
        Ok(pair) => pair,
        Err(_) => return, // same entity or missing
    };

    let outcome = match resolve_one_hit(&mut caster_attrs.0, &mut target_attrs.0, skill, &registry.0, &mut rng.0) {
        Ok(o) => o,
        Err(_) => return,
    };

    let life_after = target_attrs.0.current_life;
    commands.trigger(DamageResolved {
        caster: ev.caster, target: ev.target, skill_id: ev.skill_id.clone(),
        total_damage: outcome.total_damage, is_killing_blow: outcome.is_killing_blow,
        life_after, mana_spent: outcome.mana_spent,
    });
    for eff in &outcome.effects_applied {
        commands.trigger(EffectApplied { target: ev.target, effect_id: eff.id.clone(), total_duration: eff.total_duration, stacks: eff.stacks });
    }
    if outcome.is_killing_blow || !target_attrs.0.is_alive() {
        commands.trigger(EntityDied { target: ev.target, killer: Some(ev.caster) });
    }
}
