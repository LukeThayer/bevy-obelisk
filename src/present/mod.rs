use crate::events::{CastPhaseChanged, DamageResolved, EntityDied};
use bevy::prelude::*;

#[cfg(feature = "present")]
pub mod debug_viz;

pub struct ObeliskPresentPlugin;
impl Plugin for ObeliskPresentPlugin {
    fn build(&self, app: &mut App) {
        // Gameplay debug visualization (projectile mesh + hit/death reactions always; gizmo
        // drawing only under `debug-gizmos`). Presentation-only, never touches gameplay state.
        app.add_plugins(debug_viz::ObeliskDebugVizPlugin);

        // Read-only: log gameplay events. Real VFX/audio observers attach here.
        app.add_observer(|e: On<DamageResolved>| {
            let d = e.event();
            info!(
                "DamageResolved: {:.1} dmg to {:?}{}",
                d.total_damage,
                d.target,
                if d.is_killing_blow { " (KILL)" } else { "" }
            );
        });
        app.add_observer(|e: On<CastPhaseChanged>| {
            let c = e.event();
            info!(
                "Cast {} phase {:?} -> {:?} @ {:.2}s",
                c.skill_id, c.from, c.to, c.elapsed
            );
        });
        app.add_observer(|e: On<EntityDied>| info!("EntityDied: {:?}", e.event().target));
        // Avian debug gizmos for hit/hurtboxes. Render-dependent, so opt-in behind
        // `debug-gizmos` (off by default) — keeps `present` usable without a render backend.
        // Run the playground with `--features debug-gizmos` to see colliders.
        #[cfg(feature = "debug-gizmos")]
        app.add_plugins(avian3d::prelude::PhysicsDebugPlugin);
    }
}
