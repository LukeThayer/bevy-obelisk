use crate::core::components::Faction;
use crate::spatial::boxes::Hurtbox;
use crate::spatial::cone::point_in_cone;
use avian3d::prelude::{Collider, SpatialQuery, SpatialQueryFilter};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*; // Dir3, Vec3, Quat, Entity, Query come from here

/// Target-acquisition facade. Wraps Avian's SpatialQuery + hurtbox/faction lookups.
#[derive(SystemParam)]
pub struct ObeliskSpatial<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
    hurtboxes: Query<'w, 's, (&'static Hurtbox, &'static Transform)>,
    factions: Query<'w, 's, &'static Faction>,
}

impl ObeliskSpatial<'_, '_> {
    /// All hurtbox owners within `range` of `origin` whose faction differs from `caster_faction`.
    pub fn enemies_in_range(
        &self,
        origin: Vec3,
        range: f32,
        caster_faction: Faction,
    ) -> Vec<Entity> {
        let shape = Collider::sphere(range);
        self.spatial
            .shape_intersections(
                &shape,
                origin,
                Quat::IDENTITY,
                &SpatialQueryFilter::default(),
            )
            .into_iter()
            .filter_map(|hit_e| self.hurtboxes.get(hit_e).ok().map(|(h, _)| h.owner))
            .filter(|&owner| {
                self.factions
                    .get(owner)
                    .copied()
                    .unwrap_or(Faction::Neutral)
                    != caster_faction
            })
            .collect()
    }

    /// The single nearest enemy within `range`, or None.
    pub fn nearest_enemy(
        &self,
        origin: Vec3,
        range: f32,
        caster_faction: Faction,
    ) -> Option<Entity> {
        self.enemies_in_range(origin, range, caster_faction)
            .into_iter()
            .filter_map(|e| {
                self.hurtboxes
                    .get(e)
                    .ok()
                    .map(|(_, tf)| (e, tf.translation.distance_squared(origin)))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(e, _)| e)
    }

    /// Enemies within a cone (apex `origin`, axis `dir`, full `angle_deg`, slant `range`).
    pub fn cone_targets(
        &self,
        origin: Vec3,
        dir: Vec3,
        angle_deg: f32,
        range: f32,
        caster_faction: Faction,
    ) -> Vec<Entity> {
        let half = angle_deg.to_radians() * 0.5;
        self.enemies_in_range(origin, range, caster_faction)
            .into_iter()
            .filter(|&e| {
                self.hurtboxes
                    .get(e)
                    .map(|(_, tf)| point_in_cone(origin, dir, half, range, tf.translation))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// First hurtbox owner struck by a ray from `origin` along `dir` within `range`.
    pub fn raycast_target(&self, origin: Vec3, dir: Dir3, range: f32) -> Option<Entity> {
        let hit =
            self.spatial
                .cast_ray(origin, dir, range, true, &SpatialQueryFilter::default())?;
        self.hurtboxes.get(hit.entity).ok().map(|(h, _)| h.owner)
    }

    /// Whether the straight segment a->b is unobstructed (no collider strictly between).
    pub fn los_clear(&self, a: Vec3, b: Vec3) -> bool {
        let delta = b - a;
        let dist = delta.length();
        if dist <= f32::EPSILON {
            return true;
        }
        let dir = Dir3::new(delta).unwrap_or(Dir3::Z);
        match self
            .spatial
            .cast_ray(a, dir, dist, true, &SpatialQueryFilter::default())
        {
            Some(hit) => hit.distance >= dist - 0.01,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testkit::ObeliskTestApp;
    use bevy::ecs::system::RunSystemOnce;
    use stat_core::StatBlock;

    fn spawn(t: &mut ObeliskTestApp, id: &str, faction: Faction, pos: Vec3) -> Entity {
        let mut b = StatBlock::with_id(id);
        b.max_life.base = 50.0;
        b.current_life = 50.0;
        let e = t
            .app
            .world_mut()
            .spawn((
                Combatant,
                Attributes(b),
                faction,
                ObeliskId(id.into()),
                Transform::from_translation(pos),
            ))
            .id();
        let mut c = t.app.world_mut().commands();
        insert_hurtbox(&mut c, e, 0.5, pos);
        e
    }

    #[test]
    fn nearest_enemy_picks_the_closest_other_faction() {
        let mut t = ObeliskTestApp::new(1);
        let _player = spawn(&mut t, "player", Faction::Player, Vec3::ZERO);
        let near = spawn(&mut t, "near", Faction::Enemy, Vec3::new(0.0, 0.0, 2.0));
        let _far = spawn(&mut t, "far", Faction::Enemy, Vec3::new(0.0, 0.0, 8.0));
        let _ally = spawn(&mut t, "ally", Faction::Player, Vec3::new(0.0, 0.0, 1.0));
        t.app.update();
        t.app.update();
        t.app.update();

        let got = t
            .app
            .world_mut()
            .run_system_once(move |s: ObeliskSpatial| {
                s.nearest_enemy(Vec3::ZERO, 10.0, Faction::Player)
            })
            .unwrap();
        assert_eq!(
            got,
            Some(near),
            "nearest enemy is `near`, not the ally or far enemy"
        );
    }
}
