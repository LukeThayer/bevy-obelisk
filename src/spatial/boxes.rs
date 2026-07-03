use crate::assets::{CollisionShape, HitFilter, HitMode};
use avian3d::prelude::*;
use bevy::prelude::*;
use std::collections::HashMap;

/// Defensive volume on a combatant. Spawned as a static Avian collider so SpatialQuery can find it.
#[derive(Component, Debug)]
pub struct Hurtbox {
    pub owner: Entity,
}

/// Offensive volume spawned during an active collision window.
#[derive(Component, Debug)]
pub struct Hitbox {
    pub caster: Entity,
    pub skill_id: String,
    pub window_id: String,
    pub filter: HitFilter,
    pub mode: HitMode,
    /// The authored shape (drives the SpatialQuery + cone test).
    pub shape: CollisionShape,
    /// Normalized facing direction (cone axis / projectile heading).
    pub aim: Vec3,
    /// Seconds this hitbox has existed (for re-hit interval timing).
    pub age: f32,
    /// If set, a target may be hit again this many seconds after its last hit.
    pub rehit_interval: Option<f32>,
    /// Seconds remaining before the window expires.
    pub remaining: f32,
    /// target -> `age` at which it was last hit.
    pub hit_log: HashMap<Entity, f32>,
    /// FirstOnly: set true after the single hit so the box stops hitting.
    pub done: bool,
    /// Optional per-cast charge carried from the `ActiveCast`, forwarded to `HitConfirmed` so the
    /// resolve can scale damage. `None` = uncharged (1.0x).
    pub charge: Option<u8>,
    /// Copied from the window's authored `strikes` flag. `false` = a carrier volume: both hit
    /// paths (`detect_overlaps` and `resolve_beam_hits`) skip it — it can never produce a
    /// `HitConfirmed`, but it still moves, ends, and fires its cues/events.
    pub strikes: bool,
    /// `VolumeMotion::Beam`: this hitbox strikes its designated target directly (no overlap
    /// test — `detect_overlaps` skips it; `resolve_beam_hits` handles it).
    pub is_beam: bool,
    /// The beam's designated victim: the cast's entity aim for a scheduled beam window, or a
    /// chain hop's found candidate (Task 12, spec D5 — see `end_hitboxes`). `None` on a beam =
    /// nothing to strike (the paid fizzle: the window just fuses out).
    pub beam_target: Option<Entity>,
    /// How many chain hops preceded this hitbox (0 = the initial window). Populated by
    /// `end_hitboxes`' chain-hop arm (Task 12) from rules `can_chain`/`chain_count`.
    pub hop: u8,
    /// Entities already struck by EARLIER hitboxes in this chain (this hitbox's own victims
    /// are in `hit_log`). Chain-hop searches exclude `visited ∪ hit_log` so a chain never
    /// revisits a victim (Task 12, see `hop`).
    pub visited: Vec<Entity>,
    /// Trigger-generation depth this hitbox was spawned at (0 = a player cast). Copied from
    /// `ChainPayload.depth`.
    pub depth: u8,
    /// Seconds accumulated toward this hitbox's next emitter spawn boundary (Task 11, spec
    /// §3.2) — only meaningful when this window authors an `Emitter`; otherwise always `0.0`
    /// and untouched by `tick_emitters`. Lives on the hitbox (not a side table) so it needs no
    /// extra bookkeeping when the hitbox despawns.
    pub emit_elapsed: f32,
    /// `true` when this hitbox was spawned BY an emitter (Task 11) rather than the phase
    /// schedule or a triggered execution. Extends `is_free_hit`
    /// (`src/combat/system.rs`) so an emitted shard's hits never bill mana, and routes its
    /// `HitWindowOpened` to the `emit_{window_id}` cue instead of `on_window_{window_id}`
    /// (`src/vfx.rs::cue_on_window`).
    pub emitted: bool,
}

impl Hitbox {
    /// Whether `target` may be hit right now given mode + re-hit interval.
    pub fn can_hit(&self, target: Entity) -> bool {
        if self.done {
            return false;
        }
        match self.mode {
            HitMode::EveryTick => true,
            HitMode::FirstOnly | HitMode::OncePerTarget => match self.hit_log.get(&target) {
                None => true,
                Some(&last) => self.rehit_interval.is_some_and(|i| self.age - last >= i),
            },
        }
    }

    /// Record a hit on `target` and apply FirstOnly stop semantics.
    pub fn register_hit(&mut self, target: Entity) {
        self.hit_log.insert(target, self.age);
        if matches!(self.mode, HitMode::FirstOnly) {
            self.done = true;
        }
    }
}

/// Bundle of components that make `owner` a SpatialQuery-discoverable hurtbox at `pos`.
/// Uses `RigidBody::Static` because (per the probe) a static collider is included in
/// `SpatialQuery` shape intersections.
pub fn insert_hurtbox(commands: &mut Commands, owner: Entity, radius: f32, pos: Vec3) {
    commands.entity(owner).insert((
        Hurtbox { owner },
        RigidBody::Static,
        Collider::sphere(radius),
        Transform::from_translation(pos),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{CollisionShape, HitFilter, HitMode};

    fn hitbox(mode: HitMode, rehit: Option<f32>) -> Hitbox {
        Hitbox {
            caster: Entity::PLACEHOLDER,
            skill_id: "s".into(),
            window_id: "w".into(),
            filter: HitFilter::Enemies,
            mode,
            shape: CollisionShape::Sphere { radius: 1.0 },
            aim: Vec3::Z,
            age: 0.0,
            rehit_interval: rehit,
            remaining: 5.0,
            hit_log: HashMap::new(),
            done: false,
            charge: None,
            strikes: true,
            is_beam: false,
            beam_target: None,
            hop: 0,
            visited: Vec::new(),
            depth: 0,
            emit_elapsed: 0.0,
            emitted: false,
        }
    }

    #[test]
    fn once_per_target_hits_once() {
        let t = Entity::from_raw_u32(7).unwrap();
        let mut hb = hitbox(HitMode::OncePerTarget, None);
        assert!(hb.can_hit(t));
        hb.register_hit(t);
        assert!(!hb.can_hit(t), "no second hit without a re-hit interval");
    }

    #[test]
    fn first_only_stops_after_one_target() {
        let mut hb = hitbox(HitMode::FirstOnly, None);
        let a = Entity::from_raw_u32(1).unwrap();
        let b = Entity::from_raw_u32(2).unwrap();
        assert!(hb.can_hit(a));
        hb.register_hit(a);
        assert!(!hb.can_hit(b), "FirstOnly stops after the first target");
    }

    #[test]
    fn every_tick_always_hits() {
        let t = Entity::from_raw_u32(3).unwrap();
        let mut hb = hitbox(HitMode::EveryTick, None);
        hb.register_hit(t);
        assert!(hb.can_hit(t), "EveryTick re-hits the same target");
    }

    #[test]
    fn rehit_interval_allows_re_hit_after_delay() {
        let t = Entity::from_raw_u32(4).unwrap();
        let mut hb = hitbox(HitMode::OncePerTarget, Some(0.5));
        hb.register_hit(t);
        assert!(!hb.can_hit(t), "too soon");
        hb.age = 0.6;
        assert!(hb.can_hit(t), "interval elapsed -> re-hit allowed");
    }
}
