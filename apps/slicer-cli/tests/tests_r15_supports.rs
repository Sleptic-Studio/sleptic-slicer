// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use sc_mesh_core::{IndexedMesh, IndexedTriangle, Vertex, transform::Transform3D};
use sc_print_profiles::support_settings::SupportSettings;
use sc_scene::{LeafPayload, Scene};
use sc_sl1cer::profile::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

// One connected, closed comb: six small downward teeth share an upper
// bridge, 6 mm apart (a 2 mm gap is a short bridge that prints without
// supports, R63). 12 mm deep, so each gap still needs several contacts
// away from the teeth walls holding it (R67). Voxel boundary faces are
// welded; interior faces are omitted.
fn comb() -> IndexedMesh {
    let cells: BTreeSet<_> = (0..24)
        .flat_map(|x| {
            (0..6).flat_map(move |y| {
                // Gaps 10 mm tall: branches leave a trunk near the part (R86),
                // so pillars only a few millimetres tall stay separate.
                (4..16).filter_map(move |z| (z >= 14 || x % 4 == 0).then_some([x, y, z]))
            })
        })
        .collect();
    let mut map = BTreeMap::new();
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for &[x, y, z] in &cells {
        let corners = [
            [x, y, z],
            [x + 1, y, z],
            [x + 1, y + 1, z],
            [x, y + 1, z],
            [x, y, z + 1],
            [x + 1, y, z + 1],
            [x + 1, y + 1, z + 1],
            [x, y + 1, z + 1],
        ];
        for (neighbour, quad) in [
            ([x, y, z - 1], [0, 3, 2, 1]),
            ([x, y, z + 1], [4, 5, 6, 7]),
            ([x, y - 1, z], [0, 1, 5, 4]),
            ([x + 1, y, z], [1, 2, 6, 5]),
            ([x, y + 1, z], [2, 3, 7, 6]),
            ([x - 1, y, z], [3, 0, 4, 7]),
        ] {
            if cells.contains(&neighbour) {
                continue;
            }
            let ids = quad.map(|i| {
                *map.entry(corners[i]).or_insert_with(|| {
                    let id = vertices.len() as u32;
                    vertices.push(Vertex::new([
                        (corners[i][0] as f32 - 12.0) * 2.0,
                        (corners[i][1] as f32 - 3.0) * 2.0,
                        (corners[i][2] - 4) as f32 * 2.0,
                    ]));
                    id
                })
            });
            for triangle in [[ids[0], ids[1], ids[2]], [ids[0], ids[2], ids[3]]] {
                let normal = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                    &triangle.map(|i| vertices[i as usize]),
                )
                .unwrap();
                faces.push(IndexedTriangle::from_raw_indices(normal, triangle));
            }
        }
    }
    IndexedMesh::new(None, vertices, faces)
}

#[test]
fn many_small_overhangs_reduce_trunks_by_forty_percent_without_islands() {
    let mut source = Scene::new();
    let id = source
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(comb())),
        )
        .unwrap();
    source.add_root(id).unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(80., 40., 40.).unwrap(),
        PixelResolution::new(800, 400).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 200).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let mut counts = Vec::new();
    for tree in [false, true] {
        let settings = SupportSettings {
            tree_supports: tree,
            ..Default::default()
        };
        let (scene, structure, analysis) =
            sc_print_glue::supports::auto_supported_scene_analyzed(&source, &settings, &profile)
                .unwrap();
        assert!(analysis.islands.is_empty(), "{:?}", analysis.islands);
        counts.push(structure.columns.len());
        if tree {
            let leans = structure.branch_inclinations_deg();
            assert!(!leans.is_empty());
            for lean in leans {
                assert!(lean <= settings.branch_angle_deg + 1e-5, "{lean}");
            }
        }
        if let Some(folder) = std::env::var_os("SC_R15_OUTPUT") {
            let style = if tree { "tree" } else { "columns" };
            let mut file = std::fs::File::create(
                std::path::PathBuf::from(folder).join(format!("comb-{style}.3mf")),
            )
            .unwrap();
            sc_project_formats::write_project(
                &scene,
                &Default::default(),
                sc_project_formats::EmbedProfiles::No,
                None,
                None,
                sc_project_formats::ProjectFileType::ThreeMf,
                &mut file,
            )
            .unwrap();
        }
    }
    println!("R15 comb columns={} tree={}", counts[0], counts[1]);
    assert!(counts[1] * 100 <= counts[0] * 60, "{counts:?}");
    if let Some(folder) = std::env::var_os("SC_R15_OUTPUT") {
        let folder = std::path::PathBuf::from(folder);
        for model in ["benchy", "comb"] {
            let mut combined = Scene::new();
            let mut plates = combined.plates().clone();
            let second = plates.add();
            for (index, style) in ["columns", "tree"].into_iter().enumerate() {
                let loaded = sc_project_formats::load_scene(
                    &folder.join(format!("{model}-{style}.3mf")),
                    None,
                    |_| {},
                )
                .unwrap();
                let mut remap = BTreeMap::new();
                loaded
                    .scene
                    .for_each_expanded_leaf::<sc_scene::SceneError, _>(
                        sc_scene::CsgPolicy::Descend,
                        |id, world, mesh| {
                            let mut meta = loaded
                                .scene
                                .node(id)
                                .unwrap()
                                .meta
                                .clone()
                                .unwrap_or_default();
                            if !meta
                                .comment
                                .as_deref()
                                .is_some_and(|c| c.starts_with("sc-generated-support"))
                            {
                                meta.name = Some(style.to_string());
                            }
                            let mut mesh = mesh.clone();
                            mesh.meta.get_or_insert_with(Default::default).name = meta.name.clone();
                            let new = combined.add_leaf(
                                Some(meta),
                                *world,
                                LeafPayload::Mesh(Arc::new(mesh)),
                            )?;
                            combined.add_root(new)?;
                            remap.insert(id.get(), new);
                            plates.assignments.insert(
                                new,
                                if index == 0 {
                                    plates.order[0].id
                                } else {
                                    second
                                },
                            );
                            Ok(())
                        },
                    )
                    .unwrap();
                for &id in remap.values() {
                    let leaf = combined.remove_leaf(id).unwrap();
                    let mut meta = leaf.meta.unwrap_or_default();
                    if let Some(comment) = &mut meta.comment {
                        *comment = comment
                            .lines()
                            .map(|line| {
                                line.strip_prefix("owner = ")
                                    .and_then(|s| s.parse::<u32>().ok())
                                    .and_then(|old| remap.get(&old))
                                    .map_or_else(
                                        || line.to_owned(),
                                        |new| format!("owner = {}", new.get()),
                                    )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                            + "\n";
                    }
                    combined
                        .install_leaf_at(
                            id,
                            leaf.attachment,
                            leaf.payload,
                            leaf.local_transform,
                            Some(meta),
                        )
                        .unwrap();
                }
            }
            combined.set_plates(plates);
            let mut file =
                std::fs::File::create(folder.join(format!("{model}-comparison.3mf"))).unwrap();
            sc_project_formats::write_project(
                &combined,
                &Default::default(),
                sc_project_formats::EmbedProfiles::No,
                None,
                None,
                sc_project_formats::ProjectFileType::ThreeMf,
                &mut file,
            )
            .unwrap();
        }
    }
}
