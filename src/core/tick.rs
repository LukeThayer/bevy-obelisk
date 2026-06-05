use bevy::prelude::*;
use crate::core::components::Attributes;
use crate::events::{DotTicked, EffectExpired, EntityDied};

/// Drive obelisk's tick_effects once per fixed step. tick_effects is immutable (returns a
/// new block), so we replace the component's StatBlock and emit what happened.
pub fn tick_effects_system(
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Attributes)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs_f64();
    for (e, mut attrs) in &mut q {
        if attrs.effects_is_empty_fast() { continue; }
        let (new_block, result) = attrs.0.tick_effects(dt);
        let was_alive = attrs.0.is_alive();
        attrs.0 = new_block;

        if result.dot_damage > 0.0 {
            // Attribute DoT to the effects that ticked; for the slice, emit a single rollup.
            commands.trigger(DotTicked { target: e, effect_id: String::new(), dot_damage: result.dot_damage, life_remaining: result.life_remaining });
        }
        for id in &result.expired_effects {
            commands.trigger(EffectExpired { target: e, effect_id: id.clone() });
        }
        if was_alive && result.is_dead {
            commands.trigger(EntityDied { target: e, killer: None });
        }
    }
}
