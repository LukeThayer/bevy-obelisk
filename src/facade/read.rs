use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crate::core::components::Attributes;
use crate::core::config::SkillRegistry;
use crate::core::cooldown::Cooldowns;
use crate::events::CastRejectReason;

/// Read-only consumer facade for HUD / UI / AI. Holds no `&mut` — safe to use anywhere.
#[derive(SystemParam)]
pub struct ObeliskRead<'w, 's> {
    attrs: Query<'w, 's, &'static Attributes>,
    registry: Res<'w, SkillRegistry>,
    cooldowns: Res<'w, Cooldowns>,
}

impl ObeliskRead<'_, '_> {
    pub fn life_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.current_life)
    }
    pub fn max_life_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.computed_max_life())
    }
    pub fn mana_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.current_mana)
    }
    pub fn max_mana_of(&self, e: Entity) -> Option<f64> {
        self.attrs.get(e).ok().map(|a| a.0.computed_max_mana())
    }
    pub fn is_alive(&self, e: Entity) -> bool {
        self.attrs.get(e).map(|a| a.0.is_alive()).unwrap_or(false)
    }
    pub fn effect_count(&self, e: Entity) -> usize {
        self.attrs.get(e).map(|a| a.0.effects.len()).unwrap_or(0)
    }
    pub fn has_effect(&self, e: Entity, effect_id: &str) -> bool {
        self.attrs.get(e).map(|a| a.0.effects.iter().any(|ef| ef.id == effect_id)).unwrap_or(false)
    }
    pub fn cooldown_remaining(&self, e: Entity, skill_id: &str) -> f32 {
        self.cooldowns.remaining(e, skill_id)
    }
    /// Can `e` begin casting `skill_id` right now? Checks skill exists, mana (obelisk
    /// `can_use_skill`), and cooldown. Range/LOS are validated at cast time (need a target).
    pub fn can_cast(&self, e: Entity, skill_id: &str) -> Result<(), CastRejectReason> {
        let Some(skill) = self.registry.0.get(skill_id) else {
            return Err(CastRejectReason::UnknownSkill);
        };
        let Ok(attrs) = self.attrs.get(e) else {
            return Err(CastRejectReason::NoTarget);
        };
        if !attrs.0.can_use_skill(skill) {
            return Err(CastRejectReason::InsufficientMana);
        }
        if !self.cooldowns.is_ready(e, skill_id) {
            return Err(CastRejectReason::OnCooldown);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use bevy::ecs::system::RunSystemOnce;
    use stat_core::StatBlock;

    #[test]
    fn reads_life_and_can_cast() {
        let mut t = ObeliskTestApp::new(1);
        let mut block = StatBlock::with_id("hero");
        block.max_life.base = 80.0;
        block.current_life = 80.0;
        block.max_mana.base = 50.0;
        block.current_mana = 50.0;
        let hero = t.app.world_mut().spawn((
            Combatant, Attributes(block), Faction::Player, ObeliskId("hero".into()), Transform::default(),
        )).id();
        t.app.update();

        let life = t.app.world_mut().run_system_once(move |r: ObeliskRead| r.life_of(hero)).unwrap();
        assert_eq!(life, Some(80.0));
        let can = t.app.world_mut().run_system_once(move |r: ObeliskRead| r.can_cast(hero, "firebolt")).unwrap();
        assert!(can.is_ok(), "hero with mana + no cooldown can cast firebolt: {can:?}");
        let bad = t.app.world_mut().run_system_once(move |r: ObeliskRead| r.can_cast(hero, "nope")).unwrap();
        assert_eq!(bad, Err(CastRejectReason::UnknownSkill));
    }
}
