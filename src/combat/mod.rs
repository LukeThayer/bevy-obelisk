pub mod resolve;
pub mod system;
pub use resolve::{resolve_one_hit, resolve_one_hit_charged, HitOutcome};

use bevy::prelude::*;
pub struct ObeliskCombatPlugin;
impl Plugin for ObeliskCombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(system::on_hit_confirmed);
    }
}
