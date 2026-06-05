use crate::assets::HitFilter;
use crate::core::components::Faction;

/// Whether a hitbox with `filter` (owned by `caster_faction`) may hit a target.
/// `is_self` is true when the target entity IS the caster.
pub fn passes_filter(
    filter: HitFilter,
    caster_faction: Faction,
    target_faction: Faction,
    is_self: bool,
) -> bool {
    match filter {
        HitFilter::Caster => is_self,
        HitFilter::All => !is_self,
        HitFilter::Enemies => !is_self && target_faction != caster_faction,
        HitFilter::Allies => !is_self && target_faction == caster_faction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemies_hits_other_faction_only() {
        assert!(passes_filter(HitFilter::Enemies, Faction::Player, Faction::Enemy, false));
        assert!(!passes_filter(HitFilter::Enemies, Faction::Player, Faction::Player, false));
        assert!(!passes_filter(HitFilter::Enemies, Faction::Player, Faction::Enemy, true));
    }
    #[test]
    fn allies_hits_same_faction_only() {
        assert!(passes_filter(HitFilter::Allies, Faction::Player, Faction::Player, false));
        assert!(!passes_filter(HitFilter::Allies, Faction::Player, Faction::Enemy, false));
    }
    #[test]
    fn all_hits_anyone_but_self() {
        assert!(passes_filter(HitFilter::All, Faction::Player, Faction::Enemy, false));
        assert!(passes_filter(HitFilter::All, Faction::Player, Faction::Player, false));
        assert!(!passes_filter(HitFilter::All, Faction::Player, Faction::Player, true));
    }
    #[test]
    fn caster_hits_only_self() {
        assert!(passes_filter(HitFilter::Caster, Faction::Player, Faction::Player, true));
        assert!(!passes_filter(HitFilter::Caster, Faction::Player, Faction::Enemy, false));
    }
}
