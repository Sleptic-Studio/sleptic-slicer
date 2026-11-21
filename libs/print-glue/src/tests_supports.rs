// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use crate::supports::{SupportError, SupportPoint, build_supports, sample_contacts};
use sc_mesh_core::IndexedMesh;
use sc_mesh_fixtures::unit_cube_mesh;
use sc_print_profiles::support_settings::SupportSettings;

fn slab() -> IndexedMesh {
    let mut mesh = unit_cube_mesh();
    for vertex in &mut mesh.vertices {
        *vertex = sc_mesh_core::Vertex::new([vertex[0] * 20.0, vertex[1] * 20.0, vertex[2] + 5.5]);
    }
    mesh
}

#[test]
fn separate_models_receive_owned_rafts_that_follow_plate_assignment() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, Scene};
    let mut source = Scene::new();
    let mut models = Vec::new();
    for x in [-30.0, 30.0] {
        let leaf = source
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 0.0])),
                LeafPayload::Mesh(std::sync::Arc::new(slab())),
            )
            .unwrap();
        source.add_root(leaf).unwrap();
        models.push(leaf);
    }
    let (mut supported, _) =
        crate::supports::geometric_supported_scene(&source, &SupportSettings::default()).unwrap();
    let mut plates = supported.plates().clone();
    let second = plates.add();
    plates.assignments.insert(models[1], second);
    supported.set_plates(plates);
    let mut rafts = 0;
    for id in supported
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap()
    {
        let comment = supported
            .node(id)
            .unwrap()
            .meta
            .as_ref()
            .and_then(|m| m.comment.as_deref())
            .unwrap_or("");
        if comment.starts_with("sc-generated-support") {
            let owner = comment
                .lines()
                .find_map(|l| l.strip_prefix("owner = ")?.parse::<u32>().ok())
                .unwrap();
            let model = *models.iter().find(|id| id.get() == owner).unwrap();
            assert_eq!(supported.plate_of(id), supported.plate_of(model));
            if comment.starts_with("sc-generated-support-raft") {
                rafts += 1;
            }
        }
    }
    assert_eq!(rafts, 2);
}

#[test]
fn lifted_flat_cube_has_deterministic_spaced_bottom_grid() {
    let mesh = slab();
    let settings = SupportSettings::default();
    let points = sample_contacts(&mesh, &settings).unwrap();
    assert_eq!(points, sample_contacts(&mesh, &settings).unwrap());
    assert!(points.len() >= 20, "{}", points.len());
    assert!(
        points
            .iter()
            .all(|p| p.position[2] == 5.0 && p.normal[2] == -1.0)
    );
    for (i, point) in points.iter().enumerate() {
        for other in &points[i + 1..] {
            let dx = point.position[0] - other.position[0];
            let dy = point.position[1] - other.position[1];
            assert!(dx * dx + dy * dy >= 15.99);
        }
    }
}

#[test]
fn columns_and_chamfered_raft_are_closed_with_correct_normals() {
    let mesh = slab();
    let settings = SupportSettings {
        lattice_raft: false,
        ..Default::default()
    };
    let points = sample_contacts(&mesh, &settings).unwrap();
    let structure = build_supports(&mesh, &points, &settings).unwrap();
    assert_eq!(structure.columns.len(), points.len());
    for mesh in structure.columns.iter().chain(structure.raft.iter()) {
        let diagnosis = mesh.diagnose();
        assert!(!diagnosis.has_problems(), "{diagnosis:?}");
    }
    let raft = structure.raft.unwrap();
    assert!(raft.vertices.iter().any(|v| v[2] == 0.0));
    assert!(
        raft.vertices
            .iter()
            .any(|v| v[2] == settings.raft_thickness_mm as f32)
    );
    assert!(
        raft.faces
            .iter()
            .any(|f| (f.normal[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5)
    );
}

#[test]
fn no_raft_empty_contacts_and_invalid_inputs() {
    let mesh = slab();
    let settings = SupportSettings {
        raft: false,
        ..Default::default()
    };
    assert_eq!(build_supports(&mesh, &[], &settings).unwrap().raft, None);
    let point = SupportPoint {
        position: [0.0, 0.0, 5.0],
        normal: [0.0, 0.0, -1.0],
    };
    let structure = build_supports(&mesh, &[point], &settings).unwrap();
    assert!(structure.raft.is_none());
    assert!(structure.columns[0].vertices.iter().any(|v| v[2] == 0.0));
    assert_eq!(
        build_supports(
            &mesh,
            &[SupportPoint {
                position: [0.0, 0.0, f32::NAN],
                ..point
            }],
            &settings
        ),
        Err(SupportError::InvalidContact)
    );
    let mut invalid = mesh;
    invalid.vertices[0] = sc_mesh_core::Vertex::new([f32::NAN, 0.0, 0.0]);
    assert_eq!(
        sample_contacts(&invalid, &settings),
        Err(SupportError::InvalidMesh)
    );
}

#[test]
fn lower_geometry_obstructs_column_and_is_never_drilled_through() {
    let mesh = slab();
    let point = SupportPoint {
        position: [0.0, 0.0, 6.0],
        normal: [0.0, 0.0, -1.0],
    };
    let structure = build_supports(&mesh, &[point], &SupportSettings::default()).unwrap();
    assert!(structure.points.is_empty());
    assert!(structure.raft.is_none());
}

#[test]
fn lifted_scene_preserves_input_and_first_layers_slice_the_raft() {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::{
        AaLevel, BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
    };
    let mut source = Scene::new();
    let mut mesh = slab();
    for v in &mut mesh.vertices {
        *v.as_array_mut().get_mut(2).unwrap() -= 4.9;
    }
    let leaf = source
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(std::sync::Arc::new(mesh)),
        )
        .unwrap();
    source.add_root(leaf).unwrap();
    let before = source.clone();
    let (scene, structure) =
        crate::supports::geometric_supported_scene(&source, &SupportSettings::default()).unwrap();
    assert_eq!(source, before);
    assert_eq!(
        scene.roots().len(),
        source.roots().len() + structure.columns.len() + 1
    );
    let model = sc_scene::bake_leaf_to_world(&scene, leaf).unwrap();
    assert!((model.local_aabb().unwrap().min()[2] - 5.1).abs() < 1e-5);
    let profile = SliceProfile::new(
        BuildVolume::new(60.0, 60.0, 100.0).unwrap(),
        PixelResolution::new(600, 600).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 60).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let sliced = sc_sl1cer::api::slice_scene(&scene, &profile).unwrap();
    let raft = sliced.last().unwrap().stack();
    assert!(
        raft.layers()[..6]
            .iter()
            .all(|layer| !layer.polygons().is_empty())
    );
    assert!(
        raft.layers()[10..]
            .iter()
            .all(|layer| layer.polygons().is_empty())
    );
    assert!(
        sliced[0].stack().layers()[..50]
            .iter()
            .all(|layer| layer.polygons().is_empty())
    );
}

#[test]
fn extreme_dimensions_return_error_instead_of_panicking() {
    let settings = SupportSettings {
        base_diameter_mm: f64::MAX,
        ..Default::default()
    };
    let point = SupportPoint {
        position: [0.0, 0.0, 5.0],
        normal: [0.0, 0.0, -1.0],
    };
    // The base pad first narrows to clear the slab (R82); the raft under
    // it is still unrepresentable or past the sampling limit.
    assert!(matches!(
        build_supports(&slab(), &[point], &settings),
        Err(SupportError::UnrepresentableGeometry | SupportError::SamplingLimit)
    ));
    let settings = SupportSettings {
        spacing_mm: 1e-6,
        ..Default::default()
    };
    assert_eq!(
        sample_contacts(&slab(), &settings),
        Err(SupportError::SamplingLimit)
    );
}

fn sphere() -> IndexedMesh {
    use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
    use sc_mesh_core::{IndexedTriangle, Vertex};
    let mut vertices = vec![
        [1.0_f32, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut faces = vec![
        [0_u32, 1, 4],
        [1, 2, 4],
        [2, 3, 4],
        [3, 0, 4],
        [1, 0, 5],
        [2, 1, 5],
        [3, 2, 5],
        [0, 3, 5],
    ];
    for _ in 0..3 {
        let mut midpoints = std::collections::BTreeMap::new();
        let mut midpoint = |a: u32, b: u32| {
            *midpoints.entry((a.min(b), a.max(b))).or_insert_with(|| {
                let p: [f32; 3] = std::array::from_fn(|axis| {
                    (vertices[a as usize][axis] + vertices[b as usize][axis]) / 2.0
                });
                let length = p.iter().map(|v| v * v).sum::<f32>().sqrt();
                let index = vertices.len() as u32;
                vertices.push(p.map(|v| v / length));
                index
            })
        };
        faces = faces
            .into_iter()
            .flat_map(|[a, b, c]| {
                let ab = midpoint(a, b);
                let bc = midpoint(b, c);
                let ca = midpoint(c, a);
                [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]
            })
            .collect();
    }
    let vertices: Vec<_> = vertices
        .into_iter()
        .map(|[x, y, z]| Vertex::new([x * 10.0, y * 10.0, z * 10.0 + 15.0]))
        .collect();
    let faces = faces
        .into_iter()
        .map(|indices| {
            IndexedTriangle::from_raw_indices(
                compute_normal_from_triangle_vertices(&indices.map(|i| vertices[i as usize]))
                    .unwrap(),
                indices,
            )
        })
        .collect();
    IndexedMesh::new(None, vertices, faces)
}

#[test]
fn sphere_has_bottom_contact_and_surrounding_contacts_on_surface() {
    let mesh = sphere();
    assert!(!mesh.diagnose().has_problems());
    let settings = SupportSettings {
        tree_supports: false,
        ..Default::default()
    };
    let points = sample_contacts(&mesh, &settings).unwrap();
    assert_eq!(points[0].position, [0.0, 0.0, 5.0]);
    // The six hexagonal-grid neighbours around the bottom.
    let pitch = 4.0 * 1.5f32.sqrt();
    let row = pitch * 3f32.sqrt() / 2.0;
    for [x, y] in [
        [pitch, 0.0],
        [-pitch, 0.0],
        [pitch / 2.0, row],
        [-pitch / 2.0, row],
        [pitch / 2.0, -row],
        [-pitch / 2.0, -row],
    ] {
        assert!(
            points.iter().any(|p| (p.position[0] - x).abs() < 1e-3
                && (p.position[1] - y).abs() < 1e-3
                && p.position[2] > 5.0),
            "{x} {y} {points:?}"
        );
    }
    let structure = build_supports(&mesh, &points, &settings).unwrap();
    assert_eq!(structure.points, points);
    assert!(
        structure
            .columns
            .iter()
            .all(|m| !m.diagnose().has_problems())
    );
}

#[test]
fn distinct_local_minima_are_kept_even_inside_grid_spacing() {
    let mut mesh = unit_cube_mesh();
    for v in &mut mesh.vertices {
        v.as_array_mut()[2] += 5.5;
    }
    let offset = mesh.vertices.len() as u32;
    let mut second = mesh.clone();
    for v in &mut second.vertices {
        v.as_array_mut()[0] += 2.0;
    }
    mesh.vertices.extend(second.vertices);
    mesh.faces.extend(second.faces.into_iter().map(|mut f| {
        f.vertices = f.vertices.map(|i| i.offset(offset));
        f
    }));
    let points = sample_contacts(&mesh, &SupportSettings::default()).unwrap();
    assert!(points.iter().any(|p| p.position[0] < 0.0));
    assert!(points.iter().any(|p| p.position[0] > 1.0));
}

#[test]
fn floating_block_island_completion_all_mirrors() {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let mut source = Scene::new();
    let mut mesh = unit_cube_mesh();
    for v in &mut mesh.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] * 0.08 + 2.075, v[1] * 0.08 - 1.925, v[2] + 0.5]);
    }
    let leaf = source
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(std::sync::Arc::new(mesh)),
        )
        .unwrap();
    source.add_root(leaf).unwrap();
    // A shallow 0.02 mm sink: with the 0.1 mm default (R84) even this
    // sub-pixel tip overlaps the block and leaves no island to complete.
    let settings = SupportSettings {
        tip_contact_diameter_mm: 0.01,
        spacing_mm: 20.0,
        contact_depth_mm: 0.02,
        ..Default::default()
    };
    for x in [false, true] {
        for y in [false, true] {
            let profile = SliceProfile::new(
                BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
                PixelResolution::new(400, 400).unwrap(),
                // Layers thinner than a pixel: the self-supporting reach is
                // 0 (strict rule), so the sub-pixel geometric tip really
                // leaves an island for completion to fix. At 0.1 mm layers
                // it sits within the 45 degree reach and counts as held.
                LayerSchedule::uniform(0.04, 0.04, 153).unwrap(),
                Mirror { x, y },
                AaLevel::Off,
                PackingMode::None,
            );
            assert_eq!(sc_print_job::self_supporting_reach_px(&profile), 0);
            let (geometric, _) =
                crate::supports::geometric_supported_scene(&source, &settings).unwrap();
            let merge = |scene: &Scene| {
                sc_sl1cer::contour::merge_stacks(
                    sc_sl1cer::api::slice_scene(scene, &profile)
                        .unwrap()
                        .into_iter()
                        .map(|l| l.into_stack())
                        .collect(),
                )
                .unwrap()
            };
            assert!(!sc_print_job::find_islands(&merge(&geometric), &profile).is_empty());
            // R90: the early-stopping check agrees, across the 64-layer
            // batches the scan is prepared in (153 layers here).
            assert!(sc_print_job::has_islands(&merge(&geometric), &profile));
            let (scene, structure, analysis) =
                crate::supports::auto_supported_scene_analyzed(&source, &settings, &profile)
                    .unwrap();
            assert!(analysis.islands.is_empty(), "{analysis:?}");
            assert!(!sc_print_job::has_islands(&merge(&scene), &profile));
            assert!(structure.points.len() > 1);
            // Completion reports islands and volume (no print-risk scan, R75).
            assert_eq!(
                analysis,
                sc_print_job::analyze_islands(&merge(&scene), &profile)
            );
        }
    }
}

#[test]
fn island_completion_bridges_from_model_below_without_drilling_through_it() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let mut scene = Scene::new();
    // The lower model is 19 mm wide, nearly the whole 20 mm plate: no column
    // could lean (R83) or come in from the side (R85) past it to the plate,
    // so the island's column has to land on it. It is 2 mm tall, so its top
    // is clear of the 1 mm raft.
    for (z, width, height) in [(1.0, 19.0, 2.0), (3.5, 1.0, 1.0)] {
        let leaf = scene
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, z])).compose(
                    &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([
                        width, width, height,
                    ])),
                ),
                LeafPayload::Mesh(std::sync::Arc::new(unit_cube_mesh())),
            )
            .unwrap();
        scene.add_root(leaf).unwrap();
    }
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        PixelResolution::new(400, 400).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 90).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let settings = SupportSettings {
        spacing_mm: 20.0,
        ..Default::default()
    };
    let (_, structure, remaining) =
        crate::supports::auto_supported_scene(&scene, &settings, &profile).unwrap();
    assert!(remaining.is_empty(), "{remaining:?}");
    // The bridging column lands on the lower model on a contact tip, not a
    // wide base pad that would scar the part it lands on (R58). The lower
    // model has a flat base, so it stays on the plate (R38): the landing is
    // on its top at 2 mm.
    let foot = |m: &IndexedMesh| {
        let low = m
            .vertices
            .iter()
            .map(|v| v[2])
            .fold(f32::INFINITY, f32::min);
        let ring: Vec<_> = m
            .vertices
            .iter()
            .filter(|v| (v[2] - low).abs() < 1e-4)
            .collect();
        let cx = ring.iter().map(|v| v[0]).sum::<f32>() / ring.len() as f32;
        let cy = ring.iter().map(|v| v[1]).sum::<f32>() / ring.len() as f32;
        let radius = ring
            .iter()
            .map(|v| (v[0] - cx).hypot(v[1] - cy))
            .fold(0.0f32, f32::max);
        (low, radius)
    };
    let landing = structure
        .columns
        .iter()
        .map(foot)
        .find(|(_, radius)| {
            (f64::from(*radius) - settings.tip_contact_diameter_mm / 2.0).abs() < 0.01
        })
        .expect("a column landing on the lower model on a contact tip");
    assert!(
        (f64::from(landing.0) - (2.0 - settings.contact_depth_mm)).abs() < 0.01,
        "lands at z {}",
        landing.0
    );
}

/// Sleptic (R58): supports should be built like Chitubox / Lychee's. Tall
/// columns are cross-braced to a neighbour; short ones are left alone, and
/// every column stays one closed mesh per contact.
#[test]
fn r58_tall_columns_are_cross_braced_short_ones_are_not() {
    let high = {
        let mut mesh = unit_cube_mesh();
        for vertex in &mut mesh.vertices {
            *vertex =
                sc_mesh_core::Vertex::new([vertex[0] * 20.0, vertex[1] * 20.0, vertex[2] + 30.5]);
        }
        mesh
    };
    // Plain columns: trees merge instead of bracing.
    let settings = SupportSettings {
        tree_supports: false,
        ..Default::default()
    };
    // A brace reaches over to a neighbouring pillar, so a braced column
    // spreads wider than its own foot.
    let plain = settings.base_diameter_mm as f32 + 1.0;
    let spread = |m: &IndexedMesh| {
        let span = |axis: usize| {
            let (lo, hi) = m.vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                (lo.min(v[axis]), hi.max(v[axis]))
            });
            hi - lo
        };
        span(0).max(span(1))
    };
    for (mesh, braced) in [(high, true), (slab(), false)] {
        let points = sample_contacts(&mesh, &settings).unwrap();
        let structure = build_supports(&mesh, &points, &settings).unwrap();
        assert_eq!(structure.columns.len(), points.len());
        for column in &structure.columns {
            let diagnosis = column.diagnose();
            assert!(!diagnosis.has_problems(), "{diagnosis:?}");
        }
        let with_braces = structure
            .columns
            .iter()
            .filter(|m| spread(m) > plain)
            .count();
        if braced {
            assert!(
                with_braces * 2 >= structure.columns.len() / 2,
                "{with_braces}"
            );
            // Rods stay between the columns' shafts: nothing below the
            // raft top or above the model.
            for column in &structure.columns {
                for v in &column.vertices {
                    assert!(v[2] >= 0.0 && v[2] <= 31.0, "{v:?}");
                }
            }
        } else {
            assert_eq!(with_braces, 0);
        }
    }
}

#[test]
fn contacts_keep_model_ownership_across_world_transforms() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, Scene};
    let mut scene = Scene::new();
    let mut ids = Vec::new();
    for x in [-10., 10.] {
        let id = scene
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([x, 0., 5.])),
                LeafPayload::Mesh(std::sync::Arc::new(unit_cube_mesh())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
        ids.push(id);
    }
    let points = [-10., 10.].map(|x| SupportPoint {
        position: [x, 0., 4.5],
        normal: [0., 0., -1.],
    });
    assert_eq!(
        crate::supports::support_owners(&scene, &points),
        ids.iter().copied().map(Some).collect::<Vec<_>>()
    );
    let (generated, structure) =
        crate::supports::geometric_supported_scene(&scene, &SupportSettings::default()).unwrap();
    let columns: Vec<_> = generated
        .roots()
        .iter()
        .filter_map(|id| {
            generated
                .node(*id)
                .and_then(|n| n.meta.as_ref())
                .and_then(|m| m.comment.as_deref())
        })
        .filter(|s| s.starts_with("sc-generated-support\n"))
        .collect();
    assert_eq!(columns.len(), structure.points.len());
    assert!(columns.iter().all(|comment| {
        comment
            .lines()
            .any(|line| ids.iter().any(|id| line == format!("owner = {}", id.get())))
    }));
}

/// The plate area a raft covers, seen from above: its bars and pads overlap
/// (R105), so mesh volume would count the overlaps twice.
fn covered_area(mesh: &IndexedMesh) -> f64 {
    let cell = 0.2;
    let lo = mesh.local_aabb().unwrap().min();
    let hi = mesh.local_aabb().unwrap().max();
    let nx = ((hi[0] - lo[0]) as f64 / cell).ceil() as usize + 1;
    let ny = ((hi[1] - lo[1]) as f64 / cell).ceil() as usize + 1;
    let mut covered = vec![false; nx * ny];
    for f in &mesh.faces {
        let [a, b, c] = f.vertices.map(|i| mesh.vertices[usize::from(i)].coords().map(f64::from));
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 {
            continue;
        }
        let (x0, x1) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
        let (y0, y1) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
        let ix0 = ((x0 - f64::from(lo[0])) / cell).floor().max(0.0) as usize;
        let iy0 = ((y0 - f64::from(lo[1])) / cell).floor().max(0.0) as usize;
        let ix1 = (((x1 - f64::from(lo[0])) / cell).ceil() as usize).min(nx - 1);
        let iy1 = (((y1 - f64::from(lo[1])) / cell).ceil() as usize).min(ny - 1);
        for iy in iy0..=iy1 {
            for ix in ix0..=ix1 {
                let p = [f64::from(lo[0]) + (ix as f64 + 0.5) * cell, f64::from(lo[1]) + (iy as f64 + 0.5) * cell];
                let side = |u: [f64; 3], v: [f64; 3]| (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0]);
                let (s1, s2, s3) = (side(a, b), side(b, c), side(c, a));
                if (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0) || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0) {
                    covered[iy * nx + ix] = true;
                }
            }
        }
    }
    covered.iter().filter(|c| **c).count() as f64 * cell * cell
}

/// The lattice raft is closed, open in between and covers clearly less of the
/// plate than the solid raft (R105: a smooth spider web).
#[test]
fn lattice_raft_is_closed_has_openings_and_uses_less_resin() {
    let mesh = cuboid([-16.0, -16.0, 6.0], [16.0, 16.0, 8.0]);
    let contacts: Vec<SupportPoint> = (0..16)
        .map(|i| SupportPoint {
            position: [-12.0 + f32::from(i as u8 % 4) * 8.0, -12.0 + f32::from(i as u8 / 4) * 8.0, 6.0],
            normal: [0.0, 0.0, -1.0],
        })
        .collect();
    let settings = SupportSettings::default();
    let raft = |lattice: bool| {
        build_supports(&mesh, &contacts, &SupportSettings { lattice_raft: lattice, ..settings })
            .unwrap()
            .raft
            .unwrap()
    };
    let (solid, lattice) = (raft(false), raft(true));
    assert!(!lattice.diagnose().has_problems());
    let (solid, lattice) = (covered_area(&solid), covered_area(&lattice));
    assert!(
        lattice < solid * 0.75,
        "lattice covers {lattice:.0} mm2 against solid {solid:.0} mm2"
    );
}

#[test]
fn strength_presets_change_contact_tip_geometry_and_penetration() {
    use sc_print_profiles::support_settings::SupportPreset;
    let mesh = slab();
    let contact = [SupportPoint {
        position: [0.0, 0.0, 5.0],
        normal: [0.0, 0.0, -1.0],
    }];
    let mut tip_radii = Vec::new();
    for preset in [
        SupportPreset::Light,
        SupportPreset::Medium,
        SupportPreset::Heavy,
    ] {
        let mut settings = SupportSettings::default();
        settings.apply_preset(preset);
        settings.validate().unwrap();
        let support = build_supports(&mesh, &contact, &settings)
            .unwrap()
            .columns
            .remove(0);
        let top = support
            .vertices
            .iter()
            .map(|v| v[2])
            .max_by(f32::total_cmp)
            .unwrap();
        assert!((f64::from(top) - 5.0 - settings.contact_depth_mm).abs() < 1e-6);
        let radius = support
            .vertices
            .iter()
            .filter(|v| v[2] == top)
            .map(|v| v[0].hypot(v[1]))
            .max_by(f32::total_cmp)
            .unwrap();
        tip_radii.push(radius);
    }
    assert!(tip_radii[0] < tip_radii[1] && tip_radii[1] < tip_radii[2]);
}

#[test]
fn column_diameter_scales_with_area_and_respects_both_bounds() {
    let s = SupportSettings::default();
    let d = crate::supports::column_diameter;
    assert_eq!(d(&s, 0.01), s.min_column_diameter_mm);
    assert_eq!(d(&s, 10000.0), s.max_column_diameter_mm);
    assert!(d(&s, 32.0) > d(&s, 16.0));
}

#[test]
fn symmetry_detection_and_mirrored_contacts_require_geometric_match() {
    let mesh = slab();
    assert!(crate::supports::symmetry_plane(&mesh).is_some());
    let settings = SupportSettings {
        symmetric_supports: true,
        ..Default::default()
    };
    let points = sample_contacts(&mesh, &settings).unwrap();
    let (n, offset) = crate::supports::symmetry_plane(&mesh).unwrap();
    for p in &points {
        let distance = 2.0 * (n[0] * p.position[0] + n[1] * p.position[1] - offset);
        assert!(points.iter().any(|q| (0..2).all(|axis| {
            (q.position[axis] - (p.position[axis] - distance * n[axis])).abs() < 0.01
        })));
    }
    let mut asymmetric = mesh;
    asymmetric.vertices[0] = sc_mesh_core::Vertex::new([-7.123, -8.234, 4.876]);
    assert!(crate::supports::symmetry_plane(&asymmetric).is_none());
}

#[test]
fn tree_clusters_keep_all_contacts_with_fewer_trunks() {
    let mut mesh = slab();
    for v in &mut mesh.vertices {
        *v = sc_mesh_core::Vertex::new([v[0], v[1], v[2] + 20.0]);
    }
    let columns = SupportSettings::default();
    let points = sample_contacts(&mesh, &columns).unwrap();
    let tree = SupportSettings {
        tree_supports: true,
        ..columns
    };
    let result = build_supports(&mesh, &points, &tree).unwrap();
    assert_eq!(result.points.len(), points.len());
    assert!(result.columns.len() < points.len());
    let narrow = build_supports(
        &mesh,
        &points,
        &SupportSettings {
            branch_angle_deg: 1.0,
            ..tree
        },
    )
    .unwrap();
    assert_eq!(
        narrow.columns.len(),
        points.len(),
        "angle limit must prevent steep branches"
    );
    for p in points {
        assert!(result.points.contains(&p));
    }
    for mesh in result.columns {
        assert!(mesh.validate_faces().is_ok());
    }
}

#[test]
fn painted_keep_adds_contacts_and_avoid_overrides_keep() {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene, SceneMetadata};
    let make = |comment: &str| {
        let mut scene = Scene::new();
        let id = scene
            .add_leaf(
                Some(SceneMetadata {
                    name: None,
                    comment: Some(comment.into()),
                }),
                Transform3D::Identity,
                LeafPayload::Mesh(std::sync::Arc::new(slab())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
        scene
    };
    let s = SupportSettings {
        spacing_mm: 20.0,
        ..Default::default()
    };
    let (_, base) = crate::supports::geometric_supported_scene(&make(""), &s).unwrap();
    let (_, keep) =
        crate::supports::geometric_supported_scene(&make("sc-support-paint 2 2 5 3 1"), &s)
            .unwrap();
    assert!(keep.points.len() > base.points.len());
    let (_, avoid) = crate::supports::geometric_supported_scene(
        &make("sc-support-paint 2 2 5 3 1\nsc-support-paint 0 0 5 30 0"),
        &s,
    )
    .unwrap();
    assert!(avoid.points.is_empty());
}

#[test]
fn tree_fixture_keeps_zero_raster_islands_with_fewer_support_leaves() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 15.0])),
            LeafPayload::Mesh(std::sync::Arc::new(slab())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(50.0, 50.0, 50.0).unwrap(),
        PixelResolution::new(500, 500).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 270).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let s = SupportSettings {
        tree_supports: true,
        ..Default::default()
    };
    let (_, structure, analysis) =
        crate::supports::auto_supported_scene_analyzed(&scene, &s, &profile).unwrap();
    assert!(structure.columns.len() < structure.points.len());
    assert!(analysis.islands.is_empty(), "{:?}", analysis.islands);
}

#[test]
fn symmetry_checks_surfaces_and_detects_rotated_planes() {
    let mut mesh = slab();
    mesh.faces.remove(6);
    mesh.faces.remove(0);
    assert!(
        crate::supports::symmetry_plane(&mesh).is_none(),
        "symmetric vertices alone are insufficient"
    );
    let mut mesh = slab();
    for v in &mut mesh.vertices {
        let angle = 0.37_f32;
        *v = sc_mesh_core::Vertex::new([
            v[0] * angle.cos() - v[1] * angle.sin(),
            v[0] * angle.sin() + v[1] * angle.cos(),
            v[2],
        ]);
    }
    assert!(crate::supports::symmetry_plane(&mesh).is_some());
}

#[test]
fn larger_island_load_changes_actual_shaft_geometry() {
    let large = slab();
    let mut small = large.clone();
    for v in &mut small.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] / 10.0, v[1] / 10.0, v[2]]);
    }
    let settings = SupportSettings {
        min_column_diameter_mm: 0.4,
        ..Default::default()
    };
    let contact = SupportPoint {
        position: [0., 0., 5.],
        normal: [0., 0., -1.],
    };
    let a = build_supports(&small, &[contact], &settings).unwrap();
    let b = build_supports(&large, &[contact], &settings).unwrap();
    let diameter = |s: &crate::supports::SupportStructure| {
        2.0 * f64::from(s.columns[0].vertices[crate::supports::ROUND_SIDES as usize][0])
    };
    assert!(diameter(&b) > diameter(&a));
    assert!(diameter(&b) <= settings.max_column_diameter_mm + 1e-6);
}

#[test]
fn avoid_paint_wins_over_raster_island_completion() {
    use sc_mesh_core::transform::Transform3D;
    use sc_sl1cer::profile::*;
    let mut scene = sc_scene::Scene::new();
    let id = scene
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: None,
                comment: Some("sc-support-paint 0 0 5 30 0".into()),
            }),
            Transform3D::Identity,
            sc_scene::LeafPayload::Mesh(std::sync::Arc::new(slab())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(50., 50., 50.).unwrap(),
        PixelResolution::new(100, 100).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 120).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let (_, structure, analysis) = crate::supports::auto_supported_scene_analyzed(
        &scene,
        &SupportSettings::default(),
        &profile,
    )
    .unwrap();
    assert!(structure.points.is_empty());
    assert!(structure.columns.is_empty());
    assert!(
        !analysis.islands.is_empty(),
        "forbidden supports must remain a reported print risk"
    );
}

/// Lapras, butterfly, Devil and vice grips failed with tree supports ("invalid
/// support contact"): several branch outlines in a merged tree mesh got no
/// material-side vote (ring points did not match their segments bit for bit),
/// fell back to even-odd parity and were cut out of the trunk as holes. The
/// fragments became raster islands inside the supports, under the model,
/// where no support can go. These are the five contacts of the Lapras trunk
/// that failed (x, y, z in mm, already lifted).
#[test]
fn lapras_tree_trunk_slices_without_false_holes() {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let contacts: Vec<_> = [
        [-4.0, -32.0, 11.186971],
        [-4.0, -28.0, 8.576808],
        [0.0, -28.0, 8.330431],
        [4.0, -32.0, 11.200936],
        [4.0, -28.0, 8.572021],
    ]
    .into_iter()
    .map(|position| SupportPoint {
        position,
        normal: [0.0, 0.0, -1.0],
    })
    .collect();
    // One shell directly above all five contacts (so they share a trunk),
    // high enough never to block a branch.
    let mut far = slab();
    for v in &mut far.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] * 0.4, v[1] * 0.4 - 30.0, v[2] + 7.0]);
    }
    // A solid raft: a lattice's openings are real holes in these layers.
    let settings = SupportSettings {
        tree_supports: true,
        lattice_raft: false,
        shaft_diameter_mm: 1.2,
        raft_thickness_mm: 1.0,
        base_height_mm: 0.6,
        tip_length_mm: 1.2,
        ..Default::default()
    };
    let structure = build_supports(&far, &contacts, &settings).unwrap();
    assert!(structure.columns.len() < contacts.len(), "branches merged");
    let mut scene = Scene::new();
    for mesh in structure.columns.iter().chain(structure.raft.iter()) {
        let id = scene
            .add_leaf(
                None,
                Transform3D::Identity,
                LeafPayload::Mesh(std::sync::Arc::new(mesh.clone())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
    }
    let profile = SliceProfile::new(
        BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
        PixelResolution::new(3840, 2400).unwrap(),
        LayerSchedule::uniform(0.05, 0.05, 260).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let sc_sl1cer::SliceOutcome::Completed(leaves) = sc_sl1cer::api::slice_scene_with_fallback(
        &scene,
        &profile,
        &sc_sl1cer::CancelToken::new(),
        |_| {},
        false,
    )
    .unwrap() else {
        unreachable!()
    };
    let stack =
        sc_sl1cer::contour::merge_stacks(leaves.into_iter().map(|l| l.into_stack()).collect())
            .unwrap();
    // Up to the trunk top every branch section lies inside the trunk, so
    // any hole there is false. Higher up, diverging branches may enclose a
    // real gap, which is fine as long as nothing floats.
    for layer in stack.layers().iter().filter(|l| l.z_mm() < 1.75) {
        for polygon in layer.polygons() {
            assert!(
                polygon.holes().is_empty(),
                "false hole at z = {}",
                layer.z_mm()
            );
        }
    }
    let analysis = sc_print_job::analyze_layers(&stack, &profile);
    assert!(analysis.islands.is_empty(), "{:?}", analysis.islands);
}

/// vice_grips_v25_full.stl (229 mm on a 192 mm plate) failed tree supports
/// with "invalid support contact": the raster clips the model at the plate
/// edge, which leaves islands down in the support zone where no column fits.
/// Unsupportable islands must stay reported, never fail the whole job.
#[test]
fn model_hanging_past_the_plate_edge_still_gets_supports() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([14.0, 3.0, 2.0])).compose(
                &Transform3D::Affine(AffineTransform3D::from_rotation_x(0.4)),
            ),
            LeafPayload::Mesh(std::sync::Arc::new(slab())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(40.0, 40.0, 60.0).unwrap(),
        PixelResolution::new(800, 800).unwrap(),
        LayerSchedule::uniform(0.05, 0.05, 900).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    for tree_supports in [false, true] {
        let s = SupportSettings {
            tree_supports,
            ..Default::default()
        };
        crate::supports::auto_supported_scene_analyzed(&scene, &s, &profile)
            .expect("clipped islands must not fail support generation");
    }
}

#[test]
fn r22_direct_model_and_supported_neighbour_share_layer_one() {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let mut source = Scene::new();
    let mut ids = Vec::new();
    for (x, z, name) in [(-15.0, 0.5, "Direct"), (15.0, 2.5, "Supported")] {
        let mut mesh = unit_cube_mesh();
        for v in &mut mesh.vertices {
            *v = sc_mesh_core::Vertex::new([v[0] * 10.0 + x, v[1] * 10.0, v[2] + z]);
        }
        let id = source
            .add_leaf(
                Some(sc_scene::SceneMetadata {
                    name: Some(name.into()),
                    comment: None,
                }),
                Transform3D::Identity,
                LeafPayload::Mesh(std::sync::Arc::new(mesh)),
            )
            .unwrap();
        source.add_root(id).unwrap();
        ids.push(id);
    }
    let settings = SupportSettings::default();
    let profile = SliceProfile::new(
        BuildVolume::new(60.0, 40.0, 20.0).unwrap(),
        PixelResolution::new(600, 400).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 90).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let (scene, structure, analysis) =
        crate::supports::auto_supported_scene_analyzed(&source, &settings, &profile).unwrap();
    assert!(analysis.islands.is_empty());
    assert!(!structure.points.is_empty());
    assert!(structure.points.iter().all(|p| p.position[0] > 0.0));
    assert_eq!(
        sc_scene::bake_leaf_to_world(&source, ids[0]).unwrap(),
        sc_scene::bake_leaf_to_world(&scene, ids[0]).unwrap()
    );
    assert!(
        sc_scene::bake_leaf_to_world(&scene, ids[1])
            .unwrap()
            .local_aabb()
            .unwrap()
            .min()[2]
            >= 7.0
    );
    let results = crate::supports::model_results(&scene, &structure, &settings);
    assert!(
        results
            .iter()
            .any(|m| m.name == "Direct" && m.direct && m.contacts == 0)
    );
    assert!(
        results
            .iter()
            .any(|m| m.name == "Supported" && !m.direct && m.contacts > 0)
    );
    let slices = sc_sl1cer::api::slice_scene(&scene, &profile).unwrap();
    assert!(!slices[0].stack().layers()[0].polygons().is_empty());
    assert!(slices[1].stack().layers()[0].polygons().is_empty());
    assert!(
        !slices.last().unwrap().stack().layers()[0]
            .polygons()
            .is_empty()
    );
    let direct = sc_scene::bake_leaf_to_world(&scene, ids[0]).unwrap();
    assert!(sample_contacts(&direct, &settings).unwrap().is_empty());
}

#[test]
fn r22_painted_direct_model_is_still_skipped_by_automatic_generation() {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let mut source = Scene::new();
    let mut mesh = unit_cube_mesh();
    for v in &mut mesh.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] * 20.0, v[1] * 20.0, v[2] + 0.5]);
    }
    let id = source
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: Some("Painted direct".into()),
                comment: Some("sc-support-paint 0 0 0 5 1\n".into()),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(std::sync::Arc::new(mesh)),
        )
        .unwrap();
    source.add_root(id).unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(60.0, 60.0, 20.0).unwrap(),
        PixelResolution::new(600, 600).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 10).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let (generated, structure, analysis) =
        crate::supports::auto_supported_scene_analyzed(&source, &Default::default(), &profile)
            .unwrap();
    assert_eq!(source, generated);
    assert!(
        structure.points.is_empty() && structure.columns.is_empty() && structure.raft.is_none()
    );
    assert!(analysis.islands.is_empty());
}

#[test]
fn horizontal_boss_completion_is_spaced_and_has_no_raster_islands() {
    use sc_mesh_core::transform::Transform3D;
    use sc_mesh_core::{IndexedTriangle, Normal, Vertex};
    use sc_sl1cer::profile::*;
    let sides = 96usize;
    let mut vertices = Vec::new();
    for x in [-4.0_f32, 4.0] {
        for i in 0..sides {
            let angle = i as f32 * std::f32::consts::TAU / sides as f32;
            vertices.push(Vertex::new([x, angle.cos(), angle.sin() + 1.0]));
        }
    }
    vertices.extend([Vertex::new([-4.0, 0.0, 1.0]), Vertex::new([4.0, 0.0, 1.0])]);
    let mut indices = Vec::new();
    for i in 0..sides {
        let j = (i + 1) % sides;
        indices.extend([
            [i, j, i + sides],
            [j, j + sides, i + sides],
            [2 * sides, j, i],
            [2 * sides + 1, i + sides, j + sides],
        ]);
    }
    let faces = indices
        .into_iter()
        .map(|f| {
            let [a, b, c] = f.map(|i| vertices[i].to_f64());
            let n = b.diff(&a).cross_product(&c.diff(&a));
            let len = n.norm();
            IndexedTriangle::from_raw_indices(
                Normal::new(n.coords().map(|v| (v / len) as f32)),
                f.map(|i| i as u32),
            )
        })
        .collect();
    let mesh = IndexedMesh {
        meta: None,
        vertices,
        faces,
    };
    let mut source = sc_scene::Scene::new();
    let id = source
        .add_leaf(
            None,
            Transform3D::Identity,
            sc_scene::LeafPayload::Mesh(std::sync::Arc::new(mesh)),
        )
        .unwrap();
    source.add_root(id).unwrap();
    let settings = SupportSettings {
        spacing_mm: 2.0,
        ..Default::default()
    };
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 12.0, 20.0).unwrap(),
        PixelResolution::new(400, 240).unwrap(),
        LayerSchedule::uniform(0.05, 0.05, 160).unwrap(),
        Mirror { x: false, y: false },
        AaLevel::X4,
        PackingMode::None,
    );
    let (_, structure, islands) =
        crate::supports::auto_supported_scene(&source, &settings, &profile).unwrap();
    assert!(islands.is_empty(), "{} islands", islands.len());
    assert!(
        structure.points.len() <= 24,
        "{} contacts for an 8mm boss at 2mm spacing",
        structure.points.len()
    );
}

/// Closed axis-aligned boxes merged into one mesh (one model).
fn boxes(parts: &[([f32; 3], [f32; 3])]) -> IndexedMesh {
    use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
    use sc_mesh_core::{IndexedTriangle, Vertex};
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
            [0, 2, 1],
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
            let ids = tri.map(|i: u32| base + i);
            let normal =
                compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize])).unwrap();
            faces.push(IndexedTriangle::from_raw_indices(normal, ids));
        }
    }
    IndexedMesh::new(None, vertices, faces)
}

fn r38_profile() -> sc_sl1cer::profile::SliceProfile {
    use sc_sl1cer::profile::*;
    SliceProfile::new(
        BuildVolume::new(100.0, 100.0, 40.0).unwrap(),
        PixelResolution::new(500, 500).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 400).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    )
}

fn one_model_scene(mesh: IndexedMesh) -> (sc_scene::Scene, sc_scene::NodeId) {
    use sc_mesh_core::transform::Transform3D;
    let mut scene = sc_scene::Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            sc_scene::LeafPayload::Mesh(std::sync::Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    (scene, id)
}

// Sleptic (R38): auto supports lifted flat-based parts into the air onto a giant
// raft. A part standing on a real base keeps it on the plate; only its
// overhang gets columns (from the plate), and there is no raft.
#[test]
fn r38_flat_based_part_stays_on_the_plate_and_only_its_overhang_is_supported() {
    // A stem on the plate carrying a wide slab that overhangs both sides.
    let mesh = boxes(&[
        ([-3.0, -3.0, 0.0], [3.0, 3.0, 12.0]),
        ([-15.0, -3.0, 10.0], [-3.0, 3.0, 12.0]),
        ([3.0, -3.0, 10.0], [15.0, 3.0, 12.0]),
    ]);
    let (scene, id) = one_model_scene(mesh);
    let (generated, structure, remaining) =
        crate::supports::auto_supported_scene(&scene, &SupportSettings::default(), &r38_profile())
            .unwrap();
    let model = sc_scene::bake_leaf_to_world(&generated, id).unwrap();
    assert!(
        model.local_aabb().unwrap().min()[2].abs() < 1e-4,
        "model was lifted"
    );
    assert!(structure.raft.is_none(), "an anchored part needs no raft");
    assert!(!structure.columns.is_empty());
    for p in &structure.points {
        assert!(
            p.position[0].abs() >= 3.0 - 1e-4,
            "contact under the stem: {p:?}"
        );
    }
    assert!(remaining.is_empty(), "{remaining:?}");
}

// Sleptic (R38): a part that prints as it stands gets nothing at all.
#[test]
fn r38_part_that_needs_no_supports_gets_none_and_is_not_lifted() {
    let (scene, id) = one_model_scene(boxes(&[([-5.0, -5.0, 0.0], [5.0, 5.0, 8.0])]));
    let (generated, structure, _) =
        crate::supports::auto_supported_scene(&scene, &SupportSettings::default(), &r38_profile())
            .unwrap();
    assert!(structure.columns.is_empty() && structure.raft.is_none());
    let model = sc_scene::bake_leaf_to_world(&generated, id).unwrap();
    assert!(model.local_aabb().unwrap().min()[2].abs() < 1e-4);
}

// Sleptic (R38): the raft ignored the work area and was the bounding rectangle of
// every support. A lifted part's raft follows its own column footprint and
// never leaves the plate, even for a part near the edge.
#[test]
fn r38_lifted_raft_follows_the_footprint_and_stays_on_the_plate() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    let (mut scene, id) = one_model_scene(sphere());
    let placed = Transform3D::Affine(AffineTransform3D::from_translation([47.5, 0.0, 1.0]))
        .compose(&Transform3D::Affine(AffineTransform3D::from_uniform_scale(
            1.0,
        )));
    scene.set_local_transform(id, placed).unwrap();
    let (_, structure, _) =
        crate::supports::auto_supported_scene(&scene, &SupportSettings::default(), &r38_profile())
            .unwrap();
    let raft = structure
        .raft
        .expect("a sphere has no base: lifted onto a raft");
    assert!(!raft.diagnose().has_problems());
    for v in &raft.vertices {
        assert!(
            v[0].abs() <= 50.0 + 1e-4 && v[1].abs() <= 50.0 + 1e-4,
            "raft off the plate: {v:?}"
        );
    }
}

// The rounded raft outline: a convex hull grown with round corners, with a
// vertex count independent of the radius (rings stack into one mesh).
#[test]
fn r38_rounded_offset_keeps_its_vertex_count_across_radii() {
    let hull = crate::supports::convex_hull(vec![
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 3.0],
        [0.0, 3.0],
        [2.0, 1.0],
        [4.0, 0.0],
    ]);
    assert_eq!(hull.len(), 4);
    let a = crate::supports::rounded_offset(&hull, 3.0);
    let b = crate::supports::rounded_offset(&hull, 1.5);
    assert_eq!(a.len(), b.len());
    for [x, y] in &a {
        assert!(*x >= -3.0 - 1e-9 && *x <= 7.0 + 1e-9 && *y >= -3.0 - 1e-9 && *y <= 6.0 + 1e-9);
    }
}

// Sweep: a flat-based part whose overhangs needed no columns read "Jaw: 0
// supports"; with nothing generated it is reported as printing directly.
#[test]
fn r41_model_without_generated_supports_is_reported_direct() {
    let (scene, _) = one_model_scene(boxes(&[([-5.0, -5.0, 0.0], [5.0, 5.0, 8.0])]));
    let (generated, structure, _) =
        crate::supports::auto_supported_scene(&scene, &SupportSettings::default(), &r38_profile())
            .unwrap();
    let results =
        crate::supports::model_results(&generated, &structure, &SupportSettings::default());
    assert_eq!(results.len(), 1);
    assert!(results[0].direct && results[0].contacts == 0);
}

/// Sleptic: six identical parts, arranged and auto-supported, came out with
/// different supports (one edge supported on one copy, not the other).
/// Copies of one mesh at odd sub-pixel offsets now get the same contacts,
/// each measured from its own position.
#[test]
fn identical_copies_get_identical_supports() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    let mut cube = unit_cube_mesh();
    for v in &mut cube.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] * 8.0, v[1] * 8.0, v[2] * 8.0]);
    }
    // At 45 degrees the faces sit exactly on the overhang angle; at 0.9 rad
    // one face overhangs for real (grid contacts, raster completion).
    for angle in [std::f64::consts::FRAC_PI_4, 0.9] {
        let mut scene = Scene::new();
        let rid = scene.add_mesh_resource_arc(std::sync::Arc::new(cube.clone()));
        let offsets = [
            [-31.37, -14.11],
            [-9.83, -14.11],
            [11.29, -14.11],
            [-31.37, 13.07],
            [-9.83, 13.07],
            [11.29, 13.07],
        ];
        let mut models = Vec::new();
        for [x, y] in offsets {
            let leaf = scene
                .add_leaf(
                    None,
                    Transform3D::Affine(AffineTransform3D::from_translation([x, y, 6.0])).compose(
                        &Transform3D::Affine(AffineTransform3D::from_rotation_y(angle)),
                    ),
                    LeafPayload::MeshResource(rid),
                )
                .unwrap();
            scene.add_root(leaf).unwrap();
            models.push(leaf);
        }
        let profile = SliceProfile::new(
            BuildVolume::new(100.0, 60.0, 30.0).unwrap(),
            PixelResolution::new(500, 300).unwrap(),
            LayerSchedule::uniform(0.1, 0.1, 300).unwrap(),
            Mirror::default(),
            AaLevel::Off,
            PackingMode::None,
        );
        let settings = SupportSettings::default();
        let (scene, structure, _) =
            crate::supports::auto_supported_scene(&scene, &settings, &profile).unwrap();
        let owners = crate::supports::support_owners(&scene, &structure.points);
        let relative = |model: sc_scene::NodeId, [x, y]: [f64; 2]| {
            let mut points: Vec<[i64; 3]> = structure
                .points
                .iter()
                .zip(&owners)
                .filter(|(_, o)| **o == Some(model))
                .map(|(p, _)| {
                    [
                        ((f64::from(p.position[0]) - x) * 100.0).round() as i64,
                        ((f64::from(p.position[1]) - y) * 100.0).round() as i64,
                        (f64::from(p.position[2]) * 100.0).round() as i64,
                    ]
                })
                .collect();
            points.sort();
            points
        };
        let first = relative(models[0], offsets[0]);
        assert!(!first.is_empty());
        for (model, offset) in models.iter().zip(offsets).skip(1) {
            assert_eq!(
                relative(*model, offset),
                first,
                "angle {angle}, copy at {offset:?}"
            );
        }
    }
}

/// Sleptic: a plate filled with copies of one part should not be worked out
/// part by part; the part is supported once and Fill plate's copies get the
/// result. A tagged copy the user has since turned is supported on its own.
#[test]
fn fill_plate_copies_get_their_parts_supports() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, Scene, SceneMetadata};
    use sc_sl1cer::profile::*;
    let mut cube = unit_cube_mesh();
    for v in &mut cube.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] * 8.0, v[1] * 8.0, v[2] * 8.0]);
    }
    let mesh = std::sync::Arc::new(cube);
    let mut scene = Scene::new();
    let pose = |x: f64, y: f64, angle: f64| {
        Transform3D::Affine(AffineTransform3D::from_translation([x, y, 6.0])).compose(
            &Transform3D::Affine(AffineTransform3D::from_rotation_y(angle)),
        )
    };
    let part = scene
        .add_leaf(
            None,
            pose(-30.0, -12.0, 0.9),
            LeafPayload::Mesh(mesh.clone()),
        )
        .unwrap();
    scene.add_root(part).unwrap();
    let mut copies = Vec::new();
    for (x, y, angle) in [
        (-8.0, -12.0, 0.9),
        (14.0, -12.0, 0.9),
        (-30.0, 13.0, 0.9),
        (-8.0, 13.0, 0.3),
    ] {
        let copy = scene
            .add_leaf(
                Some(SceneMetadata {
                    name: None,
                    comment: Some(format!("sc-fill-copy-of = {}\n", part.get())),
                }),
                pose(x, y, angle),
                LeafPayload::Mesh(mesh.clone()),
            )
            .unwrap();
        scene.add_root(copy).unwrap();
        copies.push((copy, [x + 30.0, y + 12.0], angle == 0.9));
    }
    let profile = SliceProfile::new(
        BuildVolume::new(100.0, 60.0, 30.0).unwrap(),
        PixelResolution::new(500, 300).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 300).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let settings = SupportSettings::default();
    let fills = crate::supports::fill_copies(&scene);
    assert_eq!(fills.len(), 3, "the turned copy is not a copy any more");
    let (scene, structure, _) =
        crate::supports::auto_supported_scene(&scene, &settings, &profile).unwrap();
    let owners = crate::supports::support_owners(&scene, &structure.points);
    let of = |model: sc_scene::NodeId, [dx, dy]: [f64; 2]| {
        let mut points: Vec<[i64; 3]> = structure
            .points
            .iter()
            .zip(&owners)
            .filter(|(_, o)| **o == Some(model))
            .map(|(p, _)| {
                [
                    ((f64::from(p.position[0]) - dx) * 1000.0).round() as i64,
                    ((f64::from(p.position[1]) - dy) * 1000.0).round() as i64,
                    (f64::from(p.position[2]) * 1000.0).round() as i64,
                ]
            })
            .collect();
        points.sort();
        points
    };
    let original = of(part, [0.0, 0.0]);
    assert!(!original.is_empty());
    // R84: each identical copy also gets the part's own columns, moved (a
    // tree has fewer columns than contacts, and copies took the next
    // part's columns).
    let lowest = |m: &IndexedMesh| {
        let x = m
            .vertices
            .iter()
            .map(|v| v[0])
            .fold(f32::INFINITY, f32::min);
        let y = m
            .vertices
            .iter()
            .map(|v| v[1])
            .fold(f32::INFINITY, f32::min);
        [f64::from(x), f64::from(y)]
    };
    let starts: Vec<[f64; 2]> = structure.columns.iter().map(lowest).collect();
    let near_part = |c: &[f64; 2]| (-42.0..-18.0).contains(&c[0]) && (-24.0..0.0).contains(&c[1]);
    let part_columns: Vec<[f64; 2]> = starts.iter().copied().filter(near_part).collect();
    assert!(!part_columns.is_empty());
    for (_, offset, same) in copies.iter().filter(|c| c.2) {
        let _ = same;
        for c in &part_columns {
            let moved = [c[0] + offset[0], c[1] + offset[1]];
            assert!(
                starts
                    .iter()
                    .any(|s| (s[0] - moved[0]).abs() < 1e-3 && (s[1] - moved[1]).abs() < 1e-3),
                "no copy of the column at {c:?} at offset {offset:?}"
            );
        }
    }
    for (copy, offset, same) in copies {
        let own = of(copy, offset);
        if same {
            assert_eq!(own, original, "copy at {offset:?}");
        } else {
            assert!(!own.is_empty(), "the turned copy is supported on its own");
            assert_ne!(own, original);
        }
    }
}

/// Sleptic: auto supports sometimes land in spots nobody could remove them
/// from. A ceiling inside a pocket (walls all round, open below) gets no
/// overhang-grid supports; the same ceiling in the open does.
#[test]
fn overhangs_inside_a_closed_pocket_get_no_grid_supports() {
    // Pocket: a ceiling 7 mm across inside, 1 mm walls hanging 6 mm below
    // it (narrower than the 8 mm tool reach). Open: a 12 mm ceiling alone.
    let mesh = boxes(&[
        ([-4.5, -4.5, 10.0], [4.5, 4.5, 11.0]),
        ([-4.5, -4.5, 4.0], [-3.5, 4.5, 10.0]),
        ([3.5, -4.5, 4.0], [4.5, 4.5, 10.0]),
        ([-3.5, -4.5, 4.0], [3.5, -3.5, 10.0]),
        ([-3.5, 3.5, 4.0], [3.5, 4.5, 10.0]),
        ([20.0, -6.0, 10.0], [32.0, 6.0, 11.0]),
    ]);
    let points =
        crate::supports::sample_overhang_contacts(&mesh, &SupportSettings::default()).unwrap();
    let under = |lo: f32, hi: f32| {
        points
            .iter()
            .filter(|p| {
                p.position[0] > lo && p.position[0] < hi && (p.position[2] - 10.0).abs() < 0.01
            })
            .count()
    };
    // Inside the pocket at most the ceiling's own lowest point remains.
    assert!(under(-3.4, 3.4) <= 1, "{}", under(-3.4, 3.4));
    assert!(under(20.5, 31.5) >= 4, "{}", under(20.5, 31.5));
}

/// Sleptic's first priority: the part must be held. A part standing on the
/// plate with an island lower than the shortest column it could have there
/// (a piece 1 mm up) was left unsupported; it is lifted onto supports.
#[test]
fn a_low_island_on_a_part_on_the_plate_is_still_supported() {
    use sc_sl1cer::profile::*;
    let mesh = boxes(&[
        ([-10.0, -10.0, 0.0], [10.0, 10.0, 4.0]),
        ([14.0, -2.0, 1.0], [18.0, 2.0, 4.0]),
    ]);
    let (scene, _) = one_model_scene(mesh);
    let profile = SliceProfile::new(
        BuildVolume::new(60.0, 40.0, 30.0).unwrap(),
        PixelResolution::new(600, 400).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 300).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let (_, structure, analysis) = crate::supports::auto_supported_scene_analyzed(
        &scene,
        &SupportSettings::default(),
        &profile,
    )
    .unwrap();
    assert!(analysis.islands.is_empty(), "{:?}", analysis.islands);
    assert!(!structure.points.is_empty());
}

#[test]
fn an_island_contact_just_off_the_part_moves_onto_it() {
    // A sliver island narrower than a pixel lights a pixel whose centre is
    // off the part: its contact had no owner, so no column was ever built.
    let (scene, id) = one_model_scene(boxes(&[([-5.0, -5.0, 2.0], [5.0, 5.0, 6.0])]));
    let off = [5.03, 1.0, 2.0];
    let point = |position| SupportPoint {
        position,
        normal: [0.0, 0.0, -1.0],
    };
    assert_eq!(
        crate::supports::support_owners(&scene, &[point(off)]),
        vec![None]
    );
    let on = crate::supports::snap_onto_model(&scene, off).expect("part within reach");
    assert!(
        on[0] < 5.0 && on[0] > 4.9 && (on[1] - 1.0).abs() < 0.2 && on[2] == 2.0,
        "{on:?}"
    );
    assert_eq!(
        crate::supports::support_owners(&scene, &[point(on)]),
        vec![Some(id)]
    );
    assert_eq!(
        crate::supports::snap_onto_model(&scene, [8.0, 1.0, 2.0]),
        None
    );
}

/// An L bracket extruded 10 mm along Y: a 3 mm wall (x 0..3) up to z 20 and
/// a shelf (z 18..20) reaching out to x 12, its far end a free rim.
fn l_bracket() -> IndexedMesh {
    use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
    use sc_mesh_core::{IndexedTriangle, Vertex};
    let outline: [[f32; 2]; 6] = [
        [0.0, 0.0],
        [3.0, 0.0],
        [3.0, 18.0],
        [12.0, 18.0],
        [12.0, 20.0],
        [0.0, 20.0],
    ];
    let vertices: Vec<Vertex> = [0.0f32, 10.0]
        .iter()
        .flat_map(|&y| outline.map(|[x, z]| Vertex::new([x, y, z])))
        .collect();
    let mut faces: Vec<[u32; 3]> = vec![[0, 1, 2], [0, 2, 5], [2, 3, 4], [2, 4, 5]];
    faces.extend([[6, 8, 7], [6, 11, 8], [8, 10, 9], [8, 11, 10]]);
    for i in 0..6u32 {
        let j = (i + 1) % 6;
        faces.extend([[i, j + 6, j], [i, i + 6, j + 6]]);
    }
    let faces = faces
        .into_iter()
        .map(|ids| {
            IndexedTriangle::from_raw_indices(
                compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize])).unwrap(),
                ids,
            )
        })
        .collect();
    IndexedMesh::new(None, vertices, faces)
}

#[test]
fn a_shelf_needs_no_contacts_right_beside_the_wall_holding_it() {
    let mesh = l_bracket();
    assert!(!mesh.diagnose().has_problems());
    let points =
        crate::supports::sample_overhang_contacts(&mesh, &SupportSettings::default()).unwrap();
    let shelf: Vec<[f32; 3]> = points
        .iter()
        .map(|p| p.position)
        .filter(|p| (p[2] - 18.0).abs() < 1e-3)
        .collect();
    // Hex grid from the bracket's centre (x 6): rows reach x 3.55, which
    // is 0.55 mm from the wall the shelf hangs from.
    assert!(shelf.iter().all(|p| p[0] > 3.0 + 1.4), "{shelf:?}");
    // The free rim at x 12 is held by nothing: the contact 1.1 mm from it stays.
    assert!(shelf.iter().any(|p| p[0] > 10.5), "{shelf:?}");
    assert!(shelf.iter().any(|p| (p[0] - 6.0).abs() < 1e-3), "{shelf:?}");
}

/// Two lattice cells meeting only at a corner share an edge with four
/// faces (non-manifold); the slicer refused most lattice rafts. One of the
/// empty cells beside each such touch is filled, preferring one inside.
#[test]
fn lattice_cells_never_touch_only_at_a_corner() {
    // 3 x 3: a pad cell at (0, 0) and a bar cell at (1, 1), and the other
    // diagonal at (2, 1) / (1, 2) once (1, 1)'s neighbour is decided.
    let mut filled = vec![false; 9];
    for (x, y) in [(0, 0), (1, 1), (2, 2)] {
        filled[y * 3 + x] = true;
    }
    let out = crate::supports::close_corner_touches(filled, 3, 3, |x, y| !(x == 1 && y == 0));
    let at = |x: usize, y: usize| out[y * 3 + x];
    for y in 0..2 {
        for x in 0..2 {
            let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1));
            assert!(
                !(a && d && !b && !c) && !(b && c && !a && !d),
                "corner touch at {x} {y}"
            );
        }
    }
    // (1, 0) is outside the outline, so (0, 1) was filled instead.
    assert!(at(0, 1) && !at(1, 0));
}

/// Sleptic's motor mount, after Auto prepare: exactly as deep as the plate,
/// with overhangs at its edges, its 4 mm column bases reached 1.3 mm past
/// the plate (red walls). A base is moved inward onto the plate.
#[test]
fn column_bases_stay_on_the_plate_beside_a_part_as_deep_as_it() {
    use sc_sl1cer::profile::*;
    // A block on the plate (y -20..20, the plate's full depth) and a ledge
    // beside it at the plate's edge, needing a column from the plate.
    let mesh = boxes(&[
        ([-10.0, -20.0, 0.0], [0.0, 20.0, 4.0]),
        ([2.0, 17.0, 6.0], [10.0, 20.0, 8.0]),
    ]);
    let (scene, _) = one_model_scene(mesh);
    let profile = SliceProfile::new(
        BuildVolume::new(60.0, 40.0, 30.0).unwrap(),
        PixelResolution::new(600, 400).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 300).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let (_, structure, analysis) =
        crate::supports::auto_supported_scene(&scene, &SupportSettings::default(), &profile)
            .unwrap();
    assert!(analysis.is_empty(), "{analysis:?}");
    assert!(!structure.columns.is_empty());
    for column in &structure.columns {
        assert!(!column.diagnose().has_problems());
        for v in &column.vertices {
            assert!(
                v[1].abs() <= 20.0 + 1e-3 && v[0].abs() <= 30.0 + 1e-3,
                "{v:?}"
            );
        }
    }
}

fn cuboid(lo: [f32; 3], hi: [f32; 3]) -> IndexedMesh {
    let mut mesh = unit_cube_mesh();
    for v in &mut mesh.vertices {
        *v = sc_mesh_core::Vertex::new(std::array::from_fn(
            |i| {
                if v[i] < 0.0 { lo[i] } else { hi[i] }
            },
        ));
    }
    mesh
}

fn down(position: [f32; 3]) -> SupportPoint {
    SupportPoint {
        position,
        normal: [0.0, 0.0, -1.0],
    }
}

/// R82/R83 (Sleptic: a Benchy's columns were half inside its bow and cabin
/// wall, and one fused to the wall 27 layers below its contact): a column
/// keeps a 0.25 mm gap from a wall; one that cannot drop straight down
/// beside it leans away. Below the tip no column comes within the gap.
#[test]
fn columns_beside_a_wall_keep_clear_of_it() {
    let wall = cuboid([0.0, -10.0, 0.0], [2.0, 10.0, 20.0]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let result = build_supports(
        &wall,
        &[
            down([2.25, 0.0, 15.0]),
            down([2.5, 0.0, 15.0]),
            down([6.0, 0.0, 15.0]),
        ],
        &settings,
    )
    .unwrap();
    assert_eq!(result.points.len(), 3);
    for column in &result.columns {
        let min_x = column
            .vertices
            .iter()
            .filter(|v| v[2] < 15.0 - settings.tip_length_mm as f32)
            .map(|v| v[0])
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_x >= 2.2,
            "column comes to x {min_x}, within the gap of the wall"
        );
    }
    // The two beside the wall lean away from it: their feet stand farther
    // out than their contacts. The far one drops straight.
    let feet: Vec<f32> = result
        .columns
        .iter()
        .map(|c| {
            let foot: Vec<_> = c.vertices.iter().filter(|v| v[2] < 0.01).collect();
            foot.iter().map(|v| v[0]).sum::<f32>() / foot.len() as f32
        })
        .collect();
    assert!(feet[0] > 2.25 + 0.3 && feet[1] > 2.5 + 0.2, "{feet:?}");
    assert!((feet[2] - 6.0).abs() < 0.01, "{feet:?}");
}

/// R83 (Sleptic: a column standing on the part leaves two contact marks where
/// one would do): an island over a pedestal gets a column that leans past
/// the pedestal to the plate, not one standing on it.
#[test]
fn an_island_over_the_part_leans_to_the_plate_instead_of_standing_on_it() {
    let mut mesh = cuboid([-2.0, -2.0, 0.0], [2.0, 2.0, 10.0]);
    let ledge = cuboid([-10.0, -10.0, 15.0], [10.0, 10.0, 17.0]);
    let offset = mesh.vertices.len() as u32;
    mesh.vertices.extend(ledge.vertices);
    mesh.faces.extend(ledge.faces.iter().map(|f| {
        sc_mesh_core::IndexedTriangle::from_raw_indices(
            f.normal,
            f.vertices.map(|i| i.get() + offset),
        )
    }));
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let result = crate::supports::build_supports_with_tolerance(
        &mesh,
        None,
        &[down([1.0, 0.0, 15.0])],
        &settings,
        0.05,
    )
    .unwrap();
    assert_eq!(result.columns.len(), 1);
    let column = &result.columns[0];
    let lowest = column
        .vertices
        .iter()
        .map(|v| v[2])
        .fold(f32::INFINITY, f32::min);
    assert!(lowest < 0.01, "stands at z {lowest}, not on the plate");
    for v in &column.vertices {
        let inside = v[0].abs() < 2.0 && v[1].abs() < 2.0 && v[2] < 10.0;
        assert!(!inside, "{v:?} is inside the pedestal");
    }
}

/// R82: two contacts either side of a wall are not joined into one tree
/// trunk whose branches would run along inside the wall's face, although
/// their centre lines clear it.
#[test]
fn tree_branches_do_not_run_through_a_wall() {
    // A wall on the plate under one ledge: the contacts are on the ledge.
    let mut wall = cuboid([-1.0, -10.0, 0.0], [1.0, 2.0, 10.0]);
    let ledge = cuboid([-5.0, 1.0, 15.0], [5.0, 10.0, 17.0]);
    let offset = wall.vertices.len() as u32;
    wall.vertices.extend(ledge.vertices);
    wall.faces.extend(ledge.faces.iter().map(|f| {
        sc_mesh_core::IndexedTriangle::from_raw_indices(
            f.normal,
            f.vertices.map(|i| i.get() + offset),
        )
    }));
    let settings = SupportSettings {
        raft: false,
        tree_supports: true,
        ..Default::default()
    };
    let points = [down([-3.0, 2.5, 15.0]), down([3.0, 2.5, 15.0])];
    let result = build_supports(&wall, &points, &settings).unwrap();
    assert_eq!(result.points.len(), 2);
    assert_eq!(
        result.columns.len(),
        2,
        "merged into a trunk through the wall"
    );
    let far = [down([-3.0, 6.0, 15.0]), down([3.0, 6.0, 15.0])];
    let result = build_supports(&wall, &far, &settings).unwrap();
    assert_eq!(
        result.columns.len(),
        1,
        "clear of the wall they still merge"
    );
}

/// R83: a column leaning down to the plate is at least 1/20 of its height
/// thick, so a tall one does not sway and snap under the peel.
#[test]
fn a_tall_leaning_column_to_the_plate_is_thick_enough() {
    let mut mesh = cuboid([-2.0, -2.0, 0.0], [2.0, 2.0, 10.0]);
    let ledge = cuboid([-10.0, -10.0, 50.0], [10.0, 10.0, 52.0]);
    let offset = mesh.vertices.len() as u32;
    mesh.vertices.extend(ledge.vertices);
    mesh.faces.extend(ledge.faces.iter().map(|f| {
        sc_mesh_core::IndexedTriangle::from_raw_indices(
            f.normal,
            f.vertices.map(|i| i.get() + offset),
        )
    }));
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let result = crate::supports::build_supports_with_tolerance(
        &mesh,
        None,
        &[down([1.0, 0.0, 50.0])],
        &settings,
        0.05,
    )
    .unwrap();
    let column = &result.columns[0];
    let lowest = column
        .vertices
        .iter()
        .map(|v| v[2])
        .fold(f32::INFINITY, f32::min);
    assert!(lowest < 0.01, "stands at z {lowest}, not on the plate");
    // The shaft's lower ring, at the top of the foot.
    let shaft: Vec<_> = column
        .vertices
        .iter()
        .filter(|v| v[2] > 0.3 && v[2] < 1.0)
        .collect();
    assert!(!shaft.is_empty());
    let width = shaft.iter().map(|v| v[0]).fold(f32::MIN, f32::max)
        - shaft.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
    assert!(width >= 2.5 - 0.05, "a 50 mm column {width} mm wide");
}

/// R84: tall columns are braced into one network, not loose pairs. Two
/// pairs 3 mm apart, 6 mm from each other: nearest-neighbour bracing tied
/// each pair only to itself; the network also ties the pairs together.
#[test]
fn tall_columns_are_braced_into_one_network() {
    let ceiling = cuboid([-20.0, -5.0, 30.0], [20.0, 5.0, 32.0]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let xs = [-6.0f32, -3.0, 3.0, 6.0];
    let points: Vec<_> = xs.iter().map(|&x| down([x, 0.0, 30.0])).collect();
    let result = build_supports(&ceiling, &points, &settings).unwrap();
    assert_eq!(result.columns.len(), 4);
    // A rod from -3 to 3 lives in one of those two columns' meshes.
    let reaches =
        |column: &IndexedMesh, x: f32| column.vertices.iter().any(|v| (v[0] - x).abs() < 0.5);
    let index = |x: f32| {
        result
            .points
            .iter()
            .position(|p| p.position[0] == x)
            .unwrap()
    };
    let (b, c) = (index(-3.0), index(3.0));
    assert!(
        reaches(&result.columns[b], 3.0) || reaches(&result.columns[c], -3.0),
        "the two pairs are not braced to each other"
    );
}

/// R84: a brace rod that would pass through or close to a part is left out.
#[test]
fn braces_do_not_cross_a_wall_between_columns() {
    let mesh = boxes(&[
        ([-20.0, -5.0, 30.0], [20.0, 5.0, 32.0]),
        ([-0.5, -5.0, 0.0], [0.5, 5.0, 25.0]),
    ]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let result = build_supports(
        &mesh,
        &[down([-3.0, 0.0, 30.0]), down([3.0, 0.0, 30.0])],
        &settings,
    )
    .unwrap();
    for column in &result.columns {
        for v in &column.vertices {
            assert!(
                !(v[0].abs() < 0.5 + 0.2 && v[2] < 25.0),
                "{v:?} is in or beside the wall"
            );
        }
    }
}

/// R84: where the branches leave a tree trunk there is a ball,
/// wider than the trunk, so the joint is not its thinnest point.
#[test]
fn tree_trunks_have_a_ball_where_the_branches_leave() {
    let ceiling = cuboid([-10.0, -10.0, 20.0], [10.0, 10.0, 22.0]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: true,
        ..Default::default()
    };
    let points = [down([-1.5, 0.0, 20.0]), down([1.5, 0.0, 20.0])];
    let result = build_supports(&ceiling, &points, &settings).unwrap();
    assert_eq!(result.columns.len(), 1, "the two contacts share a trunk");
    // The joint is the widest thing above the foot: wider than the trunk.
    let trunk = &result.columns[0];
    let sides = crate::supports::ROUND_SIDES as usize;
    let top = trunk.vertices[2 * sides..3 * sides]
        .iter()
        .map(|v| v[2])
        .fold(f32::MIN, f32::max);
    let shaft = trunk.vertices[2 * sides].coords()[0].hypot(trunk.vertices[2 * sides].coords()[1]);
    let widest = trunk
        .vertices
        .iter()
        .filter(|v| (v[2] - top).abs() < 0.3)
        .map(|v| v[0].hypot(v[1]))
        .fold(0.0f32, f32::max);
    assert!(
        widest > shaft * 1.1,
        "widest {widest} at the junction, trunk {shaft}"
    );
}

/// R84: the raft lies under the columns' feet. A column leaning past a
/// pedestal stands away from its contact; its foot is on the raft.
#[test]
fn the_raft_lies_under_a_leaning_columns_foot() {
    let mesh = boxes(&[
        ([-4.0, -4.0, 1.0], [4.0, 4.0, 10.0]),
        ([-10.0, -10.0, 15.0], [10.0, 10.0, 17.0]),
    ]);
    // A tight margin: a raft laid round the contact would miss the foot.
    let settings = SupportSettings {
        tree_supports: false,
        raft_margin_mm: 0.5,
        ..Default::default()
    };
    let result = crate::supports::build_supports_with_tolerance(
        &mesh,
        None,
        &[down([3.0, 0.0, 15.0])],
        &settings,
        0.05,
    )
    .unwrap();
    let column = &result.columns[0];
    let low = column
        .vertices
        .iter()
        .map(|v| v[2])
        .fold(f32::INFINITY, f32::min);
    let foot: Vec<_> = column
        .vertices
        .iter()
        .filter(|v| v[2] <= low + 1e-4)
        .collect();
    let fx = foot.iter().map(|v| v[0]).sum::<f32>() / foot.len() as f32;
    let fy = foot.iter().map(|v| v[1]).sum::<f32>() / foot.len() as f32;
    let raft = result.raft.as_ref().expect("a raft");
    let (lo_x, hi_x) = raft
        .vertices
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v[0]), b.max(v[0])));
    let (lo_y, hi_y) = raft
        .vertices
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v[1]), b.max(v[1])));
    assert!(
        fx > 4.5,
        "the column leans past the pedestal (foot at {fx})"
    );
    let margin = (settings.base_diameter_mm / 2.0) as f32;
    assert!(
        fx - margin >= lo_x && fx + margin <= hi_x && fy - margin >= lo_y && fy + margin <= hi_y,
        "foot at ({fx}, {fy}) is not inside the raft ({lo_x}..{hi_x}, {lo_y}..{hi_y})"
    );
}

/// R84: a skate raft is a pad under each foot, close pads
/// joined by a strip and far ones left apart; it is one closed slab and
/// uses less resin than the lattice over the same columns.
#[test]
fn a_skate_raft_is_pads_joined_to_close_neighbours() {
    let ceiling = cuboid([-30.0, -10.0, 20.0], [30.0, 10.0, 22.0]);
    let skate = SupportSettings {
        tree_supports: false,
        skate_raft: true,
        ..Default::default()
    };
    let points = [
        down([-20.0, 0.0, 20.0]),
        down([-14.0, 0.0, 20.0]),
        down([20.0, 0.0, 20.0]),
    ];
    let result = build_supports(&ceiling, &points, &skate).unwrap();
    let raft = result.raft.expect("a raft");
    raft.validated_faces()
        .unwrap()
        .validate_edge_manifold()
        .unwrap();
    let covered = |x: f32| {
        raft.vertices
            .iter()
            .any(|v| (v[0] - x).abs() < 1.5 && v[1].abs() < 3.0)
    };
    assert!(
        covered(-20.0) && covered(-14.0) && covered(20.0),
        "a pad under each foot"
    );
    assert!(covered(-17.0), "the close pads are joined");
    assert!(!covered(0.0), "the far pad stands alone");
    let lattice = build_supports(
        &ceiling,
        &points,
        &SupportSettings {
            skate_raft: false,
            ..skate
        },
    )
    .unwrap()
    .raft
    .unwrap();
    let area = |m: &IndexedMesh| -> f64 {
        m.faces
            .iter()
            .filter(|f| f.normal.coords()[2] > 0.9)
            .map(|f| {
                let [a, b, c] = f
                    .vertices
                    .map(|i| m.vertices[i.get() as usize].coords().map(f64::from));
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum()
    };
    assert!(
        area(&raft) < area(&lattice),
        "skate {} vs lattice {}",
        area(&raft),
        area(&lattice)
    );
}

/// R85 asked for a side approach here (one contact, a near-flat rod over the
/// deck's edge). R91: a rod leaning past 45 degrees needs supporting itself,
/// so a contact over a wide deck is held by a support standing on the deck,
/// as Lychee and Chitubox do, and nothing leans past 45 degrees.
#[test]
fn an_island_over_a_wide_deck_is_held_without_an_overhanging_rod() {
    let mesh = boxes(&[
        ([-15.0, -15.0, 0.0], [15.0, 15.0, 10.0]),
        ([-3.0, -3.0, 14.0], [3.0, 3.0, 16.0]),
    ]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let result = crate::supports::build_supports_with_tolerance(
        &mesh,
        None,
        &[down([0.0, 0.0, 14.0])],
        &settings,
        0.05,
    )
    .unwrap();
    assert_eq!(result.columns.len(), 1, "the contact is held");
    for path in &result.paths {
        assert!(path.inclination_deg <= 45.0 + 1e-6, "{path:?}");
    }
    // Nothing sinks into the deck below its contact on it.
    for v in result.columns[0].vertices.iter().filter(|v| v[2] < 9.5) {
        assert!(
            !(v[0].abs() < 15.0 && v[1].abs() < 15.0),
            "{v:?} is inside the deck"
        );
    }
}

/// R86: a tip leans along the surface it holds (here 30 degrees), and the
/// pillar under its joint stands straight on a wide foot.
#[test]
fn a_tip_follows_its_surface_and_the_pillar_stands_straight() {
    let ceiling = cuboid([-10.0, -10.0, 20.0], [10.0, 10.0, 22.0]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let point = SupportPoint {
        position: [0.0, 0.0, 20.0],
        normal: [0.5, 0.0, -0.866_025_4],
    };
    let result = build_supports(&ceiling, &[point], &settings).unwrap();
    let sides = crate::supports::ROUND_SIDES as usize;
    let pillar = &result.columns[0].vertices[..3 * sides];
    let centre = |ring: &[sc_mesh_core::Vertex]| {
        [0, 1].map(|axis| ring.iter().map(|v| v[axis]).sum::<f32>() / ring.len() as f32)
    };
    // The joint is one tip length (2 mm) from the contact along the normal.
    for ring in pillar.chunks(sides) {
        let [x, y] = centre(ring);
        assert!(
            (x - 1.0).abs() < 1e-3 && y.abs() < 1e-3,
            "pillar axis at ({x}, {y})"
        );
    }
    assert!(
        pillar[..sides].iter().all(|v| v[2] == 0.0),
        "the foot stands on the plate"
    );
    let radius = |ring: &[sc_mesh_core::Vertex]| (ring[0][0] - 1.0).hypot(ring[0][1]);
    assert!(
        radius(&pillar[..sides]) > radius(&pillar[sides..2 * sides]) * 1.5,
        "a wide foot"
    );
    let top = pillar[2 * sides..]
        .iter()
        .map(|v| v[2])
        .fold(f32::MIN, f32::max);
    assert!(
        (top - (20.0 - 2.0 * 0.866_025_4)).abs() < 1e-3,
        "pillar top at {top}"
    );
}

/// R86: the branches of a tree leave its trunk near the part, within the
/// branch angle; the trunk carries the load straight down from there. They
/// used to fan out from the plate, each as long as the tree was tall.
#[test]
fn tree_branches_leave_the_trunk_near_the_part() {
    let ceiling = cuboid([-10.0, -10.0, 30.0], [10.0, 10.0, 32.0]);
    let settings = SupportSettings {
        raft: false,
        ..Default::default()
    };
    let points = [down([-1.5, 0.0, 30.0]), down([1.5, 0.0, 30.0])];
    let result = build_supports(&ceiling, &points, &settings).unwrap();
    assert_eq!(result.columns.len(), 1, "the two contacts share a trunk");
    assert_eq!(result.points.len(), 2);
    assert_eq!(result.paths.len(), 2);
    for path in &result.paths {
        assert!(
            path.inclination_deg <= settings.branch_angle_deg + 1e-6,
            "{path:?}"
        );
        assert!(path.rise_mm < 3.0, "a branch {path:?} on a 30 mm tree");
    }
    let sides = crate::supports::ROUND_SIDES as usize;
    let trunk = &result.columns[0].vertices[..3 * sides];
    let top = trunk[2 * sides..]
        .iter()
        .map(|v| v[2])
        .fold(f32::MIN, f32::max);
    assert!(top > 24.0, "the trunk ends at {top}");
    assert!(
        trunk.iter().all(|v| v[0].hypot(v[1]) < 2.5),
        "the trunk stands between them"
    );
}

/// R86: one trunk carries at most eight contacts, so a single broken trunk
/// cannot drop a whole part.
#[test]
fn a_trunk_carries_at_most_eight_contacts() {
    let ceiling = cuboid([-20.0, -20.0, 40.0], [20.0, 20.0, 42.0]);
    let settings = SupportSettings {
        raft: false,
        ..Default::default()
    };
    let points: Vec<_> = (0..4)
        .flat_map(|i| (0..3).map(move |j| down([i as f32 * 2.0 - 3.0, j as f32 * 2.0 - 2.0, 40.0])))
        .collect();
    let result = build_supports(&ceiling, &points, &settings).unwrap();
    assert_eq!(result.points.len(), 12);
    assert!(
        result.columns.len() >= 2,
        "{} trunks for 12 contacts",
        result.columns.len()
    );
    assert!(result.columns.len() <= 4);
    for column in &result.columns {
        column.validated_faces().unwrap();
    }
}

/// R87: a contact with the part below it gets a slim angled arm to a clear
/// spot, then a straight pillar to the plate (Lychee's "search for ground").
/// R86 dropped such contacts unless the old leaning column fitted, so the
/// bar spanning a frame's window went unsupported.
#[test]
fn an_arm_carries_a_blocked_contact_to_the_plate() {
    let mesh = boxes(&[
        ([-3.0, -3.0, 0.0], [3.0, 3.0, 10.0]),
        ([-10.0, -10.0, 20.0], [10.0, 10.0, 22.0]),
    ]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let result = build_supports(&mesh, &[down([0.0, 0.0, 20.0])], &settings).unwrap();
    assert_eq!(result.columns.len(), 1, "the contact is held");
    let column = &result.columns[0];
    let foot: Vec<_> = column.vertices.iter().filter(|v| v[2] <= 1e-4).collect();
    assert!(!foot.is_empty(), "it stands on the plate");
    let n = foot.len() as f32;
    let (fx, fy) = (
        foot.iter().map(|v| v[0]).sum::<f32>() / n,
        foot.iter().map(|v| v[1]).sum::<f32>() / n,
    );
    assert!(
        fx.abs() > 3.0 || fy.abs() > 3.0,
        "the pillar stands beside the block: ({fx}, {fy})"
    );
    for v in &column.vertices {
        assert!(v[2] >= 0.0, "{v:?} is under the plate");
        assert!(
            !(v[0].abs() < 3.0 && v[1].abs() < 3.0 && v[2] < 10.0),
            "{v:?} is inside the block"
        );
    }
    // The arm is slimmer than a pillar: round the joint under the tip
    // nothing is wider than the branch diameter.
    let branch = (settings.shaft_diameter_mm * 0.7) as f32 / 2.0;
    let joint_z = 20.0 - settings.tip_length_mm as f32;
    let widest = column
        .vertices
        .iter()
        .filter(|v| (v[2] - joint_z).abs() < 0.6 && v[0].hypot(v[1]) < 2.0)
        .map(|v| v[0].hypot(v[1]))
        .fold(0.0f32, f32::max);
    assert!(
        widest > 0.0 && widest <= branch + 0.05,
        "arm radius {widest}, branch {branch}"
    );
}

/// R87: a lone tall pillar with no neighbour to brace to gets a companion
/// pillar beside it, braced to it, which touches no part; the raft is laid
/// under both feet. R86 left a 60 mm pole standing alone.
#[test]
fn a_lone_tall_pillar_gets_a_braced_companion() {
    let ceiling = cuboid([-3.0, -3.0, 40.0], [3.0, 3.0, 42.0]);
    let settings = SupportSettings {
        tree_supports: false,
        ..Default::default()
    };
    let result = build_supports(&ceiling, &[down([0.0, 0.0, 40.0])], &settings).unwrap();
    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.points.len(), 1, "the companion adds no contact");
    let floor = settings.raft_thickness_mm;
    let feet = crate::supports::raft_feet(&result.columns, floor);
    assert_eq!(feet.len(), 2, "pillar and companion each stand on a foot");
    let raft = result.raft.as_ref().expect("a raft");
    for foot in &feet {
        let [x, y, _] = foot.position;
        let inside = raft
            .vertices
            .iter()
            .any(|v| (v[0] - x).abs() < 0.5 && (v[1] - y).abs() < 6.0)
            || raft.vertices.iter().map(|v| v[0]).fold(f32::MIN, f32::max) > x + 2.0;
        assert!(inside, "the raft reaches the foot at ({x}, {y})");
    }
    for v in result.columns.iter().flat_map(|c| &c.vertices) {
        assert!(v[2] >= 0.0, "{v:?} is under the plate");
        assert!(v[2] <= 40.0 + 0.2, "{v:?} is above the contact");
    }
}

/// R87: a tree's branches are slimmer than its trunk (only the trunk carries
/// the sum), so the crown near the part is light. R86 drew every branch as
/// thick as its pillar would have been.
#[test]
fn tree_branches_are_slimmer_than_the_trunk() {
    let ceiling = cuboid([-10.0, -10.0, 20.0], [10.0, 10.0, 22.0]);
    let settings = SupportSettings {
        raft: false,
        ..Default::default()
    };
    let points = [down([-1.5, 0.0, 20.0]), down([1.5, 0.0, 20.0])];
    let result = build_supports(&ceiling, &points, &settings).unwrap();
    assert_eq!(result.columns.len(), 1);
    let trunk = &result.columns[0];
    // The pillar's top ring is the trunk.
    let sides = crate::supports::ROUND_SIDES as usize;
    let trunk_radius =
        trunk.vertices[2 * sides].coords()[0].hypot(trunk.vertices[2 * sides].coords()[1]);
    // Round each leaf joint (one tip length under its contact) everything
    // belongs to the tip, its ball and the branch.
    let joint_z = 20.0 - settings.tip_length_mm as f32;
    let branch_radius = trunk
        .vertices
        .iter()
        .filter(|v| (v[2] - joint_z).abs() < 0.6 && (v[0] + 1.5).abs() < 1.2)
        .map(|v| (v[0] + 1.5).hypot(v[1]))
        .fold(0.0f32, f32::max);
    assert!(branch_radius > 0.0, "no branch at the joint");
    assert!(
        branch_radius < trunk_radius * 0.9,
        "branch {branch_radius}, trunk {trunk_radius}"
    );
}

/// R87: nothing a support builds goes below the build plate, with or
/// without a raft, for plain columns, trees, arms and companions. An older
/// leaning column's foot dipped 0.08 mm under the plate, which the app
/// marked as outside the printer.
#[test]
fn no_support_goes_below_the_plate() {
    let fixtures = [
        slab(),
        boxes(&[
            ([-3.0, -3.0, 0.0], [3.0, 3.0, 10.0]),
            ([-10.0, -10.0, 20.0], [10.0, 10.0, 22.0]),
        ]),
        cuboid([-3.0, -3.0, 40.0], [3.0, 3.0, 42.0]),
        sphere(),
    ];
    for mesh in &fixtures {
        for (raft, tree) in [(true, true), (true, false), (false, true), (false, false)] {
            let settings = SupportSettings {
                raft,
                tree_supports: tree,
                ..Default::default()
            };
            let Ok(points) = sample_contacts(mesh, &settings) else {
                continue;
            };
            let structure = build_supports(mesh, &points, &settings).unwrap();
            for v in structure
                .columns
                .iter()
                .chain(structure.raft.iter())
                .flat_map(|m| &m.vertices)
            {
                assert!(
                    v[2] >= 0.0,
                    "{v:?} under the plate (raft {raft}, trees {tree})"
                );
            }
        }
    }
}

/// R87: the raft is 1 mm thick by default and its edge slopes at 45 degrees
/// from a short lip on the plate, so a scraper slides under it.
#[test]
fn the_raft_edge_is_bevelled_for_a_scraper() {
    let settings = SupportSettings::default();
    assert!((settings.raft_thickness_mm - 1.0).abs() < 1e-9);
    let ceiling = cuboid([-5.0, -5.0, 10.0], [5.0, 5.0, 12.0]);
    let result = build_supports(&ceiling, &[down([0.0, 0.0, 10.0])], &settings).unwrap();
    let raft = result.raft.expect("a raft");
    let reach = |z: f32| {
        raft.vertices
            .iter()
            .filter(|v| (v[2] - z).abs() < 1e-4)
            .map(|v| v[0])
            .fold(f32::MIN, f32::max)
    };
    let (bottom, top) = (reach(0.0), reach(1.0));
    assert!(
        raft.vertices
            .iter()
            .all(|v| v[2] >= 0.0 && v[2] <= 1.0 + 1e-4)
    );
    assert!(
        (bottom - top - 0.8).abs() < 0.05,
        "the top is 0.8 mm in from the bottom edge: {bottom} against {top}"
    );
}

/// R88: round support parts have enough sides to look round in the viewer
/// (10 and 12 sides read as faceted, low-resolution columns).
#[test]
fn support_parts_are_smooth_enough_to_look_round() {
    assert!(crate::supports::ROUND_SIDES >= 24);
    let ceiling = cuboid([-5.0, -5.0, 10.0], [5.0, 5.0, 12.0]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let result = build_supports(&ceiling, &[down([0.0, 0.0, 10.0])], &settings).unwrap();
    let foot = result.columns[0]
        .vertices
        .iter()
        .filter(|v| v[2] == 0.0)
        .count();
    assert_eq!(foot, crate::supports::ROUND_SIDES as usize);
}

/// R89: when the tip along the normal is clear
/// but every arm from its joint is blocked, the straight-down tip is tried
/// too. Only the first clear tip was kept, and the contact was dropped.
#[test]
fn arm_search_tries_the_clear_down_tip_after_the_along_tip() {
    let mesh = boxes(&[
        // The contact sits on this box's lower right edge; its normal
        // bisects the two faces' normals (+X and -Z).
        ([-3.0, -3.0, 20.0], [0.0, 3.0, 22.0]),
        // A shelf under the along-normal joint, beside the down tip's route.
        ([0.8, -40.0, 17.7], [40.0, 40.0, 17.85]),
        // A post that keeps either tip from dropping straight down.
        ([-0.3, -0.3, 0.0], [0.3, 0.3, 10.0]),
    ]);
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let point = SupportPoint {
        position: [0.0, 0.0, 20.0],
        normal: [
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            -std::f32::consts::FRAC_1_SQRT_2,
        ],
    };
    let result = build_supports(&mesh, &[point], &settings).unwrap();
    assert_eq!(result.points.len(), 1, "the second clear tip must be tried");
    assert_eq!(result.columns.len(), 1);
    assert!(
        result.columns[0].vertices.iter().any(|v| v[2].abs() < 1e-4),
        "the pillar reaches the plate"
    );
}

/// R89: each foot of a column is found where it stands. Grouping foot
/// vertices by their distance to a foot's first vertex let the companion's
/// near edge join the pillar's foot, moving both centres by about 1.5 mm.
#[test]
fn raft_feet_are_found_at_their_own_centres() {
    let ceiling = cuboid([-3.0, -3.0, 40.0], [3.0, 3.0, 42.0]);
    let settings = SupportSettings {
        tree_supports: false,
        ..Default::default()
    };
    let result = build_supports(&ceiling, &[down([0.0, 0.0, 40.0])], &settings).unwrap();
    let feet = crate::supports::raft_feet(&result.columns, settings.raft_thickness_mm);
    assert_eq!(feet.len(), 2);
    let mut distances: Vec<f32> = feet
        .iter()
        .map(|f| f.position[0].hypot(f.position[1]))
        .collect();
    distances.sort_by(f32::total_cmp);
    assert!(
        distances[0] < 0.01,
        "the pillar's foot is under it: {distances:?}"
    );
    assert!(
        (distances[1] - 7.0).abs() < 0.01,
        "the companion's foot is 7 mm away: {distances:?}"
    );
}

/// R89: with no raft margin the bevelled edge still leaves the raft's top
/// under every foot (it used to cut 0.8 mm in under a 4 mm foot).
#[test]
fn the_raft_top_covers_the_feet_without_a_margin() {
    let settings = SupportSettings {
        raft_margin_mm: 0.0,
        ..Default::default()
    };
    let ceiling = cuboid([-5.0, -5.0, 10.0], [5.0, 5.0, 12.0]);
    let result = build_supports(&ceiling, &[down([0.0, 0.0, 10.0])], &settings).unwrap();
    let raft = result.raft.expect("a raft");
    let top = settings.raft_thickness_mm as f32;
    let reach = raft
        .vertices
        .iter()
        .filter(|v| (v[2] - top).abs() < 1e-4)
        .map(|v| v[0].hypot(v[1]))
        // The top's centre cap vertex is not its edge.
        .filter(|&r| r > 0.1)
        .fold(f32::MAX, f32::min);
    assert!(
        // Up to the polygon's chord sag; the bevel used to cut in to 1.2 mm.
        reach >= (settings.base_diameter_mm / 2.0) as f32 - 0.02,
        "the raft top reaches {reach} mm, the foot {} mm",
        settings.base_diameter_mm / 2.0
    );
}

/// R89: the ball at the top of the pillar under an angled arm is as wide as
/// the pillar, so it keeps the pillar's gap from the part. A small feature
/// just above the knee, clear of the thin arm and of the pillar below the
/// knee, was fused into that ball.
#[test]
fn the_knee_ball_keeps_clear_of_the_part() {
    let settings = SupportSettings {
        raft: false,
        tree_supports: false,
        ..Default::default()
    };
    let block = ([-3.0f32, -3.0, 0.0], [3.0f32, 3.0, 10.0]);
    let roof = ([-10.0f32, -10.0, 20.0], [10.0f32, 10.0, 22.0]);
    let contact = down([0.0, 0.0, 20.0]);
    // Where the knee lands with nothing else around: the top of its pillar.
    let first = build_supports(&boxes(&[block, roof]), &[contact], &settings).unwrap();
    let sides = crate::supports::ROUND_SIDES as usize;
    let top_ring = &first.columns[0].vertices[2 * sides..3 * sides];
    let n = sides as f32;
    let knee = [
        top_ring.iter().map(|v| v[0]).sum::<f32>() / n,
        top_ring.iter().map(|v| v[1]).sum::<f32>() / n,
        top_ring[0][2],
    ];
    let radius = (top_ring[0][0] - knee[0]).hypot(top_ring[0][1] - knee[1]);
    // A small nub beside the knee, square to the arm on its outer upper
    // side: 0.75 mm from the knee, inside the 0.78 mm ball but clear of the
    // arm's 0.67 mm and the pillar top's 0.70 mm checks.
    let joint = [0.0f32, 0.0, 20.0 - settings.tip_length_mm as f32];
    let out = knee[0].hypot(knee[1]);
    let (ux, uy) = (knee[0] / out, knee[1] / out);
    // The arm, in the plane of the knee's radius and z: from knee to joint.
    let (ar, az) = (joint[0] * ux + joint[1] * uy - out, joint[2] - knee[2]);
    let al = ar.hypot(az);
    // Square to it, pointing outward and up.
    let (pr, pz) = (az / al, -ar / al);
    let (pr, pz) = if pr < 0.0 { (-pr, -pz) } else { (pr, pz) };
    let half = 0.04;
    let d = radius * 0.96 + half;
    let c = [
        knee[0] + ux * pr * d,
        knee[1] + uy * pr * d,
        knee[2] + pz * d,
    ];
    let nub = (
        [c[0] - half, c[1] - half, c[2] - half],
        [c[0] + half, c[1] + half, c[2] + half],
    );
    let mesh = boxes(&[block, roof, nub]);
    let result = build_supports(&mesh, &[contact], &settings).unwrap();
    assert_eq!(result.points.len(), 1, "the contact is still held");
    // The ball on the knee now chosen keeps the pillar's gap from the nub.
    let top_ring = &result.columns[0].vertices[2 * sides..3 * sides];
    let chosen = [
        top_ring.iter().map(|v| v[0]).sum::<f32>() / n,
        top_ring.iter().map(|v| v[1]).sum::<f32>() / n,
        top_ring[0][2],
    ];
    let ball = (top_ring[0][0] - chosen[0]).hypot(top_ring[0][1] - chosen[1]);
    let gap = (0..3)
        .map(|k| {
            let d = (chosen[k] - c[k]).abs() - half;
            d.max(0.0).powi(2)
        })
        .sum::<f32>()
        .sqrt();
    assert!(
        gap > ball,
        "the knee ball (radius {ball}) is {gap} mm from the nub at {c:?}"
    );
}

/// R89: wide feet stay one foot each, a raft with no margin
/// is still a closed mesh, and a companion too far away to be braced to a
/// short shaft is left out.
#[test]
fn wide_feet_and_zero_margins_keep_rafts_and_companions_sound() {
    let ceiling = cuboid([-3.0, -3.0, 40.0], [3.0, 3.0, 42.0]);
    // A 10 mm foot: its ring's vertices are 1.3 mm apart.
    let wide = SupportSettings {
        tree_supports: false,
        base_diameter_mm: 10.0,
        ..Default::default()
    };
    let result = build_supports(&ceiling, &[down([0.0, 0.0, 40.0])], &wide).unwrap();
    let feet = crate::supports::raft_feet(&result.columns, wide.raft_thickness_mm);
    assert!(
        feet.len() <= 2,
        "{} feet for a pillar and its companion",
        feet.len()
    );
    // No raft margin: a straight edge, still closed.
    let tight = SupportSettings {
        raft_margin_mm: 0.0,
        ..Default::default()
    };
    let small = cuboid([-5.0, -5.0, 10.0], [5.0, 5.0, 12.0]);
    let result = build_supports(&small, &[down([0.0, 0.0, 10.0])], &tight).unwrap();
    let raft = result.raft.expect("a raft");
    assert!(!raft.diagnose().has_problems(), "{:?}", raft.diagnose());
    // A 14 mm foot pushes the companion 16 mm out: too far to brace to a
    // 22 mm shaft, so there is none.
    let short = cuboid([-3.0, -3.0, 23.0], [3.0, 3.0, 25.0]);
    let huge = SupportSettings {
        tree_supports: false,
        base_diameter_mm: 14.0,
        ..Default::default()
    };
    let result = build_supports(&short, &[down([0.0, 0.0, 23.0])], &huge).unwrap();
    let feet = crate::supports::raft_feet(&result.columns, huge.raft_thickness_mm);
    assert_eq!(feet.len(), 1, "an unbraced companion was added");
}

/// Models that exercise every kind of support member: plain pillars, trees,
/// angled arms past a block, a tall lone pillar with its companion, a
/// contact over a wide deck, a sphere and a slab.
fn member_fixtures() -> Vec<(IndexedMesh, Vec<SupportPoint>)> {
    let arm = boxes(&[
        ([-3.0, -3.0, 0.0], [3.0, 3.0, 10.0]),
        ([-10.0, -10.0, 20.0], [10.0, 10.0, 22.0]),
    ]);
    let deck = boxes(&[
        ([-15.0, -15.0, 0.0], [15.0, 15.0, 10.0]),
        ([-3.0, -3.0, 14.0], [3.0, 3.0, 16.0]),
    ]);
    let ceiling = cuboid([-10.0, -10.0, 20.0], [10.0, 10.0, 22.0]);
    let tall = cuboid([-3.0, -3.0, 40.0], [3.0, 3.0, 42.0]);
    // Low over a block: no room for an arm at 45 degrees to clear it; R87's
    // fallback leaned it to 65.
    let low = boxes(&[
        ([-3.0, -3.0, 0.0], [3.0, 3.0, 3.0]),
        ([-8.0, -8.0, 6.0], [8.0, 8.0, 8.0]),
    ]);
    let mut out = vec![
        (low, vec![down([0.0, 0.0, 6.0])]),
        (arm, vec![down([0.0, 0.0, 20.0]), down([1.0, 1.5, 20.0])]),
        (deck, vec![down([0.0, 0.0, 14.0])]),
        (
            ceiling,
            vec![
                down([-1.5, 0.0, 20.0]),
                down([1.5, 0.0, 20.0]),
                down([0.0, 3.0, 20.0]),
                down([5.0, -4.0, 20.0]),
            ],
        ),
        (tall, vec![down([0.0, 0.0, 40.0])]),
    ];
    for mesh in [sphere(), slab()] {
        let points = sample_contacts(&mesh, &SupportSettings::default()).unwrap_or_default();
        out.push((mesh, points));
    }
    out
}

/// The axis inclination (degrees from vertical) of every straight two-ring
/// part of a support mesh (tips, branches, arms, brace rods), measured on the
/// geometry itself, so a member nothing records is still checked.
fn bar_inclinations(mesh: &IndexedMesh) -> Vec<f64> {
    let n = mesh.vertices.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for face in &mesh.faces {
        let ids = face.vertices.map(usize::from);
        for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2])] {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let mut parts: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = root(&mut parent, i);
        parts.entry(r).or_default().push(i);
    }
    let mut out = Vec::new();
    for ids in parts.values() {
        // Two rings of equal size: a straight bar (pillars have three or
        // four rings, balls many plus two caps).
        if ids.len() < 16 || ids.len() % 2 != 0 {
            continue;
        }
        let half = ids.len() / 2;
        let centre = |range: &[usize]| {
            let mut c = glam::DVec3::ZERO;
            for &i in range {
                c += glam::DVec3::from_array(mesh.vertices[i].coords().map(f64::from));
            }
            c / range.len() as f64
        };
        let d = centre(&ids[half..]) - centre(&ids[..half]);
        if d.length() < 0.3 {
            continue;
        }
        out.push(d.truncate().length().atan2(d.z.abs()).to_degrees());
    }
    out
}

/// R91: no support member leans more than 45 degrees from vertical, so no
/// support has an underside that would itself need supporting: tree
/// branches, the angled arms that carry a contact past the part (they went
/// to 55 and 65 degrees) and the old leaning columns (their side approach
/// went to 80), even with the branch angle set past 45.
#[test]
fn no_support_member_leans_past_45_degrees() {
    for angle in [40.0, 45.0, 70.0] {
        for tree in [true, false] {
            let settings = SupportSettings {
                raft: false,
                tree_supports: tree,
                branch_angle_deg: angle,
                ..Default::default()
            };
            for (index, (mesh, points)) in member_fixtures().into_iter().enumerate() {
                let result = crate::supports::build_supports_with_tolerance(
                    &mesh, None, &points, &settings, 0.05,
                )
                .unwrap();
                for column in &result.columns {
                    for lean in bar_inclinations(column) {
                        assert!(
                            lean <= 45.0 + 0.5,
                            "fixture {index}: a bar leans {lean:.1} degrees (branch angle {angle}, trees {tree})"
                        );
                    }
                }
                for path in &result.paths {
                    assert!(path.inclination_deg <= 45.0 + 1e-6, "{path:?}");
                }
            }
        }
    }
}

/// R91: sliced on their own, the supports have no islands: no ball, arm,
/// brace or companion starts in mid air, so nothing in a support needs a
/// support. (Columns standing on the part rest on it, so they are left
/// out.)
#[test]
fn supports_alone_have_no_islands() {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    for (raft, tree) in [(true, true), (false, true), (true, false)] {
        let settings = SupportSettings {
            raft,
            tree_supports: tree,
            ..Default::default()
        };
        for (index, (mesh, points)) in member_fixtures().into_iter().enumerate() {
            let result = crate::supports::build_supports_with_tolerance(
                &mesh, None, &points, &settings, 0.05,
            )
            .unwrap();
            let floor = if raft {
                settings.raft_thickness_mm as f32
            } else {
                0.0
            } + 1e-3;
            let mut scene = Scene::new();
            let mut height = 0.0f32;
            for part in result
                .columns
                .iter()
                .filter(|c| c.vertices.iter().map(|v| v[2]).fold(f32::MAX, f32::min) <= floor)
                .chain(result.raft.iter())
            {
                height = part.vertices.iter().map(|v| v[2]).fold(height, f32::max);
                let leaf = scene
                    .add_leaf(
                        None,
                        Transform3D::Identity,
                        LeafPayload::Mesh(std::sync::Arc::new(part.clone())),
                    )
                    .unwrap();
                scene.add_root(leaf).unwrap();
            }
            if height <= 0.0 {
                continue;
            }
            let layers = (height / 0.05).ceil() as u32 + 2;
            let profile = SliceProfile::new(
                BuildVolume::new(80.0, 80.0, 80.0).unwrap(),
                PixelResolution::new(1600, 1600).unwrap(),
                LayerSchedule::uniform(0.05, 0.05, layers).unwrap(),
                Mirror { x: false, y: false },
                AaLevel::Off,
                PackingMode::None,
            );
            let stack = sc_sl1cer::contour::merge_stacks(
                sc_sl1cer::api::slice_scene(&scene, &profile)
                    .unwrap()
                    .into_iter()
                    .map(|l| l.into_stack())
                    .collect(),
            )
            .unwrap();
            let islands = sc_print_job::find_islands(&stack, &profile);
            assert!(
                islands.is_empty(),
                "fixture {index} (raft {raft}, trees {tree}): {islands:?}"
            );
        }
    }
}

/// A box of half extents `half`, turned `deg` about X and centred `lift` mm
/// up, with the rotation to take a point back into the box's own frame.
fn tilted_box(half: [f64; 3], deg: f64, lift: f64) -> (IndexedMesh, glam::DMat3) {
    use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
    use sc_mesh_core::{IndexedTriangle, Vertex};
    let turn = glam::DMat3::from_rotation_x(deg.to_radians());
    let vertices: Vec<Vertex> = (0..8u32)
        .map(|i| {
            let local = glam::DVec3::new(
                if i & 1 == 0 { -half[0] } else { half[0] },
                if i & 2 == 0 { -half[1] } else { half[1] },
                if i & 4 == 0 { -half[2] } else { half[2] },
            );
            let w = turn * local + glam::DVec3::Z * lift;
            Vertex::new([w.x as f32, w.y as f32, w.z as f32])
        })
        .collect();
    let faces = [
        [0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4],
        [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5],
    ]
    .map(|ids: [u32; 3]| {
        let normal = compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize])).unwrap();
        IndexedTriangle::from_raw_indices(normal, ids)
    })
    .to_vec();
    (IndexedMesh::new(None, vertices, faces), turn.transpose())
}

/// R104 (the maintainer: tips on a tilted part were buried at random depths,
/// the low ones far deeper and harder to remove): no part of any support goes
/// into the model deeper than the contact depth, on faces at any slope.
#[test]
fn no_support_goes_deeper_into_the_part_than_the_contact_depth() {
    let half = [10.0, 6.0, 3.0];
    let mut worst = (0.0f64, String::new());
    for deg in [20.0f64, 35.0, 50.0] {
      // High up (full-length tips) and with its lowest corner 2 mm over the
      // plate, where tips are short: the buried ones were the low ones.
      let reach = half[1] * deg.to_radians().sin() + half[2] * deg.to_radians().cos();
      for lift in [18.0, reach + 2.0] {
        for tree in [true, false] {
            let settings = SupportSettings { raft: false, tree_supports: tree, ..Default::default() };
            let (mesh, back) = tilted_box(half, deg, lift);
            let points = sample_contacts(&mesh, &settings).unwrap();
            assert!(!points.is_empty());
            eprintln!("{deg} deg trees {tree}: {} contacts", points.len());
            let result =
                crate::supports::build_supports_with_tolerance(&mesh, None, &points, &settings, 0.05)
                    .unwrap();
            for column in &result.columns {
                for v in &column.vertices {
                    let local = back * (glam::DVec3::from_array(v.coords().map(f64::from)) - glam::DVec3::Z * lift);
                    let depth = (half[0] - local.x.abs())
                        .min(half[1] - local.y.abs())
                        .min(half[2] - local.z.abs());
                    if depth > worst.0 {
                        worst = (depth, format!("{deg} degrees, lift {lift:.1}, trees {tree}"));
                    }
                }
            }
        }
      }
    }
    // The whole Auto supports path (lift, raft, island passes, fallbacks).
    for deg in [20.0f64, 35.0, 50.0] {
        let (mesh, back) = tilted_box(half, deg, 18.0);
        let before = mesh.local_aabb().unwrap().min()[2] as f64;
        let (scene, id) = one_model_scene(mesh);
        let (generated, structure, _) = crate::supports::auto_supported_scene(
            &scene,
            &SupportSettings::default(),
            &r38_profile(),
        )
        .unwrap();
        let after = sc_scene::bake_leaf_to_world(&generated, id).unwrap().local_aabb().unwrap().min()[2] as f64;
        let centre = 18.0 + after - before;
        for column in &structure.columns {
            for v in &column.vertices {
                let local = back * (glam::DVec3::from_array(v.coords().map(f64::from)) - glam::DVec3::Z * centre);
                let depth = (half[0] - local.x.abs())
                    .min(half[1] - local.y.abs())
                    .min(half[2] - local.z.abs());
                if depth > worst.0 {
                    worst = (depth, format!("{deg} degrees, auto supports"));
                }
            }
        }
    }
    eprintln!("worst support depth {:.3} mm ({})", worst.0, worst.1);
    let allowed = SupportSettings::default().contact_depth_mm + 0.02;
    assert!(worst.0 <= allowed, "a support reaches {:.3} mm into the part ({}), allowed {allowed}", worst.0, worst.1);
}

/// The separate solids of a support mesh (tubes, balls, pillars), as vertex
/// lists, found by the faces that join them.
fn mesh_parts(mesh: &IndexedMesh) -> Vec<Vec<glam::DVec3>> {
    let n = mesh.vertices.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for face in &mesh.faces {
        let ids = face.vertices.map(usize::from);
        for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2])] {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let mut parts: std::collections::BTreeMap<usize, Vec<glam::DVec3>> = Default::default();
    for i in 0..n {
        let r = root(&mut parent, i);
        parts
            .entry(r)
            .or_default()
            .push(glam::DVec3::from_array(mesh.vertices[i].coords().map(f64::from)));
    }
    parts.into_values().collect()
}

/// R104 fixtures: a slab whose underside is 2 mm and 9 mm over the plate,
/// so the low contacts get short tips.
fn low_and_high_slabs() -> Vec<(IndexedMesh, Vec<SupportPoint>)> {
    [2.0f32, 9.0]
        .into_iter()
        .map(|z| {
            let mesh = cuboid([-6.0, -6.0, z], [6.0, 6.0, z + 2.0]);
            let points = sample_contacts(&mesh, &SupportSettings::default()).unwrap_or_default();
            (mesh, points)
        })
        .collect()
}

/// R104 (the maintainer: "the cone overlaps the sphere sometimes"): a tip
/// cone ends inside the ball under it, never wider than the ball, so no lip
/// sticks out of the joint.
#[test]
fn tip_cones_end_inside_their_ball() {
    let settings = SupportSettings { raft: false, ..Default::default() };
    let mut checked = 0;
    for (mesh, points) in low_and_high_slabs() {
        let result =
            crate::supports::build_supports_with_tolerance(&mesh, None, &points, &settings, 0.05)
                .unwrap();
        for column in &result.columns {
            let parts = mesh_parts(column);
            let balls: Vec<(glam::DVec3, f64)> = parts
                .iter()
                // Balls: hundreds of vertices; tubes are two rings of at most
                // 48 (96), pillars three or four rings.
                .filter(|p| p.len() > 200)
                .map(|p| {
                    let c = p.iter().copied().sum::<glam::DVec3>() / p.len() as f64;
                    let r = p.iter().map(|v| (*v - c).length()).fold(0.0, f64::max);
                    (c, r)
                })
                .collect();
            for part in parts.iter().filter(|p| p.len() <= 96 && p.len() % 2 == 0) {
                let half = part.len() / 2;
                let ring = |r: &[glam::DVec3]| {
                    let c = r.iter().copied().sum::<glam::DVec3>() / r.len() as f64;
                    (c, r.iter().map(|v| (*v - c).length()).fold(0.0, f64::max))
                };
                let (a, b) = (ring(&part[..half]), ring(&part[half..]));
                let wide = if a.1 > b.1 { a } else { b };
                if let Some((_, ball)) = balls.iter().find(|(c, _)| (*c - wide.0).length() < 0.05) {
                    checked += 1;
                    assert!(
                        wide.1 <= ball + 0.02,
                        "a cone ends {:.2} mm wide in a {:.2} mm ball",
                        wide.1,
                        ball
                    );
                }
            }
        }
    }
    assert!(checked > 0, "no tip cone found");
}

/// R104 (the maintainer: tip contacts looked random, the low ones fatter and
/// buried): no tip cone widens faster than a full-length tip under the widest
/// column, so a short tip near the plate ends narrower instead of being a
/// stubby wide cone.
#[test]
fn short_tips_are_cut_short_not_squashed() {
    let settings = SupportSettings { raft: false, ..Default::default() };
    let contact = settings.tip_contact_diameter_mm / 2.0;
    let mut tapers = Vec::new();
    for (mesh, points) in low_and_high_slabs() {
        let result =
            crate::supports::build_supports_with_tolerance(&mesh, None, &points, &settings, 0.05)
                .unwrap();
        for column in &result.columns {
            for part in mesh_parts(column).iter().filter(|p| p.len() <= 96 && p.len() % 2 == 0) {
                let half = part.len() / 2;
                let ring = |r: &[glam::DVec3]| {
                    let c = r.iter().copied().sum::<glam::DVec3>() / r.len() as f64;
                    (c, r.iter().map(|v| (*v - c).length()).fold(0.0, f64::max))
                };
                let (a, b) = (ring(&part[..half]), ring(&part[half..]));
                let (narrow, wide) = if a.1 < b.1 { (a, b) } else { (b, a) };
                // A tip: it narrows to the contact diameter.
                if (narrow.1 - contact).abs() < 0.02 && wide.1 > narrow.1 + 0.02 {
                    tapers.push((wide.1 - narrow.1) / (wide.0 - narrow.0).length());
                }
            }
        }
    }
    assert!(tapers.len() > 4, "{tapers:?}");
    let full = (settings.max_column_diameter_mm / 2.0 - contact) / settings.tip_length_mm;
    let (lo, hi) = tapers.iter().fold((f64::MAX, 0.0f64), |(lo, hi), t| (lo.min(*t), hi.max(*t)));
    assert!(hi <= full + 0.02, "tip tapers range from {lo:.3} to {hi:.3} mm per mm (a full tip: {full:.3})");
}



/// R105: the lattice raft's struts join natural neighbours only: a square
/// grid of feet gets its 12 side struts and no crossing diagonals.
#[test]
fn raft_struts_join_grid_neighbours_without_diagonals() {
    let feet: Vec<[f64; 2]> = (0..9).map(|i| [f64::from(i % 3) * 5.0, f64::from(i / 3) * 5.0]).collect();
    let struts = crate::supports::raft_struts(&feet);
    assert_eq!(struts.len(), 12, "{struts:?}");
    for (a, b) in struts {
        let d = (feet[a][0] - feet[b][0]).hypot(feet[a][1] - feet[b][1]);
        assert!((d - 5.0).abs() < 1e-9, "strut {a}-{b} is {d} mm");
    }
}

/// R105: scattered feet always make one raft whose struts never cross, even
/// two clusters far apart.
#[test]
fn raft_struts_are_one_piece_and_never_cross() {
    let mut seed = 7u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / f64::from(1u32 << 31)
    };
    for count in [2usize, 5, 30, 120] {
        let feet: Vec<[f64; 2]> = (0..count)
            .map(|i| {
                let off = if i % 2 == 0 { 0.0 } else { 60.0 };
                [off + next() * 20.0, next() * 20.0]
            })
            .collect();
        let struts = crate::supports::raft_struts(&feet);
        let mut group: Vec<usize> = (0..count).collect();
        fn find(g: &mut [usize], i: usize) -> usize {
            if g[i] != i {
                let r = find(g, g[i]);
                g[i] = r;
            }
            g[i]
        }
        for &(a, b) in &struts {
            let (ra, rb) = (find(&mut group, a), find(&mut group, b));
            group[ra] = rb;
        }
        let root = find(&mut group, 0);
        assert!((0..count).all(|i| find(&mut group, i) == root), "{count} feet: raft in pieces");
        let cross = |p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]| {
            let o = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            o(p, q, r) * o(p, q, s) < -1e-9 && o(r, s, p) * o(r, s, q) < -1e-9
        };
        for (i, &(a, b)) in struts.iter().enumerate() {
            for &(c, d) in &struts[i + 1..] {
                assert!(!cross(feet[a], feet[b], feet[c], feet[d]), "{count} feet: struts {a}-{b} and {c}-{d} cross");
            }
        }
    }
}

/// R105: a raft's adhesion target follows Auto orient's adequate adhesion: a
/// 10 mm cube needs 20 + 0.15 x 100 + 0.5 x 10 + 0.01 x 1000 = 50 mm2.
#[test]
fn raft_target_follows_the_model() {
    let cube = cuboid([0.0, 0.0, 5.0], [10.0, 10.0, 15.0]);
    let tris: Vec<[sc_mesh_core::Vertex; 3]> = cube
        .faces
        .iter()
        .map(|f| f.vertices.map(|i| cube.vertices[usize::from(i)]))
        .collect();
    let target = crate::supports::raft_target_area(&tris);
    assert!((target - 50.0).abs() < 0.5, "{target}");
}

/// R105: pads stay minimal while struts and pads already reach the target,
/// and grow just enough when they do not.
#[test]
fn lattice_pads_grow_only_to_meet_the_adhesion_target() {
    let feet: Vec<[f64; 2]> = (0..16).map(|i| [f64::from(i % 4) * 8.0, f64::from(i / 4) * 8.0]).collect();
    let struts = crate::supports::raft_struts(&feet);
    let least = 2.0 + 0.5;
    assert_eq!(crate::supports::lattice_pad_radius(&feet, &struts, 2.0, 0.6, 50.0), least);
    let big = crate::supports::lattice_pad_radius(&feet, &struts, 2.0, 0.6, 2000.0);
    assert!(big > least, "{big}");
    let strut_area: f64 = struts
        .iter()
        .map(|&(a, b)| ((feet[a][0] - feet[b][0]).hypot(feet[a][1] - feet[b][1]) - 2.0 * least).max(0.0) * 1.2)
        .sum();
    let total = strut_area + 16.0 * std::f64::consts::PI * big * big;
    assert!((total - 2000.0).abs() < 1.0, "{total}");
}




/// A part lifted onto a raft as Auto prepare leaves it: a box of half
/// extents `half` tilted 30 degrees (it touches the plate along an edge, so it
/// does not print on the plate), resting on it at x offset `x`.
fn lifted_part(half: [f64; 3], x: f64) -> IndexedMesh {
    let (mesh, _) = tilted_box(half, 30.0, 0.0);
    let low = mesh.vertices.iter().map(|v| v[2]).fold(f32::INFINITY, f32::min);
    let vertices = mesh
        .vertices
        .iter()
        .map(|v| sc_mesh_core::Vertex::new([v[0] + x as f32, v[1], v[2] - low]))
        .collect();
    IndexedMesh::new(None, vertices, mesh.faces.clone())
}

/// R106: Auto prepare tunes every support setting to the parts: a small
/// part gets fine tips and slim shafts, a big or tall part strong ones; the
/// same part many times over, or one big flat part, gets the lattice raft;
/// the user's own choices (overhang angle, trees) are kept; and the result
/// is always valid.
#[test]
fn support_choice_tunes_every_setting_to_the_parts() {
    use crate::supports::choose_supports;
    let user = SupportSettings { overhang_angle_deg: 38.0, tree_supports: false, ..Default::default() };
    let plate = 223.6 * 126.5;
    let mini = |x: f64| lifted_part([5.0, 5.0, 10.0], x);
    let small = choose_supports(&[mini(0.0)], plate, &user);
    let tall = choose_supports(&[lifted_part([10.0, 10.0, 75.0], 0.0)], plate, &user);
    for c in [&small, &tall] {
        assert_eq!(c.lifted, 1, "{c:?}");
        c.settings.validate().unwrap();
        assert_eq!(c.settings.overhang_angle_deg, 38.0);
        assert!(!c.settings.tree_supports);
        assert!(c.settings.contact_depth_mm >= 0.1, "two layers at least");
    }
    assert!(small.load < 0.2, "{small:?}");
    assert!(small.settings.tip_contact_diameter_mm < 0.35);
    assert!(small.settings.shaft_diameter_mm < 1.0);
    assert!(!small.lattice);
    assert!(tall.load > 0.8, "{tall:?}");
    assert!(tall.settings.tip_contact_diameter_mm > 0.6);
    assert!(tall.settings.shaft_diameter_mm > 1.8);
    assert!(tall.settings.spacing_mm < small.settings.spacing_mm, "heavier parts get closer contacts");
    assert!(tall.settings.lift_mm > small.settings.lift_mm);
    let many: Vec<IndexedMesh> = (0..12).map(|i| mini(f64::from(i) * 25.0)).collect();
    let crowd = choose_supports(&many, plate, &user);
    assert!(crowd.lattice && crowd.settings.lattice_raft, "{crowd:?}");
    assert_eq!(crowd.settings.tip_contact_diameter_mm, small.settings.tip_contact_diameter_mm, "more of the same part: same tips");
    let flat = choose_supports(&[lifted_part([30.0, 20.0, 1.5], 0.0)], plate, &user);
    assert!(flat.lattice, "{flat:?}");
    // The most demanding part sets the plate's settings.
    let mixed = choose_supports(&[mini(0.0), lifted_part([10.0, 10.0, 75.0], 60.0)], plate, &user);
    assert_eq!(mixed.settings.shaft_diameter_mm, tall.settings.shaft_diameter_mm);
}

/// R107 (the maintainer: a Benchy's supports changed oddly with R106): a part
/// that prints on the plate does not drive the tuning, since the plate holds
/// its weight and peel; with only such parts the user's settings stand, and
/// they do not count toward the raft.
#[test]
fn parts_on_the_plate_keep_the_users_support_settings() {
    use crate::supports::choose_supports;
    let user = SupportSettings::default();
    let plate = 223.6 * 126.5;
    // A wide hull on a flat base, with a deck overhanging it, as a Benchy.
    let boat = boxes(&[
        ([0.0, 0.0, 0.0], [50.0, 25.0, 10.0]),
        ([-5.0, -3.0, 10.0], [55.0, 28.0, 14.0]),
    ]);
    let alone = choose_supports(std::slice::from_ref(&boat), plate, &user);
    assert_eq!(alone.lifted, 0);
    assert_eq!(alone.settings, user, "{alone:?}");
    // Beside a lifted part, the boat neither raises the load nor adds raft.
    let lifted = lifted_part([5.0, 5.0, 10.0], 90.0);
    let both = choose_supports(&[boat, lifted.clone()], plate, &user);
    let only = choose_supports(&[lifted], plate, &user);
    assert_eq!(both.lifted, 1);
    assert_eq!(both.settings, only.settings);
    assert_eq!(both.raft_share, only.raft_share);
}
