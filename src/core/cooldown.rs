use bevy::prelude::*;
use std::collections::HashMap;

/// Per-(entity, skill) cooldown timers (seconds remaining). Absent/<=0 = ready.
#[derive(Resource, Default)]
pub struct Cooldowns {
    remaining: HashMap<(Entity, String), f32>,
}

impl Cooldowns {
    pub fn is_ready(&self, e: Entity, skill: &str) -> bool {
        self.remaining
            .get(&(e, skill.to_string()))
            .is_none_or(|&r| r <= 0.0)
    }
    pub fn remaining(&self, e: Entity, skill: &str) -> f32 {
        self.remaining
            .get(&(e, skill.to_string()))
            .copied()
            .unwrap_or(0.0)
    }
    pub fn start(&mut self, e: Entity, skill: &str, duration: f32) {
        if duration > 0.0 {
            self.remaining.insert((e, skill.to_string()), duration);
        }
    }
}

use crate::events::CooldownReady;

/// Decrement cooldowns each fixed step; emit CooldownReady + remove when they reach zero.
pub fn tick_cooldowns(
    time: Res<Time<Fixed>>,
    mut cooldowns: ResMut<Cooldowns>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let mut ready: Vec<(Entity, String)> = Vec::new();
    for (key, rem) in cooldowns.remaining.iter_mut() {
        *rem -= dt;
        if *rem <= 0.0 {
            ready.push(key.clone());
        }
    }
    for key in ready {
        cooldowns.remaining.remove(&key);
        commands.trigger(CooldownReady {
            caster: key.0,
            skill_id: key.1,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_when_absent_then_busy_after_start() {
        let e = Entity::from_raw_u32(1).unwrap();
        let mut cd = Cooldowns::default();
        assert!(cd.is_ready(e, "firebolt"), "absent = ready");
        cd.start(e, "firebolt", 2.0);
        assert!(!cd.is_ready(e, "firebolt"), "busy after start");
        assert!((cd.remaining(e, "firebolt") - 2.0).abs() < 1e-6);
        cd.start(e, "other", 0.0);
        assert!(cd.is_ready(e, "other"), "zero-duration start is a no-op");
    }
}
