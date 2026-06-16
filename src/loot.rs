use crate::core::config::CombatRng;
use crate::events::{EntityDied, LootDropped};
use bevy::prelude::*;

/// Drop tables (from `tables_core`). Insert this resource when you have loot config.
#[derive(Resource)]
pub struct DropTables(pub tables_core::DropTableRegistry);

/// Optional loot generator (from `loot_core`) for turning item drops into full `Item`s.
#[derive(Resource)]
pub struct ItemGenerator(pub loot_core::Generator);

/// Which drop table an entity rolls on death.
#[derive(Component, Clone, Debug)]
pub struct DropTableId(pub String);

/// On death, roll the dead entity's drop table (if any) and emit `LootDropped`.
pub fn roll_drops_on_death(
    death: On<EntityDied>,
    tables: Option<Res<DropTables>>,
    drop_ids: Query<&DropTableId>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
) {
    let Some(tables) = tables else { return };
    let target = death.event().target;
    let Ok(table_id) = drop_ids.get(target) else {
        return;
    };
    if let Ok(drops) = tables.0.roll(&table_id.0, 1.0, 1.0, 1, &mut rng.0) {
        if !drops.is_empty() {
            commands.trigger(LootDropped {
                source: target,
                drops,
            });
        }
    }
}

pub struct ObeliskLootPlugin;
impl Plugin for ObeliskLootPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(roll_drops_on_death);
    }
}
