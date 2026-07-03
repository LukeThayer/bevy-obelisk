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
    hurtboxes: Query<(Entity, &Hurtbox, &Transform)>,
    factions: Query<&Faction>,
    spatial: SpatialQuery,
) {
    for (mut hitbox, hb_tf) in &mut hitboxes {
        // Beams have no overlap semantics — `resolve_beam_hits` strikes their designated
        // target directly.
        if hitbox.is_beam {
            continue;
        }
        let collider = crate::spatial::shapes::to_collider(&hitbox.shape);
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
            let Ok((owner_e, hurt, hurt_tf)) = hurtboxes.get(hurt_e) else {
                continue;
            };
            let target = hurt.owner;
            if let crate::assets::CollisionShape::Cone { angle, range } = hitbox.shape {
                let half = angle.to_radians() * 0.5;
                if !crate::spatial::cone::point_in_cone(
                    hb_tf.translation,
                    hitbox.aim,
                    half,
                    range,
                    hurt_tf.translation,
                ) {
                    continue;
                }
            }
            if !hitbox.can_hit(target) {
                continue;
            }

            let target_faction = factions.get(target).copied().unwrap_or(Faction::Neutral);
            let is_self = target == hitbox.caster;
            if !crate::spatial::filter::passes_filter(
                hitbox.filter,
                caster_faction,
                target_faction,
                is_self,
            ) {
                continue;
            }

            hitbox.register_hit(target);
            commands.trigger(HitConfirmed {
                caster: hitbox.caster,
                target,
                skill_id: hitbox.skill_id.clone(),
                window_id: hitbox.window_id.clone(),
                charge: hitbox.charge,
                position: hb_tf.translation,
                depth: hitbox.depth,
                hop: hitbox.hop,
            });
            let _ = owner_e;
        }
    }
}

/// Strike each beam hitbox's DESIGNATED target: no overlap test — acquisition already picked
/// the victim (the cast's entity aim, or a retarget hop). Moves the hitbox onto the victim so
/// its `HitEntity` end (and the chained retarget search) happens AT the victim, applies the
/// faction filter, registers the hit (`FirstOnly` → done → `end_hitboxes` ends it next tick),
/// and emits the same `HitConfirmed` the overlap path does. A beam whose target is gone (died
/// mid-chain) or was never designated (direction-aimed cast: the paid fizzle) strikes nothing
/// and fuses out.
pub fn resolve_beam_hits(
    mut commands: Commands,
    mut hitboxes: Query<(&mut Hitbox, &mut Transform)>,
    victims: Query<&Transform, Without<Hitbox>>,
    factions: Query<&Faction>,
) {
    for (mut hitbox, mut hb_tf) in &mut hitboxes {
        if !hitbox.is_beam || hitbox.done {
            continue;
        }
        let Some(target) = hitbox.beam_target else {
            continue;
        };
        if !hitbox.can_hit(target) {
            continue;
        }
        let Ok(victim_tf) = victims.get(target) else {
            continue;
        };
        let caster_faction = factions
            .get(hitbox.caster)
            .copied()
            .unwrap_or(Faction::Neutral);
        let target_faction = factions.get(target).copied().unwrap_or(Faction::Neutral);
        if !crate::spatial::filter::passes_filter(
            hitbox.filter,
            caster_faction,
            target_faction,
            target == hitbox.caster,
        ) {
            continue;
        }
        hb_tf.translation = victim_tf.translation;
        hitbox.register_hit(target);
        commands.trigger(HitConfirmed {
            caster: hitbox.caster,
            target,
            skill_id: hitbox.skill_id.clone(),
            window_id: hitbox.window_id.clone(),
            charge: hitbox.charge,
            position: hb_tf.translation,
            depth: hitbox.depth,
            hop: hitbox.hop,
        });
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

    fn make_physics_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .add_plugins(bevy::mesh::MeshPlugin)
            .add_plugins(bevy::scene::ScenePlugin)
            .add_plugins(avian3d::prelude::PhysicsPlugins::new(FixedUpdate))
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(100),
            ))
            .insert_resource(Time::<Fixed>::from_hz(60.0));
        app.finish();
        app.cleanup();
        app
    }

    #[test]
    fn spatial_query_finds_an_overlapping_hurtbox() {
        let mut app = make_physics_app();
        app.init_resource::<Found>();

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

    /// Collects SpatialQuery results for the moving-owner probe into a resource.
    #[derive(Resource, Default)]
    struct MoveProbe {
        found_at_new: bool,
        found_at_old: bool,
    }

    fn move_probe_sys(spatial: SpatialQuery, mut probe: ResMut<MoveProbe>) {
        let new_pos = Vec3::new(0.0, 0.0, 5.0);
        let hits_new = spatial.shape_intersections(
            &Collider::sphere(0.5),
            new_pos,
            Quat::IDENTITY,
            &SpatialQueryFilter::default(),
        );
        let hits_old = spatial.shape_intersections(
            &Collider::sphere(0.5),
            Vec3::ZERO,
            Quat::IDENTITY,
            &SpatialQueryFilter::default(),
        );
        probe.found_at_new = !hits_new.is_empty();
        probe.found_at_old = !hits_old.is_empty();
    }

    /// Regression test: a hurtbox must follow its owner when the owner's Transform is moved.
    ///
    /// This verifies the empirical behavior: after moving the owner entity, `SpatialQuery`
    /// must find the hurtbox at the NEW position and not at the old one.
    #[test]
    fn hurtbox_tracks_a_moving_owner() {
        let mut app = make_physics_app();
        app.init_resource::<MoveProbe>();

        // Spawn the owner and attach a hurtbox at the origin.
        let owner = app.world_mut().spawn_empty().id();
        {
            let world = app.world_mut();
            let mut commands = world.commands();
            insert_hurtbox(&mut commands, owner, 0.5, Vec3::ZERO);
        }
        // Let physics register the collider.
        app.update();
        app.update();
        app.update();

        // Move the owner to a new position.
        let new_pos = Vec3::new(0.0, 0.0, 5.0);
        app.world_mut()
            .entity_mut(owner)
            .get_mut::<Transform>()
            .expect("owner should have Transform")
            .translation = new_pos;

        // Let physics propagate the transform change.
        app.update();
        app.update();
        app.update();

        // Run the probe system to capture SpatialQuery results.
        app.add_systems(Update, move_probe_sys);
        app.update();

        let probe = app.world().resource::<MoveProbe>();
        assert!(
            probe.found_at_new,
            "hurtbox must be detectable at the owner's new position (0,0,5)"
        );
        assert!(
            !probe.found_at_old,
            "hurtbox must NOT be detectable at the old position (0,0,0) after the owner moved"
        );
    }
}
