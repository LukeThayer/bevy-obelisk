use bevy::prelude::*;
use crate::assets::{HitFilter, HitMode};

/// Defensive volume on a combatant. Spawned as a static Avian collider so SpatialQuery can find it.
#[derive(Component, Debug)]
pub struct Hurtbox { pub owner: Entity }

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
