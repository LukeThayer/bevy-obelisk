pub mod boxes;
pub mod detect; // Task 14
pub mod projectile;
pub mod shapes; // Task 15
pub use boxes::{Hitbox, Hurtbox};

use bevy::prelude::*;
pub struct ObeliskSpatialPlugin;
impl Plugin for ObeliskSpatialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(avian3d::prelude::PhysicsPlugins::new(FixedUpdate));
    }
}
