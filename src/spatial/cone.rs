use bevy::prelude::*;

/// True if `point` lies within a cone/sector with apex `apex`, central axis `axis`
/// (need not be normalized), half-angle `half_angle_rad`, and slant `range`.
pub fn point_in_cone(apex: Vec3, axis: Vec3, half_angle_rad: f32, range: f32, point: Vec3) -> bool {
    let to_point = point - apex;
    let dist = to_point.length();
    if dist > range || dist <= f32::EPSILON {
        return dist <= range; // apex itself counts as inside
    }
    let axis_n = axis.normalize_or_zero();
    if axis_n == Vec3::ZERO {
        return true; // degenerate axis -> treat as a sphere
    }
    let cos = to_point.normalize().dot(axis_n).clamp(-1.0, 1.0);
    cos >= half_angle_rad.cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_ahead_is_inside() {
        assert!(point_in_cone(
            Vec3::ZERO,
            Vec3::Z,
            45f32.to_radians(),
            3.0,
            Vec3::new(0.0, 0.0, 2.0)
        ));
    }
    #[test]
    fn behind_is_outside() {
        assert!(!point_in_cone(
            Vec3::ZERO,
            Vec3::Z,
            45f32.to_radians(),
            3.0,
            Vec3::new(0.0, 0.0, -2.0)
        ));
    }
    #[test]
    fn beyond_range_is_outside() {
        assert!(!point_in_cone(
            Vec3::ZERO,
            Vec3::Z,
            45f32.to_radians(),
            3.0,
            Vec3::new(0.0, 0.0, 5.0)
        ));
    }
    #[test]
    fn just_outside_the_angle_is_outside() {
        let p = Vec3::new(0.0, 2.0, 1.0); // ~63 deg from +Z
        assert!(!point_in_cone(
            Vec3::ZERO,
            Vec3::Z,
            45f32.to_radians(),
            3.0,
            p
        ));
    }
    #[test]
    fn within_the_angle_is_inside() {
        let p = Vec3::new(0.0, 0.5, 2.0); // ~14 deg from +Z
        assert!(point_in_cone(
            Vec3::ZERO,
            Vec3::Z,
            45f32.to_radians(),
            3.0,
            p
        ));
    }
}
