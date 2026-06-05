use bevy::prelude::*;
use crate::events::{DamageResolved, EntityDied, CastPhaseChanged};

pub struct ObeliskPresentPlugin;
impl Plugin for ObeliskPresentPlugin {
    fn build(&self, app: &mut App) {
        // Read-only: log gameplay events. Real VFX/audio observers attach here.
        app.add_observer(|e: On<DamageResolved>| {
            let d = e.event();
            info!("DamageResolved: {:.1} dmg to {:?}{}", d.total_damage, d.target, if d.is_killing_blow { " (KILL)" } else { "" });
        });
        app.add_observer(|e: On<CastPhaseChanged>| {
            let c = e.event();
            info!("Cast {} phase {:?} -> {:?} @ {:.2}s", c.skill_id, c.from, c.to, c.elapsed);
        });
        app.add_observer(|e: On<EntityDied>| info!("EntityDied: {:?}", e.event().target));
        // Avian debug gizmos for hit/hurtboxes (render-only; safe to add only in client builds).
        app.add_plugins(avian3d::prelude::PhysicsDebugPlugin);
    }
}
