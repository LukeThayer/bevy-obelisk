use bevy::prelude::*;

/// Straight-line projectile motion for a moving hitbox. World-space speed (NOT speed-scaled).
#[derive(Component, Debug)]
pub struct Projectile { pub velocity: Vec3 }

/// Integrate projectile transforms each fixed step. Runs before ResolveHits so detection
/// uses the updated position.
pub fn move_projectiles(time: Res<Time<Fixed>>, mut q: Query<(&Projectile, &mut Transform)>) {
    let dt = time.delta_secs();
    for (proj, mut tf) in &mut q {
        tf.translation += proj.velocity * dt;
    }
}
