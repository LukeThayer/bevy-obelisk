use crate::core::components::Faction;
use crate::events::HitConfirmed;
use crate::spatial::boxes::{Hitbox, Hurtbox};
use avian3d::prelude::*;
use bevy::prelude::*;

/// For each active hitbox, query overlapping hurtboxes via SpatialQuery, apply the
/// faction filter + HitMode dedupe, and emit HitConfirmed. Detection is in FixedUpdate
/// so it is deterministic and authoritative.
pub fn detect_overlaps(
    mut commands: Commands,
    mut hitboxes: Query<(&mut Hitbox, &Transform)>,
    hurtboxes: Query<(Entity, &Hurtbox)>,
    factions: Query<&Faction>,
    spatial: SpatialQuery,
) {
    for (mut hitbox, hb_tf) in &mut hitboxes {
        let collider = Collider::sphere(0.5); // slice: bolt radius; future: store the hitbox's own collider
        let hits = spatial.shape_intersections(
            &collider,
            hb_tf.translation,
            hb_tf.rotation,
            &SpatialQueryFilter::default(),
        );

        let caster_faction = factions
            .get(hitbox.caster)
            .copied()
            .unwrap_or(Faction::Neutral);
        for hurt_e in hits {
            let Ok((owner_e, hurt)) = hurtboxes.get(hurt_e) else {
                continue;
            };
            let target = hurt.owner;
            if target == hitbox.caster {
                continue;
            }
            if hitbox.already_hit.contains(&target) {
                continue;
            }

            // Faction filter (HitFilter::Enemies for the slice).
            let target_faction = factions.get(target).copied().unwrap_or(Faction::Neutral);
            let is_enemy = target_faction != caster_faction;
            if !is_enemy {
                continue;
            }

            hitbox.already_hit.push(target);
            commands.trigger(HitConfirmed {
                caster: hitbox.caster,
                target,
                skill_id: hitbox.skill_id.clone(),
                window_id: hitbox.window_id.clone(),
            });
            let _ = owner_e;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::spatial::boxes::{insert_hurtbox, Hurtbox};
    use avian3d::prelude::*;
    use bevy::prelude::*;
    use std::time::Duration;

    #[derive(Resource, Default)]
    struct Found(bool);

    fn detect_sys(spatial: SpatialQuery, q: Query<&Hurtbox>, mut found: ResMut<Found>) {
        let hits = spatial.shape_intersections(
            &Collider::sphere(0.5),
            Vec3::ZERO,
            Quat::IDENTITY,
            &SpatialQueryFilter::default(),
        );
        found.0 = hits.iter().any(|e| q.get(*e).is_ok());
    }

    #[test]
    fn spatial_query_finds_an_overlapping_hurtbox() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .add_plugins(bevy::mesh::MeshPlugin)
            .add_plugins(bevy::scene::ScenePlugin)
            .add_plugins(avian3d::prelude::PhysicsPlugins::new(FixedUpdate))
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(100),
            ))
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .init_resource::<Found>();
        app.finish();
        app.cleanup();

        let dummy = app.world_mut().spawn_empty().id();
        {
            let world = app.world_mut();
            let mut commands = world.commands();
            insert_hurtbox(&mut commands, dummy, 0.5, Vec3::ZERO);
        }
        // apply the deferred insert + let physics register the collider (>=2 ticks)
        app.update();
        app.update();
        app.update();

        // run the detection system once
        app.add_systems(Update, detect_sys);
        app.update();

        assert!(
            app.world().resource::<Found>().0,
            "SpatialQuery should find the static hurtbox collider"
        );
    }
}
