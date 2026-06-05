use bevy::prelude::*;
use stat_core::StatBlock;

/// The GAS-style AttributeSet: an obelisk StatBlock. Only sim systems hold `&mut`.
#[derive(Component, Clone, Debug)]
pub struct Attributes(pub StatBlock);

impl Default for Attributes {
    fn default() -> Self { Attributes(StatBlock::new()) }
}

/// Team / faction for hit filtering.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Faction { Player, Enemy, Neutral }
impl Default for Faction { fn default() -> Self { Faction::Neutral } }

/// Skill ids this entity can cast.
#[derive(Component, Clone, Debug, Default)]
pub struct SkillSlots(pub Vec<String>);

/// Ergonomic marker. Inserting `Combatant` auto-requires the rest.
/// NOTE: required components attach each type's `Default`, so `Combatant` alone yields a
/// valid-but-EMPTY StatBlock. Real stats come from inserting `Attributes(real_block)` at spawn.
#[derive(Component, Default)]
#[require(Attributes, Faction, SkillSlots, crate::ids::ObeliskId, Transform)]
pub struct Combatant;

impl Attributes {
    /// True if the StatBlock has no active effects (skip tick clone).
    pub fn effects_is_empty_fast(&self) -> bool { self.0.effects.is_empty() }
}
