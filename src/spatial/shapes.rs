use avian3d::prelude::Collider;
use crate::assets::CollisionShape;

/// Convert an authoring shape to an Avian collider. (Cone approximated by a sphere for the
/// slice; a true cone/sector test is future work.)
pub fn to_collider(shape: &CollisionShape) -> Collider {
    match shape {
        CollisionShape::Sphere { radius } => Collider::sphere(*radius),
        CollisionShape::Capsule { radius, height } => Collider::capsule(*radius, *height),
        CollisionShape::Cone { range, .. } => Collider::sphere(*range), // slice approximation
    }
}
