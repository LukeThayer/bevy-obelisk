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

/// Map an optional per-cast charge byte to a damage / projectile-speed multiplier.
///
/// `None` is an exact no-op (`1.0`, the default uncharged path — must NOT perturb any golden).
/// `Some(c)` maps linearly into `[0.5, 2.0]`: `Some(0)` → `0.5`, `Some(255)` → `2.0`.
/// The multiply this drives must NEVER draw RNG (determinism).
pub fn charge_mult(charge: Option<u8>) -> f32 {
    charge.map_or(1.0, |c| 0.5 + (c as f32 / 255.0) * 1.5)
}

/// Pending cast request, consumed by the Validate system.
#[derive(Component, Debug)]
pub struct PendingCast {
    pub skill_id: String,
    pub aim: CastAim,
    /// Optional per-cast charge. `None` = uncharged (1.0x, the default). Scales BOTH the
    /// projectile speed and the damage dealt via [`charge_mult`].
    pub charge: Option<u8>,
}

/// EntityCommands verbs to request a cast.
pub trait CastSkillExt {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self;
    fn cast_skill_at_point(&mut self, skill_id: impl Into<String>, point: Vec3) -> &mut Self;
    fn cast_skill_dir(&mut self, skill_id: impl Into<String>, dir: Dir3) -> &mut Self;
    /// As [`cast_skill_at`](CastSkillExt::cast_skill_at) but carrying a charge (see [`charge_mult`]).
    fn cast_skill_at_charged(
        &mut self,
        skill_id: impl Into<String>,
        target: Entity,
        charge: u8,
    ) -> &mut Self;
    /// As [`cast_skill_at_point`](CastSkillExt::cast_skill_at_point) but carrying a charge.
    fn cast_skill_at_point_charged(
        &mut self,
        skill_id: impl Into<String>,
        point: Vec3,
        charge: u8,
    ) -> &mut Self;
    /// As [`cast_skill_dir`](CastSkillExt::cast_skill_dir) but carrying a charge.
    fn cast_skill_dir_charged(
        &mut self,
        skill_id: impl Into<String>,
        dir: Dir3,
        charge: u8,
    ) -> &mut Self;
    /// Interrupt this entity's in-flight cast: cancels any `ActiveCast` and any still-pending
    /// `PendingCast` (so no further hit windows open / damage resolves for that cast). No-op if
    /// the entity is not casting. Mirrors the internal cancel done by `advance_casts` on
    /// completion (`remove::<ActiveCast>` / `remove::<PendingCast>`).
    fn interrupt_cast(&mut self) -> &mut Self;
}

impl CastSkillExt for EntityCommands<'_> {
    fn cast_skill_at(&mut self, skill_id: impl Into<String>, target: Entity) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Entity(target),
            charge: None,
        });
        self
    }
    fn cast_skill_at_point(&mut self, skill_id: impl Into<String>, point: Vec3) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Point(point),
            charge: None,
        });
        self
    }
    fn cast_skill_dir(&mut self, skill_id: impl Into<String>, dir: Dir3) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Direction(dir),
            charge: None,
        });
        self
    }
    fn cast_skill_at_charged(
        &mut self,
        skill_id: impl Into<String>,
        target: Entity,
        charge: u8,
    ) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Entity(target),
            charge: Some(charge),
        });
        self
    }
    fn cast_skill_at_point_charged(
        &mut self,
        skill_id: impl Into<String>,
        point: Vec3,
        charge: u8,
    ) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Point(point),
            charge: Some(charge),
        });
        self
    }
    fn cast_skill_dir_charged(
        &mut self,
        skill_id: impl Into<String>,
        dir: Dir3,
        charge: u8,
    ) -> &mut Self {
        self.insert(PendingCast {
            skill_id: skill_id.into(),
            aim: CastAim::Direction(dir),
            charge: Some(charge),
        });
        self
    }
    fn interrupt_cast(&mut self) -> &mut Self {
        self.remove::<(super::state::ActiveCast, PendingCast)>();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::charge_mult;

    #[test]
    fn charge_mult_maps_endpoints() {
        assert_eq!(charge_mult(None), 1.0, "None is an exact no-op");
        assert_eq!(charge_mult(Some(0)), 0.5, "Some(0) is the floor 0.5x");
        assert_eq!(charge_mult(Some(255)), 2.0, "Some(255) is the ceiling 2.0x");
    }
}
