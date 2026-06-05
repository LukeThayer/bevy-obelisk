use bevy::prelude::*;

/// How a cast is aimed. Resolved to a facing direction at validation time.
#[derive(Clone, Copy, Debug)]
pub enum CastAim {
    /// Aim at an entity (direction = toward its transform; range = distance to it).
    Entity(Entity),
    /// Aim at a ground point.
    Point(Vec3),
    /// Aim along an explicit direction (no target entity / range gate).
    Direction(Dir3),
}

/// Pending cast request, consumed by the Validate system.
#[derive(Component, Debug)]
pub struct PendingCast {
    pub skill_id: String,
    pub aim: CastAim,
}

/// EntityCommands verbs to request a cast.
pub trait CastSkillExt {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self;
    fn cast_skill_at_point(&mut self, skill_id: impl Into<String>, point: Vec3) -> &mut Self;
    fn cast_skill_dir(&mut self, skill_id: impl Into<String>, dir: Dir3) -> &mut Self;
}

impl CastSkillExt for EntityCommands<'_> {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Entity(target),
        });
        self
    }
    fn cast_skill_at_point(&mut self, skill_id: impl Into<String>, point: Vec3) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Point(point),
        });
        self
    }
    fn cast_skill_dir(&mut self, skill_id: impl Into<String>, dir: Dir3) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Direction(dir),
        });
        self
    }
}
