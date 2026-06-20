use crate::core::components::{Attributes, Combatant, SkillSlots};
use crate::ids::ObeliskId;
use bevy::prelude::*;
use stat_core::StatBlock;

/// Verb-style EntityCommands extensions for spawning + granting.
pub trait ObeliskCommandsExt {
    /// Turn this entity into a combatant with a REAL StatBlock (sets Attributes + ObeliskId from `block.id`).
    fn make_combatant(&mut self, block: StatBlock) -> &mut Self;
    /// Add a skill id to this entity's SkillSlots.
    fn grant_skill(&mut self, skill_id: impl Into<String>) -> &mut Self;
    /// Grant barrier (energy shield) to this entity's StatBlock.
    fn grant_barrier(&mut self, amount: f64) -> &mut Self;
    /// Grant elude stacks to this entity's StatBlock.
    fn grant_elude(&mut self, stacks: u32) -> &mut Self;
    /// Apply an obelisk effect (by config id) to this entity, sourced from itself.
    /// Looks up the global EffectRegistry, builds the Effect, adds it; emits EffectApplied.
    fn apply_obelisk_effect(&mut self, effect_id: impl Into<String>) -> &mut Self;
    /// Rebuild this entity's StatBlock from the given stat sources (e.g. a skill tree's
    /// `to_stat_source()`, equipped items). Replaces prior source-derived stats.
    fn apply_stat_sources(
        &mut self,
        sources: Vec<Box<dyn stat_core::source::StatSource>>,
    ) -> &mut Self;
}

impl ObeliskCommandsExt for EntityCommands<'_> {
    fn make_combatant(&mut self, block: StatBlock) -> &mut Self {
        let id = block.id.clone();
        self.insert((Combatant, Attributes(block), ObeliskId(id)));
        self
    }
    fn grant_skill(&mut self, skill_id: impl Into<String>) -> &mut Self {
        let skill_id = skill_id.into();
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut slots) = entity.get_mut::<SkillSlots>() {
                slots.0.push(skill_id);
            } else {
                entity.insert(SkillSlots(vec![skill_id]));
            }
        });
        self
    }
    fn grant_barrier(&mut self, amount: f64) -> &mut Self {
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut attrs) = entity.get_mut::<Attributes>() {
                attrs.0.apply_barrier(amount);
            }
        });
        self
    }
    fn grant_elude(&mut self, stacks: u32) -> &mut Self {
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut attrs) = entity.get_mut::<Attributes>() {
                attrs.0.grant_elude_stacks(stacks);
            }
        });
        self
    }
    fn apply_stat_sources(
        &mut self,
        sources: Vec<Box<dyn stat_core::source::StatSource>>,
    ) -> &mut Self {
        self.queue(move |mut entity: EntityWorldMut| {
            if let Some(mut attrs) = entity.get_mut::<Attributes>() {
                attrs.0.rebuild_from_sources(&sources);
            }
        });
        self
    }
    fn apply_obelisk_effect(&mut self, effect_id: impl Into<String>) -> &mut Self {
        let effect_id = effect_id.into();
        self.queue(move |mut entity: EntityWorldMut| {
            if !stat_core::config::effect_registry_initialized() {
                return;
            }
            let source_id = entity
                .get::<Attributes>()
                .map(|a| a.0.id.clone())
                .unwrap_or_default();
            let Some(config) = stat_core::effect_registry().get(&effect_id) else {
                return;
            };
            let effect = config.to_effect(&source_id);
            let target = entity.id();
            let (total_duration, stacks, triggered) = {
                let Some(mut attrs) = entity.get_mut::<Attributes>() else {
                    return;
                };
                let triggered = attrs.0.add_effect(effect);
                let applied = attrs.0.effects.iter().find(|e| e.id == effect_id);
                (
                    applied.map(|e| e.total_duration).unwrap_or(0.0),
                    applied.map(|e| e.stacks).unwrap_or(1),
                    triggered,
                )
            };
            entity.world_scope(|world| {
                world.trigger(crate::events::EffectApplied {
                    target,
                    effect_id: effect_id.clone(),
                    total_duration,
                    stacks,
                });
                // Surface every effect-condition trigger (OnApply / OnMaxStacks) so it is never
                // silently dropped. Full auto-firing of the triggered skills from this command
                // closure is deferred to the game/engine; here source == target since this is a
                // self-applied effect.
                for te in &triggered {
                    world.trigger(crate::events::TriggerFired {
                        source: target,
                        target,
                        skill_id: te.skill_id.clone(),
                        effect_id: te.effect_id.clone(),
                    });
                }
            });
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;

    #[test]
    fn apply_obelisk_effect_adds_a_status() {
        let mut t = ObeliskTestApp::new(1); // harness loads the burn effect registry from fixtures
        let mut block = StatBlock::with_id("victim");
        block.max_life.base = 100.0;
        block.current_life = 100.0;
        let e = t
            .app
            .world_mut()
            .spawn((
                crate::prelude::Combatant,
                Attributes(block),
                crate::prelude::Faction::Enemy,
                ObeliskId("victim".into()),
                Transform::default(),
            ))
            .id();
        t.app.update();
        t.app
            .world_mut()
            .commands()
            .entity(e)
            .apply_obelisk_effect("burn");
        t.app.update();
        let attrs = t.app.world().entity(e).get::<Attributes>().unwrap();
        assert!(
            attrs.0.effects.iter().any(|ef| ef.id == "burn"),
            "burn effect should be applied"
        );
    }

    #[test]
    fn apply_stat_sources_rebuilds_from_a_source() {
        use stat_core::source::StatSource;
        use stat_core::stat_block::StatAccumulator;

        struct LifeSource;
        impl StatSource for LifeSource {
            fn id(&self) -> &str {
                "test_life"
            }
            fn apply(&self, stats: &mut StatAccumulator) {
                stats.life_flat += 50.0;
            }
        }

        let mut t = ObeliskTestApp::new(1);
        let e = t
            .app
            .world_mut()
            .spawn((
                Combatant,
                Attributes(StatBlock::with_id("hero")),
                crate::prelude::Faction::Player,
                ObeliskId("hero".into()),
                Transform::default(),
            ))
            .id();
        let base_life = t
            .app
            .world()
            .entity(e)
            .get::<Attributes>()
            .unwrap()
            .0
            .computed_max_life();

        t.app
            .world_mut()
            .commands()
            .entity(e)
            .apply_stat_sources(vec![Box::new(LifeSource)]);
        t.app.update();

        let new_life = t
            .app
            .world()
            .entity(e)
            .get::<Attributes>()
            .unwrap()
            .0
            .computed_max_life();
        assert!(
            new_life > base_life,
            "a flat-life source should raise computed_max_life ({base_life} -> {new_life})"
        );
    }

    /// Batch 3 (self-buffs + stat-block rebuild): applying the `empower` stat-modifier effect to an
    /// entity via the public `apply_obelisk_effect` verb runs obelisk's apply -> `add_effect` ->
    /// `rebuild()` round trip, so the entity's COMPUTED `global_fire_damage` increased-layer reflects
    /// the buff. This is the direct stat-rebuild readback the golden trace can't show (the firebolt
    /// Damage line proves the buff is *observable* end-to-end, but the computed stat VALUE itself is
    /// not in the event trace — only here can we read the StatBlock back).
    #[test]
    fn apply_obelisk_effect_modifies_computed_stat() {
        let mut t = ObeliskTestApp::new(1); // harness loads the empower effect from fixtures
        let mut block = StatBlock::with_id("caster");
        block.max_life.base = 100.0;
        block.current_life = 100.0;
        let e = t
            .app
            .world_mut()
            .spawn((
                crate::prelude::Combatant,
                Attributes(block),
                crate::prelude::Faction::Player,
                ObeliskId("caster".into()),
                Transform::default(),
            ))
            .id();
        t.app.update();

        // Baseline: no increased fire damage (global_fire_damage starts at base 0, increased 0).
        let before = t
            .app
            .world()
            .entity(e)
            .get::<Attributes>()
            .unwrap()
            .0
            .global_fire_damage
            .increased;
        assert!(
            before.abs() < 1e-9,
            "baseline increased fire damage should be 0 (got {before})"
        );

        // Apply the empower self-buff via the public verb (the apply -> rebuild round trip).
        t.app
            .world_mut()
            .commands()
            .entity(e)
            .apply_obelisk_effect("empower");
        t.app.update();

        let attrs = t.app.world().entity(e).get::<Attributes>().unwrap();
        assert!(
            attrs.0.effects.iter().any(|ef| ef.id == "empower"),
            "empower effect should be applied"
        );
        // The empower modifier (increased_fire_damage = 50) flows through rebuild into the
        // computed global_fire_damage increased layer (value/100 = 0.50).
        let after = attrs.0.global_fire_damage.increased;
        assert!(
            (after - 0.50).abs() < 1e-9,
            "empower should raise global_fire_damage.increased to 0.50 (got {after})"
        );
        // And the increased multiplier the damage pipeline reads is 1.50 (the observable boost).
        assert!(
            (attrs.0.global_fire_damage.total_increased_multiplier() - 1.50).abs() < 1e-9,
            "increased multiplier should be 1.50 (got {})",
            attrs.0.global_fire_damage.total_increased_multiplier()
        );
    }

    /// Batch 3: the buff's effect on the computed stat is REVERSIBLE. Apply a finite-duration
    /// empower variant (`empower_brief`, 0.1s) via the public verb, confirm the computed stat rose,
    /// then `tick_effects` past its expiry — which runs obelisk's expire -> `rebuild_from_effects`
    /// path — and assert the computed stat REVERTS to its base value (apply -> rebuild -> expire ->
    /// rebuild).
    #[test]
    fn self_buff_removal_reverts_stat() {
        let mut t = ObeliskTestApp::new(1);
        let mut block = StatBlock::with_id("caster");
        block.max_life.base = 100.0;
        block.current_life = 100.0;
        let e = t
            .app
            .world_mut()
            .spawn((
                crate::prelude::Combatant,
                Attributes(block),
                crate::prelude::Faction::Player,
                ObeliskId("caster".into()),
                Transform::default(),
            ))
            .id();
        t.app.update();

        t.app
            .world_mut()
            .commands()
            .entity(e)
            .apply_obelisk_effect("empower_brief");
        t.app.update();

        // Buff is active: increased fire damage rose to 0.50.
        let raised = t
            .app
            .world()
            .entity(e)
            .get::<Attributes>()
            .unwrap()
            .0
            .global_fire_damage
            .increased;
        assert!(
            (raised - 0.50).abs() < 1e-9,
            "empower_brief should raise increased fire damage to 0.50 (got {raised})"
        );

        // Tick 1 second of effects (> the 0.1s duration): the effect expires and obelisk rebuilds.
        {
            let mut entity_mut = t.app.world_mut().entity_mut(e);
            let mut attrs = entity_mut.get_mut::<Attributes>().unwrap();
            let (ticked, _result) = attrs.0.tick_effects(1.0);
            attrs.0 = ticked;
        }

        let attrs = t.app.world().entity(e).get::<Attributes>().unwrap();
        assert!(
            !attrs.0.effects.iter().any(|ef| ef.id == "empower_brief"),
            "empower_brief should have expired"
        );
        let reverted = attrs.0.global_fire_damage.increased;
        assert!(
            reverted.abs() < 1e-9,
            "increased fire damage should revert to base 0 after expiry (got {reverted})"
        );
    }

    #[test]
    fn make_combatant_and_grants_apply() {
        let mut t = ObeliskTestApp::new(1);
        let mut block = StatBlock::with_id("orc");
        block.max_life.base = 60.0;
        block.current_life = 60.0;
        block.set_max_barrier(100.0); // so apply_barrier has headroom
        let e = t.app.world_mut().spawn_empty().id();
        t.app
            .world_mut()
            .commands()
            .entity(e)
            .make_combatant(block)
            .grant_skill("firebolt")
            .grant_barrier(25.0);
        t.app.update();

        let attrs = t
            .app
            .world()
            .entity(e)
            .get::<Attributes>()
            .expect("Attributes inserted");
        assert_eq!(attrs.0.id, "orc");
        assert!(
            attrs.0.current_barrier >= 25.0,
            "barrier granted (got {})",
            attrs.0.current_barrier
        );
        let slots = t
            .app
            .world()
            .entity(e)
            .get::<SkillSlots>()
            .expect("SkillSlots");
        assert!(slots.0.contains(&"firebolt".to_string()));
    }
}
