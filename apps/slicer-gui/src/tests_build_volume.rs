// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::Vertex;

use super::*;

/// 100 x 60 x 80 mm: small and uneven, so axes cannot be mixed up.
const VOLUME: BuildVolume = BuildVolume::new(100.0, 60.0, 80.0);

fn part(min: [f32; 3], max: [f32; 3]) -> Aabb {
    Aabb::from_min_max(Vertex::new(min), Vertex::new(max))
}

fn moved(min: [f32; 3], max: [f32; 3], delta: [f32; 3]) -> [f32; 3] {
    VOLUME.allowed_move(part(min, max), Vec3::from(delta)).to_array()
}

#[test]
fn the_volume_is_centred_on_the_plate_and_rises_from_it() {
    assert_eq!(VOLUME.envelope_min_mm(), Vec3::new(-50.0, -30.0, 0.0));
    assert_eq!(VOLUME.envelope_max_mm(), Vec3::new(50.0, 30.0, 80.0));
    assert_eq!((VOLUME.width_mm(), VOLUME.depth_mm(), VOLUME.height_mm()), (100.0, 60.0, 80.0));
    let default = FALLBACK_VOLUME;
    assert_eq!((default.width_mm(), default.depth_mm(), default.height_mm()), (192.0, 120.0, 200.0));
}

#[test]
fn a_part_fits_up_to_the_walls_with_a_micron_of_slack() {
    assert!(VOLUME.holds_box(part([-10.0, -10.0, 0.0], [10.0, 10.0, 10.0])));
    assert!(VOLUME.holds_box(part([-50.0, -30.0, 0.0], [50.0, 30.0, 80.0])));
    assert!(VOLUME.holds_box(part([-50.0005, -30.0, -0.0005], [50.0005, 30.0, 80.0])));
    for (min, max) in [
        ([-10.0, -10.0, 0.0], [50.01, 10.0, 10.0]),
        ([-50.01, -10.0, 0.0], [0.0, 10.0, 10.0]),
        ([-10.0, -30.01, 0.0], [0.0, 10.0, 10.0]),
        ([-10.0, -10.0, -0.01], [0.0, 10.0, 10.0]),
        ([-10.0, -10.0, 0.0], [0.0, 10.0, 80.01]),
    ] {
        assert!(!VOLUME.holds_box(part(min, max)), "{min:?} {max:?}");
    }
    assert!(VOLUME.holds_box(Aabb::empty()));
}

#[test]
fn a_move_inside_is_kept_and_one_through_a_wall_stops_at_it() {
    let (min, max) = ([-10.0, -10.0, 0.0], [10.0, 10.0, 10.0]);
    assert_eq!(moved(min, max, [0.0; 3]), [0.0; 3]);
    assert_eq!(moved(min, max, [5.0, -3.0, 2.0]), [5.0, -3.0, 2.0]);
    // Only the axis that hits a wall is cut; the rest slides along it.
    assert_eq!(moved(min, max, [100.0, 7.0, 0.0]), [40.0, 7.0, 0.0]);
    assert_eq!(moved(min, max, [-100.0, -100.0, -5.0]), [-40.0, -20.0, 0.0]);
    assert_eq!(moved(min, max, [0.0, 0.0, 500.0]), [0.0, 0.0, 70.0]);
    // Touching a wall: no further, but away freely.
    let touching = ([-50.0, -10.0, 0.0], [-30.0, 10.0, 10.0]);
    assert_eq!(moved(touching.0, touching.1, [-1.0, 0.0, 0.0]), [0.0; 3]);
    assert_eq!(moved(touching.0, touching.1, [3.0, 0.0, 0.0]), [3.0, 0.0, 0.0]);
}

#[test]
fn a_part_already_outside_may_only_come_back() {
    // Through the +X wall by 5 mm and through the floor by 1 mm.
    let (min, max) = ([40.0, -10.0, -1.0], [55.0, 10.0, 9.0]);
    assert_eq!(moved(min, max, [2.0, 0.0, -1.0]), [0.0; 3]);
    // Back towards the inside, as far as the opposite wall allows.
    assert_eq!(moved(min, max, [-20.0, 0.0, 3.0]), [-20.0, 0.0, 3.0]);
    assert_eq!(moved(min, max, [-500.0, 0.0, 500.0]), [-90.0, 0.0, 71.0]);
    // Wider than the volume on X: X never moves; Y still does.
    assert_eq!(moved([-60.0, 0.0, 0.0], [60.0, 5.0, 5.0], [-3.0, 4.0, 0.0]), [0.0, 4.0, 0.0]);
}

#[test]
fn framing_covers_the_volume_and_any_scene_outside_it() {
    let (centre, radius) = VOLUME.framing_sphere(None);
    assert_eq!(centre, Vec3::new(0.0, 0.0, 40.0));
    assert_eq!(radius, Vec3::new(100.0, 60.0, 80.0).length() * 0.5);
    assert_eq!(VOLUME.framing_sphere(Some((Vec3::new(-1.0, -1.0, 0.0), Vec3::splat(1.0)))), (centre, radius));
    let (centre, radius) = VOLUME.framing_sphere(Some((Vec3::new(0.0, 0.0, 0.0), Vec3::new(150.0, 0.0, 0.0))));
    assert_eq!(centre, Vec3::new(50.0, 0.0, 40.0));
    assert_eq!(radius, Vec3::new(200.0, 60.0, 80.0).length() * 0.5);
    // An inverted (empty) scene box changes nothing.
    let empty = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    assert_eq!(VOLUME.framing_sphere(Some(empty)), VOLUME.framing_sphere(None));
}
