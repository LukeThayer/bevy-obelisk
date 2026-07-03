pub mod resolve;
pub mod system;
pub use resolve::{resolve_one_hit, resolve_one_hit_charged, HitOutcome};

use bevy::prelude::*;
pub struct ObeliskCombatPlugin;
impl Plugin for ObeliskCombatPlugin {
    fn build(&self, app: &mut App) {
        // Ticket 5 (phase-3 prerequisite): warn-throttle state shared by `on_hit_confirmed` and
        // its facade twin (`src/facade/combat.rs::resolve_skill_hit`) — see
        // `system::WarnedConditions`'s doc.
        app.init_resource::<system::WarnedConditions>();
        app.add_observer(system::on_hit_confirmed);
    }
}
