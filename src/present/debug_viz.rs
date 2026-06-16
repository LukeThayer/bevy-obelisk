//! Gameplay debug-visualization (presentation-only, render-dependent).
//!
//! `ObeliskDebugVizPlugin` draws combat geometry as `Gizmos` (hurtbox/hitbox/cone/cast-ring,
//! gated behind the `debug-gizmos` feature so `present` stays usable without a render backend),
//! gives spawned `Projectile` hitboxes a small emissive mesh so the bolt is visible, and reacts
//! to gameplay events by flashing / fading the target's material.
//!
//! Everything here is PURELY cosmetic: systems mutate only `StandardMaterial` assets and
//! `Transform` scale, never gameplay state. The reactions guard every component access, so on a
//! headless app (no meshes/materials) they no-op gracefully and cannot perturb determinism.

use crate::events::{DamageResolved, EntityDied, HitConfirmed};
use crate::spatial::boxes::Hitbox;
use crate::spatial::projectile::Projectile;
use bevy::prelude::*;

#[cfg(feature = "debug-gizmos")]
use crate::assets::CollisionShape;
#[cfg(feature = "debug-gizmos")]
use crate::spatial::boxes::Hurtbox;
#[cfg(feature = "debug-gizmos")]
use crate::timeline::state::{ActiveCast, SkillPhase};

/// Presentation-only debug visualization: gizmos (under `debug-gizmos`), a projectile mesh, and
/// hit/death material reactions.
pub struct ObeliskDebugVizPlugin;

impl Plugin for ObeliskDebugVizPlugin {
    fn build(&self, app: &mut App) {
        // Give freshly-spawned projectile hitboxes a visible emissive mesh, then advance the
        // cosmetic reaction timers. Both run in Update (presentation schedule).
        app.add_systems(Update, (give_projectiles_a_mesh, tick_reactions));

        // Hit/death reactions are observers: they flash or fade the target's material.
        app.add_observer(on_hit_flash::<HitConfirmed>);
        app.add_observer(on_damage_flash);
        app.add_observer(on_death_fade);

        // The gizmo drawing is render-dependent and noisy; keep it behind `debug-gizmos`.
        #[cfg(feature = "debug-gizmos")]
        app.add_systems(Update, draw_combat_gizmos);
    }
}

// ----------------------------------------------------------------------------------------------
// Projectile mesh
// ----------------------------------------------------------------------------------------------

/// Marks a projectile hitbox that already received its debug mesh (so we insert it once).
#[derive(Component)]
struct ProjectileVizDone;

/// Insert a small emissive sphere `Mesh3d` + `MeshMaterial3d` on each newly-spawned `Projectile`
/// hitbox so the bolt is visible. No-ops on headless apps that lack the mesh/material asset
/// resources (they aren't present without a render backend, so the system simply doesn't run).
#[allow(clippy::type_complexity)]
fn give_projectiles_a_mesh(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<Entity, (With<Projectile>, With<Hitbox>, Without<ProjectileVizDone>)>,
) {
    for e in &q {
        let mesh = meshes.add(Sphere::new(0.2));
        let mat = mats.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.6, 0.1),
            emissive: LinearRgba::new(3.0, 1.5, 0.2, 1.0),
            ..default()
        });
        commands
            .entity(e)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat), ProjectileVizDone));
    }
}

// ----------------------------------------------------------------------------------------------
// Hit / death material reactions (purely cosmetic; guarded so headless is a no-op)
// ----------------------------------------------------------------------------------------------

/// Short-lived emissive flash applied to a hit target's material.
#[derive(Component)]
struct FlashTimer {
    remaining: f32,
    total: f32,
}

impl Default for FlashTimer {
    fn default() -> Self {
        Self {
            remaining: 0.18,
            total: 0.18,
        }
    }
}

/// Greys-out + shrinks a dying entity over time.
#[derive(Component)]
struct DeathFade {
    remaining: f32,
    total: f32,
}

impl Default for DeathFade {
    fn default() -> Self {
        Self {
            remaining: 0.5,
            total: 0.5,
        }
    }
}

/// Generic flash trigger usable for any event that carries a `target` (e.g. `HitConfirmed`).
fn on_hit_flash<E: HitTarget>(ev: On<E>, mut commands: Commands) {
    if let Ok(mut ec) = commands.get_entity(ev.event().target()) {
        ec.try_insert(FlashTimer::default());
    }
}

/// `DamageResolved` flashes the target (separate handler so it can read damage-specific fields if
/// we want to scale the flash later; for now it just flashes).
fn on_damage_flash(ev: On<DamageResolved>, mut commands: Commands) {
    if let Ok(mut ec) = commands.get_entity(ev.event().target) {
        ec.try_insert(FlashTimer::default());
    }
}

/// `EntityDied` greys + shrinks the target.
fn on_death_fade(ev: On<EntityDied>, mut commands: Commands) {
    if let Ok(mut ec) = commands.get_entity(ev.event().target) {
        ec.try_insert(DeathFade::default());
    }
}

/// Advance the cosmetic reaction timers and mutate the target material/transform. Guards every
/// access: if a target has no `MeshMaterial3d` / its material asset is missing (headless), it
/// no-ops. Never touches gameplay state.
fn tick_reactions(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(
        Entity,
        &mut FlashTimer,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
    mut fades: Query<(
        Entity,
        &mut DeathFade,
        Option<&MeshMaterial3d<StandardMaterial>>,
        &mut Transform,
    )>,
) {
    let dt = time.delta_secs();

    for (e, mut flash, mat_handle) in &mut flashes {
        flash.remaining -= dt;
        // 1.0 at flash start -> 0.0 when it ends.
        let t = (flash.remaining / flash.total).clamp(0.0, 1.0);
        if let Some(mh) = mat_handle {
            if let Some(mat) = mats.get_mut(&mh.0) {
                // White-hot emissive that fades back to nothing.
                mat.emissive = LinearRgba::new(2.0 * t, 2.0 * t, 2.0 * t, 1.0);
            }
        }
        if flash.remaining <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.remove::<FlashTimer>();
            }
        }
    }

    for (_e, mut fade, mat_handle, mut tf) in &mut fades {
        fade.remaining -= dt;
        // 1.0 at death -> 0.0 at end of fade.
        let t = (fade.remaining / fade.total).clamp(0.0, 1.0);
        if let Some(mh) = mat_handle {
            if let Some(mat) = mats.get_mut(&mh.0) {
                // Fade to a dim grey corpse.
                let grey = 0.15;
                let base = LinearRgba::from(mat.base_color);
                mat.base_color = Color::LinearRgba(LinearRgba::new(
                    base.red * t + grey * (1.0 - t),
                    base.green * t + grey * (1.0 - t),
                    base.blue * t + grey * (1.0 - t),
                    base.alpha,
                ));
                mat.emissive = LinearRgba::BLACK;
            }
        }
        // Shrink toward a small remnant.
        let scale = 0.3 + 0.7 * t;
        tf.scale = Vec3::splat(scale);
        // Keep `DeathFade` once expired (it leaves the corpse small & grey); no gameplay effect.
    }
}

/// Helper trait so a single observer can flash any event that exposes a hit `target`.
trait HitTarget: Event {
    fn target(&self) -> Entity;
}
impl HitTarget for HitConfirmed {
    fn target(&self) -> Entity {
        self.target
    }
}

// ----------------------------------------------------------------------------------------------
// Gizmos (debug-gizmos only)
// ----------------------------------------------------------------------------------------------

#[cfg(feature = "debug-gizmos")]
mod gizmo_colors {
    use bevy::color::Color;
    pub const HURTBOX: Color = Color::srgb(0.2, 0.9, 0.3);
    pub const HITBOX: Color = Color::srgb(0.95, 0.25, 0.2);
    pub const CONE: Color = Color::srgb(0.95, 0.55, 0.1);
    pub const WINDUP: Color = Color::srgb(0.95, 0.85, 0.2);
    pub const ACTIVE: Color = Color::srgb(0.95, 0.25, 0.2);
    pub const RECOVERY: Color = Color::srgb(0.3, 0.5, 0.95);
}

/// Draw every hurtbox sphere, active hitbox shape, and a cast-phase ring per `ActiveCast`.
#[cfg(feature = "debug-gizmos")]
fn draw_combat_gizmos(
    mut gizmos: Gizmos,
    hurtboxes: Query<(&Transform, Option<&avian3d::prelude::ColliderAabb>), With<Hurtbox>>,
    hitboxes: Query<(&Transform, &Hitbox)>,
    casts: Query<(&Transform, &ActiveCast)>,
) {
    // Hurtboxes: a green sphere at the owner. Derive the radius from Avian's auto-computed
    // world-space `ColliderAabb` (half its largest extent); fall back to a representative radius
    // if the AABB isn't available yet.
    for (tf, aabb) in &hurtboxes {
        let radius = aabb
            .map(|a| (a.size().max_element() * 0.5).max(0.05))
            .unwrap_or(0.6);
        let center = aabb.map(|a| a.center()).unwrap_or(tf.translation);
        gizmos.sphere(center, radius, gizmo_colors::HURTBOX);
    }

    // Active hitboxes: draw the authored shape at its world transform.
    for (tf, hb) in &hitboxes {
        let pos = tf.translation;
        match hb.shape {
            CollisionShape::Sphere { radius } => {
                gizmos.sphere(pos, radius, gizmo_colors::HITBOX);
            }
            CollisionShape::Capsule { radius, height } => {
                // Approximate a capsule with two end spheres + a connecting line along +Y.
                let half = (height * 0.5).max(0.0);
                let up = tf.rotation * Vec3::Y * half;
                gizmos.sphere(pos + up, radius, gizmo_colors::HITBOX);
                gizmos.sphere(pos - up, radius, gizmo_colors::HITBOX);
                gizmos.line(pos - up, pos + up, gizmo_colors::HITBOX);
            }
            CollisionShape::Cone { angle, range } => {
                // `angle` is the FULL cone angle in DEGREES (matching the sim's source of
                // truth in spatial/detect.rs); draw_cone wants the half-angle in radians.
                let half_angle_rad = angle.to_radians() * 0.5;
                draw_cone(&mut gizmos, pos, hb.aim, half_angle_rad, range, gizmo_colors::CONE);
            }
        }
    }

    // Cast-phase ring: a flat ring (sphere) at the caster, colored by phase.
    for (tf, cast) in &casts {
        let color = match cast.phase {
            SkillPhase::Windup => gizmo_colors::WINDUP,
            SkillPhase::Active => gizmo_colors::ACTIVE,
            SkillPhase::Recovery => gizmo_colors::RECOVERY,
            SkillPhase::Done => continue,
        };
        gizmos.sphere(tf.translation, 0.9, color);
    }
}

/// Draw a cone/sector as a fan of lines + a bounding arc along `aim` (normalized facing).
#[cfg(feature = "debug-gizmos")]
fn draw_cone(
    gizmos: &mut Gizmos,
    apex: Vec3,
    aim: Vec3,
    half_angle_rad: f32,
    range: f32,
    color: Color,
) {
    let dir = aim.normalize_or_zero();
    if dir == Vec3::ZERO || range <= 0.0 {
        return;
    }
    // A rotation that maps +X (the arc's start vertex) onto the cone axis.
    let rotation = Quat::from_rotation_arc(Vec3::X, dir);
    // Edge lines of the sector in the horizontal plane (rotate `dir` by ±half_angle about +Y).
    let left = Quat::from_axis_angle(Vec3::Y, half_angle_rad) * dir;
    let right = Quat::from_axis_angle(Vec3::Y, -half_angle_rad) * dir;
    gizmos.line(apex, apex + left * range, color);
    gizmos.line(apex, apex + right * range, color);
    gizmos.line(apex, apex + dir * range, color);
    // Bounding arc spanning the full sector angle, centered on `dir`.
    let iso = Isometry3d::new(
        apex,
        rotation * Quat::from_axis_angle(Vec3::Z, -half_angle_rad),
    );
    gizmos.arc_3d(half_angle_rad * 2.0, range, iso, color);
}
