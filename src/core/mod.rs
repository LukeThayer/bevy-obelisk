pub mod components;
pub mod config;
pub mod cooldown;
pub mod tick;
pub use components::{Attributes, Combatant, Faction, SkillSlots};

use crate::core::config::{CombatRng, SkillRegistry};
use crate::ids::{sync_index_added, sync_index_removed, ObeliskEntityIndex};
use crate::ObeliskSet;
use bevy::prelude::*;

pub struct ObeliskCorePlugin;
impl Plugin for ObeliskCorePlugin {
    fn build(&self, app: &mut App) {
        // Insert sensible defaults so a consumer who forgets `add_obelisk_skills` /
        // `seed_combat_rng` gets graceful behavior (empty registry, seed-0 RNG) instead of a
        // panic on a missing `Res`. `init_resource` only inserts if absent, so an earlier
        // consumer call is preserved.
        app.init_resource::<ObeliskEntityIndex>()
            .init_resource::<SkillRegistry>()
            .init_resource::<CombatRng>()
            .init_resource::<crate::core::cooldown::Cooldowns>()
            .add_systems(Update, (sync_index_added, sync_index_removed))
            .add_systems(
                FixedUpdate,
                tick::tick_effects_system.in_set(ObeliskSet::TickEffects),
            )
            .add_systems(
                FixedUpdate,
                crate::core::cooldown::tick_cooldowns.in_set(crate::ObeliskSet::TickEffects),
            );
    }
}
