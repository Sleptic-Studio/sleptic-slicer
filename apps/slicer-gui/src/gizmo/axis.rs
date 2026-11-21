// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The three handle axes, which follow the world's axes (not the part's).
use glam::Vec3;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Axis {
    X = 0,
    Y = 1,
    Z = 2,
}

pub(crate) const ALL_AXES: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

impl Axis {
    pub const fn unit_vector(self) -> Vec3 {
        Vec3::AXES[self as usize]
    }
}

/// The plane square to `axis`, as two unit vectors `(u, v)` with
/// `axis × u = v`: the next two axes in cyclic order (Y, Z for X; Z, X for
/// Y; X, Y for Z). Angles from `u` towards `v` then turn counter-clockwise
/// about the axis by the right-hand rule; code that knows the camera turns
/// that into a sense on screen.
pub(crate) fn axis_basis(axis: Axis) -> (Vec3, Vec3) {
    let next = |step: usize| Vec3::AXES[(axis as usize + step) % 3];
    (next(1), next(2))
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::{ALL_AXES, axis_basis};

    #[test]
    fn axes_match_the_world_and_their_planes_are_right_handed() {
        let world = [Vec3::X, Vec3::Y, Vec3::Z];
        for (axis, along) in ALL_AXES.iter().zip(world) {
            assert_eq!(axis.unit_vector(), along);
            let (u, v) = axis_basis(*axis);
            assert_eq!(along.cross(u), v, "{axis:?}");
            assert!(u.dot(along) == 0.0 && v.dot(along) == 0.0 && u.dot(v) == 0.0);
        }
    }
}
