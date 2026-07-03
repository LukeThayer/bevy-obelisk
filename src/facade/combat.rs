use crate::assets::CastTimelineHandles;
use crate::combat::resolve::resolve_one_hit;
use crate::combat::system::{
    eval_condition_obelisk_side, is_invalid_timeline_target, is_unsupported_timeline_condition,
    partition_conditions,
};
use crate::core::components::Attributes;
use crate::core::config::{CombatRng, SkillRegistry};
use crate::events::{DamageResolved, EffectApplied, EntityDied};
use crate::timeline::triggered::{execute_skill_timeline, ExecPayload};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Authoritative programmatic combat entry: resolve a skill hit WITHOUT the spatial pipeline
/// (scripted damage, AI that picked a target via ObeliskSpatial, etc.). Routes through the
/// deterministic `resolve_one_hit` (never `thread_rng`) and emits the same events.
///
/// Final-review fix wave, item 2 ("facade bypass"): a skill whose conditions name a
/// TIMELINE-target skill (e.g. fireball's `always -> fireball_explosion`, spec §3.2 / Task 7)
/// used to resolve as an ordinary inline packet here — `resolve_one_hit` got the registry's
/// UNSTRIPPED skill, so the timeline-target condition fired inside stat_core's calc path and
/// applied its damage invisibly (the facade never surfaces `outcome.triggered_skill_hits`),
/// silently diverging from `on_hit_confirmed`'s spatial-execution behavior for the exact same
/// skill. Fixed by mirroring `on_hit_confirmed` exactly: `partition_conditions` strips
/// timeline-target conditions from the resolve clone, then each matched condition is executed
/// SPATIALLY via `execute_skill_timeline` (reusing `eval_condition_obelisk_side`, the same
/// evaluation the observer uses) at the TARGET's `Transform` position, depth 1 (the facade has
/// no depth concept of its own, so its own hits are implicitly depth 0). This SystemParam
/// already carries `Commands` (needed for the existing `DamageResolved`/etc. triggers), so full
/// spatial execution — not just a strip + warn — was viable; `resolve_aoe` inherits this for
/// free since it calls `resolve_skill_hit` per target.
#[derive(SystemParam)]
pub struct ObeliskCombat<'w, 's> {
    attrs: Query<'w, 's, &'static mut Attributes>,
    transforms: Query<'w, 's, &'static Transform>,
    registry: Res<'w, SkillRegistry>,
    handles: Res<'w, CastTimelineHandles>,
    rng: ResMut<'w, CombatRng>,
    commands: Commands<'w, 's>,
}

impl ObeliskCombat<'_, '_> {
    /// Resolve one hit of `skill_id` from `caster` onto `target`. Returns total damage dealt,
    /// or None if the skill/entities are missing or caster==target. Emits DamageResolved /
    /// EffectApplied / EntityDied for the primary hit, and (item 2) spatially executes any
    /// matched timeline-target condition instead of folding it into the primary packet.
    pub fn resolve_skill_hit(
        &mut self,
        caster: Entity,
        target: Entity,
        skill_id: &str,
    ) -> Option<f64> {
        let skill = self.registry.0.get(skill_id)?.clone();

        // Item 2: split off timeline-target conditions exactly as `on_hit_confirmed` does —
        // stat_core must never resolve one of these as an inline packet. See
        // `combat::system::partition_conditions`'s doc for the full load-order reasoning.
        let (timeline_targets, packet_conditions) =
            partition_conditions(&skill.conditions, &self.handles);
        for cond in &timeline_targets {
            if is_invalid_timeline_target(cond, &self.handles) {
                warn!(
                    "facade: skill '{}' condition triggers timeline skill '{}' with additional \
                     = false — timeline-target conditions must be additional = true (v1); \
                     treating as additional",
                    skill_id, cond.trigger_skill
                );
            }
            if is_unsupported_timeline_condition(cond, &self.handles) {
                warn!(
                    "facade: skill '{}' condition triggers timeline skill '{}' using \
                     EveryNthHit — forbidden on timeline-target conditions (v1, spec §3.2); \
                     skipping, this trigger will never fire",
                    skill_id, cond.trigger_skill
                );
            }
        }
        let skill_for_resolve = if timeline_targets.is_empty() {
            skill
        } else {
            let mut s = skill;
            s.conditions = packet_conditions;
            s
        };

        let [mut caster_a, mut target_a] = self.attrs.get_many_mut([caster, target]).ok()?;
        let outcome = resolve_one_hit(
            &mut caster_a.0,
            &mut target_a.0,
            &skill_for_resolve,
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
            is_critical: outcome.is_critical,
            damage_prevented: outcome.damage_prevented,
            life_gained: outcome.life_gained,
            mana_gained: outcome.mana_gained,
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

        // Item 2 — "the fireball moment", facade edition: execute each matched timeline-target
        // condition SPATIALLY, at the TARGET's world position (the facade has no hit/impact
        // position of its own the way a spatial hitbox does — the target it just resolved
        // against is the closest analogue), one trigger-depth deeper than a top-level facade
        // call. `execute_skill_timeline` itself enforces `MAX_TRIGGER_DEPTH` — not re-checked
        // here, one source of truth.
        for cond in &timeline_targets {
            if is_unsupported_timeline_condition(cond, &self.handles) {
                continue;
            }
            if eval_condition_obelisk_side(&cond.condition, &outcome) {
                let position = self
                    .transforms
                    .get(target)
                    .map(|t| t.translation)
                    .unwrap_or(Vec3::ZERO);
                execute_skill_timeline(
                    &mut self.commands,
                    caster,
                    &cond.trigger_skill,
                    ExecPayload {
                        position,
                        direction: Vec3::X,
                        target: Some(target),
                        charge: None,
                        depth: 1,
                    },
                );
            }
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

    // -----------------------------------------------------------------------------------------
    // Final review, item 2: the facade must treat timeline-target conditions EXACTLY like
    // `on_hit_confirmed` — strip them from the resolve clone (never an inline packet) and
    // execute them spatially instead. Mirrors `tests/triggered_exec.rs`'s fireball/
    // fireball_explosion fixture, condensed for the facade's simpler (no cast, no travel) entry
    // point.
    // -----------------------------------------------------------------------------------------

    use crate::assets::{
        CastTimeline, CollisionShape, CollisionWindow, HitFilter, HitMode, PhaseDurations,
        VolumeMotion, WindowAnchor, WindowPhase, WindowSpawn,
    };

    /// `fireball` (bolt_damage) `always ->` `fireball_explosion` (15 fire), additional = true —
    /// the v1-required shape (see `combat::system::is_invalid_timeline_target`'s doc).
    fn fireball_pair_toml(bolt_damage: f64) -> String {
        format!(
            r#"
[[skills]]
id = "fireball"
name = "Fireball"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[[skills.conditions]]
trigger_skill = "fireball_explosion"
type = "always"
additional = true
[skills.damage]
base_damages = [{{ type = "fire", min = {bolt_damage}, max = {bolt_damage} }}]

[[skills]]
id = "fireball_explosion"
name = "Fireball Explosion"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{{ type = "fire", min = 15.0, max = 15.0 }}]
"#,
        )
    }

    /// The explosion's own timeline: one offset-0 `Active` `Static` sphere anchored
    /// `CastPoint` (resolves at the triggered execution's payload position — for the facade,
    /// the TARGET's `Transform`), wide enough to reach a hurtbox sitting right at that position.
    fn fireball_explosion_timeline() -> CastTimeline {
        CastTimeline {
            skill_id: "fireball_explosion".into(),
            phase_durations: PhaseDurations {
                windup: 0.0,
                active: 1.0,
                recovery: 0.0,
            },
            collision_windows: vec![CollisionWindow {
                id: "blast".into(),
                spawn: WindowSpawn::Scheduled {
                    phase: WindowPhase::Active,
                    offset: 0.0,
                },
                anchor: WindowAnchor::CastPoint,
                anchor_offset: Vec3::ZERO,
                strikes: true,
                active_duration: 0.2,
                shape: CollisionShape::Sphere { radius: 1.5 },
                motion: VolumeMotion::Static,
                motion_direction: Default::default(),
                hit_filter: HitFilter::Enemies,
                hit_mode: HitMode::OncePerTarget,
                rehit_interval: None,
                emitter: None,
            }],
            acquisition: Default::default(),
            vfx_cues: std::collections::HashMap::new(),
            chain_radius: 6.0,
            chargeable: false,
            max_hold: 1.0,
            cues: std::collections::HashMap::new(),
        }
    }

    /// Player + dummy (WITH a hurtbox at the dummy's `Transform` — the explosion needs
    /// something to hit) with both `fireball` and `fireball_explosion` registered in
    /// `SkillRegistry` AND `CastTimelineHandles`/`Assets<CastTimeline>`.
    fn harness_with_fireball_pair(seed: u64, bolt_damage: f64) -> (ObeliskTestApp, Entity, Entity) {
        let mut t = ObeliskTestApp::new(seed);
        let skills = stat_core::config::parse_skills(&fireball_pair_toml(bolt_damage)).unwrap();
        t.app
            .world_mut()
            .resource_mut::<SkillRegistry>()
            .0
            .extend(skills);
        let blast_handle = t
            .app
            .world_mut()
            .resource_mut::<Assets<CastTimeline>>()
            .add(fireball_explosion_timeline());
        t.app
            .world_mut()
            .resource_mut::<CastTimelineHandles>()
            .0
            .insert("fireball_explosion".into(), blast_handle);

        let caster = spawn(&mut t, "caster", Faction::Player, 100.0);
        let dummy_pos = Vec3::new(0.0, 0.0, 2.0);
        let dummy = t
            .app
            .world_mut()
            .spawn((
                Combatant,
                Attributes({
                    let mut b = StatBlock::with_id("dummy");
                    b.max_life.base = 500.0;
                    b.current_life = 500.0;
                    b
                }),
                Faction::Enemy,
                ObeliskId("dummy".into()),
                Transform::from_translation(dummy_pos),
            ))
            .id();
        {
            let mut commands = t.app.world_mut().commands();
            insert_hurtbox(&mut commands, dummy, 0.6, dummy_pos);
        }
        t.app.update();
        (t, caster, dummy)
    }

    /// The core item-2 regression: before the fix, the facade handed the UNSTRIPPED skill to
    /// `resolve_one_hit`, so `fireball_explosion`'s 15 damage resolved as an ordinary inline
    /// packet — folded into the SAME `resolve_skill_hit` call, applied before this function even
    /// returns. Post-fix, the explosion is stripped and only executes (later, spatially, once
    /// `advance_triggered_execs` ticks) — so immediately after `resolve_skill_hit` returns, the
    /// target's life must reflect ONLY the primary bolt's damage.
    #[test]
    fn resolve_skill_hit_does_not_inline_a_timeline_target_condition() {
        let (mut t, caster, dummy) = harness_with_fireball_pair(30, 20.0);
        let dmg = t
            .app
            .world_mut()
            .run_system_once(move |mut c: ObeliskCombat| {
                c.resolve_skill_hit(caster, dummy, "fireball")
            })
            .unwrap();
        t.app.world_mut().flush();

        let life_after = t
            .app
            .world()
            .entity(dummy)
            .get::<Attributes>()
            .unwrap()
            .0
            .current_life;
        assert_eq!(
            dmg,
            Some(20.0),
            "resolve_skill_hit must report ONLY the primary bolt's damage"
        );
        assert!(
            (500.0 - life_after - 20.0).abs() < 1e-6,
            "life delta must be the primary hit ALONE (20.0) — got a delta of {}, meaning the \
             timeline-target condition resolved inline instead of being stripped",
            500.0 - life_after
        );
    }

    /// The other half: the stripped condition isn't just dropped — it actually executes,
    /// spatially, via `execute_skill_timeline`, once the world ticks far enough for
    /// `advance_triggered_execs` to spawn the explosion's window and the spatial pipeline to
    /// detect the hit against the dummy's hurtbox. That hit goes through the REAL
    /// `on_hit_confirmed` observer, so it shows up as its own `DamageResolved`.
    #[test]
    fn resolve_skill_hit_executes_timeline_target_condition_spatially() {
        let (mut t, caster, dummy) = harness_with_fireball_pair(31, 20.0);
        t.app
            .world_mut()
            .run_system_once(move |mut c: ObeliskCombat| {
                c.resolve_skill_hit(caster, dummy, "fireball")
            })
            .unwrap();
        t.advance_ticks(30);

        let rec = t.rec();
        let ids: Vec<&str> = rec
            .damage_resolved
            .iter()
            .map(|d| d.skill_id.as_str())
            .collect();
        assert!(
            ids.contains(&"fireball"),
            "the primary facade hit still resolves, got {ids:?}"
        );
        assert!(
            ids.contains(&"fireball_explosion"),
            "the timeline-target condition must execute spatially, got {ids:?}"
        );
        assert_eq!(
            ids.iter().filter(|i| **i == "fireball_explosion").count(),
            1,
            "exactly once, got {ids:?}"
        );
        let explosion_depth_zero_free = rec
            .damage_resolved
            .iter()
            .find(|d| d.skill_id == "fireball_explosion")
            .map(|d| d.mana_spent)
            .unwrap();
        assert_eq!(
            explosion_depth_zero_free, 0.0,
            "a depth>0 triggered execution must resolve mana-free"
        );
    }
}
