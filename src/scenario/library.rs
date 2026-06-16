use crate::core::components::Faction;
use crate::scenario::{Action, Aim, Scenario};
use bevy::prelude::Vec3;

pub fn firebolt_kill() -> Scenario {
    Scenario::new("firebolt_kill", 42, 600)
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
}

/// Cone cleave: player + 3 enemies (two inside the 120° arc, one directly behind).
/// Mirrors `tests/spatial_targeting.rs::cone_cleave_hits_multiple_enemies_in_arc_but_not_behind`.
/// Expect exactly two Damage lines (front_a, front_b); the entity behind is outside the cone.
pub fn cone_cleave() -> Scenario {
    Scenario::new("cone_cleave", 7, 120)
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor(
            "front_a",
            Faction::Enemy,
            50.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .actor(
            "front_b",
            Faction::Enemy,
            50.0,
            0.0,
            Vec3::new(1.0, 0.0, 1.5),
        )
        .actor(
            "behind",
            Faction::Enemy,
            50.0,
            0.0,
            Vec3::new(0.0, 0.0, -2.0),
        )
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Dir(Vec3::Z),
            },
        )
}

/// Faction filter: player + a same-faction ally directly in front of the cleave.
/// Mirrors `tests/spatial_targeting.rs::cleave_does_not_hit_allies`.
/// The `Enemies` hit filter must NOT damage the ally (zero Damage lines).
pub fn faction_filter() -> Scenario {
    Scenario::new("faction_filter", 7, 120)
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("ally", Faction::Player, 50.0, 0.0, Vec3::new(0.0, 0.0, 2.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Dir(Vec3::Z),
            },
        )
}

/// Out of range: an enemy 10 units away, cleave range is 3.
/// Mirrors `tests/spatial_targeting.rs::out_of_range_cast_is_rejected`.
/// Expect `CastRejected reason=OutOfRange` and no Damage.
pub fn out_of_range() -> Scenario {
    Scenario::new("out_of_range", 7, 30)
        .cast_asset("cleave")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("cleave")
        .actor("far", Faction::Enemy, 50.0, 0.0, Vec3::new(0.0, 0.0, 10.0))
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "cleave".into(),
                aim: Aim::Entity("far".into()),
            },
        )
}

/// Line of sight (blocked): a static obstacle between the player and the target.
/// Mirrors the blocked half of `tests/spatial_targeting.rs::cast_blocked_by_obstacle_is_rejected_then_clears`.
/// The obstacle is spawned at tick 0 so its static collider is registered (visible to
/// `SpatialQuery` from the 2nd update) before the firebolt cast at tick 3; expect
/// `CastRejected reason=NoLineOfSight` and nothing else (a clean, blocked-only golden).
pub fn line_of_sight() -> Scenario {
    Scenario::new("line_of_sight", 1, 30)
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor(
            "target",
            Faction::Enemy,
            100.0,
            0.0,
            Vec3::new(0.0, 0.0, 6.0),
        )
        .at(
            0,
            Action::Obstacle {
                pos: Vec3::new(0.0, 0.0, 3.0),
                radius: 1.0,
            },
        )
        .at(
            3,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("target".into()),
            },
        )
}

/// Already casting: cast firebolt, then cast again while the first is still in windup.
/// Mirrors `tests/vertical_slice.rs::second_cast_while_casting_is_rejected`.
/// The firebolt windup is ~18 ticks, so a second cast a few ticks later is rejected
/// `AlreadyCasting`. The dummy has high life so it survives long enough for a clean golden.
pub fn already_casting() -> Scenario {
    Scenario::new("already_casting", 1, 60)
        .cast_asset("firebolt")
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .with_skill("firebolt")
        .actor(
            "dummy",
            Faction::Enemy,
            200.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .at(
            1,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
        .at(
            4,
            Action::Cast {
                caster: "player".into(),
                skill: "firebolt".into(),
                aim: Aim::Entity("dummy".into()),
            },
        )
}

/// Apply effect: directly apply `burn` to an enemy (no cast pipeline).
/// Expect `EffectApplied burn`. NOTE: a *directly*-applied burn carries no triggering
/// damage, so its DoT DPS (= `base_damage_percent * status_damage * …`, obelisk
/// `calculate_status_dot_dps`) is 0 — hence no `DotTicked` lines (correct engine behavior;
/// damage-driven DoTs are exercised by `firebolt_kill`, where the bolt's hit seeds the burn).
pub fn apply_effect() -> Scenario {
    Scenario::new("apply_effect", 1, 120)
        .actor("player", Faction::Player, 100.0, 100.0, Vec3::ZERO)
        .actor(
            "dummy",
            Faction::Enemy,
            100.0,
            0.0,
            Vec3::new(0.0, 0.0, 2.0),
        )
        .at(
            1,
            Action::ApplyEffect {
                target: "dummy".into(),
                effect: "burn".into(),
            },
        )
}

/// Netcode egress: identical to `firebolt_kill` but recording the buffered `NetEvent`
/// wire stream into the trace. The golden additionally contains `Net` lines carrying the
/// stable String ids (the replication egress).
pub fn netcode_egress() -> Scenario {
    Scenario::new("netcode_egress", 42, 600)
        .cast_asset("firebolt")
        .recording_net()
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
}

/// The full regression matrix.
///
/// Intentionally-excluded scenarios (covered elsewhere, not omissions):
/// - `aoe_fan`: `ObeliskCombat::resolve_aoe` is not driven through the cast/event pipeline
///   (it is a programmatic SystemParam call), so a golden event-trace is awkward. It is
///   covered by a direct `#[test]` in `src/facade/combat.rs` instead.
/// - `vfx_cues`: the `Cue` lines (firebolt_cast/firebolt_impact) are already exercised by
///   `firebolt_kill` (and `netcode_egress`), so a dedicated scenario would be redundant.
/// - `stat_sources`: covered by the existing `src/verbs.rs` unit test.
///
/// Deferred to batch B (need new fixtures / a `DropTables` runner extension):
/// `cooldown_gate`, `trigger_cascade`, `loot_on_death`.
pub fn feature_matrix() -> Vec<Scenario> {
    vec![
        firebolt_kill(),
        cone_cleave(),
        faction_filter(),
        out_of_range(),
        line_of_sight(),
        already_casting(),
        apply_effect(),
        netcode_egress(),
    ]
}
