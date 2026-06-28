use bevy::prelude::*;
use stat_core::{Skill, StatBlock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillPhase {
    Windup,
    Active,
    Recovery,
    Done,
}

/// Per-cast runtime state. Effective (speed-scaled) durations are snapshotted at cast start.
#[derive(Component, Debug)]
pub struct ActiveCast {
    pub skill_id: String,
    /// The aimed target entity, if any (for reference / future single-target rules).
    pub target: Option<Entity>,
    /// Resolved, normalized facing direction (projectile heading / cone axis).
    pub aim_dir: Vec3,
    pub phase: SkillPhase,
    pub elapsed: f32,
    /// Effective phase durations (seconds), already divided by the caster's speed rate.
    pub windup: f32,
    pub active: f32,
    pub recovery: f32,
    /// Window ids already spawned this cast (so we open each once).
    pub fired_windows: Vec<String>,
    /// Optional per-cast charge, snapshotted from the `PendingCast`. `None` = uncharged (1.0x).
    /// Scales projectile speed at spawn and damage at resolve via `charge_mult`.
    pub charge: Option<u8>,
    /// Per-cast muzzle offset (world units), snapshotted from the `PendingCast`. Added to the
    /// caster origin at projectile / hitbox spawn. `Vec3::ZERO` (the default) = spawn at the
    /// caster origin, byte-identical to the pre-offset behaviour.
    pub muzzle_offset: Vec3,
}

impl ActiveCast {
    pub fn total_duration(&self) -> f32 {
        self.windup + self.active + self.recovery
    }
    /// Which phase a given elapsed time falls in.
    pub fn phase_at(&self, t: f32) -> SkillPhase {
        if t < self.windup {
            SkillPhase::Windup
        } else if t < self.windup + self.active {
            SkillPhase::Active
        } else if t < self.total_duration() {
            SkillPhase::Recovery
        } else {
            SkillPhase::Done
        }
    }
}

/// The playback rate for a cast's timeline. `1.0` = play as authored; `2.0` = twice as fast.
/// Picks cast_speed for spells, attack_speed for attacks, then applies the skill's modifier.
pub fn effective_rate(caster: &StatBlock, skill: &Skill) -> f64 {
    let base = if skill.is_spell() {
        caster.cast_speed.compute()
    } else if skill.is_attack() {
        caster.attack_speed.compute()
    } else {
        1.0
    };
    skill.effective_speed(base).max(0.0001) // guard rate > 0
}

/// Build the speed-scaled phase durations from authored base durations.
pub fn scale_durations(base: (f32, f32, f32), rate: f64) -> (f32, f32, f32) {
    let r = rate as f32;
    (base.0 / r, base.1 / r, base.2 / r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::{Skill, StatBlock};

    fn spell(asm: f64) -> Skill {
        let toml = format!(
            r#"
[[skills]]
id = "s"
name = "TestSpell"
tags = ["spell"]
targeting = "single_enemy"
delivery = "instant"
attack_speed_modifier = {asm}
[skills.damage]
"#
        );
        stat_core::config::parse_skills(&toml)
            .unwrap()
            .remove("s")
            .unwrap()
    }

    #[test]
    fn cast_speed_halves_durations_at_double_speed() {
        let skill = spell(1.0);
        let mut caster = StatBlock::with_id("p");
        caster.cast_speed.base = 2.0; // 2x cast speed
        let rate = effective_rate(&caster, &skill);
        assert!((rate - 2.0).abs() < 1e-6);
        // base windup 0.4 -> effective 0.2
        assert!((0.4_f32 / rate as f32 - 0.2).abs() < 1e-4);
    }

    #[test]
    fn attack_speed_modifier_slows_a_slow_weapon() {
        let mut caster = StatBlock::with_id("p");
        caster.attack_speed.base = 1.0;
        let attack_toml = r#"
[[skills]]
id = "a"
name = "TestAttack"
tags = ["attack"]
targeting = "single_enemy"
delivery = "melee"
attack_speed_modifier = 0.8
[skills.damage]
"#;
        let atk = stat_core::config::parse_skills(attack_toml)
            .unwrap()
            .remove("a")
            .unwrap();
        let rate = effective_rate(&caster, &atk);
        assert!(
            (rate - 0.8).abs() < 1e-6,
            "rate = base(1.0) * modifier(0.8)"
        );
    }
}
