// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_scene::node::LeafPayload;
use std::sync::Arc;

#[test]
fn direct_raster_cache_checks_each_exact_key_once_in_parallel() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cache = DirectRasterCache::default();
    let checks = AtomicUsize::new(0);
    let verdicts: Vec<_> = (0..64usize)
        .into_par_iter()
        .map(|i| {
            // Distinct geometry/schedule bits must never share either verdict.
            let key = vec![i as u32 % 2, 0, 1.0f32.to_bits()];
            cache.get_or_check(key, || {
                checks.fetch_add(1, Ordering::SeqCst);
                i % 2 == 0
            })
        })
        .collect();
    assert_eq!(checks.load(Ordering::SeqCst), 2);
    assert_eq!(verdicts, (0..64).map(|i| i % 2 == 0).collect::<Vec<_>>());
    assert!(!cache.get_or_check(vec![0, 0, 1.0f32.to_bits() + 1], || false));
    assert!(cache.get_or_check(vec![0, 1, 1.0f32.to_bits()], || true));
}

#[test]
fn lazy_direct_keeps_eager_winner_shortest_and_availability() {
    let mesh = sc_mesh_fixtures::unit_cube_mesh();
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let template = evaluate(
        &vertices,
        &triangle_sample(&mesh, &vertices),
        DQuat::IDENTITY,
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        0.5,
        false,
    );
    let compare = |a: &&Candidate, b: &&Candidate| {
        if a.metrics.zero_support || b.metrics.zero_support {
            direct_pose_order(a, b, 0.05)
        } else {
            a.source.cmp(&b.source)
        }
    };
    for mask in 0..16 {
        for supported_time in [0.0, 1.0, 2.0, 3.0, 5.0] {
            let optimistic: Vec<_> = (0..5)
                .map(|i| {
                    let mut c = template.clone();
                    c.source = i;
                    c.metrics.fits = true;
                    c.metrics.zero_support = i < 4;
                    c.metrics.height = [2.0, 1.0, 1.0, 3.0, 1.0][i];
                    c.metrics.print_minutes = if i == 4 {
                        supported_time
                    } else {
                        c.metrics.height
                    };
                    c.metrics.bed_contact_area = i as f64;
                    c
                })
                .collect();
            let eager: Vec<_> = optimistic
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let mut c = c.clone();
                    if i < 4 && mask & (1 << i) == 0 {
                        c.metrics.zero_support = false;
                        c.metrics.print_minutes += 2.0;
                        c.metrics.fits = i % 2 == 0;
                    }
                    c
                })
                .collect();
            let mut lazy = optimistic;
            let mut checks = 0;
            resolve_direct_finalists(&mut lazy, 0.05, |i| {
                checks += 1;
                eager[i].clone()
            });
            let chosen = |cs: &Vec<Candidate>| {
                cs.iter()
                    .filter(|c| c.metrics.fits)
                    .min_by(compare)
                    .unwrap()
                    .source
            };
            let shortest = |cs: &Vec<Candidate>| {
                cs.iter()
                    .filter(|c| c.metrics.fits)
                    .min_by(|a, b| a.metrics.print_minutes.total_cmp(&b.metrics.print_minutes))
                    .map(|c| format!("{} {:?}", c.source, c.metrics))
                    .unwrap()
            };
            assert_eq!(chosen(&lazy), chosen(&eager));
            assert_eq!(shortest(&lazy), shortest(&eager));
            assert_eq!(
                lazy.iter()
                    .any(|c| c.metrics.fits && c.metrics.zero_support),
                eager
                    .iter()
                    .any(|c| c.metrics.fits && c.metrics.zero_support)
            );
            if mask == 15 {
                assert!(checks < 4);
            }
        }
    }
}

#[test]
fn direct_raster_key_preserves_vertex_and_schedule_bits() {
    let mesh = sc_mesh_fixtures::unit_cube_mesh();
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let c = evaluate(
        &vertices,
        &triangle_sample(&mesh, &vertices),
        DQuat::IDENTITY,
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        0.5,
        false,
    );
    let key = direct_raster_key(&c, &mesh);
    assert_eq!(key, direct_raster_key(&c.clone(), &mesh.clone()));
    let mut changed = c.clone();
    changed.metrics.height = f64::from_bits(c.metrics.height.to_bits() + 1);
    assert_ne!(key, direct_raster_key(&changed, &mesh));
    let mut changed_mesh = mesh.clone();
    let mut xyz = changed_mesh.vertices[0].coords();
    xyz[0] = f32::from_bits(xyz[0].to_bits() + 1);
    changed_mesh.vertices[0] = sc_mesh_core::Vertex::new(xyz);
    assert_ne!(key, direct_raster_key(&c, &changed_mesh));
}

#[test]
fn direct_raster_cache_keeps_finalized_metrics_and_placement() {
    use sc_print_primitives::{Mirror, PixelResolution, PixelSizeMicrons};
    let volume = BuildVolume::new(20.0, 20.0, 20.0).unwrap();
    let context = Context {
        hardware: Some(
            sc_print_profiles::PrinterHardware::new(
                volume,
                PixelResolution::new(400, 400).unwrap(),
                PixelSizeMicrons::new(50.0, 50.0).unwrap(),
                Mirror::default(),
            )
            .unwrap(),
        ),
        ..Default::default()
    };
    let (scene, id) = fixture([10.0, 10.0, 5.0], 0.0);
    let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let triangles = triangle_sample(&mesh, &vertices);
    let tree = crate::hollow::Tree::from_mesh(&mesh);
    let cache = DirectRasterCache::default();
    for yaw in [0.0, 0.0, 0.001] {
        let mut uncached = evaluate(
            &vertices,
            &triangles,
            DQuat::from_rotation_z(yaw),
            volume,
            45.0f64.to_radians().cos(),
            false,
        );
        contextualize(&mut uncached, volume, context);
        let mut cached = uncached.clone();
        finalize(
            &mut uncached,
            &mesh,
            &tree,
            volume,
            context,
            45.0,
            Some(DVec2::ZERO),
            None,
        );
        finalize(
            &mut cached,
            &mesh,
            &tree,
            volume,
            context,
            45.0,
            Some(DVec2::ZERO),
            Some(&cache),
        );
        assert!(cached.metrics.zero_support);
        assert_eq!(
            format!("{:?}", cached.metrics),
            format!("{:?}", uncached.metrics)
        );
        assert_eq!(cached.rotation, uncached.rotation);
        assert_eq!(
            (
                cached.min,
                cached.max,
                cached.footprint_min,
                cached.footprint_max
            ),
            (
                uncached.min,
                uncached.max,
                uncached.footprint_min,
                uncached.footprint_max
            )
        );
    }
    assert_eq!(cache.entries.lock().unwrap().len(), 2);
    for preset in [Preset::Balanced, Preset::Fastest] {
        let eager =
            search_impl(&scene, id, volume, 45.0, preset.weights(), context, false).unwrap();
        let lazy = search_impl(&scene, id, volume, 45.0, preset.weights(), context, true).unwrap();
        // Includes chosen, shortest, original, availability and the exact delta.
        assert_eq!(format!("{lazy:?}"), format!("{eager:?}"));
    }
}

fn fixture(scale: [f64; 3], yaw: f64) -> (Scene, NodeId) {
    let mut scene = Scene::new();
    let transform = Transform3D::Affine(AffineTransform3D::from_rotation_z(yaw)).compose(
        &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale(scale)),
    );
    let id = scene
        .add_leaf(
            None,
            transform,
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    (scene, id)
}
/// A box tilted about X so it rests on an edge: no flat base, real overhangs.
/// R38 keeps flat bases on the plate, so support-scoring tests need this.
fn tilted_fixture(scale: [f64; 3], tilt: f64) -> (Scene, NodeId) {
    let mut scene = Scene::new();
    let tilted = Transform3D::Affine(AffineTransform3D::from_rotation_x(tilt)).compose(
        &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale(scale)),
    );
    // Rest it on the plate like every model the app places.
    let mut probe = sc_mesh_fixtures::unit_cube_mesh();
    for v in &mut probe.vertices {
        *v = tilted.apply_to_vertex(*v);
    }
    let min_z = probe
        .vertices
        .iter()
        .map(|v| f64::from(v[2]))
        .fold(f64::INFINITY, f64::min);
    let transform = Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, -min_z]))
        .compose(&tilted);
    let id = scene
        .add_leaf(
            None,
            transform,
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    (scene, id)
}
#[test]
fn fastest_preserves_flat_plate_and_cube() {
    for scale in [[20.0, 20.0, 1.0], [10.0; 3]] {
        let (scene, id) = fixture(scale, 0.0);
        let result = search(
            &scene,
            id,
            BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
            45.0,
            Preset::Fastest.weights(),
        )
        .unwrap();
        assert!(result.chosen.zero_support);
        assert!((result.chosen.height - scale[2]).abs() < 1e-5);
        assert!(result.chosen.stair_area < 1e-8);
    }
}
#[test]
fn yaw_recovers_long_part_on_narrow_plate() {
    let (scene, id) = fixture([180.0, 10.0, 5.0], 0.7);
    let result = search_with_context(
        &scene,
        id,
        BuildVolume::new(185.0, 20.0, 8.0).unwrap(),
        45.0,
        Preset::Fastest.weights(),
        Context {
            supports: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!result.original.fits);
    assert!(result.chosen.fits);
    assert!(!result.retained);
    let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    let (lo, hi) = bounds(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(result.delta.apply_to_vertex(*v).coords().map(f64::from))),
    )
    .unwrap();
    assert!(
        hi.x - lo.x <= 185.0001 && hi.y - lo.y <= 20.0001 && hi.z <= 8.0001 && lo.z.abs() < 1e-3
    );
}
#[test]
fn nothing_fits_keeps_exact_original() {
    let (scene, id) = fixture([200.0; 3], 0.7);
    let result = search(
        &scene,
        id,
        BuildVolume::new(10.0, 10.0, 10.0).unwrap(),
        45.0,
        Preset::Balanced.weights(),
    )
    .unwrap();
    assert!(result.retained);
    assert_eq!(result.delta, Transform3D::Identity);
    assert!(!result.chosen.fits);
}
#[test]
fn fitting_original_cannot_be_replaced_by_oversize_choice() {
    let (scene, id) = fixture([180.0, 100.0, 4.0], 0.0);
    for preset in [
        Preset::Balanced,
        Preset::FewestSupports,
        Preset::Fastest,
        Preset::BestQuality,
        Preset::SmallestFootprint,
    ] {
        let result = search(
            &scene,
            id,
            BuildVolume::new(192.0, 120.0, 30.0).unwrap(),
            45.0,
            preset.weights(),
        )
        .unwrap();
        assert!(result.original.fits && result.chosen.fits);
    }
}
#[test]
fn footprint_and_fastest_presets_trade_height_for_space() {
    let (scene, id) = fixture([40.0, 20.0, 2.0], 0.0);
    let run = |p: Preset| {
        search(
            &scene,
            id,
            BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
            45.0,
            p.weights(),
        )
        .unwrap()
    };
    let fast = run(Preset::Fastest);
    let small = run(Preset::SmallestFootprint);
    assert!(fast.chosen.height <= small.chosen.height);
    assert_eq!(small.chosen.height, fast.chosen.height);
    assert!(small.chosen.zero_support);
}
#[test]
fn hull_removes_collinear_and_inner_points() {
    let h = hull(vec![
        DVec2::ZERO,
        DVec2::X,
        DVec2::Y,
        DVec2::ONE,
        DVec2::splat(0.5),
    ]);
    assert_eq!(h.len(), 4);
}

#[test]
fn exact_yaw_finds_narrow_diagonal_fit_not_a_hull_edge_rectangle() {
    let (scene, id) = fixture([180.0, 10.0, 5.0], 0.0);
    let volume = BuildVolume::new(135.0, 135.0, 6.0).unwrap();
    let result = search_with_context(
        &scene,
        id,
        volume,
        45.0,
        Preset::Fastest.weights(),
        Context {
            supports: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!result.original.fits);
    assert!(result.chosen.fits);
    assert!(result.chosen.width <= 135.00001 && result.chosen.depth <= 135.00001);
}

#[test]
fn presets_move_support_speed_quality_and_footprint_in_expected_directions() {
    let (scene, id) = fixture([40.0, 20.0, 2.0], 0.0);
    let volume = BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    let run = |p: Preset| {
        search(&scene, id, volume, 45.0, p.weights())
            .unwrap()
            .chosen
    };
    let fast = run(Preset::Fastest);
    let support = run(Preset::FewestSupports);
    let balanced = run(Preset::Balanced);
    let quality = run(Preset::BestQuality);
    let compact = run(Preset::SmallestFootprint);
    assert_eq!(support.support_area, 0.0);
    assert_eq!(fast.support_area, 0.0);
    assert!(fast.height <= balanced.height && fast.height <= support.height);
    assert!(quality.stair_area <= balanced.stair_area + 1e-7);
    assert_eq!(compact.height, fast.height);
}
#[test]
fn l_bracket_has_a_single_minimum_patch_after_support_orientation() {
    let outline = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 2.0],
        [2.0, 2.0],
        [2.0, 10.0],
        [0.0, 10.0],
    ];
    let mut vertices = Vec::new();
    for z in [0.0, 2.0] {
        for [x, y] in outline {
            vertices.push(sc_mesh_core::Vertex::new([x, y, z]));
        }
    }
    let mut faces = Vec::new();
    let mut triangles = Vec::new();
    for [a, b, c] in [[0u32, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]] {
        triangles.push([c, b, a]);
        triangles.push([a + 6, b + 6, c + 6]);
    }
    for i in 0..6u32 {
        let j = (i + 1) % 6;
        triangles.extend([[i, j, j + 6], [i, j + 6, i + 6]]);
    }
    for ids in triangles {
        let normal = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
            &ids.map(|i| vertices[i as usize]),
        )
        .unwrap();
        faces.push(sc_mesh_core::IndexedTriangle::from_raw_indices(normal, ids));
    }
    let mesh = sc_mesh_core::IndexedMesh::new(None, vertices, faces);
    mesh.validated_faces()
        .unwrap()
        .validate_edge_manifold()
        .unwrap();
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_rotation_y(0.7)),
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let result = search(
        &scene,
        id,
        BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
        45.0,
        Preset::FewestSupports.weights(),
    )
    .unwrap();
    assert_eq!(result.chosen.minima, 0);
    assert!(result.chosen.zero_support);
    // Verify the chosen bare model with the real raster island detector, not
    // only the orientation estimator's topological minimum count.
    let mut oriented = Scene::new();
    let node = oriented
        .add_leaf(
            None,
            result
                .delta
                .compose(&scene.node(id).unwrap().local_transform),
            LeafPayload::Mesh(Arc::clone(scene.leaf_mesh_arc(id).unwrap())),
        )
        .unwrap();
    oriented.add_root(node).unwrap();
    use sc_sl1cer::profile::{
        AaLevel, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
    };
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        PixelResolution::new(400, 400).unwrap(),
        LayerSchedule::uniform(0.05, 0.05, (result.chosen.height / 0.05).ceil() as u32).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let sc_sl1cer::SliceOutcome::Completed(leaves) = sc_sl1cer::api::slice_scene_with_fallback(
        &oriented,
        &profile,
        &sc_sl1cer::CancelToken::new(),
        |_| {},
        true,
    )
    .unwrap() else {
        panic!("cancelled");
    };
    let stack = sc_sl1cer::contour::merge_stacks(
        leaves
            .into_iter()
            .map(sc_sl1cer::LeafSlices::into_stack)
            .collect(),
    )
    .unwrap();
    assert!(sc_print_job::find_islands(&stack, &profile).is_empty());
}

#[test]
fn contact_quality_accounts_for_geometric_detail_and_tip_footprint() {
    let volume = BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    let (cube, id) = tilted_fixture([1.0; 3], 0.6);
    let detailed = measure(&cube, id, volume, 45.0).unwrap();
    let (mut plate, id) = tilted_fixture([10.0, 10.0, 0.01], 0.6);
    let before = plate.node(id).unwrap().local_transform;
    plate
        .set_local_transform(
            id,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 5.0]))
                .compose(&before),
        )
        .unwrap();
    let flat = measure(&plate, id, volume, 45.0).unwrap();
    assert!(
        detailed.detail_contact_area / detailed.support_area
            > flat.detail_contact_area / flat.support_area + 0.1
    );
    assert!(flat.contact_area > 0.0 && detailed.contact_area > 0.0);
    let mesh = sc_scene::bake_leaf_to_world(&cube, cube.roots()[0]).unwrap();
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let candidate = evaluate(
        &vertices,
        &triangle_sample(&mesh, &vertices),
        DQuat::IDENTITY,
        volume,
        45.0f64.to_radians().cos(),
        false,
    );
    let mut without_detail = candidate.clone();
    without_detail.metrics.detail_contact_area = 0.0;
    assert!(costs(&candidate, 0.0)[2] > costs(&without_detail, 0.0)[2]);
}

#[test]
fn orient_keeps_separate_fitting_object_centres_instead_of_overlapping_them() {
    let volume = BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    for x in [-60.0, 60.0] {
        let mut scene = Scene::new();
        let transform = Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 10.0]))
            .compose(&Transform3D::Affine(AffineTransform3D::from_rotation_y(
                0.4,
            )))
            .compose(&Transform3D::Affine(
                AffineTransform3D::from_non_uniform_scale([20.0, 10.0, 1.0]),
            ));
        let id = scene
            .add_leaf(
                None,
                transform,
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
        let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
        let before = bounds(
            mesh.vertices
                .iter()
                .map(|v| DVec3::from_array(v.coords().map(f64::from))),
        )
        .unwrap();
        let result = search(&scene, id, volume, 45.0, Preset::Fastest.weights()).unwrap();
        assert!(!result.retained);
        let after =
            bounds(mesh.vertices.iter().map(|v| {
                DVec3::from_array(result.delta.apply_to_vertex(*v).coords().map(f64::from))
            }))
            .unwrap();
        assert!(
            (((before.0 + before.1) - (after.0 + after.1)) * 0.5)
                .truncate()
                .length()
                < 1e-4
        );
        assert!(
            after.0.x >= -96.0001
                && after.1.x <= 96.0001
                && after.0.y >= -60.0001
                && after.1.y <= 60.0001
        );
    }
}

#[test]
fn minutes_and_hours_have_different_user_costs_without_candidate_ranges() {
    assert_eq!(time_cost(4.0), 0.0);
    assert!((time_cost(65.0) - 9.6).abs() < 1e-9);
    assert!(time_cost(245.0) > 60.0);
    // Same contact saving, same fivefold duration ratio. At small scale it
    // saves effort; at large scale the hours overwhelm four saved contacts.
    let score = |minutes: f64, contacts: f64, shortest: f64| {
        let m = Metrics {
            print_minutes: minutes,
            contacts,
            bed_contact_area: 100.0,
            copies: 1,
            ..Default::default()
        };
        user_costs(m, shortest).iter().sum::<f64>()
    };
    assert!(score(5.0, 1.0, 1.0) < score(1.0, 5.0, 1.0));
    assert!(score(300.0, 1.0, 60.0) > score(60.0, 5.0, 60.0));
}
#[test]
fn adequate_adhesion_wins_but_oversized_solid_bases_cost_peel() {
    let poor = Metrics {
        bed_contact_area: 1.0,
        peak_area: 100.0,
        height: 10.0,
        print_minutes: 60.0,
        copies: 1,
        ..Default::default()
    };
    let adequate = Metrics {
        bed_contact_area: 40.0,
        ..poor
    };
    let huge = Metrics {
        bed_contact_area: 10000.0,
        ..poor
    };
    assert!(user_costs(adequate, 60.0)[2] < user_costs(poor, 60.0)[2]);
    assert!(user_costs(adequate, 60.0)[2] < user_costs(huge, 60.0)[2]);
}
#[test]
fn blocked_fewest_contacts_lose_to_reachable_contacts() {
    let clean = Metrics {
        contacts: 5.0,
        copies: 1,
        ..Default::default()
    };
    let blocked = Metrics {
        contacts: 2.0,
        blocked_contacts: 2,
        expected_blocked_contacts: 2.0,
        recessed_contacts: 1,
        expected_recessed_contacts: 1.0,
        ..clean
    };
    assert!(user_costs(blocked, 0.0)[0] > user_costs(clean, 0.0)[0]);
    // Two stacked solids: a downward contact under the upper solid has a
    // shadowed vertical drop. A contact under the lower solid is accessible.
    let (scene, id) = fixture([10.0, 10.0, 2.0], 0.0);
    let mut mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    for v in &mut mesh.vertices {
        let p = v.coords();
        *v = sc_mesh_core::Vertex::new([p[0], p[1], p[2] + 5.0]);
    }
    let points = [4.0, 10.0].map(|z| crate::supports::SupportPoint {
        position: [2.0, 2.0, z],
        normal: [0.0, 0.0, -1.0],
    });
    let (count, _, blocked) = crate::supports::orientation_access(&mesh, &points);
    assert_eq!(count, 1);
    assert_eq!(blocked, vec![false, true]);
}
#[test]
fn most_per_plate_uses_packed_count_including_support_bases() {
    let (scene, id) = fixture([12.0, 8.0, 2.0], 0.0);
    let plate = BuildVolume::new(60.0, 40.0, 50.0).unwrap();
    let result = search(&scene, id, plate, 45.0, Preset::SmallestFootprint.weights()).unwrap();
    assert!(result.chosen.zero_support);
    assert!(result.chosen.copies >= result.original.copies);
    assert_eq!(copies_per_plate(12.0, 8.0, plate, 3.0), 12);
    assert_eq!(copies_per_plate(22.0, 18.0, plate, 3.0), 4);
    let costs = user_costs(result.chosen, 0.0);
    assert_eq!(costs[3], Rates::BATCH / result.chosen.copies as f64);
}
#[test]
fn simple_tilts_are_preferred_only_within_small_benefits() {
    assert_eq!(compound_tilt(DVec3::Z), 0.0);
    assert_eq!(
        compound_tilt(DQuat::from_rotation_x(0.3).inverse() * DVec3::Z),
        0.0
    );
    assert!(
        compound_tilt(
            (DQuat::from_rotation_x(0.3) * DQuat::from_rotation_y(0.3)).inverse() * DVec3::Z
        ) > 0.0
    );
}

#[test]
fn per_plate_yaw_chooses_twelve_copies_over_ten_with_identical_bed_area() {
    let (scene, id) = fixture([12.0, 8.0, 2.0], std::f64::consts::FRAC_PI_2);
    let plate = BuildVolume::new(60.0, 40.0, 2.1).unwrap();
    let context = Context {
        supports: false,
        ..Default::default()
    };
    let result = search_with_context(
        &scene,
        id,
        plate,
        45.0,
        Preset::SmallestFootprint.weights(),
        context,
    )
    .unwrap();
    assert_eq!(result.original.copies, 10);
    assert_eq!(result.chosen.copies, 12);
    assert!((result.original.bed_contact_area - result.chosen.bed_contact_area).abs() < 1e-5);
}

#[test]
fn collapsed_stl_faces_do_not_disable_real_contact_and_access_measurements() {
    let (scene, id) = fixture([20.0, 20.0, 2.0], 0.0);
    let plate = BuildVolume::new(60.0, 60.0, 60.0).unwrap();
    let clean = measure(&scene, id, plate, 45.0).unwrap();
    let mut mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    let mut bad = mesh.faces[0];
    bad.vertices[1] = bad.vertices[0];
    mesh.faces.push(bad);
    let mut dirty = Scene::new();
    let dirty_id = dirty
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    dirty.add_root(dirty_id).unwrap();
    let measured = measure(&dirty, dirty_id, plate, 45.0).unwrap();
    assert_eq!(clean.contacts, measured.contacts);
    assert_eq!(clean.copies, measured.copies);
    assert_eq!(clean.blocked_contacts, measured.blocked_contacts);
    assert_eq!(clean.bed_contact_area, measured.bed_contact_area);
    assert!(
        dirty
            .mesh_at_leaf(dirty_id)
            .unwrap()
            .validate_faces()
            .is_err()
    );
}
#[test]
fn shadowed_c_bracket_chooses_accessible_supports() {
    let mut pieces = Scene::new();
    for (size, position) in [
        ([10.0, 10.0, 2.0], [0.0, 0.0, 1.0]),
        ([10.0, 10.0, 2.0], [0.0, 0.0, 19.0]),
        ([2.0, 10.0, 20.0], [4.0, 0.0, 10.0]),
    ] {
        let transform = Transform3D::Affine(AffineTransform3D::from_translation(position)).compose(
            &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale(size)),
        );
        let id = pieces
            .add_leaf(
                None,
                transform,
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        pieces.add_root(id).unwrap();
    }
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(pieces.merge_meshes().unwrap().unwrap())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let plate = BuildVolume::new(60.0, 60.0, 60.0).unwrap();
    let result = search(&scene, id, plate, 45.0, Preset::Balanced.weights()).unwrap();
    assert!(result.original.blocked_contacts > 0);
    assert!(result.chosen.blocked_contacts < result.original.blocked_contacts);
    assert!(!result.retained);
}

#[test]
fn off_centre_raft_packing_matches_generated_model_and_support_union() {
    let (mut scene, id) = fixture([2.0, 2.0, 20.0], 0.0);
    scene
        .set_local_transform(
            id,
            Transform3D::Affine(AffineTransform3D::from_rotation_y(0.5))
                .compose(&scene.node(id).unwrap().local_transform),
        )
        .unwrap();
    scene.drop_at([0.0, 0.0], 0.0).unwrap();
    let plate = BuildVolume::new(60.0, 40.0, 60.0).unwrap();
    let m = measure(&scene, id, plate, 45.0).unwrap();
    let (supported, _) =
        crate::supports::geometric_supported_scene(&scene, &Default::default()).unwrap();
    let mesh = supported.merge_meshes().unwrap().unwrap();
    let (lo, hi) = bounds(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(v.coords().map(f64::from))),
    )
    .unwrap();
    let expected = copies_per_plate(hi.x - lo.x, hi.y - lo.y, plate, 3.0);
    assert_eq!(expected, 9);
    assert_eq!(m.copies, expected);
}

#[test]
fn supported_fit_includes_the_offset_raft_and_support_lift() {
    let (scene, id) = fixture([58.0, 20.0, 2.0], 0.0);
    let plate = BuildVolume::new(60.0, 40.0, 60.0).unwrap();
    let supported = search(&scene, id, plate, 45.0, Preset::Balanced.weights()).unwrap();
    assert!(supported.original.fits);
    assert!(supported.original.zero_support);
    assert!(supported.chosen.fits);
    assert!(supported.chosen.copies > 0);
    let direct = measure_with_context(
        &scene,
        id,
        plate,
        45.0,
        Context {
            supports: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(direct.fits);
}

#[test]
fn source_coordinate_precision_does_not_put_the_oriented_model_below_the_gpu_bed() {
    use glam::{Mat4, Vec3};
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    let rotation = DQuat::from_rotation_y(0.6);
    for vertex in &mut mesh.vertices {
        let p = rotation
            * (DVec3::from_array(vertex.coords().map(f64::from)) * DVec3::new(12.0, 8.0, 2.0))
            + DVec3::new(100.0, 200.0, 500.0);
        *vertex = sc_mesh_core::Vertex::new(p.to_array().map(|x| x as f32));
    }
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh.clone())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let result = search_with_context(
        &scene,
        id,
        BuildVolume::new(60.0, 60.0, 60.0).unwrap(),
        45.0,
        Preset::Fastest.weights(),
        Context {
            supports: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!result.retained);
    let Transform3D::Affine(a) = result.delta else {
        panic!()
    };
    let [x, y, z, shift] = a.columns().map(|column| Vec3::from_array(column.map(|v| v as f32)));
    let matrix = Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), shift.extend(1.0));
    for vertex in &mesh.vertices {
        let p = matrix.transform_point3(Vec3::from_array(vertex.coords()));
        assert!(
            p.x >= -30.0 && p.x <= 30.0 && p.y >= -30.0 && p.y <= 30.0 && p.z >= 0.0 && p.z <= 60.0,
            "{p:?}"
        );
    }
}

#[test]
fn bed_precision_guard_reserves_the_support_lift_at_the_build_ceiling() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for vertex in &mut mesh.vertices {
        let [x, y, z] = vertex.coords();
        *vertex = sc_mesh_core::Vertex::new([-z * 2.0, y * 8.0, 500.0 + x * 12.0]);
    }
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh.clone())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let result = search(
        &scene,
        id,
        BuildVolume::new(60.0, 60.0, 7.0).unwrap(),
        45.0,
        Preset::Fastest.weights(),
    )
    .unwrap();
    assert!(result.chosen.fits);
    assert!(!result.retained);
    let (_, hi) = bounds(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(result.delta.apply_to_vertex(*v).coords().map(f64::from))),
    )
    .unwrap();
    assert!(
        hi.z + Context::default().support.lift_mm <= 7.00001,
        "{hi:?}"
    );
}

#[test]
fn inaccessible_internal_faces_do_not_become_zero_work_fitting_proposals() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for face in &mut mesh.faces {
        face.vertices.swap(1, 2);
    }
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let measured = measure(
        &scene,
        id,
        BuildVolume::new(60.0, 60.0, 60.0).unwrap(),
        45.0,
    )
    .unwrap();
    assert_eq!(measured.contacts, 0.0);
    assert_eq!(measured.copies, 0);
    assert!(!measured.fits);
}

#[test]
fn finalist_contacts_are_sampled_at_the_proposed_plate_position() {
    let (mut scene, id) = fixture([20.0, 10.0, 1.0], 0.0);
    let transform = Transform3D::Affine(AffineTransform3D::from_translation([40.3, 10.7, 10.0]))
        .compose(&Transform3D::Affine(AffineTransform3D::from_rotation_y(
            0.4,
        )))
        .compose(&scene.node(id).unwrap().local_transform);
    scene.set_local_transform(id, transform).unwrap();
    let volume = BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    let proposal = search(&scene, id, volume, 45.0, Preset::Fastest.weights()).unwrap();
    let world = proposal
        .delta
        .compose(&scene.world_transform_of(id).unwrap());
    scene.set_local_transform(id, world).unwrap();
    let actual = measure(&scene, id, volume, 45.0).unwrap();
    assert_eq!(proposal.chosen.contacts, actual.contacts);
    assert_eq!(proposal.chosen.copies, actual.copies);
    assert_eq!(proposal.chosen.blocked_contacts, actual.blocked_contacts);
}

#[test]
fn manual_work_currency_excludes_unattended_time_and_reprint_risk() {
    let m = Metrics {
        contacts: 5.0,
        blocked_contacts: 2,
        expected_blocked_contacts: 2.0,
        stair_area: 1000.0,
        ..Default::default()
    };
    assert!((hands_on_minutes(m) - 19.1875).abs() < 1e-10);
    assert_eq!(
        hands_on_minutes(m),
        hands_on_minutes(Metrics {
            print_minutes: 10000.0,
            ..m
        })
    );
}

#[test]
fn culled_candidates_are_not_charged_as_real_support_removal() {
    let discarded = Metrics {
        blocked_contacts: 1000,
        ..Default::default()
    };
    assert_eq!(hands_on_minutes(discarded), Rates::ROUTING_REVIEW);
    let one_real = Metrics {
        expected_blocked_contacts: 1.0,
        ..discarded
    };
    assert_eq!(
        hands_on_minutes(one_real),
        Rates::ROUTING_REVIEW + Rates::BLOCKED
    );
}

#[test]
fn tree_branches_stay_within_the_angle_and_longer_ones_cost_more() {
    use crate::supports::{SupportPoint, branch_paths, build_supports};
    let (mut scene, id) = fixture([20.0, 2.0, 2.0], 0.0);
    scene
        .set_local_transform(
            id,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 13.0]))
                .compose(&scene.node(id).unwrap().local_transform),
        )
        .unwrap();
    let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    let settings = sc_print_profiles::support_settings::SupportSettings {
        tree_supports: true,
        ..Default::default()
    };
    let paths = |spread: f32| {
        let points = [-spread, spread].map(|x| SupportPoint {
            position: [x, 0.0, 12.0],
            normal: [0.0, 0.0, -1.0],
        });
        let structure = build_supports(&mesh, &points, &settings).unwrap();
        assert_eq!(structure.columns.len(), 1);
        branch_paths(&structure)
    };
    // R86: branches leave the trunk as high as the angle limit allows, so
    // contacts further apart get longer branches at the same lean.
    let close = paths(1.0);
    let wide = paths(4.0);
    for pair in [&close, &wide] {
        assert_eq!(pair.len(), 2);
        assert!(pair.iter().all(|p| p.inclination_deg <= 40.001));
    }
    assert!(wide.iter().all(|p| p.inclination_deg > 30.0));
    assert!(wide[0].length_mm > close[0].length_mm * 2.0);
    assert!(branch_work_minutes(&wide) > branch_work_minutes(&close));
}
#[test]
fn lattice_adhesion_uses_the_generated_contact_caps() {
    let (scene, id) = tilted_fixture([20.0, 20.0, 2.0], 0.6);
    let plate = BuildVolume::new(60.0, 60.0, 60.0).unwrap();
    let mut context = Context::default();
    context.support.lattice_raft = true;
    let measured = measure_with_context(&scene, id, plate, 45.0, context).unwrap();
    let (generated, structure) =
        crate::supports::geometric_supported_scene(&scene, &context.support).unwrap();
    // The same raft measured on two paths: equal up to float rounding.
    let area = crate::supports::base_contact_area(structure.raft.as_ref().unwrap());
    assert!(
        (measured.bed_contact_area - area).abs() <= area * 1e-6,
        "{} against {area}",
        measured.bed_contact_area
    );
    let mesh = generated.merge_meshes().unwrap().unwrap();
    let (lo, hi) = bounds(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(v.coords().map(f64::from))),
    )
    .unwrap();
    assert_eq!(
        measured.copies,
        copies_per_plate(hi.x - lo.x, hi.y - lo.y, plate, context.gap_mm)
    );
    context.support.lattice_raft = false;
    let solid = measure_with_context(&scene, id, plate, 45.0, context).unwrap();
    assert!(measured.bed_contact_area < solid.bed_contact_area);
}

#[test]
fn bare_support_base_area_and_packing_use_the_generated_polygons() {
    let (scene, id) = tilted_fixture([12.0, 8.0, 2.0], 0.6);
    let plate = BuildVolume::new(60.0, 40.0, 60.0).unwrap();
    let mut context = Context::default();
    context.support.raft = false;
    let measured = measure_with_context(&scene, id, plate, 45.0, context).unwrap();
    let (generated, structure) =
        crate::supports::geometric_supported_scene(&scene, &context.support).unwrap();
    // Equal but for single-precision rounding of the two placements.
    assert!(
        (measured.bed_contact_area
            - structure
                .columns
                .iter()
                .map(crate::supports::base_contact_area)
                .sum::<f64>())
        .abs()
            < 1e-5
    );
    let mesh = generated.merge_meshes().unwrap().unwrap();
    let (lo, hi) = bounds(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(v.coords().map(f64::from))),
    )
    .unwrap();
    assert_eq!(
        measured.copies,
        copies_per_plate(hi.x - lo.x, hi.y - lo.y, plate, context.gap_mm)
    );
}

/// Sleptic: Fill plate should re-orient "with a heavier favor on saving space
/// ... no supports should still be favored above that". Three poses of one
/// small part: A packs 39 per plate; B packs 60 but prints 30 min longer;
/// C packs 60 with 25 more supports. Balanced keeps A over B (time beats a
/// tiny batch rate); Fill plate takes B for the copies, but still refuses C.
#[test]
fn fill_plate_weights_favor_copies_but_not_over_supports() {
    let base = Metrics {
        contacts: 10.0,
        support_area: 100.0,
        height: 20.0,
        print_minutes: 60.0,
        copies: 39,
        fits: true,
        ..Default::default()
    };
    let more_copies_taller = Metrics {
        copies: 60,
        print_minutes: 90.0,
        height: 34.0,
        ..base
    };
    let more_copies_more_supports = Metrics {
        copies: 60,
        contacts: 35.0,
        ..base
    };
    let total = |m: Metrics, w: [f64; 4]| -> f64 {
        user_costs(m, 60.0).iter().zip(w).map(|(c, w)| c * w).sum()
    };
    let balanced = Preset::Balanced.weights();
    let fill = sc_print_profiles::orient::FILL_PLATE_WEIGHTS;
    assert!(total(base, balanced) < total(more_copies_taller, balanced));
    assert!(total(more_copies_taller, fill) < total(base, fill));
    assert!(total(base, fill) < total(more_copies_more_supports, fill));
}

#[test]
fn r22_balanced_slab_is_flat_and_pin_cannot_balance_on_a_sliver() {
    let (scene, id) = fixture([40.0, 30.0, 5.0], 0.0);
    let p = search(
        &scene,
        id,
        BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
        45.0,
        Preset::Balanced.weights(),
    )
    .unwrap();
    assert!(p.chosen.zero_support);
    assert!((p.chosen.height - 5.0).abs() < 1e-4);
    let (pin, id) = fixture([0.5, 0.5, 80.0], 0.0);
    let mesh = sc_scene::bake_leaf_to_world(&pin, id).unwrap();
    assert!(direct_metrics(&mesh, DQuat::IDENTITY, 45.0f64.to_radians().cos()).is_none());
}

#[test]
fn r22_raw_copy_value_and_direct_head_start() {
    let m = |copies, zero_support| Metrics {
        copies,
        zero_support,
        ..Default::default()
    };
    assert_eq!(
        fill_copy_cost(m(5, false), 10) - fill_copy_cost(m(15, false), 10),
        fill_copy_cost(m(50, false), 10) - fill_copy_cost(m(60, false), 10)
    );
    assert!(fill_copy_cost(m(50, false), 10) < fill_copy_cost(m(10, true), 10));
    assert!(fill_copy_cost(m(84, true), 10) < fill_copy_cost(m(90, false), 10));
}

#[test]
fn r22_small_unsampled_overhang_and_above_bed_minimum_disqualify_direct() {
    let (scene, id) = fixture([40.0, 30.0, 5.0], 0.0);
    let mut mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    // A disconnected tiny elevated solid creates an above-bed local minimum.
    let mut other = sc_mesh_fixtures::unit_cube_mesh();
    for v in &mut other.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] * 0.01, v[1] * 0.01, v[2] * 0.01 + 4.0]);
    }
    let offset = mesh.vertices.len();
    mesh.vertices.extend(other.vertices);
    mesh.faces.extend(other.faces.into_iter().map(|mut f| {
        f.vertices = f
            .vertices
            .map(|i| (u32::try_from(usize::from(i) + offset).unwrap()).into());
        f
    }));
    assert!(direct_metrics(&mesh, DQuat::IDENTITY, 45.0f64.to_radians().cos()).is_none());
}

#[test]
fn r22_fill_requires_more_than_the_head_start_and_then_compares_remaining_costs() {
    let direct = Metrics {
        zero_support: true,
        copies: 84,
        print_minutes: 60.0,
        ..Default::default()
    };
    let supported = Metrics {
        copies: 90,
        ..direct
    };
    let supported = Metrics {
        zero_support: false,
        ..supported
    };
    assert_eq!(
        direct_fill_order(direct, supported, 10),
        Some(std::cmp::Ordering::Less)
    );
    let small = Metrics {
        copies: 10,
        ..direct
    };
    let large = Metrics {
        copies: 50,
        ..supported
    };
    assert_eq!(direct_fill_order(small, large, 10), None);
    let cost = |m: Metrics| {
        let c = user_costs(m, 60.0);
        2.0 * c[0] + 0.6 * c[1] + c[2] + 6.0 * fill_copy_cost(m, 10)
    };
    assert!(cost(large) < cost(small));
    assert!(
        cost(Metrics {
            contacts: 1000.0,
            ..large
        }) > cost(small)
    );
    assert_eq!(
        direct_fill_order(
            direct,
            Metrics {
                print_minutes: 90.0,
                ..direct
            },
            10
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        direct_fill_order(
            direct,
            Metrics {
                copies: 90,
                print_minutes: 90.0,
                ..direct
            },
            10
        ),
        Some(std::cmp::Ordering::Greater)
    );
}

#[test]
fn r22_elevated_lip_has_no_direct_pose_and_uses_supported_scoring() {
    let mut pieces = Scene::new();
    for (size, position) in [
        ([40.0, 30.0, 5.0], [0.0, 0.0, 2.5]),
        ([2.0, 2.0, 0.2], [0.0, 0.0, 6.0]),
    ] {
        let id = pieces
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation(position)).compose(
                    &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale(size)),
                ),
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        pieces.add_root(id).unwrap();
    }
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(pieces.merge_meshes().unwrap().unwrap())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let p = search(
        &scene,
        id,
        BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
        45.0,
        Preset::Balanced.weights(),
    )
    .unwrap();
    assert!(!p.zero_support_available);
    assert!(!p.chosen.zero_support);
    assert!(p.chosen.contacts > 0.0 && p.chosen.fits);
}

#[test]
fn r22_all_faces_check_catches_an_overhang_missing_from_the_sample() {
    let (scene, id) = fixture([40.0, 30.0, 5.0], 0.0);
    let mut mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    let repeated = mesh.faces[0];
    let first = mesh.vertices.len() as u32;
    mesh.vertices
        .extend([[0.0, 0.0, 4.0], [0.0, 0.1, 4.0], [0.1, 0.0, 4.0]].map(sc_mesh_core::Vertex::new));
    // Connect the equal-height patch to a lower vertex. This prevents the
    // topological-minimum check from masking an accidentally sampled face scan.
    mesh.faces
        .push(sc_mesh_core::IndexedTriangle::from_raw_indices(
            sc_mesh_core::Normal::new([0.0, 0.0, 1.0]),
            [first, first + 1, 0],
        ));
    mesh.faces.extend(std::iter::repeat_n(repeated, 4094));
    mesh.faces
        .push(sc_mesh_core::IndexedTriangle::from_raw_indices(
            sc_mesh_core::Normal::new([0.0, 0.0, -1.0]),
            [first, first + 1, first + 2],
        ));
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    assert!(
        !triangle_sample(&mesh, &vertices)
            .iter()
            .any(|(t, _, _)| t.iter().all(|v| v.z > 3.0))
    );
    assert_eq!(
        minimum_patches_above(&vertices, &mesh.faces, DQuat::IDENTITY, -2.5),
        0
    );
    assert!(direct_metrics(&mesh, DQuat::IDENTITY, 45.0f64.to_radians().cos()).is_none());
}

#[test]
fn r22_tall_supported_pose_has_a_stability_contact_floor() {
    let (scene, id) = fixture([2.0, 2.0, 50.0], 0.0);
    let m = measure(
        &scene,
        id,
        BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
        45.0,
    )
    .unwrap();
    assert!(!m.zero_support);
    assert!(m.stability_contacts >= 9.0);
    assert!(m.contacts >= m.stability_contacts);
}

#[test]
fn r22_printer_raster_rejects_a_geometrically_connected_but_unprintable_neck() {
    use sc_print_primitives::{Mirror, PixelResolution, PixelSizeMicrons};
    let volume = BuildVolume::new(20.0, 20.0, 20.0).unwrap();
    let hardware = sc_print_profiles::PrinterHardware::new(
        volume,
        PixelResolution::new(400, 400).unwrap(),
        PixelSizeMicrons::new(50.0, 50.0).unwrap(),
        Mirror::default(),
    )
    .unwrap();
    let mut vertices = Vec::new();
    for (width, z) in [
        (10.0, 0.0),
        (10.0, 1.0),
        (0.001, 1.1),
        (0.001, 2.0),
        (2.0, 3.5),
        (2.0, 4.0),
    ] {
        for [x, y] in [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]] {
            vertices.push(sc_mesh_core::Vertex::new([x * width, y * width, z]));
        }
    }
    let mut indices = vec![[0, 2, 1], [0, 3, 2], [20, 21, 22], [20, 22, 23]];
    for ring in 0..5u32 {
        for i in 0..4 {
            let a = ring * 4 + i;
            let b = ring * 4 + (i + 1) % 4;
            indices.extend([[a, b, b + 4], [a, b + 4, a + 4]]);
        }
    }
    let faces = indices
        .into_iter()
        .map(|ids| {
            sc_mesh_core::IndexedTriangle::from_raw_indices(
                sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                    &ids.map(|i| vertices[i as usize]),
                )
                .unwrap(),
                ids,
            )
        })
        .collect();
    let mesh = sc_mesh_core::IndexedMesh::new(None, vertices, faces);
    assert!(direct_metrics(&mesh, DQuat::IDENTITY, 45.0f64.to_radians().cos()).is_some());
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let mut candidate = evaluate(
        &vertices,
        &triangle_sample(&mesh, &vertices),
        DQuat::IDENTITY,
        volume,
        45.0f64.to_radians().cos(),
        false,
    );
    let context = Context {
        hardware: Some(hardware),
        aa: sc_sl1cer::profile::AaLevel::Off,
        ..Default::default()
    };
    assert!(!direct_raster_clear(
        &candidate,
        &mesh,
        context,
        Some(DVec2::ZERO),
        None,
    ));
    contextualize(&mut candidate, volume, context);
    finalize(
        &mut candidate,
        &mesh,
        &crate::hollow::Tree::from_mesh(&mesh),
        volume,
        context,
        45.0,
        Some(DVec2::ZERO),
        None,
    );
    assert!(!candidate.metrics.zero_support);
    assert!(candidate.metrics.contacts > 0.0);
    let mut source = Scene::new();
    let id = source
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    source.add_root(id).unwrap();
    let profile = crate::projection::hardware_profile(&hardware, crate::projection::LayerPlan { layer_height: sc_print_primitives::PositiveFiniteMm::new(0.05).unwrap(), first_layer_height: sc_print_primitives::PositiveFiniteMm::new(0.05).unwrap(), layers: std::num::NonZeroU32::new(80).unwrap() }, sc_sl1cer::profile::AaLevel::Off, sc_sl1cer::profile::PackingMode::None);
    let (generated, structure, analysis) =
        crate::supports::auto_supported_scene_analyzed(&source, &Default::default(), &profile)
            .unwrap();
    // R38: it has a real flat base, so it stays on the plate and only the
    // neck is supported (it used to be lifted 5 mm onto a raft for it).
    assert!(
        sc_scene::bake_leaf_to_world(&generated, id)
            .unwrap()
            .local_aabb()
            .unwrap()
            .min()[2]
            .abs()
            < 0.02
    );
    assert!(!structure.points.is_empty());
    assert!(structure.raft.is_none());
    assert!(analysis.islands.is_empty());
}

#[test]
fn r22_direct_original_is_moved_onto_the_plate_before_retention() {
    let (mut scene, id) = fixture([40.0, 30.0, 5.0], 0.0);
    let before = scene.node(id).unwrap().local_transform;
    scene
        .set_local_transform(
            id,
            Transform3D::Affine(AffineTransform3D::from_translation([80.0, 0.0, 2.5]))
                .compose(&before),
        )
        .unwrap();
    let p = search(
        &scene,
        id,
        BuildVolume::new(60.0, 40.0, 60.0).unwrap(),
        45.0,
        Preset::Balanced.weights(),
    )
    .unwrap();
    assert!(p.chosen.zero_support && !p.retained);
    let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
    let (lo, hi) = bounds(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(p.delta.apply_to_vertex(*v).coords().map(f64::from))),
    )
    .unwrap();
    assert!(lo.x >= -30.00001 && hi.x <= 30.00001 && lo.y >= -20.00001 && hi.y <= 20.00001);
    assert!(lo.z.abs() < 0.001);
}

// Sleptic (R39): auto orient was "way off". It scored every supported pose as
// lifted onto a raft, even counting a flat base as support area, so a part's
// broad flat face down looked like the most support work. A slab carrying a
// floating lip always needs support; a pose with a real base on the plate
// (no lift, no raft) must win over lifting it onto a raft.
#[test]
fn r39_part_that_needs_support_keeps_its_broad_flat_face_on_the_plate() {
    let mut pieces = Scene::new();
    for (size, position) in [
        ([40.0, 30.0, 5.0], [0.0, 0.0, 2.5]),
        ([2.0, 2.0, 0.2], [0.0, 0.0, 6.0]),
    ] {
        let id = pieces
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation(position)).compose(
                    &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale(size)),
                ),
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        pieces.add_root(id).unwrap();
    }
    let mut scene = Scene::new();
    // Start it standing on a narrow edge so the search has to find the face.
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_rotation_y(1.2)),
            LeafPayload::Mesh(Arc::new(pieces.merge_meshes().unwrap().unwrap())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let p = search(
        &scene,
        id,
        BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
        45.0,
        Preset::Balanced.weights(),
    )
    .unwrap();
    // A base on the plate (no lift, no raft), and the print is no taller
    // than the slab's long side: it is not tipped up onto a corner.
    assert!(p.chosen.anchored, "{:?}", p.chosen);
    assert!(p.chosen.height <= 40.0 + 1e-6, "{}", p.chosen.height);
    assert!(p.chosen.fits);
}

/// One model of several closed boxes (L-bracket with a boss: asymmetric, so
/// it has one best pose up to its own symmetry).
fn bracket_mesh() -> sc_mesh_core::IndexedMesh {
    use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
    use sc_mesh_core::{IndexedTriangle, Vertex};
    let parts: [([f32; 3], [f32; 3]); 3] = [
        ([0.0, 0.0, 0.0], [40.0, 20.0, 4.0]),
        ([0.0, 0.0, 4.0], [4.0, 20.0, 30.0]),
        ([22.0, 6.0, 4.0], [30.0, 14.0, 12.0]),
    ];
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for (lo, hi) in parts {
        let base = vertices.len() as u32;
        for i in 0..8u32 {
            vertices.push(Vertex::new([
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]));
        }
        for tri in [
            [0u32, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ] {
            let ids = tri.map(|i| base + i);
            let n =
                compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize])).unwrap();
            faces.push(IndexedTriangle::from_raw_indices(n, ids));
        }
    }
    sc_mesh_core::IndexedMesh::new(None, vertices, faces)
}

// Sleptic (R50): auto orient must give the same result every time, whatever
// position or angle the model starts in, and on repeated runs.
#[test]
fn r50_auto_orient_is_repeatable_from_any_start() {
    let volume = BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    let starts = [
        Transform3D::Identity,
        Transform3D::Affine(AffineTransform3D::from_rotation_x(0.7)),
        Transform3D::Affine(AffineTransform3D::from_rotation_y(-1.1)).compose(
            &Transform3D::Affine(AffineTransform3D::from_rotation_z(0.4)),
        ),
        Transform3D::Affine(AffineTransform3D::from_translation([30.0, -20.0, 5.0])).compose(
            &Transform3D::Affine(AffineTransform3D::from_rotation_z(2.0)),
        ),
        Transform3D::Affine(AffineTransform3D::from_rotation_x(3.0)).compose(&Transform3D::Affine(
            AffineTransform3D::from_rotation_y(0.9),
        )),
    ];
    let mut results = Vec::new();
    for start in starts {
        let mut scene = Scene::new();
        let id = scene
            .add_leaf(None, start, LeafPayload::Mesh(Arc::new(bracket_mesh())))
            .unwrap();
        scene.add_root(id).unwrap();
        for _ in 0..2 {
            let p = search(&scene, id, volume, 45.0, Preset::Balanced.weights()).unwrap();
            // The pose the model ends up in, in its own terms: where its
            // local axes point after the start transform and the delta.
            let total = p.delta.compose(&start);
            let axes = [DVec3::X, DVec3::Y, DVec3::Z].map(|v| {
                let o = total.apply_to_vertex(sc_mesh_core::Vertex::new([0.0; 3]));
                let t = total
                    .apply_to_vertex(sc_mesh_core::Vertex::new(v.to_array().map(|c| c as f32)));
                (DVec3::from_array(t.coords().map(f64::from))
                    - DVec3::from_array(o.coords().map(f64::from)))
                .normalize()
            });
            results.push((
                p.chosen.height,
                p.chosen.contacts,
                p.chosen.anchored,
                axes[0].z,
                axes[1].z,
                axes[2].z,
                // Heading too: the same model axis lies along the plate's X.
                axes[0].x.abs(),
                p.chosen.width,
            ));
        }
    }
    let first = results[0];
    for (i, r) in results.iter().enumerate() {
        assert!(
            (r.0 - first.0).abs() < 1e-3,
            "run {i}: height {} vs {}",
            r.0,
            first.0
        );
        assert!(
            (r.1 - first.1).abs() < 0.5,
            "run {i}: contacts {} vs {}",
            r.1,
            first.1
        );
        assert_eq!(r.2, first.2, "run {i}: anchored");
        // Same face down: each model axis has the same vertical component.
        for (a, b) in [
            (r.3, first.3),
            (r.4, first.4),
            (r.5, first.5),
            (r.6, first.6),
            (r.7, first.7),
        ] {
            assert!(
                (a - b).abs() < 1e-3,
                "run {i}: pose differs {r:?} vs {first:?}"
            );
        }
    }
}

/// R79: the peel-area estimate (largest layer section) is the same wherever
/// the part sits on the plate. It summed a shoelace over sampled, unclosed
/// section pieces in plate coordinates, so moving or spinning the same
/// Benchy changed it by a quarter and flipped Auto prepare's pose.
#[test]
fn peak_area_does_not_depend_on_where_the_part_sits() {
    let mesh = bracket_mesh();
    let at = |shift: DVec3| {
        let mut moved = mesh.clone();
        for v in &mut moved.vertices {
            let p = DVec3::from_array(v.coords().map(f64::from)) + shift;
            *v = sc_mesh_core::Vertex::new(p.to_array().map(|c| c as f32));
        }
        let vertices: Vec<_> = moved
            .vertices
            .iter()
            .map(|v| DVec3::from_array(v.coords().map(f64::from)))
            .collect();
        // Every other triangle only: a sample, as large meshes are scored.
        let mut triangles = triangle_sample(&moved, &vertices);
        let mut i = 0;
        triangles.retain(|_| {
            i += 1;
            i % 2 == 0
        });
        let volume = BuildVolume::new(192.0, 120.0, 200.0).unwrap();
        evaluate(
            &vertices,
            &triangles,
            DQuat::from_rotation_x(0.6),
            volume,
            0.7,
            false,
        )
        .metrics
        .peak_area
    };
    let here = at(DVec3::ZERO);
    let there = at(DVec3::new(70.0, -45.0, 3.0));
    assert!(here > 0.0);
    assert!((here - there).abs() < 1e-6 * here, "{here} vs {there}");
}

#[test]
fn identical_sampled_poses_finalize_once_and_keep_sources_and_slots() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mesh = sc_mesh_fixtures::unit_cube_mesh();
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let triangles = triangle_sample(&mesh, &vertices);
    let volume = BuildVolume::new(120.0, 68.0, 150.0).unwrap();
    let mut candidates: Vec<_> = [0.0, 0.3, 0.0, 0.3, 0.7]
        .into_iter()
        .enumerate()
        .map(|(source, angle)| Candidate {
            source,
            ..evaluate(
                &vertices,
                &triangles,
                DQuat::from_rotation_x(angle),
                volume,
                0.7,
                false,
            )
        })
        .collect();
    let finish = |c: &mut Candidate| {
        contextualize(c, volume, Context::default());
        let tree = crate::hollow::Tree::from_mesh(&mesh);
        finalize(
            c,
            &mesh,
            &tree,
            volume,
            Context {
                hardware: None,
                ..Context::default()
            },
            45.0,
            Some(DVec2::ZERO),
            None,
        );
        c.metrics.minima = minimum_patches(&vertices, &mesh.faces, c.rotation);
    };
    let sampled: Vec<_> = candidates.iter().map(|c| c.rotation).collect();
    // The separately evaluated reference stays independent even at the same pose.
    let mut reference = candidates[0].clone();
    reference.source = 99;
    candidates.push(reference);
    let mut expected = candidates.clone();
    expected.iter_mut().for_each(finish);
    let count = AtomicUsize::new(0);
    finalize_unique_poses(&mut candidates, &sampled, |c| {
        count.fetch_add(1, Ordering::SeqCst);
        finish(c);
    });
    assert_eq!(count.load(Ordering::SeqCst), 4);
    for (actual, expected) in candidates.iter().zip(expected) {
        assert_eq!(actual.source, expected.source);
        assert_eq!(actual.rotation, expected.rotation);
        assert_eq!(actual.min, expected.min);
        assert_eq!(actual.max, expected.max);
        assert_eq!(actual.footprint_min, expected.footprint_min);
        assert_eq!(actual.footprint_max, expected.footprint_max);
        assert_eq!(
            format!("{:?}", actual.metrics),
            format!("{:?}", expected.metrics)
        );
    }
}

#[test]
fn shared_contact_topology_keeps_placed_and_anchored_samples_exact() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    // Include duplicate incidences, as on non-manifold imported meshes.
    mesh.faces.push(mesh.faces[0].clone());
    let topology = crate::supports::ContactTopology::new(&mesh);
    let tree = crate::hollow::Tree::from_mesh(&mesh);
    let settings = sc_print_profiles::support_settings::SupportSettings::default();
    let bits = |points: Vec<crate::supports::SupportPoint>| -> Vec<_> {
        points
            .into_iter()
            .map(|p| (p.position.map(f32::to_bits), p.normal.map(f32::to_bits)))
            .collect()
    };
    for angle in [0.0, 0.3, 1.2, std::f64::consts::PI] {
        let rotation = DQuat::from_rotation_x(angle);
        let shift = DVec3::new(0.5, -0.3, 10.0);
        let mut placed = mesh.clone();
        for v in &mut placed.vertices {
            let p = rotation * DVec3::from_array(v.coords().map(f64::from)) + shift;
            *v = sc_mesh_core::Vertex::new(p.to_array().map(|v| v as f32));
        }
        let sample = |topology| {
            crate::supports::sample_contacts_placed(
                &placed,
                &settings,
                (&tree, rotation, shift),
                topology,
            )
            .unwrap()
        };
        assert_eq!(bits(sample(None)), bits(sample(Some(&topology))));
        assert_eq!(
            bits(crate::supports::sample_overhang_contacts(&placed, &settings).unwrap()),
            bits(
                crate::supports::sample_overhang_contacts_indexed(&placed, &settings, &topology)
                    .unwrap()
            )
        );
    }
}
