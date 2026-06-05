use crate::assets::{CollisionShape, HitFilter, HitMode};
use avian3d::prelude::*;
use bevy::prelude::*;
use std::collections::HashMap;

/// Defensive volume on a combatant. Spawned as a static Avian collider so SpatialQuery can find it.
#[derive(Component, Debug)]
pub struct Hurtbox {
    pub owner: Entity,
}

/// Offensive volume spawned during an active collision window.
#[derive(Component, Debug)]
pub struct Hitbox {
    pub caster: Entity,
    pub skill_id: String,
    pub window_id: String,
    pub filter: HitFilter,
    pub mode: HitMode,
    /// The authored shape (drives the SpatialQuery + cone test).
    pub shape: CollisionShape,
    /// Normalized facing direction (cone axis / projectile heading).
    pub aim: Vec3,
    /// Seconds this hitbox has existed (for re-hit interval timing).
    pub age: f32,
    /// If set, a target may be hit again this many seconds after its last hit.
    pub rehit_interval: Option<f32>,
    /// Seconds remaining before the window expires.
    pub remaining: f32,
    /// target -> `age` at which it was last hit.
    pub hit_log: HashMap<Entity, f32>,
    /// FirstOnly: set true after the single hit so the box stops hitting.
    pub done: bool,
}

impl Hitbox {
    /// Whether `target` may be hit right now given mode + re-hit interval.
    pub fn can_hit(&self, target: Entity) -> bool {
        if self.done {
            return false;
        }
        match self.mode {
            HitMode::EveryTick => true,
            HitMode::FirstOnly | HitMode::OncePerTarget => match self.hit_log.get(&target) {
                None => true,
                Some(&last) => self
                    .rehit_interval
                    .map_or(false, |i| self.age - last >= i),
            },
        }
    }

    /// Record a hit on `target` and apply FirstOnly stop semantics.
    pub fn register_hit(&mut self, target: Entity) {
        self.hit_log.insert(target, self.age);
        if matches!(self.mode, HitMode::FirstOnly) {
            self.done = true;
        }
    }
}

/// Bundle of components that make `owner` a SpatialQuery-discoverable hurtbox at `pos`.
/// Uses `RigidBody::Static` because (per the probe) a static collider is included in
/// `SpatialQuery` shape intersections.
pub fn insert_hurtbox(commands: &mut Commands, owner: Entity, radius: f32, pos: Vec3) {
    commands.entity(owner).insert((
        Hurtbox { owner },
        RigidBody::Static,
        Collider::sphere(radius),
        Transform::from_translation(pos),
    ));
}
