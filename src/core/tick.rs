use crate::core::components::Attributes;
use crate::events::{DotTicked, EffectExpired, EntityDied};
use bevy::prelude::*;

/// Drive obelisk's tick_effects once per fixed step. tick_effects is immutable (returns a
/// new block), so we replace the component's StatBlock and emit what happened.
pub fn tick_effects_system(
    time: Res<Time<Fixed>>,
    mut q: Query<(Entity, &mut Attributes)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs_f64();
    for (e, mut attrs) in &mut q {
        if attrs.effects_is_empty_fast() {
            continue;
        }
        if !attrs.0.is_alive() {
            continue;
        }
        let (new_block, result) = attrs.0.tick_effects(dt);
        let was_alive = attrs.0.is_alive();
        attrs.0 = new_block;

        if result.dot_damage > 0.0 {
            // Attribute DoT to the effects that ticked; for the slice, emit a single rollup.
            commands.trigger(DotTicked {
                target: e,
                effect_id: String::new(),
                dot_damage: result.dot_damage,
                life_remaining: result.life_remaining,
            });
        }
        for id in &result.expired_effects {
            commands.trigger(EffectExpired {
                target: e,
                effect_id: id.clone(),
            });
        }
        if was_alive && result.is_dead {
            commands.trigger(EntityDied {
                target: e,
                killer: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use stat_core::StatBlock;

    #[test]
    fn dead_entities_stop_ticking_dots() {
        let mut t = ObeliskTestApp::new(1);
        // Already dead (0 life) but has a damaging effect -> must NOT emit DotTicked.
        let mut block = StatBlock::with_id("corpse");
        block.max_life.base = 10.0;
        block.current_life = 0.0;
        let e = t
            .app
            .world_mut()
            .spawn((
                Combatant,
                Attributes(block),
                Faction::Enemy,
                ObeliskId("corpse".into()),
                Transform::default(),
            ))
            .id();
        t.app
            .world_mut()
            .commands()
            .entity(e)
            .apply_obelisk_effect("burn");
        t.app.update();

        #[derive(Resource, Default)]
        struct Ticks(usize);
        t.app.init_resource::<Ticks>();
        t.app
            .add_observer(|_e: On<DotTicked>, mut c: ResMut<Ticks>| c.0 += 1);
        t.advance_ticks(120);
        assert_eq!(
            t.app.world().resource::<Ticks>().0,
            0,
            "a dead entity must not emit DoT ticks"
        );
        let _ = e;
    }
}
