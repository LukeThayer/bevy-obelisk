use crate::core::components::Attributes;
use crate::events::{DotTicked, EffectExpired, EntityDied, TriggerFired};
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
        // Surface every effect-condition trigger fired during this tick (e.g. OnExpire) so it is
        // NEVER silently dropped — mirroring `apply_obelisk_effect` (OnApply/OnMaxStacks). SURFACE
        // ONLY: the triggered skill is not auto-fired here; the game observes TriggerFired to drive
        // bespoke routing. source == target since the effect lives on this entity.
        for te in &result.triggered_effects {
            commands.trigger(TriggerFired {
                source: e,
                target: e,
                skill_id: te.skill_id.clone(),
                effect_id: te.effect_id.clone(),
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

    #[test]
    fn on_expire_trigger_is_surfaced_when_an_effect_expires() {
        // Regression for a dropped-trigger bug: `tick_effects_system` previously discarded the
        // TickResult's `triggered_effects`, so an effect carrying an OnExpire trigger had it
        // SILENTLY DROPPED on expiry — violating "triggers are never silently dropped". The fix
        // surfaces each as a TriggerFired event (without auto-firing the triggered skill).
        let mut t = ObeliskTestApp::new(7);
        let mut block = StatBlock::with_id("marked");
        block.max_life.base = 100.0;
        block.current_life = 100.0;
        let e = t
            .app
            .world_mut()
            .spawn((
                Combatant,
                Attributes(block),
                Faction::Enemy,
                ObeliskId("marked".into()),
                Transform::default(),
            ))
            .id();
        t.app.update();

        // Record TriggerFired + EffectExpired for the on_expire_proc effect.
        #[derive(Resource, Default)]
        struct Fired {
            triggers: Vec<TriggerFired>,
            expired: Vec<EffectExpired>,
        }
        t.app.init_resource::<Fired>();
        t.app
            .add_observer(|ev: On<TriggerFired>, mut f: ResMut<Fired>| {
                f.triggers.push(ev.event().clone())
            });
        t.app
            .add_observer(|ev: On<EffectExpired>, mut f: ResMut<Fired>| {
                f.expired.push(ev.event().clone())
            });

        // Apply the short finite-duration OnExpire-trigger effect via the public verb.
        t.app
            .world_mut()
            .commands()
            .entity(e)
            .apply_obelisk_effect("on_expire_proc");
        t.app.update();
        assert!(
            t.app
                .world()
                .entity(e)
                .get::<Attributes>()
                .unwrap()
                .0
                .effects
                .iter()
                .any(|ef| ef.id == "on_expire_proc"),
            "on_expire_proc effect should be applied"
        );

        // Advance well past its 0.1s duration so it EXPIRES.
        t.advance_ticks(30);

        let f = t.app.world().resource::<Fired>();
        assert!(
            f.expired.iter().any(|x| x.effect_id == "on_expire_proc"),
            "the effect must emit EffectExpired on expiry"
        );
        let trig = f
            .triggers
            .iter()
            .find(|x| x.effect_id == "on_expire_proc")
            .expect("OnExpire trigger must be surfaced as TriggerFired, not silently dropped");
        assert_eq!(
            trig.skill_id, "static_discharge",
            "the surfaced trigger must carry the configured trigger_skill"
        );
        assert_eq!(
            (trig.source, trig.target),
            (e, e),
            "a self-owned effect's expiry trigger is sourced and targeted at the owner"
        );
    }
}
