// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

#[test]
fn every_face_maps_to_its_axis_view_and_center_is_pickable() {
    let views = [
        ViewAxis::Top,
        ViewAxis::Bottom,
        ViewAxis::Front,
        ViewAxis::Back,
        ViewAxis::Left,
        ViewAxis::Right,
    ];
    for (id, expected) in views.into_iter().enumerate() {
        let view = face_view(id as i32).unwrap();
        assert_eq!(
            std::mem::discriminant(&view),
            std::mem::discriminant(&expected)
        );
        let mut camera = ViewCamera::new(Vec3::ZERO, 100.0);
        camera.snap_to_axis(view);
        assert_eq!(pick(&camera, Vec2::splat(CUBE_CENTRE)), Some(id as i32));
    }
    assert!(face_view(-1).is_none());
    assert!(face_view(6).is_none());
}

#[test]
fn orbit_changes_cube_and_background_is_not_a_face() {
    let mut camera = ViewCamera::new(Vec3::ZERO, 100.0);
    let before = faces(&camera)[0].points;
    camera.turn_by_drag(100.0, 50.0, 800.0);
    assert_ne!(before, faces(&camera)[0].points);
    assert_eq!(pick(&camera, Vec2::ZERO), None);
    for face in faces(&camera) {
        let center = face.points.into_iter().sum::<Vec2>() / 4.0;
        assert_eq!(pick(&camera, center), Some(face.id));
    }
}

#[test]
fn plate_drag_plane_delta_snaps_and_respects_parent_scale() {
    use super::super::handle_drag::{DragKind, drag_to_transform};
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    let ray = crate::picking::PickRay {
        start: Vec3::new(3.35, -2.65, 10.0),
        heading: -Vec3::Z,
    };
    let op = DragKind::Plate {
        anchor: Vec3::new(1.0, 1.0, 0.0),
        snap: false,
        press: (0.0, 0.0),
        activated: true,
    };
    let parent = Transform3D::Affine(AffineTransform3D::from_uniform_scale(2.0));
    let original = Transform3D::Affine(AffineTransform3D::from_translation([10.0, 20.0, 5.0]));
    let (t, _) = drag_to_transform(op, &ray, Vec3::ZERO, &parent, &original, 0.0, 0.0).unwrap();
    let Transform3D::Affine(t) = t else {
        panic!("expected affine");
    };
    assert!((t.translation()[0] - 11.175).abs() < 0.00001);
    assert!((t.translation()[1] - 18.175).abs() < 0.00001);
    assert_eq!(t.translation()[2], 5.0);
    let op = DragKind::Plate {
        anchor: Vec3::new(1.0, 1.0, 0.0),
        snap: true,
        press: (0.0, 0.0),
        activated: true,
    };
    let (t, _) = drag_to_transform(
        op,
        &ray,
        Vec3::ZERO,
        &Transform3D::Identity,
        &original,
        0.0,
        0.0,
    )
    .unwrap();
    let Transform3D::Affine(t) = t else {
        panic!("expected affine");
    };
    assert_eq!(*t.translation(), [12.0, 16.0, 5.0]);
    assert!(
        drag_to_transform(
            op,
            &crate::picking::PickRay {
                heading: Vec3::X,
                ..ray
            },
            Vec3::ZERO,
            &parent,
            &original,
            0.0,
            0.0
        )
        .is_none()
    );
}

/// Sleptic: cube labels must sit on their faces, not billboard. Looking straight
/// at each face, its label must read left to right (not mirrored) and stay
/// inside the face; after an orbit the label moves with the face.
#[test]
fn face_labels_are_printed_on_their_faces() {
    let xs = |path: &str| -> Vec<f32> {
        path.split(|c: char| c.is_ascii_alphabetic())
            .filter_map(|chunk| chunk.split_whitespace().next())
            .filter_map(|v| v.parse::<f32>().ok())
            .collect()
    };
    for id in 0..6 {
        let mut camera = ViewCamera::new(Vec3::ZERO, 100.0);
        camera.snap_to_axis(face_view(id).unwrap());
        // An asymmetric label: 'L' starts with a stem at its left edge.
        let path = face_label_path(&camera, id, &crate::surface_text::outline("LT"));
        let x = xs(&path);
        let first = x[0];
        let (lo, hi) = x
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(
            lo > 75.0 - 34.0 && hi < 75.0 + 34.0,
            "face {id}: {lo}..{hi}"
        );
        assert!(first < 75.0, "face {id}: label reads mirrored ({first})");
    }
    let mut camera = ViewCamera::new(Vec3::ZERO, 100.0);
    let text = crate::surface_text::outline("Top");
    let before = face_label_path(&camera, 0, &text);
    camera.turn_by_drag(100.0, 50.0, 800.0);
    assert_ne!(before, face_label_path(&camera, 0, &text));
}

// R44 (Sleptic, Fusion reference): corners and the edges between faces are
// selectable too. The pointer is ray-cast into the cube; a hit near one edge
// picks that edge, near two a corner.
#[test]
fn r44_cube_picks_faces_edges_and_corners() {
    let camera = ViewCamera::new(Vec3::ZERO, 100.0);
    let view = camera.world_to_view();
    let at = |p: Vec3| {
        let v = view.transform_vector3(p) * CUBE_SCALE;
        Vec2::new(CUBE_CENTRE + v.x, CUBE_CENTRE - v.y)
    };
    // Default view looks from front (-Y), right (+X) and above (+Z).
    assert_eq!(pick_target(&camera, at(Vec3::new(0.0, -1.0, 0.0))), Some(Vec3::new(0.0, -1.0, 0.0)));
    assert_eq!(pick_target(&camera, at(Vec3::new(0.95, -0.95, 0.95))), Some(Vec3::new(1.0, -1.0, 1.0)));
    assert_eq!(pick_target(&camera, at(Vec3::new(0.0, -0.95, 0.95))), Some(Vec3::new(0.0, -1.0, 1.0)));
    assert_eq!(pick_target(&camera, Vec2::new(2.0, 2.0)), None);
    // The corner highlight reaches the rounded corner itself.
    let points = |path: &str| -> Vec<Vec2> {
        path.split(['M', 'L', 'Z'])
            .filter_map(|xy| {
                let mut it = xy.split_whitespace().map(|v| v.parse::<f32>().ok());
                Some(Vec2::new(it.next()??, it.next()??))
            })
            .collect()
    };
    let corner = points(&target_path(&camera, Vec3::new(1.0, -1.0, 1.0)));
    let tip = at(Vec3::splat(1.0 - FILLET) * Vec3::new(1.0, -1.0, 1.0)
        + Vec3::new(1.0, -1.0, 1.0).normalize() * FILLET);
    assert!(corner.iter().any(|p| p.distance(tip) < 2.0), "corner patch highlighted");
}

/// Sleptic: the fillet made the blue highlight streaky and "cube like in the
/// corners". The zones are the cube's own shapes now: a face lights exactly
/// its flat face, an edge only its rounded band, a corner only its rounded
/// patch, as one path (no per-facet outlines or doubled seams).
#[test]
fn cube_highlight_matches_the_rounded_shapes() {
    let camera = ViewCamera::new(Vec3::ZERO, 100.0);
    let polygons = |path: &str| path.matches('M').count();
    let front = target_path(&camera, -Vec3::Y);
    assert_eq!(polygons(&front), 1, "the flat face alone: {front}");
    let face = faces(&camera).into_iter().find(|f| f.id == 2).unwrap();
    assert!(front.starts_with(&format!("M{:.2} {:.2} ", face.points[0].x, face.points[0].y)));
    // An edge is its fillet band: FILLET_STEPS facets, none on a flat face.
    let edge = target_path(&camera, Vec3::new(0.0, -1.0, 1.0));
    assert_eq!(polygons(&edge), FILLET_STEPS, "{edge}");
    // A corner is its rounded patch: FILLET_STEPS^2 small triangles.
    let corner = target_path(&camera, Vec3::new(1.0, -1.0, 1.0));
    assert_eq!(polygons(&corner), FILLET_STEPS * FILLET_STEPS, "{corner}");
    // Each piece belongs to exactly one zone, so nothing is lit twice.
    assert!(rounding(&camera).iter().all(|p| p.zone.abs().element_sum() >= 2.0));
}

/// Sleptic (R54): the view cube is smaller and its edges and corners are
/// rounded. Faces stop short of the edges; fillet facets fill the gap.
#[test]
fn r54_view_cube_is_smaller_and_filleted() {
    assert!(CUBE_SCALE < 34.0 && CUBE_BOX < 150.0);
    let camera = ViewCamera::new(Vec3::ZERO, 100.0);
    let round = rounding(&camera);
    // From the default corner view: 3 visible edges fully, several more in
    // part, and the near corner, all facing the camera.
    assert!(round.len() >= 20, "{}", round.len());
    assert!(round.iter().all(|p| p.facing > 0.0));
    // Flat faces now end at 1 - FILLET, inside the old square face.
    let face = &faces(&camera)[0];
    let side = face.points[0].distance(face.points[1]);
    assert!(side < 2.0 * CUBE_SCALE * (1.0 - FILLET) + 0.01, "{side}");
}

#[test]
fn r44_view_direction_points_the_camera_at_an_edge_or_corner() {
    let mut camera = ViewCamera::new(Vec3::ZERO, 100.0);
    camera.look_from_direction(Vec3::new(1.0, -1.0, 1.0));
    let toward_eye = (camera.eye_position() - camera.look_at).normalize();
    assert!(toward_eye.dot(Vec3::new(1.0, -1.0, 1.0).normalize()) > 0.999);
    camera.look_from_direction(Vec3::Z);
    assert!((camera.eye_position() - camera.look_at).normalize().z > 0.99);
}
