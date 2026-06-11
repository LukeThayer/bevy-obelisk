use bevy::prelude::*;
use crate::core::components::{Attributes, Combatant, SkillSlots};
use crate::ids::ObeliskId;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;

    #[test]
    fn make_combatant_and_grants_apply() {
        let mut t = ObeliskTestApp::new(1);
        let mut block = StatBlock::with_id("orc");
        block.max_life.base = 60.0;
        block.current_life = 60.0;
        block.set_max_barrier(100.0); // so apply_barrier has headroom
        let e = t.app.world_mut().spawn_empty().id();
        t.app.world_mut().commands().entity(e)
            .make_combatant(block)
            .grant_skill("firebolt")
            .grant_barrier(25.0);
        t.app.update();

        let attrs = t.app.world().entity(e).get::<Attributes>().expect("Attributes inserted");
        assert_eq!(attrs.0.id, "orc");
        assert!(attrs.0.current_barrier >= 25.0, "barrier granted (got {})", attrs.0.current_barrier);
        let slots = t.app.world().entity(e).get::<SkillSlots>().expect("SkillSlots");
        assert!(slots.0.contains(&"firebolt".to_string()));
    }
}
