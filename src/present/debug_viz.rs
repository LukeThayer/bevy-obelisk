//! Gameplay debug-visualization (presentation-only, render-dependent).
//!
//! `ObeliskDebugVizPlugin` draws combat geometry as `Gizmos` (hurtbox/hitbox/cone/cast-ring,
//! gated behind the `debug-gizmos` feature so `present` stays usable without a render backend),
//! gives spawned `Projectile` hitboxes a small emissive mesh so the bolt is visible, reacts
//! to gameplay events by flashing / fading the target's material, and renders a `bevy_ui` HUD:
//! a fixed combatant **roster panel** (each `Attributes` entity's life/mana + per-skill cooldowns),
//! a **scrolling event-log panel** (a bounded ring of the last gameplay events, fed by observers),
//! and **floating damage numbers** that pop above a hit target (a `DamageResolved` observer spawns
//! a short-lived UI `Text` whose screen position tracks the target via `Camera::world_to_viewport`).
//!
//! Everything here is PURELY cosmetic: systems mutate only `StandardMaterial` assets, `Transform`
//! scale, and their OWN UI entities/resource — never gameplay state. The reactions guard every
//! component access, so on a headless app (no meshes/materials/camera) they no-op gracefully and
//! cannot perturb determinism. The headless test stack never adds `present`, so the whole plugin
//! (and this HUD) is inert there.
//!
//! HUD design note: the plan permitted a world-anchored-per-entity bars option OR a fixed
//! roster/log panel. We use the robust **fixed roster + fixed event-log panels** plus
//! **floating damage at a camera-projected screen position** — robust, headless-safe, and free of
//! per-entity follow-node bookkeeping.

use crate::core::components::{Attributes, Faction, SkillSlots};
use crate::core::cooldown::Cooldowns;
use crate::events::{
    CastBegan, CastRejected, CooldownReady, CooldownStarted, DamageResolved, EffectApplied,
    EffectExpired, EntityDied, HitConfirmed, LootDropped, TriggerFired,
};
use crate::ids::ObeliskId;
use crate::spatial::boxes::Hitbox;
use crate::spatial::projectile::Projectile;
use bevy::prelude::*;

#[cfg(feature = "debug-gizmos")]
use crate::assets::CollisionShape;
#[cfg(feature = "debug-gizmos")]
use crate::spatial::boxes::Hurtbox;
#[cfg(feature = "debug-gizmos")]
use crate::timeline::state::{ActiveCast, SkillPhase};

/// Presentation-only debug visualization: gizmos (under `debug-gizmos`), a projectile mesh,
/// hit/death material reactions, and a `bevy_ui` HUD (roster + event log + floating damage).
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

        // ---- HUD (bevy_ui): roster + event-log panels + floating damage numbers ----
        app.init_resource::<EventLog>();
        app.add_systems(
            Update,
            (
                spawn_hud_once,
                update_roster_panel,
                update_event_log_panel,
                tick_floating_damage,
            ),
        );
        // Floating damage + the event-log ring are fed by gameplay-event observers (cosmetic only).
        app.add_observer(on_damage_floating_number);
        app.add_observer(log_event::<CastBegan>);
        app.add_observer(log_event::<CastRejected>);
        app.add_observer(log_event::<HitConfirmed>);
        app.add_observer(log_event::<DamageResolved>);
        app.add_observer(log_event::<EffectApplied>);
        app.add_observer(log_event::<EffectExpired>);
        app.add_observer(log_event::<EntityDied>);
        app.add_observer(log_event::<TriggerFired>);
        app.add_observer(log_event::<CooldownStarted>);
        app.add_observer(log_event::<CooldownReady>);
        app.add_observer(log_event::<LootDropped>);

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
                draw_cone(
                    &mut gizmos,
                    pos,
                    hb.aim,
                    half_angle_rad,
                    range,
                    gizmo_colors::CONE,
                );
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

// ----------------------------------------------------------------------------------------------
// HUD: fixed roster panel + scrolling event-log panel + floating damage numbers (bevy_ui).
//
// Robust, headless-safe presentation: spawn nothing until first run, query only entities we own
// or read-only gameplay components, and guard every camera/transform lookup so a partially
// configured app (no camera, no UiPlugin) simply renders nothing. Never touches gameplay state.
// ----------------------------------------------------------------------------------------------

/// Max number of event lines kept in the scrolling log (bounded ring buffer).
const EVENT_LOG_CAP: usize = 12;

/// Seconds a floating damage number lives before despawning.
const FLOATING_DAMAGE_LIFETIME: f32 = 1.0;

/// World-space pixels-per-second the floating number drifts upward.
const FLOATING_DAMAGE_RISE: f32 = 60.0;

/// Bounded ring of recent gameplay-event lines, newest last. Cosmetic only.
#[derive(Resource, Default)]
struct EventLog {
    lines: std::collections::VecDeque<String>,
}

impl EventLog {
    fn push(&mut self, line: String) {
        if self.lines.len() >= EVENT_LOG_CAP {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}

/// Marker for the HUD root node (spawned once).
#[derive(Component)]
struct HudRoot;

/// Marker for the roster panel `Text` (rebuilt each frame from live combatants).
#[derive(Component)]
struct RosterText;

/// Marker for the event-log panel `Text` (rebuilt each frame from `EventLog`).
#[derive(Component)]
struct EventLogText;

/// A short-lived floating damage number. Tracks its target so it can follow on screen, and caches
/// the last-known world position so it keeps rising even after the target despawns (e.g. on death).
#[derive(Component)]
struct FloatingDamage {
    target: Entity,
    last_world_pos: Vec3,
    age: f32,
}

/// Spawn the HUD root + roster + event-log panels exactly once (when none exists yet). Spawning
/// `bevy_ui` nodes is safe even without a camera/UiPlugin (they're plain components); they only
/// render once a UI camera is present, so this is inert on a partially-configured app.
fn spawn_hud_once(mut commands: Commands, existing: Query<(), With<HudRoot>>) {
    if !existing.is_empty() {
        return;
    }
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // The root itself is transparent; only the child panels have backgrounds. It is an
            // informational overlay; it neither holds a background nor consumes layout space.
        ))
        .with_children(|root| {
            // Roster panel: top-left.
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(8.0),
                    top: Val::Px(8.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(6.0)),
                    min_width: Val::Px(220.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    RosterText,
                    Text::new("roster"),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.92, 1.0)),
                ));
            });

            // Event-log panel: bottom-left.
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(8.0),
                    bottom: Val::Px(8.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(6.0)),
                    min_width: Val::Px(360.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    EventLogText,
                    Text::new("events"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.8, 0.85, 0.8)),
                ));
            });
        });
}

/// Rebuild the roster text from every live combatant: `id  life/maxlife  mana/maxmana  [cd: …]`.
/// Iterates in a stable (id-sorted) order so the panel doesn't flicker; purely cosmetic.
#[allow(clippy::type_complexity)]
fn update_roster_panel(
    combatants: Query<(
        Entity,
        &Attributes,
        &ObeliskId,
        &Faction,
        Option<&SkillSlots>,
    )>,
    cooldowns: Option<Res<Cooldowns>>,
    mut roster: Query<&mut Text, With<RosterText>>,
) {
    let Ok(mut text) = roster.single_mut() else {
        return;
    };

    let mut rows: Vec<(String, String)> = Vec::new();
    for (entity, attrs, id, faction, slots) in &combatants {
        let sb = &attrs.0;
        let life = sb.current_life;
        let max_life = sb.computed_max_life();
        let mana = sb.current_mana;
        let max_mana = sb.computed_max_mana();
        let faction_tag = match faction {
            Faction::Player => "P",
            Faction::Enemy => "E",
            Faction::Neutral => "N",
        };

        let mut line = format!(
            "[{faction_tag}] {:<10} life {life:>6.1}/{max_life:<6.1} mana {mana:>5.1}/{max_mana:<5.1}",
            id.0,
        );

        // Per-skill cooldowns: query this entity's remaining cooldown per granted slot.
        if let (Some(cd), Some(slots)) = (cooldowns.as_ref(), slots) {
            let active: Vec<String> = slots
                .0
                .iter()
                .filter_map(|skill| {
                    let r = cd.remaining(entity, skill);
                    (r > 0.0).then(|| format!("{skill} {r:.1}s"))
                })
                .collect();
            if !active.is_empty() {
                line.push_str("  cd:");
                for a in active {
                    line.push(' ');
                    line.push_str(&a);
                }
            }
        }
        rows.push((id.0.clone(), line));
    }

    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let body = if rows.is_empty() {
        "roster: (no combatants)".to_string()
    } else {
        rows.into_iter()
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
            .join("\n")
    };
    if text.as_str() != body {
        text.0 = body;
    }
}

/// Rebuild the event-log panel text from the bounded ring (newest at the bottom).
fn update_event_log_panel(log: Res<EventLog>, mut q: Query<&mut Text, With<EventLogText>>) {
    if !log.is_changed() {
        return;
    }
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let body = if log.lines.is_empty() {
        "events: (none yet)".to_string()
    } else {
        log.lines.iter().cloned().collect::<Vec<_>>().join("\n")
    };
    text.0 = body;
}

/// Trait + impls so a single generic observer can summarize any gameplay event into one log line.
trait LogLine: Event {
    fn log_line(&self) -> String;
}
impl LogLine for CastBegan {
    fn log_line(&self) -> String {
        format!("cast {} ({:.2}s)", self.skill_id, self.total_duration)
    }
}
impl LogLine for CastRejected {
    fn log_line(&self) -> String {
        format!("reject {} {:?}", self.skill_id, self.reason)
    }
}
impl LogLine for HitConfirmed {
    fn log_line(&self) -> String {
        format!("hit {}", self.skill_id)
    }
}
impl LogLine for DamageResolved {
    fn log_line(&self) -> String {
        format!(
            "dmg {:.1}{} (life {:.1})",
            self.total_damage,
            if self.is_killing_blow { " KILL" } else { "" },
            self.life_after,
        )
    }
}
impl LogLine for EffectApplied {
    fn log_line(&self) -> String {
        format!("effect+ {} x{}", self.effect_id, self.stacks)
    }
}
impl LogLine for EffectExpired {
    fn log_line(&self) -> String {
        format!("effect- {}", self.effect_id)
    }
}
impl LogLine for EntityDied {
    fn log_line(&self) -> String {
        "died".to_string()
    }
}
impl LogLine for TriggerFired {
    fn log_line(&self) -> String {
        format!("trigger {} -> {}", self.effect_id, self.skill_id)
    }
}
impl LogLine for CooldownStarted {
    fn log_line(&self) -> String {
        format!("cd start {} ({:.1}s)", self.skill_id, self.duration)
    }
}
impl LogLine for CooldownReady {
    fn log_line(&self) -> String {
        format!("cd ready {}", self.skill_id)
    }
}
impl LogLine for LootDropped {
    fn log_line(&self) -> String {
        format!("loot x{}", self.drops.len())
    }
}

/// Generic observer: append a one-line summary of `E` to the event-log ring.
fn log_event<E: LogLine>(ev: On<E>, mut log: ResMut<EventLog>) {
    log.push(ev.event().log_line());
}

/// `DamageResolved` spawns a short-lived floating damage number anchored to the target. The
/// number's on-screen position is updated each frame by `tick_floating_damage`. Spawning a UI
/// `Text` node is safe headless; it only renders with a UI camera present.
fn on_damage_floating_number(
    ev: On<DamageResolved>,
    mut commands: Commands,
    transforms: Query<&GlobalTransform>,
) {
    let d = ev.event();
    // Cache the target's current world position so the number keeps drifting even if the target
    // despawns (death). Fall back to origin if the target has no transform yet.
    let last_world_pos = transforms
        .get(d.target)
        .map(|gt| gt.translation())
        .unwrap_or(Vec3::ZERO);

    let color = if d.is_killing_blow {
        Color::srgb(1.0, 0.85, 0.2)
    } else {
        Color::srgb(1.0, 0.4, 0.3)
    };

    commands.spawn((
        FloatingDamage {
            target: d.target,
            last_world_pos,
            age: 0.0,
        },
        Text::new(format!("{:.0}", d.total_damage)),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(color),
        Node {
            position_type: PositionType::Absolute,
            // Off-screen until the first `tick_floating_damage` places it; avoids a 1-frame flash
            // at (0,0).
            left: Val::Px(-1000.0),
            top: Val::Px(-1000.0),
            ..default()
        },
    ));
}

/// Advance each floating number: age it, project its (drifting) world anchor to a screen position
/// via the active camera, and despawn it once expired. Guards the camera lookup so a headless /
/// camera-less app simply ages + despawns the numbers without positioning them.
fn tick_floating_damage(
    time: Res<Time>,
    mut commands: Commands,
    camera: Query<(&Camera, &GlobalTransform)>,
    targets: Query<&GlobalTransform>,
    mut numbers: Query<(Entity, &mut FloatingDamage, &mut Node)>,
) {
    let dt = time.delta_secs();
    // Use the first camera if any (presentation app has exactly one main camera).
    let cam = camera.iter().next();

    for (e, mut fd, mut node) in &mut numbers {
        fd.age += dt;
        if fd.age >= FLOATING_DAMAGE_LIFETIME {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.despawn();
            }
            continue;
        }

        // Refresh the cached world anchor if the target is still alive.
        if let Ok(gt) = targets.get(fd.target) {
            fd.last_world_pos = gt.translation();
        }

        // Project to screen + apply an upward drift that grows with age.
        if let Some((camera, cam_tf)) = cam {
            let anchor = fd.last_world_pos + Vec3::Y * 1.6;
            if let Ok(screen) = camera.world_to_viewport(cam_tf, anchor) {
                node.left = Val::Px(screen.x - 12.0);
                node.top = Val::Px(screen.y - FLOATING_DAMAGE_RISE * fd.age);
            }
        }
    }
}
