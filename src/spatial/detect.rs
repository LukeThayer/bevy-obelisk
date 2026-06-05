// filled later

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use avian3d::prelude::*;
    use std::time::Duration;
    use crate::spatial::boxes::{insert_hurtbox, Hurtbox};

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
           .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
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
