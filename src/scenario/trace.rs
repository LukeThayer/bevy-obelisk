use crate::events::*;
use crate::ids::ObeliskEntityIndex;
use bevy::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct TraceLine {
    pub tick: usize,
    pub kind: &'static str,
    pub detail: String,
}

/// Ordered, deterministic record of a scenario's observable events.
#[derive(Resource, Default)]
pub struct Trace {
    pub lines: Vec<TraceLine>,
}

impl Trace {
    pub fn to_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| format!("{:>4}\t{}\t{}", l.tick, l.kind, l.detail))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Current fixed tick (incremented at the start of each FixedUpdate).
#[derive(Resource, Default)]
pub struct TickCounter(pub usize);

fn advance_tick(mut t: ResMut<TickCounter>) {
    t.0 += 1;
}

/// Whether to also record NetEvents (the wire egress). Set per scenario.
#[derive(Resource, Default)]
pub struct RecordNet(pub bool);

fn id(index: &ObeliskEntityIndex, e: Entity) -> String {
    index.id(e).unwrap_or("?").to_string()
}

macro_rules! push {
    ($trace:expr, $tick:expr, $kind:literal, $detail:expr) => {
        $trace.lines.push(TraceLine {
            tick: $tick.0,
            kind: $kind,
            detail: $detail,
        });
    };
}

/// Records every gameplay event into `Trace`. Add to the scenario app AFTER ObeliskSimPlugin.
pub struct TraceRecorderPlugin;

impl Plugin for TraceRecorderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Trace>()
            .init_resource::<TickCounter>()
            .init_resource::<RecordNet>();
        // tick counter runs first in FixedUpdate so event ticks are correct.
        app.add_systems(
            FixedUpdate,
            advance_tick.before(crate::ObeliskSet::Validate),
        );

        app.add_observer(
            |e: On<CastBegan>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "CastBegan",
                    format!(
                        "caster={} skill={} dur={:.3}",
                        id(&ix, e.caster),
                        e.skill_id,
                        e.total_duration
                    )
                );
            },
        );
        app.add_observer(
            |e: On<CastRejected>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "CastRejected",
                    format!(
                        "caster={} skill={} reason={:?}",
                        id(&ix, e.caster),
                        e.skill_id,
                        e.reason
                    )
                );
            },
        );
        app.add_observer(
            |e: On<CastPhaseChanged>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "CastPhase",
                    format!(
                        "caster={} skill={} {:?}->{:?}",
                        id(&ix, e.caster),
                        e.skill_id,
                        e.from,
                        e.to
                    )
                );
            },
        );
        app.add_observer(
            |e: On<HitWindowOpened>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "HitWindow",
                    format!(
                        "caster={} skill={} window={}",
                        id(&ix, e.caster),
                        e.skill_id,
                        e.window_id
                    )
                );
            },
        );
        app.add_observer(
            |e: On<HitConfirmed>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "HitConfirmed",
                    format!(
                        "caster={} target={} skill={}",
                        id(&ix, e.caster),
                        id(&ix, e.target),
                        e.skill_id
                    )
                );
            },
        );
        app.add_observer(
            |e: On<DamageResolved>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "Damage",
                    format!(
                        "caster={} target={} skill={} dmg={:.3} kill={} life_after={:.3} \
                         crit={} prevented={:.3} life_gained={:.3} mana_gained={:.3}",
                        id(&ix, e.caster),
                        id(&ix, e.target),
                        e.skill_id,
                        e.total_damage,
                        e.is_killing_blow,
                        e.life_after,
                        e.is_critical,
                        e.damage_prevented,
                        e.life_gained,
                        e.mana_gained
                    )
                );
            },
        );
        app.add_observer(
            |e: On<EffectApplied>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "EffectApplied",
                    format!(
                        "target={} effect={} dur={:.3} stacks={}",
                        id(&ix, e.target),
                        e.effect_id,
                        e.total_duration,
                        e.stacks
                    )
                );
            },
        );
        app.add_observer(
            |e: On<EffectExpired>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "EffectExpired",
                    format!("target={} effect={}", id(&ix, e.target), e.effect_id)
                );
            },
        );
        app.add_observer(
            |e: On<DotTicked>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "DotTicked",
                    format!(
                        "target={} dmg={:.3} life={:.3}",
                        id(&ix, e.target),
                        e.dot_damage,
                        e.life_remaining
                    )
                );
            },
        );
        app.add_observer(
            |e: On<EntityDied>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                let k = e.killer.map(|k| id(&ix, k)).unwrap_or("none".into());
                push!(
                    tr,
                    t,
                    "Died",
                    format!("target={} killer={}", id(&ix, e.target), k)
                );
            },
        );
        app.add_observer(
            |e: On<TriggerFired>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "TriggerFired",
                    format!(
                        "source={} target={} skill={} effect={}",
                        id(&ix, e.source),
                        id(&ix, e.target),
                        e.skill_id,
                        e.effect_id
                    )
                );
            },
        );
        app.add_observer(
            |e: On<CueEvent>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "Cue",
                    format!(
                        "cue={} source={} kind={:?}",
                        e.cue_id,
                        id(&ix, e.source),
                        e.kind
                    )
                );
            },
        );
        app.add_observer(
            |e: On<CooldownStarted>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "CooldownStarted",
                    format!(
                        "caster={} skill={} dur={:.3}",
                        id(&ix, e.caster),
                        e.skill_id,
                        e.duration
                    )
                );
            },
        );
        app.add_observer(
            |e: On<CooldownReady>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "CooldownReady",
                    format!("caster={} skill={}", id(&ix, e.caster), e.skill_id)
                );
            },
        );
        app.add_observer(
            |e: On<LootDropped>,
             ix: Res<ObeliskEntityIndex>,
             t: Res<TickCounter>,
             mut tr: ResMut<Trace>| {
                let e = e.event();
                push!(
                    tr,
                    t,
                    "Loot",
                    format!("source={} drops={:?}", id(&ix, e.source), e.drops)
                );
            },
        );
        // NetEvent recording (opt-in): NetEvents already carry String ids.
        app.add_systems(
            FixedUpdate,
            record_net
                .run_if(|r: Res<RecordNet>| r.0)
                .after(crate::ObeliskSet::TickEffects),
        );
    }
}

fn record_net(
    mut reader: bevy::prelude::MessageReader<crate::net::NetEvent>,
    t: Res<TickCounter>,
    mut tr: ResMut<Trace>,
) {
    for ev in reader.read() {
        tr.lines.push(TraceLine {
            tick: t.0,
            kind: "Net",
            detail: format!("{:?}", ev),
        });
    }
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
