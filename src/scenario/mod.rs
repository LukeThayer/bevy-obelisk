use bevy::prelude::*;
use stat_core::StatBlock;

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
    pub hurtbox_radius: f32,
}

impl ActorSpec {
    pub fn stat_block(&self) -> StatBlock {
        let mut b = StatBlock::with_id(&self.id);
        b.max_life.base = self.life;
        b.current_life = self.life;
        b.max_mana.base = self.mana;
        b.current_mana = self.mana;
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
            hurtbox_radius: 0.6,
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
    pub fn with_drop_table(mut self, table: &str) -> Self {
        if let Some(a) = self.actors.last_mut() {
            a.drop_table = Some(table.into());
        }
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
    }
    {
        let mut c = app.world_mut().commands();
        insert_hurtbox(&mut c, e, a.hurtbox_radius, pos);
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
}
