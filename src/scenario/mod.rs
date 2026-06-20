use bevy::prelude::*;
use stat_core::{StatBlock, StatType};

// Model + library + the shared step-applier are ALWAYS compiled (the playground and
// screenshot examples consume the scenario DATA without the test-support runner).
pub mod library;
// The headless golden runner + trace recorder use `testkit` + the buffered NetEvent reader,
// so they're gated behind `test-support`.
#[cfg(feature = "test-support")]
pub mod run;
#[cfg(feature = "test-support")]
pub mod trace;

/// How a scripted cast is aimed.
#[derive(Clone, Debug)]
pub enum Aim {
    Entity(String),
    Point(Vec3),
    Dir(Vec3),
}

/// A scripted action at a tick. Ids are stable string ids (resolved at run time).
#[derive(Clone, Debug)]
pub enum Action {
    Cast {
        caster: String,
        skill: String,
        aim: Aim,
    },
    ApplyEffect {
        target: String,
        effect: String,
    },
    SetMana {
        id: String,
        mana: f64,
    },
    Move {
        id: String,
        to: Vec3,
    },
    Despawn {
        id: String,
    },
    /// Spawn a non-combatant static obstacle collider at `pos` (for LOS scenarios).
    Obstacle {
        pos: Vec3,
        radius: f32,
    },
    /// Interrupt the entity's in-flight cast (cancels any `ActiveCast` / `PendingCast`) via the
    /// public `interrupt_cast` verb. No-op if the entity isn't casting.
    Interrupt {
        id: String,
    },
    /// Apply custom `(StatType, value)` modifiers to the entity mid-scenario, flowed through
    /// obelisk's REAL stat-rebuild path (the same `ScenarioStatSource` mechanism used at spawn in
    /// `ActorSpec::stat_block()`). `Vec<(StatType, f64)>` is `Clone`, so `Action` stays `Clone`.
    ApplyStatSources {
        id: String,
        stats: Vec<(StatType, f64)>,
    },
}

#[derive(Clone, Debug)]
pub struct ScriptStep {
    pub at_tick: usize,
    pub action: Action,
}

/// One actor present at scenario start.
#[derive(Clone, Debug)]
pub struct ActorSpec {
    pub id: String,
    pub faction: crate::core::components::Faction,
    pub life: f64,
    pub mana: f64,
    pub pos: Vec3,
    pub skills: Vec<String>,
    pub drop_table: Option<String>,
    /// Loot roll parameters for this actor's drop table (threaded into the loot system via a
    /// `DropRollParams` component). `None` falls back to the loot defaults (level 1, mults 1.0).
    pub level: Option<u32>,
    pub rarity_mult: Option<f64>,
    pub quantity_mult: Option<f64>,
    pub hurtbox_radius: f32,
    /// Custom stat modifiers ((StatType, value) pairs) flowed through obelisk's REAL
    /// stat-rebuild path in `stat_block()`. Empty by default (no-op for existing scenarios).
    pub stats: Vec<(StatType, f64)>,
    /// Effect ids self-applied to this actor at spawn (buffs). Empty by default.
    pub self_effects: Vec<String>,
}

/// A `StatSource` that feeds a scenario actor's custom `(StatType, value)` modifiers through
/// obelisk's real `StatBlock::rebuild_from_sources` path (so they aggregate exactly as gear /
/// skill-tree sources do). Private to the scenario harness.
struct ScenarioStatSource {
    mods: Vec<(StatType, f64)>,
}

impl stat_core::source::StatSource for ScenarioStatSource {
    fn id(&self) -> &str {
        "scenario_stats"
    }
    fn apply(&self, stats: &mut stat_core::stat_block::StatAccumulator) {
        for (stat, value) in &self.mods {
            stats.apply_stat_type(stat.clone(), *value);
        }
    }
}

impl ActorSpec {
    pub fn stat_block(&self) -> StatBlock {
        let mut b = StatBlock::with_id(&self.id);
        b.max_life.base = self.life;
        b.current_life = self.life;
        b.max_mana.base = self.mana;
        b.current_mana = self.mana;
        // Flow any custom stat modifiers through obelisk's REAL rebuild path. `rebuild()` only
        // resets the per-stat flat/increased/more layers (via `reset_to_base()`) and never touches
        // `*.base` or `current_life`/`current_mana`, so the life/mana bases set above survive.
        if !self.stats.is_empty() {
            let source = ScenarioStatSource {
                mods: self.stats.clone(),
            };
            b.rebuild_from_sources(&[Box::new(source)]);
        }
        b
    }
}

/// A declarative, deterministic scenario driven through the public integration path.
#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: String,
    pub seed: u64,
    pub ticks: usize,
    pub actors: Vec<ActorSpec>,
    pub script: Vec<ScriptStep>,
    /// Record the NetEvent egress into the trace (default false — only netcode scenarios).
    pub record_net: bool,
    /// Skill ids whose `.cast.ron` must be loaded (resolved from `assets/skills/<id>.cast.ron`).
    pub cast_assets: Vec<String>,
    /// Human-readable, self-documenting "what this scenario validates" blurb. Presentation-only
    /// metadata (shown in the playground info panel); never recorded into a `Trace`, so it cannot
    /// perturb any golden.
    pub description: String,
    /// Drop-table fixtures to load: `(table name, fixture file path)`. Empty by default; when
    /// empty, `run_scenario` falls back to the legacy single `goblin.toml` load so existing
    /// loot scenarios are byte-identical. Lets scenarios reach other / nested drop tables.
    pub drop_table_fixtures: Vec<(String, String)>,
}

impl Scenario {
    pub fn new(name: &str, seed: u64, ticks: usize) -> Self {
        Self {
            name: name.into(),
            seed,
            ticks,
            actors: vec![],
            script: vec![],
            record_net: false,
            cast_assets: vec![],
            description: String::new(),
            drop_table_fixtures: vec![],
        }
    }
    pub fn actor(
        mut self,
        id: &str,
        faction: crate::core::components::Faction,
        life: f64,
        mana: f64,
        pos: Vec3,
    ) -> Self {
        self.actors.push(ActorSpec {
            id: id.into(),
            faction,
            life,
            mana,
            pos,
            skills: vec![],
            drop_table: None,
            level: None,
            rarity_mult: None,
            quantity_mult: None,
            hurtbox_radius: 0.6,
            stats: vec![],
            self_effects: vec![],
        });
        self
    }
    /// Modify the last-added actor.
    pub fn with_skill(mut self, skill: &str) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.skills.push(skill.into());
        }
        self
    }
    /// Add a custom stat modifier to the last-added actor. Flowed through obelisk's real
    /// stat-rebuild path in `ActorSpec::stat_block()`.
    pub fn with_stat(mut self, stat: StatType, value: f64) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.stats.push((stat, value));
        }
        self
    }
    /// Self-apply an effect (by id) to the last-added actor at spawn (a starting buff).
    pub fn with_self_effect(mut self, effect: &str) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.self_effects.push(effect.into());
        }
        self
    }
    pub fn with_drop_table(mut self, table: &str) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.drop_table = Some(table.into());
        }
        self
    }
    /// Set the drop-table area level for the last-added actor's loot roll (`None` ⇒ default 1).
    pub fn with_level(mut self, level: u32) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.level = Some(level);
        }
        self
    }
    /// Set the rarity multiplier for the last-added actor's loot roll (`None` ⇒ default 1.0).
    pub fn with_rarity_mult(mut self, mult: f64) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.rarity_mult = Some(mult);
        }
        self
    }
    /// Set the quantity multiplier for the last-added actor's loot roll (`None` ⇒ default 1.0).
    pub fn with_quantity_mult(mut self, mult: f64) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.quantity_mult = Some(mult);
        }
        self
    }
    /// Register a drop-table fixture file (table name ⇒ fixture path) loaded by `run_scenario`.
    /// When none are registered the runner falls back to the default `goblin.toml` loading, so
    /// existing loot scenarios are unaffected.
    pub fn with_drop_table_fixture(mut self, name: &str, path: &str) -> Self {
        self.drop_table_fixtures.push((name.into(), path.into()));
        self
    }
    pub fn at(mut self, tick: usize, action: Action) -> Self {
        self.script.push(ScriptStep {
            at_tick: tick,
            action,
        });
        self
    }
    pub fn cast_asset(mut self, skill: &str) -> Self {
        self.cast_assets.push(skill.into());
        self
    }
    pub fn recording_net(mut self) -> Self {
        self.record_net = true;
        self
    }
    /// Attach a one-line "what this scenario validates" description (presentation-only metadata,
    /// never recorded into a `Trace`).
    pub fn describe(mut self, description: &str) -> Self {
        self.description = description.into();
        self
    }
}

use crate::prelude::*;

/// Spawn one actor via the public verbs (ObeliskId == StatBlock.id) + a hurtbox. Returns its entity.
pub fn spawn_actor(app: &mut App, a: &ActorSpec) -> Entity {
    let pos = a.pos;
    let e = app.world_mut().spawn_empty().id();
    app.world_mut()
        .commands()
        .entity(e)
        .make_combatant(a.stat_block());
    app.world_mut()
        .entity_mut(e)
        .insert((a.faction, Transform::from_translation(pos)));
    for s in &a.skills {
        app.world_mut().commands().entity(e).grant_skill(s.clone());
    }
    if let Some(tbl) = &a.drop_table {
        app.world_mut()
            .entity_mut(e)
            .insert(crate::loot::DropTableId(tbl.clone()));
        // Only attach roll params when at least one is overridden; absent ⇒ the loot system falls
        // back to its defaults (rarity 1.0 / quantity 1.0 / level 1), keeping existing goldens
        // byte-identical (the loot_on_death scenario sets none).
        if a.level.is_some() || a.rarity_mult.is_some() || a.quantity_mult.is_some() {
            let d = crate::loot::DropRollParams::default();
            app.world_mut()
                .entity_mut(e)
                .insert(crate::loot::DropRollParams {
                    rarity_mult: a.rarity_mult.unwrap_or(d.rarity_mult),
                    quantity_mult: a.quantity_mult.unwrap_or(d.quantity_mult),
                    level: a.level.unwrap_or(d.level),
                });
        }
    }
    {
        let mut c = app.world_mut().commands();
        insert_hurtbox(&mut c, e, a.hurtbox_radius, pos);
    }
    // Self-apply any starting effects (buffs), sourced from the actor itself. Mirrors how
    // `Action::ApplyEffect` is handled; the effect comes from the initialized EffectRegistry.
    for effect in &a.self_effects {
        app.world_mut()
            .commands()
            .entity(e)
            .apply_obelisk_effect(effect.clone());
    }
    e
}

/// Apply one scripted action against the running app (resolves ids via ObeliskEntityIndex).
pub fn apply_action(app: &mut App, action: &Action) {
    let id_of = |app: &App, id: &str| app.world().resource::<ObeliskEntityIndex>().entity(id);
    match action {
        Action::Cast { caster, skill, aim } => {
            if let Some(c) = id_of(app, caster) {
                match aim {
                    Aim::Entity(t) => {
                        if let Some(te) = id_of(app, t) {
                            app.world_mut()
                                .commands()
                                .entity(c)
                                .cast_skill_at(skill.clone(), te);
                        }
                    }
                    Aim::Point(p) => {
                        app.world_mut()
                            .commands()
                            .entity(c)
                            .cast_skill_at_point(skill.clone(), *p);
                    }
                    Aim::Dir(d) => {
                        if let Ok(dir) = Dir3::new(*d) {
                            app.world_mut()
                                .commands()
                                .entity(c)
                                .cast_skill_dir(skill.clone(), dir);
                        }
                    }
                }
            }
        }
        Action::ApplyEffect { target, effect } => {
            if let Some(t) = id_of(app, target) {
                app.world_mut()
                    .commands()
                    .entity(t)
                    .apply_obelisk_effect(effect.clone());
            }
        }
        Action::SetMana { id, mana } => {
            if let Some(e) = id_of(app, id) {
                if let Some(mut a) = app.world_mut().entity_mut(e).get_mut::<Attributes>() {
                    a.0.current_mana = *mana;
                }
            }
        }
        Action::Move { id, to } => {
            if let Some(e) = id_of(app, id) {
                if let Some(mut tf) = app.world_mut().entity_mut(e).get_mut::<Transform>() {
                    tf.translation = *to;
                }
            }
        }
        Action::Despawn { id } => {
            if let Some(e) = id_of(app, id) {
                app.world_mut().entity_mut(e).despawn();
            }
        }
        Action::Obstacle { pos, radius } => {
            app.world_mut().spawn((
                avian3d::prelude::RigidBody::Static,
                avian3d::prelude::Collider::sphere(*radius),
                Transform::from_translation(*pos),
            ));
        }
        Action::Interrupt { id } => {
            if let Some(e) = id_of(app, id) {
                app.world_mut().commands().entity(e).interrupt_cast();
            }
        }
        Action::ApplyStatSources { id, stats } => {
            if let Some(e) = id_of(app, id) {
                // Reuse the H1 stat mechanism: a `ScenarioStatSource` flowed through obelisk's
                // real `rebuild_from_sources` path (here via the public `apply_stat_sources` verb).
                let source = ScenarioStatSource {
                    mods: stats.clone(),
                };
                app.world_mut()
                    .commands()
                    .entity(e)
                    .apply_stat_sources(vec![Box::new(source)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::components::Faction;
    #[test]
    fn builder_assembles_a_scenario() {
        let s = Scenario::new("t", 1, 100)
            .cast_asset("firebolt")
            .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
            .with_skill("firebolt")
            .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
            .at(
                1,
                Action::Cast {
                    caster: "player".into(),
                    skill: "firebolt".into(),
                    aim: Aim::Entity("dummy".into()),
                },
            );
        assert_eq!(s.actors.len(), 2);
        assert_eq!(s.actors[0].skills, vec!["firebolt".to_string()]);
        assert_eq!(s.script.len(), 1);
        assert!(!s.record_net);
    }

    #[test]
    fn with_stat_flows_through_rebuild_without_wiping_bases() {
        // An actor with a custom stat modifier: the rebuilt StatBlock must reflect the modifier
        // AND keep its configured base life (rebuild_from_sources must not wipe the bases).
        let s = Scenario::new("stat", 1, 1)
            .actor("buffed", Faction::Player, 80.0, 40.0, Vec3::ZERO)
            .with_stat(StatType::FireResistance, 30.0);
        let actor = &s.actors[0];
        assert_eq!(actor.stats, vec![(StatType::FireResistance, 30.0)]);

        let block = actor.stat_block();
        // The custom modifier survived the rebuild (FireResistance is a flat add, no /100).
        assert!(
            (block.fire_resistance.compute() - 30.0).abs() < 1e-9,
            "fire resistance should reflect the +30 modifier (got {})",
            block.fire_resistance.compute()
        );
        // The base life set before the rebuild was NOT wiped.
        assert!(
            (block.computed_max_life() - 80.0).abs() < 1e-9,
            "base max life must survive rebuild_from_sources (got {})",
            block.computed_max_life()
        );
        assert!(
            (block.current_life - 80.0).abs() < 1e-9,
            "current life must survive rebuild (got {})",
            block.current_life
        );
        assert!(
            (block.computed_max_mana() - 40.0).abs() < 1e-9,
            "base max mana must survive rebuild (got {})",
            block.computed_max_mana()
        );
    }

    #[test]
    fn with_self_effect_records_starting_buffs() {
        let s = Scenario::new("buff", 1, 1)
            .actor("hero", Faction::Player, 100.0, 0.0, Vec3::ZERO)
            .with_self_effect("burn");
        assert_eq!(s.actors[0].self_effects, vec!["burn".to_string()]);
    }

    #[test]
    fn loot_builders_thread_roll_params_onto_the_actor() {
        let s = Scenario::new("loot_params", 1, 1)
            .actor("goblin", Faction::Enemy, 10.0, 0.0, Vec3::ZERO)
            .with_drop_table("goblin")
            .with_level(42)
            .with_rarity_mult(2.5)
            .with_quantity_mult(3.0)
            .with_drop_table_fixture("rare", "tests/fixtures/loot/rare.toml");
        let a = &s.actors[0];
        assert_eq!(a.level, Some(42));
        assert_eq!(a.rarity_mult, Some(2.5));
        assert_eq!(a.quantity_mult, Some(3.0));
        assert_eq!(
            s.drop_table_fixtures,
            vec![(
                "rare".to_string(),
                "tests/fixtures/loot/rare.toml".to_string()
            )]
        );
    }

    /// H3: `Action::ApplyStatSources` flows `(StatType, value)` mods through obelisk's REAL
    /// rebuild path mid-run; the computed stat on the resolved entity reflects the change.
    /// Driven directly on a harness app (no cast timeline needed) so we can read the StatBlock back.
    #[test]
    fn apply_stat_sources_action_changes_a_computed_stat_mid_run() {
        use crate::prelude::*;
        use crate::testkit::ObeliskTestApp;

        let mut t = ObeliskTestApp::new(1);
        let spec = ActorSpec {
            id: "target".into(),
            faction: Faction::Enemy,
            life: 100.0,
            mana: 0.0,
            pos: Vec3::ZERO,
            skills: vec![],
            drop_table: None,
            level: None,
            rarity_mult: None,
            quantity_mult: None,
            hurtbox_radius: 0.6,
            stats: vec![],
            self_effects: vec![],
        };
        let e = spawn_actor(&mut t.app, &spec);
        t.app.update(); // flush spawn + sync ObeliskEntityIndex

        // Baseline: no fire resistance.
        let before = t
            .app
            .world()
            .entity(e)
            .get::<Attributes>()
            .unwrap()
            .0
            .fire_resistance
            .compute();
        assert!(
            before.abs() < 1e-9,
            "baseline fire resistance (got {before})"
        );

        // Apply +30 FireResistance mid-run via the new action (resolves id via ObeliskEntityIndex).
        apply_action(
            &mut t.app,
            &Action::ApplyStatSources {
                id: "target".into(),
                stats: vec![(StatType::FireResistance, 30.0)],
            },
        );
        t.app.update(); // flush the queued apply_stat_sources command

        let after = t
            .app
            .world()
            .entity(e)
            .get::<Attributes>()
            .unwrap()
            .0
            .fire_resistance
            .compute();
        assert!(
            (after - 30.0).abs() < 1e-9,
            "fire resistance should reflect the +30 modifier applied mid-run (got {after})"
        );
    }

    /// H3: `Action::Interrupt` cancels an in-flight cast during windup, so NO hit confirms / damage
    /// resolves and the `ActiveCast` is gone. Driven through `run_scenario` (loads the firebolt
    /// cast timeline) and asserted off the recorded trace.
    #[cfg(feature = "test-support")]
    #[test]
    fn interrupt_action_cancels_an_in_flight_cast() {
        use crate::scenario::run::run_scenario;

        // Firebolt: CastBegan ~tick 2, Windup->Active ~tick 19, HitConfirmed ~tick 21 (see golden).
        // Cast at tick 1, interrupt at tick 5 (mid-windup, before the hit window opens).
        let s = Scenario::new("interrupt_test", 42, 600)
            .cast_asset("firebolt")
            .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
            .with_skill("firebolt")
            .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
            .at(
                1,
                Action::Cast {
                    caster: "player".into(),
                    skill: "firebolt".into(),
                    aim: Aim::Entity("dummy".into()),
                },
            )
            .at(
                5,
                Action::Interrupt {
                    id: "player".into(),
                },
            );

        let trace = run_scenario(&s).to_text();
        assert!(
            trace.contains("CastBegan"),
            "the cast should have begun before being interrupted:\n{trace}"
        );
        assert!(
            !trace.contains("HitConfirmed"),
            "interrupted cast must produce NO HitConfirmed:\n{trace}"
        );
        assert!(
            !trace.contains("\tDamage\t") && !trace.contains("Damage\tcaster"),
            "interrupted cast must produce NO Damage:\n{trace}"
        );
        assert!(
            !trace.contains("Died"),
            "no kill should occur after interruption:\n{trace}"
        );
    }
}
