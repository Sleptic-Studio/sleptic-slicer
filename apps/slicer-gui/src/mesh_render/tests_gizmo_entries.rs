// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use glam::Vec4;

use super::*;
use crate::gizmo::SIZES;

fn frame(visibility: HandleShowing) -> HandleFrame {
    HandleFrame {
        anchor: Vec3::ZERO,
        arm_length: 1.0,
        showing: visibility,
        fixed_axes: [false; 3],
        fixed_rings: [false; 3],
        under_pointer: None,
        live_drag: None,
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

/// The red, green and blue channel of each axis's own colour, which is
/// its brightest channel.
fn own_channel(entries: &[HandleEntry; 3]) -> [f32; 3] {
    [entries[0].colour[0], entries[1].colour[1], entries[2].colour[2]]
}

#[test]
fn each_handle_points_along_its_axis() {
    for entries in [
        arrow_entries(&frame(HandleShowing::Resting)),
        ring_entries(&frame(HandleShowing::Resting)),
    ] {
        for (i, axis) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
            let m = Mat4::from_cols_array_2d(&entries[i].orient);
            assert!(
                (m * Vec4::new(1.0, 0.0, 0.0, 0.0))
                    .truncate()
                    .distance(axis)
                    < 1e-5
            );
            // A turn ring drawn round +X stays in its axis's plane.
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                assert!(
                    m.transform_point3(Vec3::new(0.0, a.cos(), a.sin()))
                        .dot(axis)
                        .abs()
                        < 1e-5
                );
            }
        }
    }
}

#[test]
fn handles_are_dim_bright_while_dragged_halfway_when_hovered_and_grey_when_locked() {
    let idle = arrow_entries(&frame(HandleShowing::Resting));
    assert!(own_channel(&idle).iter().all(|&c| near(c, 0.65)));
    assert!(near(idle[0].colour[1], 0.25 * 0.65));
    assert!(near(idle[2].colour[1], 0.45 * 0.65));
    let mut f = frame(HandleShowing::Moving { axis: Axis::X });
    f.under_pointer = Some(HandleHover::Move(Axis::Y));
    let entries = arrow_entries(&f);
    assert_eq!(
        own_channel(&entries),
        [1.0, 0.825, 0.65]
    );
    assert!(0.65 < 0.825 && 0.825 < 1.0);
    // Hovering the dragged handle keeps it bright.
    f.under_pointer = Some(HandleHover::Move(Axis::X));
    assert_eq!(arrow_entries(&f)[0].colour[0], 1.0);
    // Locked beats dragged and hovered.
    f.fixed_axes = [true, false, false];
    assert_eq!(
        arrow_entries(&f)[0].colour,
        [0.35, 0.35, 0.35, 1.0]
    );
}

#[test]
fn states_count_only_for_their_own_kind_of_handle() {
    // A turn drag and a move hover leave move and scale handles dim.
    let mut f = frame(HandleShowing::Turning { axis: Axis::X });
    f.under_pointer = Some(HandleHover::Move(Axis::Z));
    assert!(
        own_channel(&stretch_cube_entries(&f))
            .iter()
            .all(|&c| near(c, 0.65))
    );
    assert_eq!(
        own_channel(&ring_entries(&f)),
        [1.0, 0.65, 0.65]
    );
    assert_eq!(
        own_channel(&arrow_entries(&f)),
        [0.65, 0.65, 0.825]
    );
    // Scale cubes lock with their axis's move handle; rings have their own.
    f.fixed_axes = [true, false, false];
    f.fixed_rings = [false, true, false];
    assert_eq!(stretch_cube_entries(&f)[0].colour[0], 0.35);
    assert_eq!(ring_entries(&f)[1].colour[1], 0.35);
    assert_ne!(ring_entries(&f)[0].colour[0], 0.35);
    // Rings and move handles share each axis's colours.
    let idle = frame(HandleShowing::Resting);
    for (a, b) in arrow_entries(&idle)
        .iter()
        .zip(ring_entries(&idle))
    {
        assert_eq!(a.colour, b.colour);
    }
}

#[test]
fn a_dragged_scale_cube_follows_the_pointer_along_its_axis() {
    let mut f = frame(HandleShowing::Stretching { axis: Axis::Y });
    f.arm_length = 2.0;
    f.live_drag = Some(LiveDrag::Stretch {
        axis: Axis::Y,
        reach: 3.0,
    });
    let entries = stretch_cube_entries(&f);
    assert_eq!(
        own_channel(&entries),
        [0.65, 1.0, 0.65]
    );
    // Its cube, at SIZES.cube_at times the length, lands 3 along Y.
    let m = Mat4::from_cols_array_2d(&entries[1].orient);
    let cube = m.transform_point3(Vec3::X * SIZES.cube_at * f.arm_length);
    assert!(cube.distance(Vec3::Y * 3.0) < 1e-5, "{cube}");
    // The other cubes do not move.
    for i in [0, 2] {
        assert_eq!(
            Mat4::from_cols_array_2d(&entries[i].orient).col(3),
            Vec4::W
        );
    }
}

#[test]
fn the_centre_cube_is_grey_at_the_pivot() {
    let mut f = frame(HandleShowing::Resize);
    f.arm_length = 7.0;
    let entry = even_cube_entry(&f);
    let m = Mat4::from_cols_array_2d(&entry.orient);
    assert!(
        m.transform_point3(Vec3::X * SIZES.cube_at * f.arm_length)
            .length()
            < 1e-5
    );
    assert_eq!(entry.colour, [0.8, 0.8, 0.8, 1.0]);
    f.under_pointer = Some(HandleHover::Even);
    assert_eq!(even_cube_entry(&f).colour, [1.0; 4]);
}

#[test]
fn the_groups_sit_end_to_end_and_fill_the_buffer() {
    use HandleGroup::*;
    let mut next = 0;
    for group in [Arrows, Rings, Cubes, Centre] {
        assert_eq!(group.first(), next, "{group:?}");
        next += group.count();
    }
    assert_eq!(next, HANDLE_ENTRY_COUNT);
    assert_eq!([Arrows.at(2), Rings.at(0), Cubes.at(2), Centre.at(0)], [2, 3, 8, 9]);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic]
fn a_slot_past_its_group_is_refused() {
    let _ = HandleGroup::Arrows.at(3);
}
