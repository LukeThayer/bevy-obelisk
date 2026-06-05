use bevy::prelude::*;

/// Pending cast request, consumed by the Validate system.
#[derive(Component, Debug)]
pub struct PendingCast {
    pub skill_id: String,
    pub target: Entity,
}

/// EntityCommands verb: request a cast at a target entity.
pub trait CastSkillExt {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self;
}

impl CastSkillExt for EntityCommands<'_> {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self {
        let skill_id = skill_id.into();
        self.insert(PendingCast { skill_id, target });
        self
    }
}
