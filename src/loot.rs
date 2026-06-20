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

/// Per-entity loot roll parameters (rarity / quantity multipliers + area level) for the entity's
/// drop-table roll on death. Absent ⇒ the roll uses the defaults (`rarity_mult` / `quantity_mult`
/// = 1.0, `level` = 1), preserving the original `roll_drops_on_death` behavior.
#[derive(Component, Clone, Debug)]
pub struct DropRollParams {
    pub rarity_mult: f64,
    pub quantity_mult: f64,
    pub level: u32,
}

impl Default for DropRollParams {
    fn default() -> Self {
        Self {
            rarity_mult: 1.0,
            quantity_mult: 1.0,
            level: 1,
        }
    }
}

/// On death, roll the dead entity's drop table (if any) and emit `LootDropped`.
pub fn roll_drops_on_death(
    death: On<EntityDied>,
    tables: Option<Res<DropTables>>,
    drop_ids: Query<&DropTableId>,
    roll_params: Query<&DropRollParams>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
) {
    let Some(tables) = tables else { return };
    let target = death.event().target;
    let Ok(table_id) = drop_ids.get(target) else {
        return;
    };
    // Per-entity roll params if present; otherwise the original defaults (1.0 / 1.0 / level 1).
    let params = roll_params.get(target).cloned().unwrap_or_default();
    if let Ok(drops) = tables.0.roll(
        &table_id.0,
        params.rarity_mult,
        params.quantity_mult,
        params.level,
        &mut rng.0,
    ) {
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
