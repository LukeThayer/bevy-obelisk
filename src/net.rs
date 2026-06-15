use serde::{Deserialize, Serialize};

/// Engine-neutral, serializable gameplay event for server->client replication.
/// Actor references are network-stable `String` ids (obelisk `StatBlock.id`), NOT `Entity`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
