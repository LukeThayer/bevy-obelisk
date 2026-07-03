use crate::assets::CastTimelineHandles;
use crate::combat::resolve::{
    combat_result_life_gained, combat_result_mana_gained, combat_result_prevented,
    resolve_one_hit_charged, HitOutcome,
};
use crate::core::components::Attributes;
use crate::core::config::{CombatRng, SkillRegistry};
use crate::events::{DamageResolved, EffectApplied, EntityDied, HitConfirmed, TriggerFired};
use crate::timeline::triggered::{execute_skill_timeline, ExecPayload};
use bevy::prelude::*;
use stat_core::combat::resolve_damage_with_rng;
use stat_core::{ConditionPhase, Skill, SkillCondition, TriggerCondition, TriggerConditionEval};

/// Hard cap on the number of triggered-effect resolutions processed for a single hit.
/// Bounds any pathological re-entrant cascade (defensive — damage resolution does not itself
/// produce new triggers, so the worklist cannot actually grow from auto-firing alone).
const MAX_TRIGGER_RESOLUTIONS: usize = 8;

/// The billing rule (spec §3.2): mana bills per-hit only for the cast's own scheduled windows.
/// Chain re-strikes (`hop > 0`) and triggered sub-casts (`depth > 0`) never passed through cast
/// validation, so they must resolve mana-free rather than fizzling (or double-billing) at
/// `resolve_one_hit_charged`'s `consume_skill_resources` step. (Task 11 extends this with
/// `|| ev.emitted`.)
fn is_free_hit(ev: &HitConfirmed) -> bool {
    ev.depth > 0 || ev.hop > 0
}

/// A clone of `s` with `mana_cost` zeroed — handed to `resolve_one_hit_charged` for free hits so
/// `consume_skill_resources` charges nothing (and never fizzles on insufficient mana). Also the
/// site Task 7's condition stripping extends — one clone, two edits.
fn free_clone(s: &Skill) -> Skill {
    let mut c = s.clone();
    c.mana_cost = 0.0;
    c
}

/// True when `cond` names a skill that has a registered timeline (Task 6/7: it will be executed
/// SPATIALLY via `execute_skill_timeline`, never resolved as an inline packet) but is marked
/// `additional = false`. That combination is a content-authoring bug: `additional = false` means
/// "replace the primary packet" — a request stat_core can only honor for a packet it actually
/// resolves, but a timeline-target condition is stripped from the resolve clone before it ever
/// reaches stat_core (see `partition_conditions`), so `additional` has NO code effect in the
/// timeline branch either way. The load-time home for this check
/// (`stat_core::config::skills::validate_skill_trigger_references`'s sibling on the obelisk-bevy
/// side, `core/config.rs::add_obelisk_skills`) can't see it — see `partition_conditions` doc for
/// why — so it is caught here, at the partition site, instead. Pure predicate (no `warn!`) so it
/// stays cheaply unit-testable without asserting on log output. `pub` (like
/// `execute_skill_timeline`/`ExecPayload`, Task 6) so `tests/triggered_exec.rs` can exercise the
/// v1 validation directly, in its honest (runtime, not load-time) form.
pub fn is_invalid_timeline_target(cond: &SkillCondition, handles: &CastTimelineHandles) -> bool {
    handles.0.contains_key(&cond.trigger_skill) && !cond.additional
}

/// Splits `conditions` into (timeline-target, packet) buckets by whether `trigger_skill` has a
/// registered `CastTimeline` handle:
///   - timeline-target: Task 6's skill B has its own timeline. `on_hit_confirmed` strips these
///     from the clone handed to `resolve_one_hit_charged` (skill A's hit resolves WITHOUT B's
///     packet folded in) and, post-resolve, evaluates each against the widened `HitOutcome`
///     (`eval_condition_obelisk_side`) — a match executes B's timeline SPATIALLY at the hit
///     position (Task 7, "the fireball moment").
///   - packet: untouched legacy path — resolved inline by stat_core exactly as today.
///
/// Load-order note: this can't be a load-time validation (`add_obelisk_skills`) because
/// `CastTimelineHandles` isn't populated until AFTER skills load — `run_scenario` (and
/// `ObeliskTestApp`) call `add_obelisk_skills` up front, then stream `.cast.ron` assets in and
/// register their handles only once loaded (see `scenario/run.rs::run_scenario`). A skill-load-
/// time check would see an empty `CastTimelineHandles` and could never catch a real misuse. Any
/// `additional = false` timeline-target condition found here is a content bug (see
/// `is_invalid_timeline_target`); `on_hit_confirmed` warns and treats it as additional regardless
/// (harmless, since `additional` never reaches stat_core for a stripped condition). `pub` for the
/// same external-test reason as `is_invalid_timeline_target`.
pub fn partition_conditions(
    conditions: &[SkillCondition],
    handles: &CastTimelineHandles,
) -> (Vec<SkillCondition>, Vec<SkillCondition>) {
    let mut timeline_targets = Vec::new();
    let mut packet_conditions = Vec::new();
    for cond in conditions {
        if handles.0.contains_key(&cond.trigger_skill) {
            timeline_targets.push(cond.clone());
        } else {
            packet_conditions.push(cond.clone());
        }
    }
    (timeline_targets, packet_conditions)
}

/// Obelisk-side evaluation of a hit-phase `TriggerCondition` naming a TIMELINE-target skill,
/// against the widened `HitOutcome` (Task 4's seam). Dispatches by `TriggerCondition::phase()` to
/// the matching `TriggerConditionEval` method, feeding it the closest `HitOutcome` equivalent:
///
///   - `PreCalculation`  -> `evaluate_pre(attacker_before, defender_before)` — the pre-hit,
///     pre-mutation snapshots (e.g. `Always`, `TargetLowLife`).
///   - `PostCalculation` -> `evaluate_post_calc(primary_packet)` — the raw, pre-mitigation primary
///     packet (e.g. `OnCrit`, `DamageOverThreshold`).
///   - `PostResolution`  -> hand-mapped against `HitOutcome` scalars, since `HitOutcome` does NOT
///     retain a stat_core `CombatResult` (Task 4 review finding): `OnKill` -> `is_killing_blow`
///     (the one PostResolution field `HitOutcome` genuinely carries). `OnOverkill` /
///     `OnBarrierBroken` need result fields `HitOutcome` doesn't surface — v1: always false
///     (documented gap, not a silent bug) rather than widening `HitOutcome` further in this task.
///   - `DefensiveResolution` -> always false. These are RECEIVER-side procs (`OnDamageTaken`,
///     `OnDodge`, etc. — evaluated by `resolve_damage_with_triggers`'s target-side triggered-
///     effect path against the hit the target is ABSORBING), never an attacker's own hit-
///     condition phase; there is no defined mapping for this integration.
///   - `Lifecycle` -> always false. `OnImpact` / `OnExpire` are the spatial layer's job (window
///     end events, Task 8+), never evaluated during hit resolution.
fn eval_condition_obelisk_side(cond: &TriggerCondition, out: &HitOutcome) -> bool {
    match cond.phase() {
        ConditionPhase::PreCalculation => {
            cond.evaluate_pre(&out.attacker_before, &out.defender_before)
        }
        ConditionPhase::PostCalculation => cond.evaluate_post_calc(&out.primary_packet),
        ConditionPhase::PostResolution => match cond {
            TriggerCondition::OnKill => out.is_killing_blow,
            // v1: not yet surfaced — HitOutcome doesn't retain the CombatResult fields
            // (barrier_before/after, life_after) these need; do NOT widen HitOutcome here.
            TriggerCondition::OnOverkill { .. } | TriggerCondition::OnBarrierBroken => false,
            _ => false,
        },
        ConditionPhase::DefensiveResolution => false,
        ConditionPhase::Lifecycle => false,
    }
}

/// Observer: when a hit is confirmed, run the deterministic resolve funnel and emit results.
pub fn on_hit_confirmed(
    hit: On<HitConfirmed>,
    mut attrs: Query<&mut Attributes>,
    registry: Res<SkillRegistry>,
    handles: Res<CastTimelineHandles>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
) {
    let ev = hit.event().clone();
    let Some(skill) = registry.0.get(&ev.skill_id) else {
        return;
    };

    // Need &mut caster + &mut target disjointly.
    let [mut caster_attrs, mut target_attrs] = match attrs.get_many_mut([ev.caster, ev.target]) {
        Ok(pair) => pair,
        Err(_) => return, // same entity or missing
    };

    // Task 7: split skill A's conditions into timeline-target (skill B has its own timeline —
    // strip from the resolve clone, execute B spatially post-resolve) vs packet (untouched
    // legacy path). See `partition_conditions` doc for the load-order reasoning and
    // `is_invalid_timeline_target` for the `additional = false` content-bug check.
    let (timeline_targets, packet_conditions) = partition_conditions(&skill.conditions, &handles);
    for cond in &timeline_targets {
        if is_invalid_timeline_target(cond, &handles) {
            warn!(
                "skill '{}' condition triggers timeline skill '{}' with additional = false — \
                 timeline-target conditions must be additional = true (v1); treating as \
                 additional (a stripped condition never reaches stat_core's resolve either way)",
                ev.skill_id, cond.trigger_skill
            );
        }
    }

    // The billing rule (spec §3.2): free hits (triggered sub-casts, chain re-strikes) resolve
    // against a mana-zeroed clone so they never bill or fizzle; the paid (hot) path keeps passing
    // the registry `&Skill` straight through — no clone. Task 7 extends this: ANY hit with a
    // timeline-target condition also needs a clone (to strip those conditions before resolve),
    // even on the paid path — the no-clone fast path is kept exactly when neither applies.
    let owned_clone_skill;
    let skill_for_resolve: &Skill = if is_free_hit(&ev) || !timeline_targets.is_empty() {
        let mut c = if is_free_hit(&ev) {
            free_clone(skill)
        } else {
            skill.clone()
        };
        if !timeline_targets.is_empty() {
            c.conditions = packet_conditions.clone();
        }
        owned_clone_skill = c;
        &owned_clone_skill
    } else {
        skill
    };

    let outcome = match resolve_one_hit_charged(
        &mut caster_attrs.0,
        &mut target_attrs.0,
        skill_for_resolve,
        &registry.0,
        &mut rng.0,
        ev.charge,
    ) {
        Ok(o) => o,
        Err(_) => return,
    };

    let life_after = target_attrs.0.current_life;
    commands.trigger(DamageResolved {
        caster: ev.caster,
        target: ev.target,
        skill_id: ev.skill_id.clone(),
        total_damage: outcome.total_damage,
        is_killing_blow: outcome.is_killing_blow,
        life_after,
        mana_spent: outcome.mana_spent,
        is_critical: outcome.is_critical,
        damage_prevented: outcome.damage_prevented,
        life_gained: outcome.life_gained,
        mana_gained: outcome.mana_gained,
    });
    for eff in &outcome.effects_applied {
        commands.trigger(EffectApplied {
            target: ev.target,
            effect_id: eff.id.clone(),
            total_duration: eff.total_duration,
            stacks: eff.stacks,
        });
    }
    if outcome.is_killing_blow || !target_attrs.0.is_alive() {
        commands.trigger(EntityDied {
            target: ev.target,
            killer: Some(ev.caster),
        });
    }

    // Task 7 — "the fireball moment": timeline-target skill conditions (stripped from
    // `skill_for_resolve` above, so stat_core never resolved them as an inline packet). Evaluate
    // each against the widened `HitOutcome` (`eval_condition_obelisk_side`); a match executes B's
    // own timeline SPATIALLY at the hit position, one trigger-depth deeper. Must run BEFORE the
    // `outcome.triggered` worklist below, which moves that field out of `outcome` — this block
    // needs the whole struct (`primary_packet` / `attacker_before` / `defender_before` /
    // `is_killing_blow`) still intact. `execute_skill_timeline` itself enforces
    // `MAX_TRIGGER_DEPTH` (warns + drops at the cap) — not re-checked here, one source of truth.
    for cond in &timeline_targets {
        if eval_condition_obelisk_side(&cond.condition, &outcome) {
            execute_skill_timeline(
                &mut commands,
                ev.caster,
                &cond.trigger_skill,
                ExecPayload {
                    position: ev.position,
                    // `HitConfirmed` carries no direction/facing. A neutral default: Task 9's
                    // authored anchors make direction mostly irrelevant for CastPoint-style
                    // windows (the common shape for a triggered explosion); a future task that
                    // triggers a DIRECTIONAL timeline (a projectile, a cone) off a hit will need
                    // a real facing and should extend `HitConfirmed` rather than guess here.
                    direction: Vec3::X,
                    target: Some(ev.target),
                    charge: ev.charge,
                    depth: ev.depth.saturating_add(1),
                },
            );
        }
    }

    // Skill-condition damage triggers (e.g. an `on_crit` condition naming a `trigger_skill`):
    // obelisk ALREADY resolved each of these packets inline in `resolve_one_hit` and the defender
    // already absorbed the damage. Surface each as its OWN secondary cast — a distinct
    // `TriggerFired` + `DamageResolved` — instead of folding it into the primary `total_damage`.
    // These are deliberately NOT added to the effect-trigger worklist below and are NEVER
    // re-resolved (no `resolve_damage_with_rng`): re-firing would double-damage the defender and
    // re-draw the seeded RNG. `effect_id` is empty because a skill condition has no originating
    // effect (this distinguishes it from the effect-cascade `TriggerFired`).
    for th in &outcome.triggered_skill_hits {
        commands.trigger(TriggerFired {
            source: ev.caster,
            target: ev.target,
            skill_id: th.secondary_skill_id.clone(),
            effect_id: String::new(),
        });
        commands.trigger(DamageResolved {
            caster: ev.caster,
            target: ev.target,
            skill_id: th.secondary_skill_id.clone(),
            total_damage: th.total_damage,
            is_killing_blow: th.is_killing_blow,
            // The defender was already mutated by `resolve_one_hit` (all packets, including this
            // triggered one), so its current life already reflects this secondary hit.
            life_after: target_attrs.0.current_life,
            mana_spent: 0.0,
            is_critical: th.is_critical,
            damage_prevented: th.damage_prevented,
            life_gained: th.life_gained,
            mana_gained: th.mana_gained,
        });
        if th.is_killing_blow {
            commands.trigger(EntityDied {
                target: ev.target,
                killer: Some(ev.caster),
            });
        }
    }

    // Process effect-condition triggers (OnApply/OnMaxStacks/OnConsume, etc.) produced by this
    // hit. Every trigger is made observable via `TriggerFired` (never silently dropped). On-hit
    // triggered SKILLS that exist in the registry are additionally auto-fired through the same
    // deterministic resolve path, against the HIT TARGET — the natural default routing for on-hit
    // triggers. Bespoke routing (on_kill / splash / target reselection) is intentionally left to
    // the game via `TriggerFired` observability.
    //
    // Determinism: every triggered resolution draws from the same seeded `rng.0`, in worklist
    // order. The worklist is depth-guarded by MAX_TRIGGER_RESOLUTIONS so a (theoretical)
    // re-entrant cascade can never run unbounded; `resolve_damage_with_rng` returns only a
    // CombatResult (no new triggers), so the list does not actually grow from auto-firing.
    let caster_source_id = caster_attrs.0.id.clone();
    let mut worklist: std::collections::VecDeque<stat_core::TriggeredEffect> =
        outcome.triggered.into_iter().collect();
    let mut resolved = 0usize;
    while let Some(te) = worklist.pop_front() {
        if resolved >= MAX_TRIGGER_RESOLUTIONS {
            break;
        }
        resolved += 1;

        // Always observable, even if the triggered skill isn't in the registry / isn't a damage
        // skill (self-buff, deferred routing, etc.).
        commands.trigger(TriggerFired {
            source: ev.caster,
            target: ev.target,
            skill_id: te.skill_id.clone(),
            effect_id: te.effect_id.clone(),
        });

        // Auto-fire on-hit triggered damage skills against the hit target.
        let Some(triggered_skill) = registry.0.get(&te.skill_id) else {
            continue;
        };
        let packet = te.to_damage_packet(triggered_skill, &mut rng.0, &caster_source_id);
        // Reuse the already-borrowed `target_attrs` (do NOT re-borrow the query).
        let (new_def, result) = resolve_damage_with_rng(&target_attrs.0, &packet, &mut rng.0);
        target_attrs.0 = new_def;

        commands.trigger(DamageResolved {
            caster: ev.caster,
            target: ev.target,
            skill_id: te.skill_id.clone(),
            total_damage: result.total_damage,
            is_killing_blow: result.is_killing_blow,
            life_after: target_attrs.0.current_life,
            mana_spent: 0.0,
            // Breakdown from the real triggered-hit CombatResult / packet (not fabricated).
            // `to_damage_packet` builds a non-crit packet, so `is_critical` is the packet's value.
            is_critical: packet.is_critical,
            damage_prevented: combat_result_prevented(&result),
            life_gained: combat_result_life_gained(&result),
            mana_gained: combat_result_mana_gained(&result),
        });
        if result.is_killing_blow {
            commands.trigger(EntityDied {
                target: ev.target,
                killer: Some(ev.caster),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CombatRng;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;
    use stat_core::{Effect, EffectCondition, EffectTrigger, StatBlock};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// End-to-end through the observer: a caster carries an `OnConsume` self-effect that the
    /// cast skill consumes; the triggered skill ("static_discharge") IS in the registry, so it
    /// must (a) emit `TriggerFired` and (b) auto-fire as a triggered damage hit against the
    /// target — without panicking or looping (the worklist is depth-guarded).
    #[test]
    fn on_hit_confirmed_emits_trigger_fired_and_fires_triggered_skill() {
        stat_core::config::ensure_constants_initialized();

        let toml = r#"
[[skills]]
id = "discharge_strike"
name = "Discharge Strike"
tags = ["attack", "physical", "melee"]
targeting = "single_enemy"
delivery = "melee"
mana_cost = 0.0
consumes_self_effect = [{ id = "charged" }]
[skills.damage]
base_damages = [{ type = "physical", min = 10.0, max = 10.0 }]

[[skills]]
id = "static_discharge"
name = "Static Discharge"
tags = ["spell", "lightning"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0
[skills.damage]
base_damages = [{ type = "lightning", min = 25.0, max = 25.0 }]
"#;
        let registry: HashMap<String, stat_core::Skill> =
            stat_core::config::parse_skills(toml).unwrap();

        let mut app = App::new();
        app.add_observer(on_hit_confirmed);
        app.insert_resource(SkillRegistry(registry));
        app.insert_resource(CombatRng(ChaCha8Rng::seed_from_u64(5)));
        app.init_resource::<CastTimelineHandles>();

        // Recorder for TriggerFired (the EventRecorder in testkit doesn't cover this event).
        let fired: Arc<Mutex<Vec<TriggerFired>>> = Arc::new(Mutex::new(Vec::new()));
        let fired_clone = fired.clone();
        app.add_observer(move |e: On<TriggerFired>| {
            fired_clone.lock().unwrap().push(e.event().clone());
        });

        // Caster carries a "charged" self-effect with an OnConsume -> static_discharge trigger.
        let mut caster_block = StatBlock::with_id("player");
        caster_block.max_mana.base = 100.0;
        caster_block.current_mana = 100.0;
        let mut charged = Effect::stat_buff("charged", "Charged", 999.0, vec![]);
        charged.conditions = vec![EffectCondition {
            trigger_skill: "static_discharge".to_string(),
            trigger: EffectTrigger::OnConsume,
        }];
        caster_block.add_effect(charged);

        let mut target_block = StatBlock::with_id("dummy");
        target_block.max_life.base = 200.0;
        target_block.current_life = 200.0;

        let caster = app.world_mut().spawn(Attributes(caster_block)).id();
        let target = app.world_mut().spawn(Attributes(target_block)).id();

        let life_before = app
            .world()
            .entity(target)
            .get::<Attributes>()
            .unwrap()
            .0
            .current_life;

        app.world_mut().trigger(HitConfirmed {
            caster,
            target,
            skill_id: "discharge_strike".to_string(),
            window_id: "w".to_string(),
            charge: None,
            position: Vec3::ZERO,
            depth: 0,
            hop: 0,
        });
        // Flush queued commands so the observer's `commands.trigger(...)` events (TriggerFired /
        // DamageResolved) actually dispatch and the target's mutated Attributes are applied.
        app.world_mut().flush();

        let fired = fired.lock().unwrap();
        assert!(
            fired
                .iter()
                .any(|t| t.skill_id == "static_discharge" && t.effect_id == "charged"),
            "TriggerFired for the OnConsume trigger must be emitted, got {:?}",
            *fired
        );

        // The triggered skill is in the registry, so it auto-fired a lightning hit against the
        // target — life must have dropped further than the primary physical hit alone.
        let life_after = app
            .world()
            .entity(target)
            .get::<Attributes>()
            .unwrap()
            .0
            .current_life;
        assert!(
            life_after < life_before - 10.0,
            "triggered static_discharge should have dealt additional damage: {} -> {}",
            life_before,
            life_after
        );
    }
}
