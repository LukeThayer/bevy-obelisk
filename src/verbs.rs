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
            let (total_duration, stacks, triggered_count) = {
                let Some(mut attrs) = entity.get_mut::<Attributes>() else {
                    return;
                };
                let triggered = attrs.0.add_effect(effect);
                let applied = attrs.0.effects.iter().find(|e| e.id == effect_id);
                (
                    applied.map(|e| e.total_duration).unwrap_or(0.0),
                    applied.map(|e| e.stacks).unwrap_or(1),
                    triggered.len(),
                )
            };
            // OnApply / OnMaxStacks trigger CASCADE (firing the triggered skills) is a later
            // batch; surface it so it isn't lost silently.
            if triggered_count > 0 {
                bevy::log::debug!(
                    "apply_obelisk_effect '{}': {} triggered effects (cascade deferred)",
                    effect_id,
                    triggered_count
                );
            }
            entity.world_scope(|world| {
                world.trigger(crate::events::EffectApplied {
                    target,
                    effect_id: effect_id.clone(),
                    total_duration,
                    stacks,
                });
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
