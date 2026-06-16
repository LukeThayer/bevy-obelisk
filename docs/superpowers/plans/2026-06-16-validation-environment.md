# Validation Environment & Integrated Regression Harness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the agent (and humans) an integrated way to validate obelisk-bevy: a scenario library driven through the public integration path that produces deterministic golden event-traces (the regression backbone), a real gameplay debug-viz layer, a rewritten windowed playground, and headless screenshots.

**Architecture:** One `Scenario` library feeds three surfaces. The backbone runs scenarios headlessly through the public sim stack and records every gameplay event into a `Trace` that's golden-diffed in `cargo test`. The visual layer (`ObeliskDebugVizPlugin`, present-side) renders combat (gizmos/mesh/HUD/log) and is shared by a rewritten windowed playground and a headless screenshot renderer. Backbone first (reliable, agent-facing); visuals + screenshots last.

**Tech Stack:** Rust 2021, Bevy 0.17.3, Avian3d 0.4.1, the obelisk libs, `serde`/`ron` (already deps), `bevy_ui` + `Gizmos` for viz (no new dep).

---

## Ground truth (verified)

- **Events** (`src/events.rs`, all `#[derive(Event)]`, observe via `On<E>`): `CastBegan{caster,skill_id,total_duration:f32}`, `CastRejected{caster,skill_id,reason:CastRejectReason}`, `CastPhaseChanged{caster,skill_id,from:SkillPhase,to:SkillPhase,elapsed:f32}`, `HitWindowOpened{caster,skill_id,window_id,hitbox}`, `HitConfirmed{caster,target,skill_id,window_id}`, `DamageResolved{caster,target,skill_id,total_damage:f64,is_killing_blow:bool,life_after:f64,mana_spent:f64}`, `EffectApplied{target,effect_id,total_duration:f64,stacks:u32}`, `EffectExpired{target,effect_id}`, `DotTicked{target,effect_id,dot_damage:f64,life_remaining:f64}`, `EntityDied{target,killer:Option<Entity>}`, `TriggerFired{source,target,skill_id,effect_id}`, `CueEvent{cue_id,source,position:Vec3,kind:CueKind}`, `CooldownStarted{caster,skill_id,duration:f32}`, `CooldownReady{caster,skill_id}`, `LootDropped{source,drops:Vec<tables_core::Drop>}`. `NetEvent` (buffered `Message`, String ids).
- `ObeliskSimPlugin` already composes assets+spatial+core+combat+vfx(cues)+net+loot — all headless. `ObeliskCorePlugin` registers `ObeliskEntityIndex` (`id(e)->Option<&str>`).
- Headless recipe (lib.rs API notes): `MinimalPlugins + AssetPlugin{file_path:"."} + bevy::mesh::MeshPlugin + bevy::scene::ScenePlugin + ObeliskSimPlugin`, `TimeUpdateStrategy::ManualDuration(1/60s)`, `Time::<Fixed>::from_hz(60)`, `app.finish(); app.cleanup();` before the first `update()`. Static collider visible to `SpatialQuery` from the 2nd update.
- `ObeliskConfigExt`: `add_obelisk_config_constants_default()`, `add_obelisk_effects(&Path)`, `add_obelisk_skills(SkillSource::Dir)`, `seed_combat_rng(u64)`. `ObeliskCommandsExt`: `make_combatant(StatBlock)` (derives `ObeliskId` from `StatBlock.id`), `cast_skill_at/_at_point/_dir`, `apply_obelisk_effect`, `grant_skill`, `apply_stat_sources`. `insert_hurtbox(&mut Commands, owner, radius, pos)`.
- Fixtures: `tests/fixtures/skills/{firebolt,cleave}.toml`, `tests/fixtures/effects/burn.toml`, `assets/skills/{firebolt,cleave}.cast.ron`. `Hitbox{caster,skill_id,window_id,filter,mode,shape,aim,age,…}`, `Hurtbox{owner}`, `ActiveCast{skill_id,phase,…}`, `Projectile{velocity}`.
- `Cargo.toml` features: `default=["present"]`, `present`, `test-support`, `debug-gizmos=["present"]`. Tests: `cargo test --features test-support --lib --tests` (~53 passing).

---

## File structure

| File | Responsibility |
|---|---|
| `src/scenario/mod.rs` | `Scenario`/`ActorSpec`/`Action`/`Aim` model + fluent builder + shared `spawn_actor`/`apply_action` helpers (ALWAYS compiled — public API only). |
| `src/scenario/trace.rs` | `Trace`/`TraceLine`/`to_text`, `TraceRecorderPlugin` (tick counter + per-event observers, stable ids, `{:.3}` floats) — `test-support`. |
| `src/scenario/run.rs` | `run_scenario(&Scenario) -> Trace` (builds the public headless app, plays the script) — `test-support`. |
| `src/scenario/library.rs` | `feature_matrix() -> Vec<Scenario>` + `scenario(name)` (ALWAYS compiled — examples consume the data). |
| `src/lib.rs` | `pub mod scenario;` (always); only `scenario::{trace,run}` are `test-support`-gated. |
| `tests/golden.rs` | golden-diff harness over `feature_matrix()`; `UPDATE_GOLDEN=1` regenerates. |
| `tests/golden/<name>.trace` | committed goldens. |
| `src/present/debug_viz.rs` | `ObeliskDebugVizPlugin` (gizmos/mesh/reactions + HUD/log), `present` feature; gizmo parts under `debug-gizmos`. |
| `examples/playground.rs` | rewritten: windowed demo (scenario picker + free-cast + viz). |
| `examples/screenshot.rs` | headless render scenario@tick → PNG. |

---

## Phase A — Regression backbone (agent-facing; build this first)

### Task 1: Scenario model + builder

**Files:** Create `src/scenario/mod.rs`; Modify `src/lib.rs`.

- [ ] **Step 1: Write `src/scenario/mod.rs`**
```rust
use bevy::prelude::*;
use stat_core::StatBlock;

// Model + library + the shared step-applier are ALWAYS compiled (the playground and
// screenshot examples consume the scenario DATA without the test-support runner).
pub mod library;
// The headless golden runner + trace recorder use `testkit` + the buffered NetEvent reader,
// so they're gated behind `test-support`.
#[cfg(feature = "test-support")]
pub mod trace;
#[cfg(feature = "test-support")]
pub mod run;

/// How a scripted cast is aimed.
#[derive(Clone, Debug)]
pub enum Aim { Entity(String), Point(Vec3), Dir(Vec3) }

/// A scripted action at a tick. Ids are stable string ids (resolved at run time).
#[derive(Clone, Debug)]
pub enum Action {
    Cast { caster: String, skill: String, aim: Aim },
    ApplyEffect { target: String, effect: String },
    SetMana { id: String, mana: f64 },
    Move { id: String, to: Vec3 },
    Despawn { id: String },
    /// Spawn a non-combatant static obstacle collider at `pos` (for LOS scenarios).
    Obstacle { pos: Vec3, radius: f32 },
}

#[derive(Clone, Debug)]
pub struct ScriptStep { pub at_tick: usize, pub action: Action }

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
        b.max_life.base = self.life; b.current_life = self.life;
        b.max_mana.base = self.mana; b.current_mana = self.mana;
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
        Self { name: name.into(), seed, ticks, actors: vec![], script: vec![], record_net: false, cast_assets: vec![] }
    }
    pub fn actor(mut self, id: &str, faction: crate::core::components::Faction, life: f64, mana: f64, pos: Vec3) -> Self {
        self.actors.push(ActorSpec { id: id.into(), faction, life, mana, pos, skills: vec![], drop_table: None, hurtbox_radius: 0.6 });
        self
    }
    /// Modify the last-added actor.
    pub fn with_skill(mut self, skill: &str) -> Self {
        if let Some(a) = self.actors.last_mut() { a.skills.push(skill.into()); } self
    }
    pub fn with_drop_table(mut self, table: &str) -> Self {
        if let Some(a) = self.actors.last_mut() { a.drop_table = Some(table.into()); } self
    }
    pub fn at(mut self, tick: usize, action: Action) -> Self { self.script.push(ScriptStep { at_tick: tick, action }); self }
    pub fn cast_asset(mut self, skill: &str) -> Self { self.cast_assets.push(skill.into()); self }
    pub fn recording_net(mut self) -> Self { self.record_net = true; self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::components::Faction;
    #[test]
    fn builder_assembles_a_scenario() {
        let s = Scenario::new("t", 1, 100)
            .cast_asset("firebolt")
            .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO).with_skill("firebolt")
            .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0,0.0,2.0))
            .at(1, Action::Cast { caster: "player".into(), skill: "firebolt".into(), aim: Aim::Entity("dummy".into()) });
        assert_eq!(s.actors.len(), 2);
        assert_eq!(s.actors[0].skills, vec!["firebolt".to_string()]);
        assert_eq!(s.script.len(), 1);
        assert!(!s.record_net);
    }
}
```

- [ ] **Step 2:** Add `pub mod scenario;` to `src/lib.rs` (below the doc block — always compiled; only `scenario::{trace,run}` are `test-support`-gated, per the module decls above). Create empty `src/scenario/library.rs` with a placeholder `pub fn feature_matrix() -> Vec<super::Scenario> { vec![] }`, and `src/scenario/{trace.rs,run.rs}` with `// filled in later tasks`.

- [ ] **Step 1b (shared step-applier — used by the runner AND both examples):** Also add to `src/scenario/mod.rs` two `pub` helpers that only touch the public API (so they compile without `test-support`), so `run_scenario`, the playground, and the screenshot renderer all spawn/script identically (DRY):
```rust
use crate::prelude::*;

/// Spawn one actor via the public verbs (ObeliskId == StatBlock.id) + a hurtbox. Returns its entity.
pub fn spawn_actor(app: &mut App, a: &ActorSpec) -> Entity {
    let pos = a.pos;
    let e = app.world_mut().spawn_empty().make_combatant(a.stat_block()).id();
    app.world_mut().entity_mut(e).insert((a.faction, Transform::from_translation(pos)));
    for s in &a.skills { app.world_mut().commands().entity(e).grant_skill(s.clone()); }
    if let Some(tbl) = &a.drop_table { app.world_mut().entity_mut(e).insert(crate::loot::DropTableId(tbl.clone())); }
    { let mut c = app.world_mut().commands(); insert_hurtbox(&mut c, e, a.hurtbox_radius, pos); }
    e
}

/// Apply one scripted action against the running app (resolves ids via ObeliskEntityIndex).
pub fn apply_action(app: &mut App, action: &Action) {
    let id_of = |app: &App, id: &str| app.world().resource::<ObeliskEntityIndex>().entity(id);
    match action {
        Action::Cast { caster, skill, aim } => if let Some(c) = id_of(app, caster) {
            match aim {
                Aim::Entity(t) => if let Some(te) = id_of(app, t) { app.world_mut().commands().entity(c).cast_skill_at(skill.clone(), te); },
                Aim::Point(p) => { app.world_mut().commands().entity(c).cast_skill_at_point(skill.clone(), *p); }
                Aim::Dir(d) => if let Ok(dir) = Dir3::new(*d) { app.world_mut().commands().entity(c).cast_skill_dir(skill.clone(), dir); },
            }
        },
        Action::ApplyEffect { target, effect } => if let Some(t) = id_of(app, target) { app.world_mut().commands().entity(t).apply_obelisk_effect(effect.clone()); },
        Action::SetMana { id, mana } => if let Some(e) = id_of(app, id) { if let Some(mut a) = app.world_mut().entity_mut(e).get_mut::<Attributes>() { a.0.current_mana = *mana; } },
        Action::Move { id, to } => if let Some(e) = id_of(app, id) { if let Some(mut tf) = app.world_mut().entity_mut(e).get_mut::<Transform>() { tf.translation = *to; } },
        Action::Despawn { id } => if let Some(e) = id_of(app, id) { app.world_mut().entity_mut(e).despawn(); },
        Action::Obstacle { pos, radius } => { app.world_mut().spawn((avian3d::prelude::RigidBody::Static, avian3d::prelude::Collider::sphere(*radius), Transform::from_translation(*pos))); }
    }
}
```
(`crate::prelude::*` brings `make_combatant`/`grant_skill`/`apply_obelisk_effect`/`cast_*`/`insert_hurtbox`/`Attributes`/`ObeliskEntityIndex`. These are all public — no `test-support` needed, so the examples can call them.)

- [ ] **Step 3:** `cargo test --features test-support --lib builder_assembles` → after Step 2 the empty submodules break `pub mod trace; …` references — temporarily stub them (the `pub mod` lines in mod.rs reference trace/run/library; the empty files satisfy that). Build, then run the test. Expected PASS.

- [ ] **Step 4: Commit**
```bash
git add src/scenario src/lib.rs
git commit -m "feat(scenario): declarative Scenario model + builder"
```

### Task 2: Trace + recorder

**Files:** Modify `src/scenario/trace.rs`.

- [ ] **Step 1: Write `src/scenario/trace.rs`** — the trace + a plugin that records every gameplay event with stable ids + a tick counter.
```rust
use bevy::prelude::*;
use crate::events::*;
use crate::ids::ObeliskEntityIndex;

#[derive(Clone, Debug, PartialEq)]
pub struct TraceLine { pub tick: usize, pub kind: &'static str, pub detail: String }

/// Ordered, deterministic record of a scenario's observable events.
#[derive(Resource, Default)]
pub struct Trace { pub lines: Vec<TraceLine> }

impl Trace {
    pub fn to_text(&self) -> String {
        self.lines.iter().map(|l| format!("{:>4}\t{}\t{}", l.tick, l.kind, l.detail)).collect::<Vec<_>>().join("\n")
    }
}

/// Current fixed tick (incremented at the start of each FixedUpdate).
#[derive(Resource, Default)]
pub struct TickCounter(pub usize);

fn advance_tick(mut t: ResMut<TickCounter>) { t.0 += 1; }

/// Whether to also record NetEvents (the wire egress). Set per scenario.
#[derive(Resource, Default)]
pub struct RecordNet(pub bool);

fn id(index: &ObeliskEntityIndex, e: Entity) -> String { index.id(e).unwrap_or("?").to_string() }

macro_rules! push { ($trace:expr, $tick:expr, $kind:literal, $detail:expr) => {
    $trace.lines.push(TraceLine { tick: $tick.0, kind: $kind, detail: $detail });
}; }

/// Records every gameplay event into `Trace`. Add to the scenario app AFTER ObeliskSimPlugin.
pub struct TraceRecorderPlugin;

impl Plugin for TraceRecorderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Trace>().init_resource::<TickCounter>().init_resource::<RecordNet>();
        // tick counter runs first in FixedUpdate so event ticks are correct.
        app.add_systems(FixedUpdate, advance_tick.before(crate::ObeliskSet::Validate));

        app.add_observer(|e: On<CastBegan>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "CastBegan", format!("caster={} skill={} dur={:.3}", id(&ix,e.caster), e.skill_id, e.total_duration));
        });
        app.add_observer(|e: On<CastRejected>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "CastRejected", format!("caster={} skill={} reason={:?}", id(&ix,e.caster), e.skill_id, e.reason));
        });
        app.add_observer(|e: On<CastPhaseChanged>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "CastPhase", format!("caster={} skill={} {:?}->{:?}", id(&ix,e.caster), e.skill_id, e.from, e.to));
        });
        app.add_observer(|e: On<HitWindowOpened>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "HitWindow", format!("caster={} skill={} window={}", id(&ix,e.caster), e.skill_id, e.window_id));
        });
        app.add_observer(|e: On<HitConfirmed>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "HitConfirmed", format!("caster={} target={} skill={}", id(&ix,e.caster), id(&ix,e.target), e.skill_id));
        });
        app.add_observer(|e: On<DamageResolved>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "Damage", format!("caster={} target={} skill={} dmg={:.3} kill={} life_after={:.3}", id(&ix,e.caster), id(&ix,e.target), e.skill_id, e.total_damage, e.is_killing_blow, e.life_after));
        });
        app.add_observer(|e: On<EffectApplied>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "EffectApplied", format!("target={} effect={} dur={:.3} stacks={}", id(&ix,e.target), e.effect_id, e.total_duration, e.stacks));
        });
        app.add_observer(|e: On<EffectExpired>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "EffectExpired", format!("target={} effect={}", id(&ix,e.target), e.effect_id));
        });
        app.add_observer(|e: On<DotTicked>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "DotTicked", format!("target={} dmg={:.3} life={:.3}", id(&ix,e.target), e.dot_damage, e.life_remaining));
        });
        app.add_observer(|e: On<EntityDied>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); let k = e.killer.map(|k| id(&ix,k)).unwrap_or("none".into()); push!(tr, t, "Died", format!("target={} killer={}", id(&ix,e.target), k));
        });
        app.add_observer(|e: On<TriggerFired>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "TriggerFired", format!("source={} target={} skill={} effect={}", id(&ix,e.source), id(&ix,e.target), e.skill_id, e.effect_id));
        });
        app.add_observer(|e: On<CueEvent>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "Cue", format!("cue={} source={} kind={:?}", e.cue_id, id(&ix,e.source), e.kind));
        });
        app.add_observer(|e: On<CooldownStarted>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "CooldownStarted", format!("caster={} skill={} dur={:.3}", id(&ix,e.caster), e.skill_id, e.duration));
        });
        app.add_observer(|e: On<CooldownReady>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "CooldownReady", format!("caster={} skill={}", id(&ix,e.caster), e.skill_id));
        });
        app.add_observer(|e: On<LootDropped>, ix: Res<ObeliskEntityIndex>, t: Res<TickCounter>, mut tr: ResMut<Trace>| {
            let e = e.event(); push!(tr, t, "Loot", format!("source={} drops={:?}", id(&ix,e.source), e.drops));
        });
        // NetEvent recording (opt-in): NetEvents already carry String ids.
        app.add_systems(FixedUpdate, record_net.run_if(|r: Res<RecordNet>| r.0).after(crate::ObeliskSet::TickEffects));
    }
}

fn record_net(mut reader: bevy::prelude::MessageReader<crate::net::NetEvent>, t: Res<TickCounter>, mut tr: ResMut<Trace>) {
    for ev in reader.read() { tr.lines.push(TraceLine { tick: t.0, kind: "Net", detail: format!("{:?}", ev) }); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn to_text_is_stable_and_ordered() {
        let tr = Trace { lines: vec![
            TraceLine { tick: 1, kind: "CastBegan", detail: "caster=player skill=firebolt dur=0.600".into() },
            TraceLine { tick: 7, kind: "Damage", detail: "caster=player target=dummy skill=firebolt dmg=20.000 kill=false life_after=5.000".into() },
        ]};
        let txt = tr.to_text();
        assert!(txt.contains("CastBegan"));
        assert!(txt.lines().count() == 2);
        assert!(txt.starts_with("   1\tCastBegan"));
    }
}
```

- [ ] **Step 2:** `cargo test --features test-support --lib trace::` and `cargo build --features test-support`. Expected PASS. **Adapt if needed:** confirm `MessageReader` (the buffered reader, per lib.rs API notes) + that observers can take `Res<ObeliskEntityIndex>` + `ResMut<Trace>` (they can — established by the net mirror observers). If the `macro_rules! push` is awkward, inline the `tr.lines.push(...)` calls.

- [ ] **Step 3: Commit**
```bash
git add src/scenario/trace.rs
git commit -m "feat(scenario): Trace + TraceRecorderPlugin (stable-id, fixed-precision event trace)"
```

### Task 3: Integration runner

**Files:** Modify `src/scenario/run.rs`.

- [ ] **Step 1: Write `src/scenario/run.rs`** — builds the public headless app, plays the script, returns the `Trace`.
```rust
use bevy::prelude::*;
use std::time::Duration;
use crate::prelude::*;
use crate::scenario::trace::{RecordNet, Trace, TraceRecorderPlugin};
use crate::scenario::{Action, Scenario};

/// Run a scenario headlessly through the public integration path; return its event trace.
pub fn run_scenario(scenario: &Scenario) -> Trace {
    // init obelisk globals from fixtures (idempotent / Once-guarded helper).
    crate::testkit::init_test_obelisk();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin { file_path: ".".into(), ..default() })
        .add_plugins(bevy::mesh::MeshPlugin)
        .add_plugins(bevy::scene::ScenePlugin)
        .add_plugins(crate::ObeliskSimPlugin)
        .add_plugins(TraceRecorderPlugin)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(RecordNet(scenario.record_net));
    app.add_obelisk_skills(SkillSource::Dir("tests/fixtures/skills".into()));
    app.seed_combat_rng(scenario.seed);
    app.finish();
    app.cleanup();

    // load referenced cast timelines
    let mut handles = vec![];
    for skill in &scenario.cast_assets {
        let h: Handle<CastTimeline> = app.world().resource::<AssetServer>().load(format!("assets/skills/{skill}.cast.ron"));
        handles.push((skill.clone(), h));
    }
    for _ in 0..3000 {
        app.update();
        if handles.iter().all(|(_, h)| app.world().resource::<Assets<CastTimeline>>().get(h).is_some()) { break; }
    }
    {
        let mut reg = app.world_mut().resource_mut::<CastTimelineHandles>();
        for (skill, h) in handles { reg.0.insert(skill, h); }
    }

    // spawn actors via the shared helper (public make_combatant verb; ObeliskId == StatBlock.id).
    for a in &scenario.actors { crate::scenario::spawn_actor(&mut app, a); }
    app.update(); // flush spawns + register hurtboxes

    // run the fixed-tick loop, applying script steps (keyed by scenario-relative tick) before advancing.
    for step_offset in 0..scenario.ticks {
        let actions: Vec<Action> = scenario.script.iter().filter(|s| s.at_tick == step_offset).map(|s| s.action.clone()).collect();
        for action in actions { crate::scenario::apply_action(&mut app, &action); }
        app.update();
    }

    app.world_mut().remove_resource::<Trace>().unwrap_or_default()
}
```
(`run.rs` imports: `use crate::scenario::{Action, Scenario};` + the trace types + `crate::prelude::*` for `SkillSource`/`CastTimeline`/`CastTimelineHandles`/`ObeliskConfigExt`. `spawn_actor`/`apply_action` come from `crate::scenario`.)

- [ ] **Step 2:** Add a quick test in `run.rs` (uses a hand-built scenario): a player casts firebolt at a dummy → the returned `Trace` is non-empty and contains a `Damage` line targeting `dummy`.
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::components::Faction;
    use crate::scenario::Aim;
    #[test]
    fn runner_produces_a_trace() {
        let s = Scenario::new("smoke", 42, 600).cast_asset("firebolt")
            .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO).with_skill("firebolt")
            .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0,0.0,2.0))
            .at(1, Action::Cast { caster: "player".into(), skill: "firebolt".into(), aim: Aim::Entity("dummy".into()) });
        let trace = run_scenario(&s);
        let txt = trace.to_text();
        assert!(txt.contains("CastBegan"), "trace:\n{txt}");
        assert!(txt.contains("Damage\tcaster=player target=dummy"), "trace:\n{txt}");
        assert!(txt.contains("Died\ttarget=dummy"), "trace:\n{txt}");
    }
}
```

- [ ] **Step 3:** `cargo test --features test-support --lib runner_produces_a_trace -- --nocapture` → PASS. Report the trace. **Debug if needed:** tick alignment (the script's `at_tick` is scenario-relative and applied just before each `app.update()`, so the trace tick the engine records is offset by the spawn-flush update — that's fine, the golden captures whatever the engine records; ensure casts fire and the asset is loaded before the cast tick — the firebolt windup is ~18 ticks so casting at tick 1 with 600 total ticks is plenty). If `make_combatant()` chaining fails, the shared `spawn_actor` already splits the `.id()` then `insert((Faction, Transform))` — confirm that path.

- [ ] **Step 4: Commit**
```bash
git add src/scenario/run.rs
git commit -m "feat(scenario): run_scenario integration runner (public path, scripted ticks)"
```

### Task 4: Golden harness + first scenario (vertical slice)

**Files:** Modify `src/scenario/library.rs`; Create `tests/golden.rs`, `tests/golden/firebolt_kill.trace`.

- [ ] **Step 1: Seed `src/scenario/library.rs`** with the first scenario + the registry:
```rust
use bevy::prelude::Vec3;
use crate::core::components::Faction;
use crate::scenario::{Action, Aim, Scenario};

pub fn firebolt_kill() -> Scenario {
    Scenario::new("firebolt_kill", 42, 600).cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO).with_skill("firebolt")
        .actor("dummy", Faction::Enemy, 25.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(1, Action::Cast { caster: "player".into(), skill: "firebolt".into(), aim: Aim::Entity("dummy".into()) })
}

/// The full regression matrix. (Grows in Task 5.)
pub fn feature_matrix() -> Vec<Scenario> {
    vec![firebolt_kill()]
}
```

- [ ] **Step 2: Write `tests/golden.rs`** — the golden-diff harness:
```rust
#![cfg(feature = "test-support")]
use obelisk_bevy::scenario::{library::feature_matrix, run::run_scenario};
use std::path::PathBuf;

fn golden_path(name: &str) -> PathBuf { PathBuf::from(format!("tests/golden/{name}.trace")) }

#[test]
fn scenarios_match_golden_traces() {
    let update = std::env::var("UPDATE_GOLDEN").is_ok();
    let mut failures = Vec::new();
    for scenario in feature_matrix() {
        let trace = run_scenario(&scenario).to_text();
        let path = golden_path(&scenario.name);
        if update {
            std::fs::write(&path, format!("{trace}\n")).expect("write golden");
            continue;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path:?}; run with UPDATE_GOLDEN=1"));
        if expected.trim_end() != trace.trim_end() {
            failures.push(format!("--- {} ---\nEXPECTED:\n{}\nGOT:\n{}\n", scenario.name, expected.trim_end(), trace));
        }
    }
    assert!(failures.is_empty(), "golden mismatch (run UPDATE_GOLDEN=1 to regenerate if intended):\n{}", failures.join("\n"));
}
```

- [ ] **Step 3: Generate + sanity-check the first golden**
Run: `UPDATE_GOLDEN=1 cargo test --features test-support --test golden` → writes `tests/golden/firebolt_kill.trace`. **Read it** and confirm it's sane: CastBegan(player,firebolt) → CastPhase transitions → HitWindow → HitConfirmed(player→dummy) → Damage(~20) → EffectApplied(burn) → DotTicked×N → Died(dummy), Cue lines (firebolt_cast/impact). Then run WITHOUT the env var: `cargo test --features test-support --test golden` → PASS (matches).

- [ ] **Step 4: Commit** (the golden is committed — it's the regression baseline)
```bash
git add src/scenario/library.rs tests/golden.rs tests/golden/firebolt_kill.trace
git commit -m "feat(scenario): golden-trace harness + firebolt_kill baseline"
```

---

## Phase B — Full feature matrix

### Task 5: Add the remaining matrix scenarios + goldens

**Files:** Modify `src/scenario/library.rs`; Create `tests/golden/<name>.trace` (×13); maybe add fixtures.

- [ ] **Step 1:** Add the remaining scenarios to `library.rs` and include them in `feature_matrix()`. Author each as a `Scenario` (reuse `firebolt`/`cleave` fixtures; add a tiny `aoe`/`buff` fixture skill only if needed). The full list + the key shape of each:
  - `cone_cleave` — player + 3 enemies (two in-arc, one behind); `cast_dir` cleave; expect Damage to the two front, not behind.
  - `faction_filter` — player + an ally in front; cleave; expect no Damage to the ally.
  - `out_of_range` — enemy 10 units away (cleave range 3); expect `CastRejected reason=OutOfRange`.
  - `line_of_sight` — `Obstacle` between player and target, cast firebolt (entity aim) → `CastRejected NoLineOfSight`; later (`Despawn` the obstacle is awkward — instead a second scenario `line_of_sight_clear` with no obstacle → CastBegan). Keep `line_of_sight` = blocked-only for a clean golden.
  - `cooldown_gate` — give firebolt a cooldown fixture (`firebolt_cd.toml` with `cooldown = 2.0`) or set it; cast twice; expect `CooldownStarted` then second `CastRejected OnCooldown`.
  - `already_casting` — cast firebolt twice a few ticks apart (within windup); expect second `CastRejected AlreadyCasting`.
  - `trigger_cascade` — reuse the authored OnConsume trigger fixture from the RF1 work (`static_discharge` + a skill that consumes a self-effect); expect `TriggerFired` + the triggered Damage.
  - `aoe_fan` — not script-castable via a hitbox; this one drives `ObeliskCombat::resolve_aoe` from a one-shot system. **Implement as a special scenario variant** OR cover `resolve_aoe` with a normal `#[test]` in `src/facade/combat.rs` instead and DROP it from the golden matrix (note which you chose — resolve_aoe isn't event-driven through the cast pipeline, so a golden scenario is awkward; a direct unit test is cleaner).
  - `netcode_egress` — firebolt_kill but `.recording_net()`; golden includes `Net` lines (stable ids).
  - `vfx_cues` — firebolt_kill but assert (via the golden) the `Cue cue=firebolt_cast` + `Cue cue=firebolt_impact` lines are present (they already appear in firebolt_kill's golden — so this may be redundant; if so, DROP and note it's covered by firebolt_kill).
  - `loot_on_death` — enemy with `with_drop_table("goblin")` + a `goblin` drop table fixture loaded; kill it; expect a `Loot` line. (Requires loading a drop table into `DropTables` — add a `DropTables` resource in `run_scenario` if any actor has a drop_table, loading from an inline/fixture table. Extend the runner minimally for this.)
  - `apply_effect` — `Action::ApplyEffect{target,effect:"burn"}` on an enemy; expect `EffectApplied` (+ DotTicked as it ticks).
  - `stat_sources` — covered better by the existing `verbs.rs` unit test; DROP from the golden matrix (note it).

  **Pragmatics:** scenarios that aren't event-driven through the cast pipeline (`aoe_fan`, `stat_sources`) are better as direct unit tests — exclude them from `feature_matrix()` and note it; redundant ones (`vfx_cues`) fold into `firebolt_kill`. The matrix should contain the genuinely-distinct event-trace scenarios.

- [ ] **Step 2:** `UPDATE_GOLDEN=1 cargo test --features test-support --test golden` to generate all goldens. **Read each** and sanity-check it matches the scenario's intent (e.g. `cone_cleave` has two Damage lines, not three; `out_of_range` has a `CastRejected OutOfRange` and no Damage). Fix any scenario that doesn't behave as intended (this is real regression authoring — the golden must reflect correct behavior).

- [ ] **Step 3:** Run without the env var → all golden tests pass. Run twice to confirm determinism (same goldens).

- [ ] **Step 4: Commit**
```bash
git add src/scenario/library.rs tests/golden/ tests/fixtures/
git commit -m "feat(scenario): full feature-matrix scenarios + golden baselines"
```

---

## Phase C — Visual layer

### Task 6: Debug-viz — gizmos, projectile mesh, hit/death reactions

**Files:** Create `src/present/debug_viz.rs`; Modify `src/present/mod.rs`.

- [ ] **Step 1: Write the gizmo + reaction systems** in `src/present/debug_viz.rs`. `ObeliskDebugVizPlugin` adds:
  - A `Gizmos` system (gated `#[cfg(feature="debug-gizmos")]`) drawing: each `Hurtbox` owner's sphere (`gizmos.sphere(pos, radius, GREEN)`); each active `Hitbox` shape at its transform (sphere/capsule wireframe; for `CollisionShape::Cone{angle,range}` draw an arc/sector along `hitbox.aim` — approximate with `gizmos.arc_3d`/line fan); a cast-phase ring on each `ActiveCast` caster colored by `SkillPhase`.
  - A `cue`/projectile mesh: an observer/system that gives spawned `Projectile` hitboxes a small emissive sphere `Mesh3d` + `MeshMaterial3d` (so the bolt is visible). (Spawn the mesh as a child or insert on the hitbox.)
  - Reactions: `On<HitConfirmed>`/`On<DamageResolved>` → flash the target's material (insert a short-lived `FlashTimer` that lerps emissive); `On<EntityDied>` → grey the material + scale down (insert a `DeathFade`).

  Provide the concrete system signatures + gizmo calls. **The Bevy 0.17 gizmo API (`gizmos.sphere`, `gizmos.arc_3d`, `Isometry3d`) and material mutation may differ — verify against docs and adapt; this is presentation-only, so iterate visually in the playground (Task 8).**

- [ ] **Step 2:** Add `#[cfg(feature = "present")] pub mod debug_viz;` to `src/present/mod.rs` and have `ObeliskPresentPlugin` (or `ObeliskPlugins`) add `ObeliskDebugVizPlugin`. Gate gizmo systems on `debug-gizmos`.

- [ ] **Step 3:** `cargo build` (default) + `cargo build --features debug-gizmos` + `cargo build --no-default-features` (debug_viz excluded). All compile. Full test suite still green (viz is render-only; headless tests don't add present).

- [ ] **Step 4: Commit**
```bash
git add src/present/debug_viz.rs src/present/mod.rs
git commit -m "feat(viz): gameplay debug-viz — hit/hurt/cone gizmos, projectile mesh, hit/death reactions"
```

### Task 7: Debug-viz — HUD bars + floating damage + event-log panel

**Files:** Modify `src/present/debug_viz.rs`.

- [ ] **Step 1:** Add `bevy_ui`-based UI to `ObeliskDebugVizPlugin`:
  - Per-combatant life/mana bars (a small UI node following the entity's screen position, or a fixed roster panel listing each `Attributes` entity's life/mana/cooldowns).
  - Floating damage text: `On<DamageResolved>` spawns a short-lived `Text2d`/UI number near the target (despawn after ~1s).
  - A scrolling event-log panel (a UI `Text` showing the last ~12 gameplay events, fed by observers).
  Provide concrete `bevy_ui`/`Text` node code. **Verify the 0.17 UI/text API (`Text`, `Node`, `Text2d`) and adapt.** If world-anchored text is too fiddly, a fixed roster + log panel is acceptable (note it).

- [ ] **Step 2:** `cargo build --features debug-gizmos` + full suite green.

- [ ] **Step 3: Commit**
```bash
git add src/present/debug_viz.rs
git commit -m "feat(viz): HUD life/mana/cooldown bars + floating damage + event-log panel"
```

### Task 8: Rewrite the windowed playground

**Files:** Rewrite `examples/playground.rs`.

- [ ] **Step 1:** Rewrite `examples/playground.rs`: `DefaultPlugins + ObeliskPlugins` (debug-viz included via present) + the obelisk config/skill init + camera/light. Use the **shared scenario library**: a resource holding `feature_matrix()`; number keys 1-9 spawn/replay the matching scenario (despawn prior actors, spawn the scenario's actors, schedule its script via a simple tick-driven runner system); `Space` free-casts the player's first skill at the nearest enemy via `ObeliskSpatial`; `R` resets. The debug-viz makes it visible.

- [ ] **Step 2:** `cargo build --example playground --features debug-gizmos` → PASS. **Manual visual check** (note for the maintainer): `cargo run --example playground --features debug-gizmos`, press `1` (firebolt_kill) and `Space` — confirm you SEE the projectile, the hit flash, the dummy dying, gizmos, and HUD/log updating. (The agent can't drive the window; this step is for the maintainer + corroborated by the screenshot renderer in Task 10.)

- [ ] **Step 3: Commit**
```bash
git add examples/playground.rs
git commit -m "feat(playground): windowed demo — scenario picker + free-cast + debug-viz"
```

---

## Phase D — Headless screenshots (highest risk; last)

### Task 9: SPIKE — headless render-to-PNG

**Files:** Create `examples/_screenshot_spike.rs` (throwaway).

- [ ] **Step 1:** Build a minimal headless render app that renders a single colored 3D scene (a camera + a lit cube) to an **off-screen `RenderTarget::Image`**, runs a few frames, copies the image back to CPU, and writes a PNG. Follow Bevy 0.17's `headless_renderer` example pattern (RenderApp + image-copy node, or the `Camera { target: RenderTarget::Image(handle) }` + readback approach). Verify a non-blank PNG is produced in THIS environment (the Metal adapter is available).

- [ ] **Step 2:** Run it: `cargo run --example _screenshot_spike` → writes `/tmp/spike.png`. `Read` the PNG to confirm it rendered (a visible cube, not blank/garbage). **Record the exact working render-to-image + readback recipe** — Task 10 reuses it.

- [ ] **Step 3:** If headless render-to-PNG **cannot** be made to work in this environment after real effort, report BLOCKED on screenshots: the `playground` (Task 8) remains the manual visual surface and the golden-trace backbone (Phases A-B) is the agent's regression tool. Do NOT fake a screenshot. If it works, delete the spike (`git rm`) after recording the recipe in a comment in `examples/screenshot.rs` (Task 10).

- [ ] **Step 4: Commit** (only the recorded recipe / or nothing if blocked)
```bash
git rm examples/_screenshot_spike.rs 2>/dev/null; git commit -m "chore: spike headless render-to-PNG; recipe recorded" --allow-empty
```

### Task 10: Screenshot renderer

**Files:** Create `examples/screenshot.rs`.

- [ ] **Step 1:** Write `examples/screenshot.rs` using Task 9's confirmed recipe: parse `--scenario <name>` + `--tick <n>` args; init obelisk config; build the headless render app (off-screen image target) + `ObeliskDebugVizPlugin` + a camera framing the action; spawn the named scenario's actors (from `feature_matrix()`/`scenario(name)`) and run its script up to `--tick`; capture the image → `screenshots/<name>-<tick>.png`.

- [ ] **Step 2:** Run: `cargo run --example screenshot --features debug-gizmos -- --scenario firebolt_kill --tick 24` → writes the PNG. (Only `debug-gizmos` is needed — the renderer uses always-compiled scenario data + `scenario::spawn_actor`/`apply_action` + the viz plugin; it never touches the `test-support`-gated trace/run modules.) `Read` it and confirm it shows the scene (player, dummy, gizmos, maybe the bolt/flash at tick 24). Report what the PNG shows.

- [ ] **Step 3: Commit**
```bash
git add examples/screenshot.rs
git commit -m "feat(screenshot): headless scenario render-to-PNG for agent visual validation"
```

---

## Phase E — Finalize

### Task 11: Docs + gates

**Files:** Modify `README.md`, `CLAUDE.md`.

- [ ] **Step 1:** Add a **"Validating changes"** section to `CLAUDE.md` (and a short note in `README.md`): the golden-trace workflow (`cargo test --features test-support --test golden`; `UPDATE_GOLDEN=1` to regenerate + review the diff), the screenshot workflow (`cargo run --example screenshot --features debug-gizmos -- --scenario X --tick N` then read the PNG), and the windowed playground (`cargo run --example playground --features debug-gizmos`). State the rule: **after any behavior change, run the golden suite; an intentional trace change must be reviewed in the golden diff before committing.**

- [ ] **Step 2: Gates:** `cargo test --features test-support --lib --tests` (incl. golden) green; `cargo clippy --features test-support --lib --tests -- -D warnings` clean; `cargo fmt --check` clean; `cargo build --no-default-features` (headless, no viz) compiles.

- [ ] **Step 3: Commit**
```bash
git add CLAUDE.md README.md
git commit -m "docs: validation workflow (golden traces, screenshots, playground)"
```

---

## Self-review notes (coverage vs spec)

- Scenario model + builder: Task 1 ✅ · Trace + recorder: Task 2 ✅ · Integration runner (public path): Task 3 ✅ · Golden harness + UPDATE_GOLDEN: Task 4 ✅ · Full matrix: Task 5 ✅ (with documented exclusions: `aoe_fan`/`stat_sources` → direct unit tests, `vfx_cues` → folded into firebolt_kill, since they aren't distinct cast-pipeline event traces) · Debug-viz (gizmos/mesh/reactions): Task 6 ✅ · HUD/log/floating text: Task 7 ✅ · Windowed playground: Task 8 ✅ · Screenshot spike + renderer: Tasks 9-10 ✅ (spike-gated; trace backbone stands if blocked) · Docs/gates: Task 11 ✅.
- Determinism: seeded RNG + `Time<Fixed>` + `{:.3}` floats + stable ids + fixed schedule (Task 2/3); a determinism meta-check is implicit in running the golden twice (Task 4/5 Step 3).

## Notes for the implementer
- The render/UI/gizmo Bevy 0.17 APIs (Tasks 6,7,9,10) are the parts most likely to need adjustment — they're presentation-only, so iterate against the playground/screenshot output and adapt; the golden backbone (Tasks 1-5) is the correctness-critical, fully-specified part.
- Keep `run_scenario` strictly on the **public** API (prelude + verbs + ObeliskSimPlugin + the documented headless recipe); the only test-support reach-in is `init_test_obelisk()` for the `Once`-guarded fixture globals.
