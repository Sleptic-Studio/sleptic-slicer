// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    contour::*,
    geom2d::Point2D,
    profile::*,
    raster::{
        accuracy::{Accuracy, rasterize_accurate},
        pixel_map::PixelMap,
        rasterize_layer,
        rle::*,
    },
    *,
};
fn profile(n: u32) -> SliceProfile {
    SliceProfile::new(
        BuildVolume::new(4., 4., 4.).unwrap(),
        PixelResolution::new(40, 40).unwrap(),
        LayerSchedule::Uniform(UniformSchedule::checked(0.1, 0.1, n).unwrap()),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    )
}
fn rectangle(a: f64, b: f64, c: f64, d: f64) -> ClosedRing {
    ClosedRing::new(vec![
        Point2D::new(a, b),
        Point2D::new(c, b),
        Point2D::new(c, d),
        Point2D::new(a, d),
    ])
}
fn pixels(r: &LayerRaster) -> Vec<u8> {
    r.runs()
        .iter()
        .flat_map(|r| std::iter::repeat_n(r.value, r.length as usize))
        .collect()
}
#[test]
fn cube_sections_keep_triangle_vertices() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for v in &mut mesh.vertices {
        let [x, y, z] = v.coords();
        *v = sc_mesh_core::Vertex::new([x + 0.5, y + 0.5, z + 0.5]);
    }
    let scene = sc_scene::Scene::from(mesh);
    let result = slice_scene(&scene, &profile(8)).unwrap();
    assert!(!result[0].tolerant());
    for l in result[0].stack().layers() {
        assert_eq!(l.polygons().len(), 1);
        let r = l.polygons()[0].outer();
        assert_eq!(r.len(), 8);
        assert!((r.signed_area() - 1.).abs() < 1e-9)
    }
}
#[test]
fn schedule_rounding_and_validation() {
    let s = UniformSchedule::checked(0.1, 0.05, 3).unwrap();
    assert_eq!(s.iter_z().collect::<Vec<_>>(), vec![0.025, 0.1, 0.2]);
    assert!(matches!(
        UniformSchedule::checked(f64::NAN, 0., 0),
        Err(InvalidLayerSchedule::LayerHeightOutOfRange(_))
    ));
    let layers = vec![
        ContourLayer::new(1., vec![]),
        ContourLayer::new(0., vec![]),
        ContourLayer::new(f64::NAN, vec![]),
    ];
    assert!(matches!(
        ContourStack::new(layers),
        Err(ContourStackError::NonFiniteZ { idx: 2, .. })
    ))
}
#[test]
fn union_respects_shell_orientation() {
    let a = rectangle(0., 0., 2., 2.);
    let b = rectangle(1., 1., 3., 3.);
    let u = section::union(vec![a, b]).unwrap();
    assert_eq!(u.iter().map(|p| p.outer().signed_area()).sum::<f64>(), 7.);
    let mut h = rectangle(1., 1., 3., 3.);
    h.reverse();
    let u = section::union(vec![rectangle(0., 0., 4., 4.), h]).unwrap();
    assert_eq!(u.len(), 1);
    assert_eq!(
        u[0].outer().signed_area() + u[0].holes()[0].signed_area(),
        12.
    )
}
#[test]
fn sparse_fill_and_aa_coverage() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(4.).unwrap(),
        PositiveFiniteMm::new(4.).unwrap(),
        PixelResolution::new(4, 4).unwrap(),
        Mirror::default(),
    );
    let layer = ContourLayer::new(
        0.5,
        vec![Polygon::new(rectangle(-1.75, -2., -1.25, 2.), vec![]).unwrap()],
    );
    let aa = pixels(&rasterize_layer(&layer, &map, AaLevel::X4, 0.));
    assert_eq!(
        aa,
        vec![128, 0, 0, 0, 128, 0, 0, 0, 128, 0, 0, 0, 128, 0, 0, 0]
    );
    assert_eq!(
        pixels(&rasterize_layer(&layer, &map, AaLevel::X4, 51.)),
        vec![0; 16]
    );
    assert_eq!(
        pixels(&rasterize_layer(&layer, &map, AaLevel::Off, 100.)),
        vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0]
    );
}
#[test]
fn mirrors_rotation_are_permutations() {
    let layer = ContourLayer::new(
        1.,
        vec![Polygon::new(rectangle(-1.8, -0.8, 0.2, 0.2), vec![]).unwrap()],
    );
    let plate = BuildVolume::new(4., 2., 4.).unwrap();
    let res = PixelResolution::new(8, 4).unwrap();
    let plain = PixelMap::new(plate.x(), plate.y(), res, Mirror::default());
    let base = pixels(&rasterize_layer(&layer, &plain, AaLevel::Off, 0.));
    for mx in [false, true] {
        for my in [false, true] {
            for rot in [
                LcdRotation::Deg0,
                LcdRotation::Deg90,
                LcdRotation::Deg180,
                LcdRotation::Deg270,
            ] {
                let map = PixelMap::new(plate.x(), plate.y(), res, Mirror { x: mx, y: my })
                    .with_calibration(MachineCalibration::new(rot, [1., 1.]).unwrap());
                let actual = pixels(&rasterize_layer(&layer, &map, AaLevel::Off, 0.));
                for y in 0..4 {
                    for x in 0..8 {
                        let p = plain.pixel_to_mm((x as f64 + 0.5, y as f64 + 0.5));
                        let q = map.mm_to_pixel(p);
                        assert_eq!(
                            actual[q.1.floor() as usize * map.width() as usize
                                + q.0.floor() as usize],
                            base[y * 8 + x]
                        );
                        let back = map.pixel_to_mm(q);
                        assert!((back.x - p.x).abs() < 1e-12 && (back.y - p.y).abs() < 1e-12)
                    }
                }
            }
        }
    }
}
#[test]
fn erosion_and_quantization() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(12.).unwrap(),
        PositiveFiniteMm::new(12.).unwrap(),
        PixelResolution::new(120, 120).unwrap(),
        Mirror::default(),
    );
    let layer = ContourLayer::new(
        1.,
        vec![Polygon::new(rectangle(-5., -5., 5., 5.), vec![]).unwrap()],
    );
    let a = Accuracy {
        bottom_compensation_mm: 0.1,
        bottom_layers: 1,
        transition_layers: 2,
        ..Default::default()
    };
    let r = rasterize_accurate(&layer, &map, AaLevel::Off, 0., a, 2);
    assert_eq!(pixels(&r).iter().filter(|&&v| v > 0).count(), 100 * 100);
    assert_eq!(a.erosion_mm(3), 0.);
    for aa in [AaLevel::Off, AaLevel::X2, AaLevel::X4, AaLevel::X8] {
        assert_eq!(
            rasterize_layer(&layer, &map, aa, 0.),
            rasterize_accurate(&layer, &map, aa, 0., Accuracy::default(), 0)
        )
    }
}
#[test]
fn accuracy_square_bottom_transition_and_normal_layers() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(20.).unwrap(),
        PositiveFiniteMm::new(20.).unwrap(),
        PixelResolution::new(200, 200).unwrap(),
        Mirror::default(),
    );
    let layer = ContourLayer::new(
        0.1,
        vec![Polygon::new(rectangle(-5., -5., 5., 5.), vec![]).unwrap()],
    );
    let accuracy = Accuracy {
        bottom_compensation_mm: 0.5,
        xy_compensation_mm: 0.2,
        bottom_layers: 1,
        transition_layers: 1,
        ..Default::default()
    };
    for (index, side) in [94, 98, 104].into_iter().enumerate() {
        for (corrections, expected) in [(accuracy, side), (Accuracy::default(), 100)] {
            let image = pixels(&rasterize_accurate(
                &layer,
                &map,
                AaLevel::X4,
                0.,
                corrections,
                index,
            ));
            assert_eq!(
                image.iter().filter(|&&v| v == 255).count(),
                expected * expected
            );
            assert_eq!(
                image.chunks(200).filter(|row| row.contains(&255)).count(),
                expected
            );
            assert_eq!(
                image[100 * 200..101 * 200]
                    .iter()
                    .filter(|&&v| v == 255)
                    .count(),
                expected
            );
        }
    }
}
#[test]
fn cancellation_is_shared_and_prevents_progress() {
    let token = CancelToken::new();
    token.clone().cancel();
    let scene = sc_scene::Scene::from(sc_mesh_fixtures::unit_cube_mesh());
    assert!(matches!(
        slice_scene_with_fallback(
            &scene,
            &profile(8),
            &token,
            |_| panic!("unexpected progress"),
            false
        )
        .unwrap(),
        SliceOutcome::Cancelled
    ))
}
#[test]
fn rle_coalesces_rows_and_ignores_zero() {
    let mut r = RleRunAccumulator::new();
    r.push(0, 0);
    r.push(1, 2);
    r.push(1, 3);
    assert_eq!(
        r.into_runs(),
        vec![RleRun {
            value: 1,
            length: 5
        }]
    );
    let p = profile(1);
    let r = rasterize_layer(
        &ContourLayer::new(0., vec![]),
        &p.pixel_map(),
        AaLevel::X8,
        0.,
    );
    assert_eq!(
        r.runs(),
        &[RleRun {
            value: 0,
            length: 1600
        }]
    )
}

#[test]
fn touching_planes_and_cavity_sections() {
    use sc_mesh_fixtures::*;
    for (mesh, z, outers, holes) in [
        (on_edge_crossing_tetrahedron(), 0., 0, 0),
        (on_edge_skim_above_tetrahedron(), 0., 0, 0),
        (bowtie_tetrahedra(), 1., 0, 0),
        (cuboid_with_z_tunnel(), 1., 1, 1),
        (cuboid_with_pinched_z_tunnel(), 1., 1, 0),
        (cuboid_with_pinched_z_tunnel(), 0.5, 1, 1),
    ] {
        let valid = mesh
            .validated_faces()
            .unwrap()
            .validated_edge_manifold()
            .unwrap();
        let topology = sc_mesh_core::indexed_mesh::MeshTopology::from_edge_manifold(&valid);
        let faces = (0..mesh.faces.len()).collect::<Vec<_>>();
        let (layer, _) = section::section(&mesh, &faces, z, Some(&topology), false).unwrap();
        assert_eq!(layer.polygons().len(), outers, "z={z}");
        assert_eq!(
            layer
                .polygons()
                .iter()
                .map(|p| p.holes().len())
                .sum::<usize>(),
            holes,
            "z={z}"
        );
        for p in layer.polygons() {
            assert!(p.outer().signed_area() > 0.);
            for h in p.holes() {
                assert!(h.signed_area() < 0.)
            }
        }
    }
}

#[test]
fn duplicate_faces_recover_without_fragments() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    mesh.faces.extend(mesh.faces.clone());
    let scene = sc_scene::Scene::from(mesh);
    assert!(slice_scene(&scene, &profile(3)).is_err());
    let SliceOutcome::Completed(leaves) =
        slice_scene_with_fallback(&scene, &profile(3), &CancelToken::new(), |_| {}, true).unwrap()
    else {
        panic!()
    };
    assert!(leaves[0].tolerant());
    assert_eq!(leaves[0].dropped_fragments(), 0);
    assert!(
        leaves[0]
            .stack()
            .layers()
            .iter()
            .all(|l| l.polygons().len() == 1)
    );
}

// This deliberately visits every pixel and every edge, independently of row buckets.
fn reference(layer: &ContourLayer, map: &PixelMap, aa: AaLevel) -> Vec<u8> {
    let w = map.width() as usize;
    let h = map.height() as usize;
    let n = match aa {
        AaLevel::Off => 1,
        AaLevel::X2 => 2,
        AaLevel::X4 => 4,
        AaLevel::X8 => 8,
    };
    let mut output = vec![0; w * h];
    for row in 0..h {
        let mut coverage = vec![0.; w];
        for k in 0..n {
            let y = row as f64 + (k as f64 + 0.5) / n as f64;
            let mut hits = vec![];
            for p in layer.polygons() {
                for r in std::iter::once(p.outer()).chain(p.holes()) {
                    for (a, b) in r.vertices().iter().zip(r.vertices().iter().cycle().skip(1)) {
                        let a = map.mm_to_pixel(*a);
                        let b = map.mm_to_pixel(*b);
                        if (a.1 > y) != (b.1 > y) {
                            hits.push((
                                (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0,
                                if b.1 > a.1 { 1 } else { -1 },
                            ))
                        }
                    }
                }
            }
            hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let mut winding = 0;
            let mut left = 0.;
            for (x, d) in hits {
                if winding != 0 {
                    for (i, c) in coverage.iter_mut().enumerate() {
                        if aa == AaLevel::Off {
                            if (i as f64 + 0.5) >= left && (i as f64 + 0.5) < x {
                                *c = 1.
                            }
                        } else {
                            *c += (x.min(i as f64 + 1.) - left.max(i as f64)).clamp(0., 1.);
                        }
                    }
                }
                winding += d;
                left = x;
            }
        }
        for (x, c) in coverage.into_iter().enumerate() {
            output[row * w + x] = (255. * c / n as f64).round().clamp(0., 255.) as u8;
        }
    }
    output
}
#[test]
fn optimized_raster_matches_independent_full_frame_reference() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(8.).unwrap(),
        PositiveFiniteMm::new(6.).unwrap(),
        PixelResolution::new(53, 41).unwrap(),
        Mirror::default(),
    );
    let mut seed = 11u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed as f64 / u32::MAX as f64
    };
    for _ in 0..30 {
        let mut polygons = vec![];
        for _ in 0..5 {
            let x = next() * 12. - 6.;
            let y = next() * 10. - 5.;
            let r = ClosedRing::new(vec![
                Point2D::new(x, y),
                Point2D::new(x + next() * 4. + 0.1, y + next()),
                Point2D::new(x + next() * 4., y + next() * 4. + 1.),
            ]);
            polygons.push(Polygon::new_unchecked(r, vec![]));
        }
        let layer = ContourLayer::new(0., polygons);
        for aa in [AaLevel::Off, AaLevel::X2, AaLevel::X4, AaLevel::X8] {
            assert_eq!(
                pixels(&rasterize_layer(&layer, &map, aa, 0.)),
                reference(&layer, &map, aa)
            );
        }
    }
}

#[test]
fn sparse_aa_visits_only_occupied_edge_rows() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(10.).unwrap(),
        PositiveFiniteMm::new(4000.).unwrap(),
        PixelResolution::new(100, 4000).unwrap(),
        Mirror::default(),
    );
    let layer = ContourLayer::new(
        0.,
        vec![Polygon::new(rectangle(-1., -1500., 1., 1500.), vec![]).unwrap()],
    );
    raster::scanline::EDGE_ROW_SAMPLES.with(|c| c.set(0));
    let r = rasterize_layer(&layer, &map, AaLevel::X4, 0.);
    raster::scanline::EDGE_ROW_SAMPLES.with(|c| assert_eq!(c.get(), 24000));
    assert_eq!(
        r.runs().iter().map(|r| r.length as u64).sum::<u64>(),
        400000
    );
}
#[test]
fn malformed_binary_contour_never_loses_pixels() {
    let p = profile(1);
    let layer = ContourLayer::new(
        0.,
        vec![Polygon::new_unchecked(
            ClosedRing::new(vec![
                Point2D::new(f64::NAN, -1.),
                Point2D::new(1., -1.),
                Point2D::new(0., 1.),
            ]),
            vec![],
        )],
    );
    let r = rasterize_layer(&layer, &p.pixel_map(), AaLevel::Off, 0.);
    assert_eq!(r.runs().iter().map(|r| r.length as u64).sum::<u64>(), 1600);
}

#[test]
fn torus_annulus_and_two_tubes_keep_their_material() {
    use sc_mesh_core::{IndexedMesh, IndexedTriangle, Normal, Vertex, indexed_mesh::MeshTopology};
    let mut vertices = vec![];
    let mut faces = vec![];
    for i in 0..32 {
        for j in 0..32 {
            let a = i as f64 * std::f64::consts::TAU / 32.;
            let b = j as f64 * std::f64::consts::TAU / 32.;
            vertices.push(Vertex::new([
                ((3. + b.cos()) * a.cos()) as f32,
                ((3. + b.cos()) * a.sin()) as f32,
                b.sin() as f32,
            ]));
        }
    }
    for i in 0..32 {
        for j in 0..32 {
            let a = i * 32 + j;
            let b = ((i + 1) % 32) * 32 + j;
            let c = ((i + 1) % 32) * 32 + (j + 1) % 32;
            let d = i * 32 + (j + 1) % 32;
            for t in [[a, b, c], [a, c, d]] {
                faces.push(IndexedTriangle::from_raw_indices(Normal::new([0.; 3]), t));
            }
        }
    }
    let mut mesh = IndexedMesh::new(None, vertices, faces);
    for sideways in [false, true] {
        if sideways {
            for v in &mut mesh.vertices {
                let [x, y, z] = v.coords();
                *v = Vertex::new([-z, y, x]);
            }
        }
        let valid = mesh
            .validated_faces()
            .unwrap()
            .validated_edge_manifold()
            .unwrap();
        let topo = MeshTopology::from_edge_manifold(&valid);
        let ids = (0..mesh.faces.len()).collect::<Vec<_>>();
        let z = if sideways { 0. } else { 0.5 };
        let (l, _) = section::section(&mesh, &ids, z, Some(&topo), false).unwrap();
        let area = l
            .polygons()
            .iter()
            .map(|p| {
                p.outer().signed_area() + p.holes().iter().map(ClosedRing::signed_area).sum::<f64>()
            })
            .sum::<f64>();
        let expected = if sideways {
            2. * std::f64::consts::PI
        } else {
            12. * std::f64::consts::PI * 0.75f64.sqrt()
        };
        assert!(
            (area / expected - 1.).abs() < 0.05,
            "{sideways}: {area} vs {expected}"
        );
        assert_eq!(l.polygons().len(), if sideways { 2 } else { 1 });
        assert_eq!(
            l.polygons().iter().map(|p| p.holes().len()).sum::<usize>(),
            if sideways { 0 } else { 1 }
        );
    }
}

#[test]
fn positive_xy_compensation_expands_material_and_reduces_holes() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(12.).unwrap(),
        PositiveFiniteMm::new(12.).unwrap(),
        PixelResolution::new(120, 120).unwrap(),
        Mirror::default(),
    );
    let layer = ContourLayer::new(
        0.,
        vec![Polygon::new(rectangle(-5., -5., 5., 5.), vec![]).unwrap()],
    );
    let a = Accuracy {
        xy_compensation_mm: 0.1,
        ..Default::default()
    };
    let r = rasterize_accurate(&layer, &map, AaLevel::Off, 0., a, 0);
    assert_eq!(pixels(&r).iter().filter(|&&v| v > 0).count(), 102 * 102);
    let mut hole = rectangle(-1., -1., 1., 1.);
    hole.reverse();
    let layer = ContourLayer::new(
        0.,
        vec![Polygon::new(rectangle(-5., -5., 5., 5.), vec![hole]).unwrap()],
    );
    for (d, expected) in [(0.1, 102 * 102 - 18 * 18), (-0.1, 98 * 98 - 22 * 22)] {
        let a = Accuracy {
            xy_compensation_mm: d,
            ..Default::default()
        };
        let r = rasterize_accurate(&layer, &map, AaLevel::Off, 0., a, 0);
        assert_eq!(pixels(&r).iter().filter(|&&v| v > 0).count(), expected);
    }
}

#[test]
fn disjoint_aa_spans_skip_zero_coverage_work() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(100.).unwrap(),
        PositiveFiniteMm::new(2.).unwrap(),
        PixelResolution::new(400, 8).unwrap(),
        Mirror::default(),
    );
    let layer = ContourLayer::new(
        0.,
        (0..80)
            .map(|i| {
                let x = -49.9 + i as f64 * 1.2;
                Polygon::new(rectangle(x, -0.8, x + 0.35, 0.8), vec![]).unwrap()
            })
            .collect(),
    );
    for aa in [AaLevel::X2, AaLevel::X4, AaLevel::X8] {
        raster::scanline::COVERAGE_SPAN_SAMPLES.with(|c| c.set(0));
        let actual = rasterize_layer(&layer, &map, aa, 0.);
        assert_eq!(pixels(&actual), reference(&layer, &map, aa));
        // Each span touches at most three boundary intervals here. Scanning
        // every span for every boundary would require over a million visits.
        raster::scanline::COVERAGE_SPAN_SAMPLES.with(|c| {
            assert!(c.get() > 0 && c.get() <= 80 * 8 * 8 * 3, "{}", c.get());
        });
    }
}

#[test]
fn compressed_quantization_matches_dense_rounding() {
    let map = PixelMap::new(
        PositiveFiniteMm::new(8.).unwrap(),
        PositiveFiniteMm::new(6.).unwrap(),
        PixelResolution::new(53, 41).unwrap(),
        Mirror::default(),
    );
    let layer = ContourLayer::new(
        0.,
        vec![
            Polygon::new(
                ClosedRing::new(vec![
                    Point2D::new(-3.73, -2.41),
                    Point2D::new(3.81, -1.23),
                    Point2D::new(1.37, 2.89),
                ]),
                vec![],
            )
            .unwrap(),
        ],
    );
    let base = pixels(&rasterize_layer(&layer, &map, AaLevel::X8, 0.));
    for levels in 2..=256 {
        for erosion in [0., 0.001] {
            raster::accuracy::DENSE_PIXELS.with(|c| c.set(0));
            let actual = rasterize_accurate(
                &layer,
                &map,
                AaLevel::X8,
                0.,
                Accuracy {
                    aa_levels: levels,
                    bottom_compensation_mm: erosion,
                    bottom_layers: 1,
                    ..Default::default()
                },
                0,
            );
            let m = u32::from(levels) - 1;
            let expected: Vec<_> = base
                .iter()
                .map(|&v| {
                    let b = (u32::from(v) * m + 127) / 255;
                    ((255 * b + m / 2) / m) as u8
                })
                .collect();
            assert_eq!(pixels(&actual), expected);
            raster::accuracy::DENSE_PIXELS.with(|c| assert_eq!(c.get(), 0));
            assert!(actual.runs().windows(2).all(|r| r[0].value != r[1].value));
        }
    }
}

#[test]
fn schedule_iteration_and_error_priority_contract() {
    let schedule = UniformSchedule::checked(0.2, 0.3, 3).unwrap();
    assert_eq!(schedule.iter_z().collect::<Vec<_>>(), vec![0.15, 0.5_f64.mul_add(0.2, 0.3), 1.5_f64.mul_add(0.2, 0.3)]);
    assert_eq!(schedule.iter_slab_tops().collect::<Vec<_>>(), vec![0.3, 1.0_f64.mul_add(0.2, 0.3), 2.0_f64.mul_add(0.2, 0.3)]);
    assert!(matches!(UniformSchedule::checked(0., 0., 0), Err(InvalidLayerSchedule::LayerHeightOutOfRange(_))));
    assert!(matches!(UniformSchedule::checked(1., 0., 0), Err(InvalidLayerSchedule::FirstLayerHeightOutOfRange(_))));
    assert!(matches!(UniformSchedule::checked(1., 1., 0), Err(InvalidLayerSchedule::ZeroTotalLayers)));
    assert_eq!(profile(1).with_support_overhang_angle(0.).support_overhang_angle(), 45.);
    assert_eq!(profile(1).with_support_overhang_angle(90.).support_overhang_angle(), 45.);
    assert_eq!(profile(1).with_support_overhang_angle(32.).support_overhang_angle(), 32.);
}

#[test]
fn progress_and_mid_slice_cancellation_contract() {
    let scene = sc_scene::Scene::from(sc_mesh_fixtures::unit_cube_mesh());
    let reports = std::sync::Mutex::new(Vec::new());
    let outcome = slice_scene_with_fallback(&scene, &profile(8), &CancelToken::new(), |p| {
        reports.lock().unwrap().push(p);
    }, false).unwrap();
    assert!(matches!(outcome, SliceOutcome::Completed(_)));
    let mut reports = reports.into_inner().unwrap();
    assert!(reports.iter().all(|p| p.total == 8));
    reports.sort_by_key(|p| p.completed);
    assert_eq!(reports.iter().map(|p| p.completed).collect::<Vec<_>>(), (1..=8).collect::<Vec<_>>());

    let token = CancelToken::new();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let outcome = slice_scene_with_fallback(&scene, &profile(8), &token, |_| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        token.cancel();
    }, false).unwrap();
    assert!(matches!(outcome, SliceOutcome::Cancelled));
    assert!(calls.load(std::sync::atomic::Ordering::Relaxed) >= 1);
}

/// Quarter-millimetre layers, sampled at 0.125, 0.375, 0.625 and 0.875 mm:
/// planes a face at 0.375 meets exactly.
fn quarter_layers() -> SliceProfile {
    SliceProfile::new(
        BuildVolume::new(4., 4., 4.).unwrap(),
        PixelResolution::new(40, 40).unwrap(),
        LayerSchedule::Uniform(UniformSchedule::checked(0.25, 0.25, 4).unwrap()),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    )
}

fn layer_areas(stack: &ContourStack) -> Vec<(usize, f64)> {
    stack
        .layers()
        .iter()
        .map(|layer| {
            let area: f64 = layer.polygons().iter().map(|p| p.outer().signed_area() - p.holes().iter().map(|h| -h.signed_area()).sum::<f64>()).sum();
            // Rounded to a millionth: the slicer's fixed-point grid leaves ulps.
            (layer.polygons().len(), (area * 1e6_f64).round() / 1e6)
        })
        .collect()
}

#[test]
fn a_step_face_on_a_sampling_plane_still_slices_whole() {
    let scene = sc_scene::Scene::from(sc_mesh_fixtures::stepped_block());
    let result = slice_scene(&scene, &quarter_layers()).expect("a closed solid slices");
    let areas = layer_areas(result[0].stack());
    assert_eq!(areas.len(), 4);
    assert_eq!(areas[0], (1, 2.0), "under the step: the whole 2 x 1 footprint");
    // At the step's own height either reading is fair (the step's top or
    // just above it), but the layer is one whole rectangle, never empty,
    // cut short or in slivers.
    let (count, area) = areas[1];
    assert_eq!(count, 1, "{areas:?}");
    assert!(area == 1.0 || area == 2.0, "{areas:?}");
    assert_eq!(areas[2], (1, 1.0));
    assert_eq!(areas[3], (1, 1.0));
}

#[test]
fn a_top_face_on_a_sampling_plane_is_all_or_nothing() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for v in &mut mesh.vertices {
        let [x, y, z] = v.coords();
        *v = sc_mesh_core::Vertex::new([x, y, (z + 0.5) * 0.375]);
    }
    let scene = sc_scene::Scene::from(mesh);
    let result = slice_scene(&scene, &quarter_layers()).expect("a closed solid slices");
    let areas = layer_areas(result[0].stack());
    assert_eq!(areas[0], (1, 1.0));
    assert!(areas[1] == (1, 1.0) || areas[1] == (0, 0.0), "{areas:?}");
    assert_eq!(&areas[2..], &[(0, 0.0), (0, 0.0)]);
}

/// The case reported for another slicer: 0.05 mm layers, sampled at their
/// middles, and a face 3.175 mm (1/8 inch) up, the middle of layer 64.
#[test]
fn an_eighth_inch_step_with_twentieth_millimetre_layers() {
    let profile = SliceProfile::new(
        BuildVolume::new(4., 4., 5.).unwrap(),
        PixelResolution::new(40, 40).unwrap(),
        LayerSchedule::Uniform(UniformSchedule::checked(0.05, 0.05, 80).unwrap()),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let scene = sc_scene::Scene::from(sc_mesh_fixtures::stepped_block_with(3.175, 4.0));
    let result = slice_scene(&scene, &profile).expect("a closed solid slices");
    for (index, (count, area)) in layer_areas(result[0].stack()).into_iter().enumerate() {
        assert_eq!(count, 1, "layer {index}");
        let expected: &[f64] = match index {
            ..63 => &[2.0],
            63 => &[1.0, 2.0],
            _ => &[1.0],
        };
        assert!(expected.contains(&area), "layer {index}: area {area}");
    }
}
