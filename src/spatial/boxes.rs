use crate::assets::{HitFilter, HitMode};
use avian3d::prelude::*;
use bevy::prelude::*;

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
    pub remaining: f32,
    pub already_hit: Vec<Entity>,
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
