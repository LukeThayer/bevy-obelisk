use bevy::prelude::*;

use crate::events::{CastBegan, DamageResolved, DotTicked, EffectApplied, EffectExpired, EntityDied};
use crate::ids::ObeliskEntityIndex;

/// Engine-neutral, serializable gameplay event for server->client replication.
/// Actor references are network-stable `String` ids (obelisk `StatBlock.id`), NOT `Entity`.
#[derive(Message, serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum NetEvent {
    CastBegan { caster: String, skill_id: String, total_duration: f32 },
    DamageResolved {
        caster: String,
        target: String,
        skill_id: String,
        total_damage: f64,
        is_killing_blow: bool,
        life_after: f64,
    },
    EffectApplied { target: String, effect_id: String, total_duration: f64, stacks: u32 },
    EffectExpired { target: String, effect_id: String },
    DotTicked { target: String, effect_id: String, dot_damage: f64, life_remaining: f64 },
    EntityDied { target: String, killer: Option<String> },
}

/// Mirrors the sim's in-process observer events into the buffered `NetEvent` stream for
/// server replication. Entity refs are translated to stable string ids via ObeliskEntityIndex.
pub struct ObeliskNetPlugin;

impl Plugin for ObeliskNetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<NetEvent>();
        app.add_observer(mirror_cast_began);
        app.add_observer(mirror_damage_resolved);
        app.add_observer(mirror_effect_applied);
        app.add_observer(mirror_effect_expired);
        app.add_observer(mirror_dot_ticked);
        app.add_observer(mirror_entity_died);
    }
}

fn id_of(index: &ObeliskEntityIndex, e: Entity) -> String {
    index.id(e).unwrap_or("").to_string()
}

fn mirror_cast_began(
    ev: On<CastBegan>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    net.write(NetEvent::CastBegan {
        caster: id_of(&index, e.caster),
        skill_id: e.skill_id.clone(),
        total_duration: e.total_duration,
    });
}

fn mirror_damage_resolved(
    ev: On<DamageResolved>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    net.write(NetEvent::DamageResolved {
        caster: id_of(&index, e.caster),
        target: id_of(&index, e.target),
        skill_id: e.skill_id.clone(),
        total_damage: e.total_damage,
        is_killing_blow: e.is_killing_blow,
        life_after: e.life_after,
    });
}

fn mirror_effect_applied(
    ev: On<EffectApplied>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    net.write(NetEvent::EffectApplied {
        target: id_of(&index, e.target),
        effect_id: e.effect_id.clone(),
        total_duration: e.total_duration,
        stacks: e.stacks,
    });
}

fn mirror_effect_expired(
    ev: On<EffectExpired>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    net.write(NetEvent::EffectExpired {
        target: id_of(&index, e.target),
        effect_id: e.effect_id.clone(),
    });
}

fn mirror_dot_ticked(
    ev: On<DotTicked>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    net.write(NetEvent::DotTicked {
        target: id_of(&index, e.target),
        effect_id: e.effect_id.clone(),
        dot_damage: e.dot_damage,
        life_remaining: e.life_remaining,
    });
}

fn mirror_entity_died(
    ev: On<EntityDied>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    net.write(NetEvent::EntityDied {
        target: id_of(&index, e.target),
        killer: e.killer.map(|k| id_of(&index, k)),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netevent_serde_round_trips() {
        let events = vec![
            NetEvent::CastBegan { caster: "player".into(), skill_id: "firebolt".into(), total_duration: 0.6 },
            NetEvent::DamageResolved {
                caster: "player".into(), target: "goblin".into(), skill_id: "firebolt".into(),
                total_damage: 20.0, is_killing_blow: false, life_after: 30.0,
            },
            NetEvent::EntityDied { target: "goblin".into(), killer: Some("player".into()) },
        ];
        let json = serde_json::to_string(&events).expect("serialize");
        let back: Vec<NetEvent> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(events, back, "NetEvent must survive a JSON round-trip unchanged");
    }
}
