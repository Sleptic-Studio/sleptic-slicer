// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::hollow::*;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{Scene, node::LeafPayload};
use sc_sl1cer::profile::{
    AaLevel, BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
};
use std::sync::Arc;

fn cube() -> sc_mesh_core::IndexedMesh {
    sc_scene::bake_mesh_to_world(
        &sc_mesh_fixtures::unit_cube_mesh(),
        &Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
    )
    .unwrap()
}

#[test]
fn cube_volume_wall_clearance_and_empty_middle_slice() {
    let original = cube();
    let result = hollow(&original, 2.0).unwrap();
    let expected = 8000.0 - 4096.0;
    let actual = 8000.0 - result.saved_mm3;
    eprintln!(
        "20 mm cube: shell={actual} mm3, saved={} mm3, pitch={} mm",
        result.saved_mm3, result.pitch_mm
    );
    assert!((actual - expected).abs() / expected < 0.05);
    let volume = result
        .mesh
        .faces
        .iter()
        .map(|f| {
            let [a, b, c] = f.vertices.map(|i| {
                glam::DVec3::from_array(
                    result.mesh.vertices[usize::from(i)].coords().map(f64::from),
                )
            });
            a.dot(b.cross(c)) / 6.0
        })
        .sum::<f64>();
    assert!((volume - actual).abs() < 1e-6, "mesh volume {volume}");
    for v in &result.mesh.vertices[original.vertices.len()..] {
        let p = v.coords();
        assert!(p.iter().all(|a| a.abs() <= 8.0 + 1e-5));
    }
    let minimum_wall = result.mesh.vertices[original.vertices.len()..]
        .iter()
        .flat_map(|v| v.coords())
        .map(|a| 10.0 - a.abs())
        .fold(f32::INFINITY, f32::min);
    eprintln!("Minimum cube wall: {minimum_wall} mm");
    assert!((minimum_wall - 2.0).abs() / 2.0 <= 0.02);
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 10.0])),
            LeafPayload::Mesh(Arc::new(result.mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(30.0, 30.0, 30.0).unwrap(),
        PixelResolution::new(300, 300).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 200).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let slices = sc_sl1cer::slice_scene(&scene, &profile).unwrap();
    let middle = &slices[0].stack().layers()[100];
    assert_eq!(middle.polygons().len(), 1);
    assert_eq!(middle.polygons()[0].holes().len(), 1);
    assert!(sc_sl1cer::grouping::point_in_ring(
        sc_sl1cer::geom2d::Point2D::new(0.0, 0.0),
        &middle.polygons()[0].holes()[0]
    ));
}

#[test]
fn hollow_rejects_bad_inputs_and_excessive_thickness() {
    for thickness in [0.0, -1.0, f64::NAN] {
        assert_eq!(
            hollow(&cube(), thickness).unwrap_err(),
            HollowError::InvalidThickness
        );
    }
    assert_eq!(hollow(&cube(), 11.0).unwrap_err(), HollowError::NoCavity);
    let mut open = cube();
    open.faces.pop();
    assert_eq!(hollow(&open, 2.0).unwrap_err(), HollowError::InvalidMesh);
}

#[test]
fn two_auto_channels_open_the_bottom_wall_and_slice_empty() {
    let source = cube();
    let cavity = hollow(&source, 2.0).unwrap();
    assert_eq!(cavity.suggested_holes.len(), 2);
    for p in &cavity.suggested_holes {
        assert!(
            p[0].abs() < 6.1 && p[1].abs() < 6.1,
            "drains must lie inside the cavity floor, away from side walls"
        );
    }
    let holes: Vec<_> = cavity
        .suggested_holes
        .iter()
        .map(|&position| DrainHole {
            position,
            direction: [0.0, 0.0, -1.0],
            diameter_mm: 3.0,
            depth_mm: position[2] + 11.0,
        })
        .collect();
    let drained = hollow_with_holes(&source, 2.0, &holes).unwrap();
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 10.0])),
            LeafPayload::Mesh(Arc::new(drained.mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(30.0, 30.0, 30.0).unwrap(),
        PixelResolution::new(300, 300).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 200).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let slices = sc_sl1cer::slice_scene(&scene, &profile).unwrap();
    let bottom = &slices[0].stack().layers()[0];
    assert_eq!(bottom.polygons().len(), 1);
    assert_eq!(bottom.polygons()[0].holes().len(), 2);
    let middle = &slices[0].stack().layers()[100];
    assert_eq!(middle.polygons()[0].holes().len(), 1);
}

#[test]
fn every_disconnected_cavity_gets_two_suggestions() {
    let mut source = cube();
    let offset = source.vertices.len() as u32;
    let other = cube();
    source.vertices.extend(other.vertices.iter().map(|v| {
        let mut p = v.coords();
        p[0] += 30.0;
        sc_mesh_core::Vertex::new(p)
    }));
    source.faces.extend(other.faces.iter().map(|f| {
        sc_mesh_core::IndexedTriangle::from_raw_indices(
            f.normal,
            f.vertices.map(|v| u32::from(v) + offset),
        )
    }));
    let result = hollow(&source, 2.0).unwrap();
    assert_eq!(result.suggested_holes.len(), 4);
    assert_eq!(
        result
            .suggested_holes
            .iter()
            .filter(|p| p[0] < 15.0)
            .count(),
        2
    );
}

#[test]
fn invalid_drain_dimensions_are_rejected() {
    let hole = DrainHole {
        position: [0.0; 3],
        direction: [0.0, 0.0, 1.0],
        diameter_mm: -3.0,
        depth_mm: 3.0,
    };
    assert_eq!(
        hollow_with_holes(&cube(), 2.0, &[hole]).unwrap_err(),
        HollowError::InvalidDrain
    );
}

/// Lapras (about 100 x 110 x 80 mm) was refused as TooLarge: the grid could not
/// coarsen past 0.25 mm. A 120 mm cube now hollows on a coarser grid, and its
/// wall is still at least the requested thickness.
#[test]
fn plate_sized_model_hollows_on_a_coarser_grid() {
    let big = sc_scene::bake_mesh_to_world(
        &sc_mesh_fixtures::unit_cube_mesh(),
        &Transform3D::Affine(AffineTransform3D::from_uniform_scale(120.0)),
    )
    .unwrap();
    let result = hollow(&big, 2.0).expect("a 120 mm cube must hollow");
    assert!(
        result.pitch_mm > 0.25 && result.pitch_mm <= 0.5,
        "pitch {}",
        result.pitch_mm
    );
    let inner = 120.0 - 2.0 * 2.0;
    assert!(
        result.saved_mm3 < inner * inner * inner,
        "wall thinner than requested"
    );
    assert!(
        result.saved_mm3 > 0.85 * inner * inner * inner,
        "saved {}",
        result.saved_mm3
    );
}

/// The hollowing dialog showed bare variant names ("InvalidMesh") for Jaw.STL
/// (28 open edges) and the butterfly (overlapping surfaces).
#[test]
fn hollow_errors_read_as_plain_advice() {
    for error in [
        HollowError::InvalidThickness,
        HollowError::InvalidMesh,
        HollowError::TooLarge,
        HollowError::NoCavity,
        HollowError::InvalidDrain,
    ] {
        let text = error.to_string();
        assert!(text.contains(' ') && text.ends_with('.'), "{text}");
        assert_ne!(text, format!("{error:?}"));
    }
}

/// A closed UV sphere of `radius` centred on the origin.
fn sphere(radius: f64) -> sc_mesh_core::IndexedMesh {
    use sc_mesh_core::{IndexedTriangle, Vertex};
    let (rings, sectors) = (48usize, 96usize);
    let mut vertices = vec![Vertex::new([0.0, 0.0, radius as f32])];
    for r in 1..rings {
        let theta = std::f64::consts::PI * r as f64 / rings as f64;
        for s in 0..sectors {
            let phi = std::f64::consts::TAU * s as f64 / sectors as f64;
            vertices.push(Vertex::new([
                (radius * theta.sin() * phi.cos()) as f32,
                (radius * theta.sin() * phi.sin()) as f32,
                (radius * theta.cos()) as f32,
            ]));
        }
    }
    vertices.push(Vertex::new([0.0, 0.0, -radius as f32]));
    let bottom = (vertices.len() - 1) as u32;
    let at = |r: usize, s: usize| (1 + (r - 1) * sectors + s % sectors) as u32;
    let mut ids = Vec::new();
    for s in 0..sectors {
        ids.push([0, at(1, s), at(1, s + 1)]);
        ids.push([bottom, at(rings - 1, s + 1), at(rings - 1, s)]);
    }
    for r in 1..rings - 1 {
        for s in 0..sectors {
            ids.push([at(r, s), at(r + 1, s), at(r + 1, s + 1)]);
            ids.push([at(r, s), at(r + 1, s + 1), at(r, s + 1)]);
        }
    }
    let faces = ids
        .into_iter()
        .map(|t| {
            let n = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                &t.map(|i| vertices[i as usize]),
            )
            .unwrap();
            IndexedTriangle::from_raw_indices(n, t)
        })
        .collect();
    sc_mesh_core::IndexedMesh::new(None, vertices, faces)
}

/// R111 (the maintainer: hollowing "pixelates" the model): the cavity wall is
/// smooth. In a hollowed 20 mm sphere every inner vertex sits on the 18 mm
/// offset sphere to well under a cell, and the wall's faces point every which
/// way rather than along the grid's three axes as a staircase's do. The
/// exterior is the original mesh, untouched.
#[test]
fn a_hollowed_sphere_has_a_smooth_round_cavity() {
    let source = sphere(20.0);
    let result = hollow(&source, 2.0).unwrap();
    let n = source.vertices.len();
    assert_eq!(&result.mesh.vertices[..n], &source.vertices[..]);
    let worst = result.mesh.vertices[n..]
        .iter()
        .map(|v| (glam::DVec3::from_array(v.coords().map(f64::from)).length() - 18.0).abs())
        .fold(0.0, f64::max);
    assert!(
        worst < result.pitch_mm * 0.75,
        "inner wall strays {worst} mm (pitch {})",
        result.pitch_mm
    );
    let axis_aligned = result.mesh.faces[source.faces.len()..]
        .iter()
        .filter(|f| {
            let [a, b, c] = f.vertices.map(|i| {
                glam::DVec3::from_array(
                    result.mesh.vertices[usize::from(i)].coords().map(f64::from),
                )
            });
            let m = (b - a).cross(c - a).normalize().abs().max_element();
            m > 0.999
        })
        .count();
    let inner = result.mesh.faces.len() - source.faces.len();
    assert!(
        axis_aligned * 10 < inner,
        "{axis_aligned} of {inner} wall faces are axis-aligned: a staircase"
    );
    // The removed volume is the 18 mm sphere's, within a percent or two.
    let expected = 4.0 / 3.0 * std::f64::consts::PI * 18.0f64.powi(3);
    assert!(
        (result.saved_mm3 - expected).abs() / expected < 0.02,
        "{} against {expected}",
        result.saved_mm3
    );
}

fn closed_manifold(mesh: &sc_mesh_core::IndexedMesh) -> bool {
    mesh.validated_faces()
        .is_ok_and(|v| v.validate_edge_manifold().is_ok())
}

/// R111 (the maintainer: drain holes "pixelate" the whole model): a hole is
/// cut as exact geometry. A 4 mm hole straight through a solid 20 mm cube
/// leaves a closed surface, removes the prism's volume to a tenth of a
/// percent, and keeps the cube's own corners: the old voxel cut rebuilt the
/// whole cube from tens of thousands of cell faces.
#[test]
fn a_drain_hole_is_cut_exactly_without_voxelizing_the_model() {
    let source = cube();
    let hole = DrainHole {
        position: [0.0, 0.0, 10.0],
        direction: [0.0, 0.0, -1.0],
        diameter_mm: 4.0,
        depth_mm: 21.0,
    };
    let result = drill_holes(&source, &[hole]).unwrap();
    assert!(closed_manifold(&result.mesh));
    assert!(
        result.mesh.faces.len() < 2_000,
        "{} faces",
        result.mesh.faces.len()
    );
    for corner in &source.vertices {
        assert!(result.mesh.vertices.contains(corner));
    }
    let n = 51.0; // sides for a 2 mm radius: 2 pi 2 / 0.25 rounded up
    let prism = n * 4.0 * (std::f64::consts::PI / n).tan() * 20.0;
    assert!(
        (result.saved_mm3 - prism).abs() / prism < 1e-3,
        "{} against {prism}",
        result.saved_mm3
    );
}

/// A hollowed sphere drilled from both sides (a mirrored pair, as the hole
/// tool places them) stays closed and smooth: the drained volume is the
/// cavity plus two short holes through the wall, nothing more.
#[test]
fn drain_holes_into_a_hollow_sphere_keep_it_closed_and_smooth() {
    let source = sphere(20.0);
    let plain = hollow(&source, 2.0).unwrap();
    let holes: Vec<_> = [1.0, -1.0]
        .map(|side| DrainHole {
            position: [20.0 * side, 0.0, 0.0],
            direction: [-side, 0.0, 0.0],
            diameter_mm: 3.0,
            depth_mm: 3.0,
        })
        .into();
    let drained = hollow_with_holes(&source, 2.0, &holes).unwrap();
    assert!(closed_manifold(&drained.mesh));
    let extra = drained.saved_mm3 - plain.saved_mm3;
    // Two 2 mm walls of a 3 mm hole, about 14 mm3 each, a little more where
    // the round surfaces meet the hole.
    assert!(
        extra > 2.0 * 12.0 && extra < 2.0 * 20.0,
        "holes removed {extra} mm3"
    );
    assert!(drained.mesh.faces.len() < plain.mesh.faces.len() + 20_000);
}

/// A long hole slanted through a solid cube, entering one face and leaving
/// another, still closes.
#[test]
fn a_slanted_through_hole_closes() {
    let hole = DrainHole {
        position: [-10.0, 1.0, 3.0],
        direction: [1.0, 0.3, -0.4],
        diameter_mm: 2.5,
        depth_mm: 40.0,
    };
    let result = drill_holes(&cube(), &[hole]).unwrap();
    assert!(closed_manifold(&result.mesh));
    assert!(result.saved_mm3 > 50.0, "{}", result.saved_mm3);
}
