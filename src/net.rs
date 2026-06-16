use bevy::prelude::*;

use crate::events::{
    CastBegan, CastRejected, DamageResolved, DotTicked, EffectApplied, EffectExpired, EntityDied,
};
use crate::ids::ObeliskEntityIndex;

/// Engine-neutral, serializable gameplay event for server->client replication.
/// Actor references are network-stable `String` ids (obelisk `StatBlock.id`), NOT `Entity`.
#[derive(Message, serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum NetEvent {
    CastBegan {
        caster: String,
        skill_id: String,
        total_duration: f32,
    },
    DamageResolved {
        caster: String,
        target: String,
        skill_id: String,
        total_damage: f64,
        is_killing_blow: bool,
        life_after: f64,
    },
    EffectApplied {
        target: String,
        effect_id: String,
        total_duration: f64,
        stacks: u32,
    },
    EffectExpired {
        target: String,
        effect_id: String,
    },
    DotTicked {
        target: String,
        effect_id: String,
        dot_damage: f64,
        life_remaining: f64,
    },
    EntityDied {
        target: String,
        killer: Option<String>,
    },
    CastRejected {
        caster: String,
        skill_id: String,
        reason: crate::events::CastRejectReason,
    },
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
        app.add_observer(mirror_cast_rejected);
    }
}

fn id_of(index: &ObeliskEntityIndex, e: Entity) -> Option<String> {
    index.id(e).map(|s| s.to_string())
}

fn mirror_cast_began(
    ev: On<CastBegan>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    let Some(caster) = id_of(&index, e.caster) else {
        bevy::log::warn!("NetEvent CastBegan dropped: caster {:?} has no ObeliskId", e.caster);
        return;
    };
    net.write(NetEvent::CastBegan {
        caster,
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
    let Some(caster) = id_of(&index, e.caster) else {
        bevy::log::warn!("NetEvent DamageResolved dropped: caster {:?} has no ObeliskId", e.caster);
        return;
    };
    let Some(target) = id_of(&index, e.target) else {
        bevy::log::warn!("NetEvent DamageResolved dropped: target {:?} has no ObeliskId", e.target);
        return;
    };
    net.write(NetEvent::DamageResolved {
        caster,
        target,
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
    let Some(target) = id_of(&index, e.target) else {
        bevy::log::warn!("NetEvent EffectApplied dropped: target {:?} has no ObeliskId", e.target);
        return;
    };
    net.write(NetEvent::EffectApplied {
        target,
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
    let Some(target) = id_of(&index, e.target) else {
        bevy::log::warn!("NetEvent EffectExpired dropped: target {:?} has no ObeliskId", e.target);
        return;
    };
    net.write(NetEvent::EffectExpired {
        target,
        effect_id: e.effect_id.clone(),
    });
}

fn mirror_dot_ticked(
    ev: On<DotTicked>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    let Some(target) = id_of(&index, e.target) else {
        bevy::log::warn!("NetEvent DotTicked dropped: target {:?} has no ObeliskId", e.target);
        return;
    };
    net.write(NetEvent::DotTicked {
        target,
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
    let Some(target) = id_of(&index, e.target) else {
        bevy::log::warn!("NetEvent EntityDied dropped: target {:?} has no ObeliskId", e.target);
        return;
    };
    net.write(NetEvent::EntityDied {
        target,
        killer: e.killer.and_then(|k| id_of(&index, k)),
    });
}

fn mirror_cast_rejected(
    ev: On<CastRejected>,
    index: Res<ObeliskEntityIndex>,
    mut net: MessageWriter<NetEvent>,
) {
    let e = ev.event();
    let Some(caster) = id_of(&index, e.caster) else {
        bevy::log::warn!("NetEvent CastRejected dropped: caster {:?} has no ObeliskId", e.caster);
        return;
    };
    net.write(NetEvent::CastRejected {
        caster,
        skill_id: e.skill_id.clone(),
        reason: e.reason.clone(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netevent_serde_round_trips() {
        let events = vec![
            NetEvent::CastBegan {
                caster: "player".into(),
                skill_id: "firebolt".into(),
                total_duration: 0.6,
            },
            NetEvent::DamageResolved {
                caster: "player".into(),
                target: "goblin".into(),
                skill_id: "firebolt".into(),
                total_damage: 20.0,
                is_killing_blow: false,
                life_after: 30.0,
            },
            NetEvent::EntityDied {
                target: "goblin".into(),
                killer: Some("player".into()),
            },
        ];
        let json = serde_json::to_string(&events).expect("serialize");
        let back: Vec<NetEvent> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            events, back,
            "NetEvent must survive a JSON round-trip unchanged"
        );
    }
}
