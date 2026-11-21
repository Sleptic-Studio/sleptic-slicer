// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::node::LeafPayload;

#[test]
fn oblique_manual_drain_reaches_the_cavity_and_misses_are_refused() {
    let source = sc_scene::bake_mesh_to_world(
        &sc_mesh_fixtures::unit_cube_mesh(),
        &Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
    )
    .unwrap();
    let initial = sc_print_glue::hollow::hollow(&source, 2.0).unwrap();
    let depth = cavity_depth(
        &initial.mesh,
        source.faces.len(),
        [0.0, 0.0, -10.0],
        [1.0, 0.0, 1.0],
    )
    .unwrap();
    assert!(
        (depth - 2.0 * 2.0_f64.sqrt()).abs() < 0.01,
        "oblique wall crossing {depth}"
    );
    assert!(
        cavity_depth(
            &initial.mesh,
            source.faces.len(),
            [9.9, 0.0, -10.0],
            [0.0, 0.0, 1.0]
        )
        .is_none()
    );
}

#[test]
fn hollow_drain_project_roundtrip_and_undo() {
    let mut scene = Scene::new();
    let original = Arc::new(sc_mesh_fixtures::unit_cube_mesh());
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
            LeafPayload::Mesh(original.clone()),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let hollow = edit(&scene, id, 2.0, HoleChange::Keep).unwrap();
    hollow.apply_to_scene(&mut scene).unwrap();
    assert_eq!(stats(&scene).1, 1);
    let drain = edit(&scene, id, 2.0, HoleChange::Auto(3.0)).unwrap();
    drain.apply_to_scene(&mut scene).unwrap();
    let before = settings(scene.mesh_at_leaf(id).unwrap()).unwrap();
    assert_eq!(before.holes.len(), 2);
    assert_eq!(stats(&scene).1, 0);
    let mut bytes = std::io::Cursor::new(Vec::new());
    sc_project_formats::threemf::write_threemf_project(
        &scene,
        &sc_project_formats::ProjectPayload::default(),
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        &mut bytes,
    )
    .unwrap();
    bytes.set_position(0);
    let loaded = sc_project_formats::threemf::read_threemf_project(&mut bytes).unwrap();
    let leaf = loaded
        .scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap()[0];
    let mesh = loaded.scene.mesh_at_leaf(leaf).unwrap();
    let restored = settings(mesh).unwrap();
    assert_eq!(restored.thickness, 2.0);
    assert_eq!(restored.holes.len(), 2);
    assert_eq!(restored.vertices, before.vertices);
    assert_eq!(stats(&loaded.scene), stats(&scene));
    let saved = stats(&scene).0;
    let scale = SceneEdit::SetLocalTransform {
        node_id: id,
        before: scene.node(id).unwrap().local_transform,
        after: Transform3D::Affine(AffineTransform3D::from_uniform_scale(40.0)),
    };
    scale.apply_to_scene(&mut scene).unwrap();
    assert!((stats(&scene).0 - saved * 8.0).abs() < 1e-6);
    scale.inverse().apply_to_scene(&mut scene).unwrap();
    drain.inverse().apply_to_scene(&mut scene).unwrap();
    assert_eq!(stats(&scene).1, 1);
    hollow.inverse().apply_to_scene(&mut scene).unwrap();
    assert!(Arc::ptr_eq(scene.leaf_mesh_arc(id).unwrap(), &original));
}

/// Local-model audit; compilation and loading are excluded from the timed region.
#[test]
#[ignore = "requires SC_BENCHY_STL"]
fn benchy_hollow_measurement() {
    let path = std::env::var_os("SC_BENCHY_STL").expect("SC_BENCHY_STL");
    let mut scene =
        sc_mesh_formats::loader::load_mesh_from_path(std::path::Path::new(&path), None, None)
            .unwrap()
            .graph;
    scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
    let id = scene.first_leaf_id().unwrap();
    let mesh = scene.mesh_at_leaf(id).unwrap();
    let start = std::time::Instant::now();
    let result = sc_print_glue::hollow::hollow(mesh, 2.0);
    eprintln!(
        "Benchy hollow: {:.6} s; result: {:?}",
        start.elapsed().as_secs_f64(),
        result
            .as_ref()
            .map(|m| (m.saved_mm3, m.pitch_mm, m.mesh.faces.len()))
    );
    assert!(result.is_ok());
}

#[test]
fn risk_drains_preserve_existing_shell_and_support_undo() {
    let mut scene = Scene::new();
    let source = sc_scene::bake_mesh_to_world(
        &sc_mesh_fixtures::unit_cube_mesh(),
        &Transform3D::Affine(AffineTransform3D::from_uniform_scale(10.0)),
    )
    .unwrap();
    let shell = sc_print_glue::hollow::hollow(&source, 1.0).unwrap().mesh;
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(shell.clone())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let edit = drain_risk(&scene, [0.0, 0.0, -3.9], 2.0).unwrap();
    edit.apply_to_scene(&mut scene).unwrap();
    let after = scene.mesh_at_leaf(id).unwrap();
    assert_ne!(after.faces.len(), shell.faces.len());
    edit.inverse().apply_to_scene(&mut scene).unwrap();
    assert_eq!(scene.mesh_at_leaf(id).unwrap(), &shell);
    edit.apply_to_scene(&mut scene).unwrap();
    // Re-slice: the middle cavity remains empty and the floor is opened.
    use sc_sl1cer::{
        CancelToken,
        profile::{
            AaLevel, BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
        },
    };
    let p = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        PixelResolution::new(200, 200).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 100).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    // This test's cube is centred at Z=0; put its minimum on the plate.
    scene.drop_at([0.0, 0.0], 0.0).unwrap();
    let outcome =
        sc_sl1cer::api::slice_scene_with_fallback(&scene, &p, &CancelToken::new(), |_| {}, true)
            .unwrap();
    let sc_sl1cer::SliceOutcome::Completed(leaves) = outcome else {
        panic!("slice cancelled")
    };
    let a = sc_print_job::analyze_layers(leaves[0].stack(), &p);
    assert!(!a.risks.iter().any(|r| matches!(
        r.kind,
        sc_print_job::RiskKind::SuctionCup | sc_print_job::RiskKind::TrappedCavity
    )));
}

/// Sleptic's library: every real STL tried (Benchy, Jaw, Lapras, ...) carries a
/// few zero-length-edge faces that slicing repairs silently. Hollowing used the
/// raw mesh and refused all of them as InvalidMesh, so hollowing never worked
/// on a real file. It now hollows a repaired copy.
#[test]
fn hollow_accepts_a_mesh_that_needs_the_usual_repair() {
    let cube = sc_mesh_fixtures::unit_cube_mesh();
    let vertices = cube.vertices.clone();
    let mut faces = cube.faces.clone();
    let first = faces[0];
    let a = usize::from(first.vertices[0]);
    // The corner diagonally opposite `a`, so the slivers share no cube edge.
    let far = (0..vertices.len())
        .max_by(|&i, &j| {
            let d = |k: usize| {
                let (p, q) = (vertices[k].coords(), vertices[a].coords());
                (0..3).map(|n| (p[n] - q[n]).powi(2)).sum::<f32>()
            };
            d(i).total_cmp(&d(j))
        })
        .unwrap();
    // A collapsed face that names the same vertex twice, as found in Benchy
    // after loading; the usual repair removes it.
    faces.push(IndexedTriangle::from_raw_indices(
        first.normal,
        [a as u32, a as u32, far as u32],
    ));
    let raw = IndexedMesh::new(None, vertices, faces);
    assert!(
        sc_print_glue::hollow::hollow(&raw, 0.1).is_err(),
        "fixture must reproduce the refused input"
    );
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
            LeafPayload::Mesh(Arc::new(raw)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let hollow = edit(&scene, id, 2.0, HoleChange::Auto(3.0)).unwrap();
    hollow.apply_to_scene(&mut scene).unwrap();
    let (saved, _) = stats(&scene);
    assert!(saved > 20.0f64.powi(3) * 0.4, "saved {saved} mm3");
}

/// R111 (the maintainer: drain holes of any length, symmetric on both sides
/// or at chosen points): a hole placed on a hollowed cube's right face runs
/// into the surface, a "left and right" mirror adds its twin on the left face
/// running the other way, a custom length is kept as typed, and holes come
/// off one at a time or all at once.
#[test]
fn drain_holes_mirror_take_a_custom_length_and_come_off() {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let apply = |scene: &mut Scene, change: HoleChange| {
        edit(scene, id, 2.0, change)
            .unwrap()
            .apply_to_scene(scene)
            .unwrap();
    };
    let holes = |scene: &Scene| settings(scene.mesh_at_leaf(id).unwrap()).unwrap().holes;
    apply(&mut scene, HoleChange::Keep);
    // A point a little off the face snaps onto it.
    apply(
        &mut scene,
        HoleChange::Add(DrainRequest {
            point: [10.2, 3.0, 1.0],
            aim: DrainAim::Surface,
            diameter_mm: 3.0,
            depth_mm: None,
            mirror: [true, false],
        }),
    );
    let placed = holes(&scene);
    assert_eq!(placed.len(), 2);
    let local = |h: &Hole| {
        (
            h.position.map(|v| v * 20.0),
            h.direction.map(|v| v.signum()),
        )
    };
    let (right, left) = (local(&placed[0]), local(&placed[1]));
    assert!(
        (right.0[0] - 10.0).abs() < 1e-3 && (right.0[1] - 3.0).abs() < 1e-3,
        "{right:?}"
    );
    assert_eq!(right.1[0], -1.0);
    assert!(
        (left.0[0] + 10.0).abs() < 1e-3 && (left.0[1] - 3.0).abs() < 1e-3,
        "{left:?}"
    );
    assert_eq!(left.1[0], 1.0);
    // Through the 2 mm wall and a little into the cavity.
    assert!(
        placed.iter().all(|h| h.depth > 2.0 && h.depth < 3.0),
        "{:?}",
        placed.iter().map(|h| h.depth).collect::<Vec<_>>()
    );
    assert_eq!(hole_rows(&scene, id).len(), 2);
    // All four sides, 15 mm long, straight down from the top.
    apply(
        &mut scene,
        HoleChange::Add(DrainRequest {
            point: [4.0, 5.0, 10.0],
            aim: DrainAim::Along([0.0, 0.0, -1.0]),
            diameter_mm: 2.0,
            depth_mm: Some(15.0),
            mirror: [true, true],
        }),
    );
    let placed = holes(&scene);
    assert_eq!(placed.len(), 6);
    let mut corners: Vec<[i32; 2]> = placed[2..]
        .iter()
        .map(|h| {
            [
                (h.position[0] * 20.0).round() as i32,
                (h.position[1] * 20.0).round() as i32,
            ]
        })
        .collect();
    corners.sort();
    assert_eq!(corners, vec![[-4, -5], [-4, 5], [4, -5], [4, 5]]);
    assert!(placed[2..].iter().all(|h| h.depth == 15.0));
    apply(&mut scene, HoleChange::Remove(0));
    assert_eq!(holes(&scene).len(), 5);
    apply(&mut scene, HoleChange::Clear);
    assert!(holes(&scene).is_empty());
}
