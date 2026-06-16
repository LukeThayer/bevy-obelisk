use crate::combat::resolve::resolve_one_hit;
use crate::core::components::Attributes;
use crate::core::config::{CombatRng, SkillRegistry};
use crate::events::{DamageResolved, EffectApplied, EntityDied};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Authoritative programmatic combat entry: resolve a skill hit WITHOUT the spatial pipeline
/// (scripted damage, AI that picked a target via ObeliskSpatial, etc.). Routes through the
/// deterministic `resolve_one_hit` (never `thread_rng`) and emits the same events.
#[derive(SystemParam)]
pub struct ObeliskCombat<'w, 's> {
    attrs: Query<'w, 's, &'static mut Attributes>,
    registry: Res<'w, SkillRegistry>,
    rng: ResMut<'w, CombatRng>,
    commands: Commands<'w, 's>,
}

impl ObeliskCombat<'_, '_> {
    /// Resolve one hit of `skill_id` from `caster` onto `target`. Returns total damage dealt,
    /// or None if the skill/entities are missing or caster==target. Emits DamageResolved /
    /// EffectApplied / EntityDied.
    pub fn resolve_skill_hit(
        &mut self,
        caster: Entity,
        target: Entity,
        skill_id: &str,
    ) -> Option<f64> {
        let skill = self.registry.0.get(skill_id)?.clone();
        let [mut caster_a, mut target_a] = self.attrs.get_many_mut([caster, target]).ok()?;
        let outcome = resolve_one_hit(
            &mut caster_a.0,
            &mut target_a.0,
            &skill,
            &self.registry.0,
            &mut self.rng.0,
        )
        .ok()?;
        let life_after = target_a.0.current_life;
        let alive = target_a.0.is_alive();
        self.commands.trigger(DamageResolved {
            caster,
            target,
            skill_id: skill_id.to_string(),
            total_damage: outcome.total_damage,
            is_killing_blow: outcome.is_killing_blow,
            life_after,
            mana_spent: outcome.mana_spent,
        });
        for ef in &outcome.effects_applied {
            self.commands.trigger(EffectApplied {
                target,
                effect_id: ef.id.clone(),
                total_duration: ef.total_duration,
                stacks: ef.stacks,
            });
        }
        if outcome.is_killing_blow || !alive {
            self.commands.trigger(EntityDied {
                target,
                killer: Some(caster),
            });
        }
        Some(outcome.total_damage)
    }

    /// Fan one cast over many targets. Targets are sorted by a STABLE key (the StatBlock id)
    /// before drawing from the seeded RNG, so iteration order can't perturb determinism.
    pub fn resolve_aoe(&mut self, caster: Entity, targets: &[Entity], skill_id: &str) -> usize {
        let mut ordered: Vec<Entity> = targets.to_vec();
        ordered.sort_by_key(|&e| {
            self.attrs
                .get(e)
                .map(|a| a.0.id.clone())
                .unwrap_or_default()
        });
        let mut hits = 0;
        for target in ordered {
            if target == caster {
                continue;
            }
            if self.resolve_skill_hit(caster, target, skill_id).is_some() {
                hits += 1;
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use bevy::ecs::system::RunSystemOnce;
    use stat_core::StatBlock;

    fn spawn(t: &mut ObeliskTestApp, id: &str, faction: Faction, life: f64) -> Entity {
        let mut b = StatBlock::with_id(id);
        b.max_life.base = life;
        b.current_life = life;
        b.max_mana.base = 100.0;
        b.current_mana = 100.0;
        t.app
            .world_mut()
            .spawn((
                Combatant,
                Attributes(b),
                faction,
                ObeliskId(id.into()),
                Transform::default(),
            ))
            .id()
    }

    // `aoe_fan` is NOT in the golden feature matrix: `resolve_aoe` is a programmatic
    // SystemParam call, not driven through the cast/hitbox event pipeline, so a golden
    // event-trace would be awkward. We cover it here directly instead — asserting it hits
    // every (non-self) target and that the same seed yields an identical, deterministic
    // outcome (the stable-sort-by-id guarantees iteration order can't perturb the RNG).
    #[test]
    fn resolve_aoe_hits_all_targets_deterministically() {
        let run = || {
            let mut t = ObeliskTestApp::new(11);
            let caster = spawn(&mut t, "caster", Faction::Player, 100.0);
            let a = spawn(&mut t, "enemy_a", Faction::Enemy, 100.0);
            let b = spawn(&mut t, "enemy_b", Faction::Enemy, 100.0);
            let c = spawn(&mut t, "enemy_c", Faction::Enemy, 100.0);
            t.app.update();
            // Include the caster in the target list to prove self is skipped.
            let targets = [a, caster, b, c];
            let hits = t
                .app
                .world_mut()
                .run_system_once(move |mut combat: ObeliskCombat| {
                    combat.resolve_aoe(caster, &targets, "firebolt")
                })
                .unwrap();
            let life = |t: &ObeliskTestApp, e: Entity| {
                t.app
                    .world()
                    .entity(e)
                    .get::<Attributes>()
                    .unwrap()
                    .0
                    .current_life
            };
            (
                hits,
                life(&t, a),
                life(&t, b),
                life(&t, c),
                life(&t, caster),
            )
        };
        let (hits, la, lb, lc, lcaster) = run();
        assert_eq!(hits, 3, "all three enemies hit, caster skipped");
        assert!(
            la < 100.0 && lb < 100.0 && lc < 100.0,
            "every enemy took damage"
        );
        assert_eq!(lcaster, 100.0, "the caster (in the list) must be skipped");
        assert_eq!(
            run(),
            (hits, la, lb, lc, lcaster),
            "same seed -> identical AoE outcome (deterministic, order-independent)"
        );
    }

    #[test]
    fn resolve_skill_hit_deals_damage_programmatically() {
        let mut t = ObeliskTestApp::new(5);
        let caster = spawn(&mut t, "caster", Faction::Player, 100.0);
        let target = spawn(&mut t, "target", Faction::Enemy, 100.0);
        t.app.update();
        let dmg = t
            .app
            .world_mut()
            .run_system_once(move |mut c: ObeliskCombat| {
                c.resolve_skill_hit(caster, target, "firebolt")
            })
            .unwrap();
        assert!(
            dmg.unwrap_or(0.0) > 0.0,
            "programmatic firebolt should deal damage"
        );
        let remaining = t
            .app
            .world()
            .entity(target)
            .get::<Attributes>()
            .unwrap()
            .0
            .current_life;
        assert!(remaining < 100.0, "target took damage (life {remaining})");
    }
}
