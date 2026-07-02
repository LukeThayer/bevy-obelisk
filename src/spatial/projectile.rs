use bevy::prelude::*;

/// Projectile motion for a moving hitbox. World-space velocity (NOT speed-scaled) plus a
/// downward gravity acceleration: `gravity == 0.0` is straight-line flight (`VolumeMotion::
/// Linear`), `gravity > 0.0` is a ballistic arc (`VolumeMotion::Ballistic`) — the initial
/// velocity comes from the caster's (free-look, possibly pitched) aim direction, so aiming up
/// lobs the projectile.
#[derive(Component, Debug)]
pub struct Projectile {
    pub velocity: Vec3,
    /// Downward acceleration in world units/s² applied each fixed step (0.0 = no arc).
    pub gravity: f32,
}

/// Integrate projectile transforms each fixed step. Runs before ResolveHits so detection
/// uses the updated position. Semi-implicit Euler (velocity first, then position): fully
/// deterministic under the fixed timestep, and bit-identical to the old straight-line
/// integration when `gravity == 0.0`.
pub fn move_projectiles(time: Res<Time<Fixed>>, mut q: Query<(&mut Projectile, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut proj, mut tf) in &mut q {
        proj.velocity.y -= proj.gravity * dt;
        let velocity = proj.velocity;
        tf.translation += velocity * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    fn ballistic_app(velocity: Vec3, gravity: f32) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin)
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / 60.0),
            ))
            .add_systems(FixedUpdate, move_projectiles);
        let e = app
            .world_mut()
            .spawn((Projectile { velocity, gravity }, Transform::default()))
            .id();
        (app, e)
    }

    #[test]
    fn zero_gravity_flies_straight() {
        let (mut app, e) = ballistic_app(Vec3::new(20.0, 0.0, 0.0), 0.0);
        step(&mut app, 61); // TimePlugin consumes the first update priming the clock
        let tf = app.world().get::<Transform>(e).unwrap();
        assert!((tf.translation.x - 20.0).abs() < 1e-3, "{}", tf.translation);
        assert_eq!(tf.translation.y, 0.0, "no drop without gravity");
    }

    #[test]
    fn gravity_arcs_the_projectile_down() {
        let (mut app, e) = ballistic_app(Vec3::new(20.0, 0.0, 0.0), 10.0);
        step(&mut app, 61);
        let tf = app.world().get::<Transform>(e).unwrap();
        assert!((tf.translation.x - 20.0).abs() < 1e-3, "horizontal unchanged");
        // Semi-implicit Euler over n steps: drop = g*dt^2 * n(n+1)/2 ≈ 5.04 at n=60, g=10.
        let expected = -10.0 * (1.0 / 60.0f32).powi(2) * (60.0 * 61.0 / 2.0);
        assert!(
            (tf.translation.y - expected).abs() < 1e-3,
            "drop {} != expected {expected}",
            tf.translation.y
        );
    }

    #[test]
    fn pitched_aim_lobs_up_then_falls() {
        // 45° up at 10 u/s with g=10: apex around t=0.7 s, back near y=0 by t≈1.4 s.
        let comp = 10.0 * std::f32::consts::FRAC_1_SQRT_2;
        let v = Vec3::new(comp, comp, 0.0);
        let (mut app, e) = ballistic_app(v, 10.0);
        step(&mut app, 43); // ~0.7 s: near apex, still above ground
        let y_apex = app.world().get::<Transform>(e).unwrap().translation.y;
        assert!(y_apex > 2.0, "apex should be well above launch: {y_apex}");
        step(&mut app, 43); // ~1.4 s: back down
        let y_end = app.world().get::<Transform>(e).unwrap().translation.y;
        assert!(y_end < y_apex - 2.0, "must fall back down: {y_end} vs {y_apex}");
    }
}
