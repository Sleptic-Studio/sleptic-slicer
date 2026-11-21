// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
#[cfg(test)]
mod visibility_tests {
    use super::super::toggle_hidden;
    #[test]
    fn independent_rows_toggle_and_restore_without_duplicates() {
        let mut hidden = vec![];
        toggle_hidden(&mut hidden, 2);
        toggle_hidden(&mut hidden, 4);
        assert_eq!(hidden, vec![2, 4]);
        toggle_hidden(&mut hidden, 2);
        assert_eq!(hidden, vec![4]);
        toggle_hidden(&mut hidden, 4);
        assert!(hidden.is_empty());
    }
}

#[cfg(test)]
mod tests_group_visibility {
    use super::super::*;
    #[test]
    fn support_group_toggle_does_not_hide_adjacent_models() {
        let mut hidden = vec![2];
        toggle_row_visibility(&mut hidden, &[2, 4, 5]);
        assert_eq!(hidden, vec![2, 4, 5]);
        toggle_row_visibility(&mut hidden, &[2, 4, 5]);
        assert!(hidden.is_empty());
    }
}

#[cfg(test)]
mod r28_file_names {
    use std::path::Path;
    use std::sync::Arc;

    use super::super::*;
    use sc_mesh_core::primitives::Vertex;
    use sc_mesh_core::transform::Transform3D;
    use sc_mesh_core::{IndexedMesh, MeshMetadata};

    fn scene_with_stl_header_name(count: usize) -> Scene {
        let mut g = Scene::new();
        for _ in 0..count {
            let mesh = IndexedMesh {
                meta: Some(MeshMetadata {
                    name: Some("solid Shape0".into()),
                    ..Default::default()
                }),
                vertices: vec![Vertex::new([0.0, 0.0, 0.0])],
                faces: vec![],
            };
            let id = g
                .add_leaf(
                    None,
                    Transform3D::Identity,
                    LeafPayload::Mesh(Arc::new(mesh)),
                )
                .unwrap();
            g.add_root(id).unwrap();
        }
        g
    }

    // Sleptic's audit: a Benchy STL showed up as "solid Shape0" (the exporter's
    // header text) instead of its file name.
    #[test]
    fn a_single_mesh_stl_is_named_after_its_file() {
        let mut g = scene_with_stl_header_name(1);
        crate::mesh_loader::name_part_after_file(&mut g, Path::new(r"C:\m\3DBenchy.stl"));
        assert_eq!(leaf_names(&g), vec!["3DBenchy".to_string()]);
    }

    #[test]
    fn multi_object_and_3mf_files_keep_their_own_names() {
        let mut g = scene_with_stl_header_name(2);
        crate::mesh_loader::name_part_after_file(&mut g, Path::new("parts.obj"));
        assert!(leaf_names(&g)[0].starts_with("solid Shape0"));
        let mut g = scene_with_stl_header_name(1);
        crate::mesh_loader::name_part_after_file(&mut g, Path::new("part.3mf"));
        assert_eq!(leaf_names(&g), vec!["solid Shape0".to_string()]);
    }

    #[test]
    fn sidebar_numbers_drop_trailing_zeros_and_counts_pluralize() {
        use crate::process_settings::trimmed_decimal;
        assert_eq!(trimmed_decimal(2.5), "2.5");
        assert_eq!(trimmed_decimal(150.0), "150");
        assert_eq!(trimmed_decimal(0.05), "0.05");
        assert_eq!(trimmed_decimal(-0.0), "0");
        assert_eq!(crate::hub::count_label(1, "plate", "plates"), "1 plate");
        assert_eq!(crate::hub::count_label(3, "plate", "plates"), "3 plates");
    }
}

#[cfg(test)]
mod r36_support_rows {
    use std::sync::Arc;

    use super::super::*;
    use sc_mesh_core::IndexedMesh;
    use sc_mesh_core::primitives::Vertex;
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::SceneMetadata;

    fn leaf(g: &mut Scene, comment: Option<String>) -> sc_scene::NodeId {
        let mesh = IndexedMesh {
            meta: None,
            vertices: vec![Vertex::new([0.0, 0.0, 0.0])],
            faces: vec![],
        };
        let meta = comment.map(|c| SceneMetadata {
            name: None,
            comment: Some(c),
        });
        let id = g
            .add_leaf(
                meta,
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(mesh)),
            )
            .unwrap();
        g.add_root(id).unwrap();
        id
    }

    // R36: the trash icon on a model's "Supports (N)" row removes that
    // model's supports; the row maps back to its owning model.
    #[test]
    fn support_rows_map_to_their_owning_model() {
        let mut g = Scene::new();
        let a = leaf(&mut g, None);
        let b = leaf(&mut g, None);
        for owner in [a, a, b] {
            leaf(
                &mut g,
                Some(format!("sc-generated-support\nowner = {}\n", owner.get())),
            );
        }
        // Rows: A, Supports(2), B, Supports(1).
        assert_eq!(support_row_flags(&g), vec![false, true, false, true]);
        assert_eq!(support_row_owner(&g, 1), Some(a));
        assert_eq!(support_row_owner(&g, 3), Some(b));
        assert_eq!(support_row_owner(&g, 0), None);
        assert_eq!(support_row_owner(&g, 9), None);
    }
}
use super::*;
use sc_mesh_core::{IndexedMesh, MeshMetadata, transform::Transform3D};
use sc_scene::{NodeId, SceneMetadata};
use std::sync::Arc;

fn add(
    scene: &mut Scene,
    name: Option<&str>,
    mesh_name: Option<&str>,
    comment: Option<String>,
    cube: bool,
) -> NodeId {
    let mut mesh = if cube {
        sc_mesh_fixtures::unit_cube_mesh()
    } else {
        IndexedMesh {
            meta: None,
            vertices: vec![],
            faces: vec![],
        }
    };
    mesh.meta = mesh_name.map(|n| MeshMetadata {
        name: Some(n.into()),
        ..Default::default()
    });
    let meta = if name.is_some() || comment.is_some() {
        Some(SceneMetadata {
            name: name.map(str::to_owned),
            comment,
        })
    } else {
        None
    };
    let id = scene
        .add_leaf(
            meta,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    id
}
#[test]
fn names_prefer_node_then_mesh_then_fallback_and_disambiguate() {
    let mut scene = Scene::new();
    add(&mut scene, Some("A"), Some("mesh"), None, false);
    add(&mut scene, Some("A"), None, None, false);
    add(&mut scene, None, Some("mesh"), None, false);
    add(&mut scene, Some("Object 5"), None, None, false);
    add(&mut scene, None, None, None, false);
    add(&mut scene, None, None, None, false);
    assert_eq!(
        build_object_list_names(&scene),
        ["A", "A (2)", "mesh", "Object 5", "Object 5 (2)", "Object 6"]
    );
}
#[test]
fn grouping_and_counts_follow_model_ownership() {
    let mut scene = Scene::new();
    let a = add(&mut scene, Some("A"), None, None, false);
    let b = add(&mut scene, Some("B"), None, None, false);
    for owner in [a, b, a] {
        add(
            &mut scene,
            None,
            None,
            Some(format!("sc-generated-support\nowner = {}\n", owner.get())),
            true,
        );
    }
    add(
        &mut scene,
        None,
        None,
        Some("sc-generated-support-raft".into()),
        true,
    );
    assert_eq!(
        build_object_list_names(&scene),
        ["A", "Supports (2)", "B", "Supports (1)"]
    );
    let rows = object_rows(&scene);
    assert_eq!(rows[1].leaf_indices, [2, 4, 5]);
    assert_eq!(rows[2].leaf_indices, [1]);
    assert_eq!(
        triangle_counts(&scene),
        ["0 triangles", "36 triangles", "0 triangles", "12 triangles"]
    );
    assert_eq!(
        scene_stats(&scene),
        SceneStats {
            object_count: 2,
            support_count: 3,
            triangle_count: 48
        }
    );
}
#[test]
fn orphan_supports_are_one_row() {
    let mut scene = Scene::new();
    for _ in 0..2 {
        add(
            &mut scene,
            None,
            None,
            Some("sc-generated-support".into()),
            true,
        );
    }
    assert_eq!(build_object_list_names(&scene), ["Supports (2)"]);
    assert_eq!(object_rows(&scene)[0].leaf_indices, [0, 1]);
}
#[test]
fn empty_and_model_only_scenes_have_consistent_counts() {
    let mut scene = Scene::new();
    assert!(build_object_list_names(&scene).is_empty());
    assert_eq!(scene_stats(&scene), SceneStats::default());
    for name in ["A", "B"] {
        add(&mut scene, Some(name), None, None, true);
    }
    assert_eq!(build_object_list_names(&scene), ["A", "B"]);
    assert_eq!(
        scene_stats(&scene),
        SceneStats {
            object_count: 2,
            support_count: 0,
            triangle_count: 24
        }
    );
}
#[test]
fn formatting_covers_singular_plural_and_grouping() {
    for (value, expected) in [
        (0, "0"),
        (7, "7"),
        (999, "999"),
        (1000, "1,000"),
        (24816, "24,816"),
        (1234567, "1,234,567"),
    ] {
        assert_eq!(format_thousands(value), expected);
    }
    for (stats, expected) in [
        (SceneStats::default(), ""),
        (
            SceneStats {
                object_count: 1,
                support_count: 0,
                triangle_count: 1,
            },
            "1 object · 1 triangle",
        ),
        (
            SceneStats {
                object_count: 3,
                support_count: 0,
                triangle_count: 24816,
            },
            "3 objects · 24,816 triangles",
        ),
        (
            SceneStats {
                object_count: 9,
                support_count: 18,
                triangle_count: 2147922,
            },
            "9 objects · 18 supports · 2,147,922 triangles",
        ),
        (
            SceneStats {
                object_count: 1,
                support_count: 1,
                triangle_count: 0,
            },
            "1 object · 1 support · 0 triangles",
        ),
    ] {
        assert_eq!(format_scene_stats(stats), expected);
    }
}
#[test]
fn nested_and_csg_leaves_follow_spatial_order() {
    use sc_scene::CsgOp;
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let named = |scene: &mut Scene, name: &str| {
        scene
            .add_leaf(
                Some(SceneMetadata {
                    name: Some(name.into()),
                    comment: None,
                }),
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap()
    };
    let before = named(&mut scene, "Before");
    scene.add_child(root, before).unwrap();
    let group = scene.add_group(None, Transform3D::Identity);
    scene.add_child(root, group).unwrap();
    let nested = named(&mut scene, "Nested");
    scene.add_child(group, nested).unwrap();
    let a = named(&mut scene, "Op1");
    let b = named(&mut scene, "Op2");
    let csg = scene
        .add_csg(None, Transform3D::Identity, CsgOp::Union, vec![a, b])
        .unwrap();
    scene.add_child(root, csg).unwrap();
    let after = named(&mut scene, "After");
    scene.add_child(root, after).unwrap();
    let names = build_object_list_names(&scene);
    assert_eq!(names, ["Before", "Nested", "Op1", "Op2", "After"]);
    let spatial = sc_mesh_spatial::SceneSpatial::from_graph(scene);
    for (index, name) in names.iter().enumerate() {
        let id = spatial.leaf_id_at_index(index).unwrap();
        assert_eq!(
            spatial
                .graph()
                .node(id)
                .unwrap()
                .meta
                .as_ref()
                .unwrap()
                .name
                .as_ref()
                .unwrap(),
            name
        );
    }
    assert!(spatial.leaf_id_at_index(names.len()).is_none());
}
#[test]
fn third_duplicate_gets_next_suffix() {
    let mut scene = Scene::new();
    for name in ["A", "A", "A", "B"] {
        add(&mut scene, Some(name), None, None, false);
    }
    assert_eq!(
        build_object_list_names(&scene),
        ["A", "A (2)", "A (3)", "B"]
    );
}
