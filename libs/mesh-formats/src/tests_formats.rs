// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    error::*, filetypes::*, loader::*, obj::*, scene_writer::*, stl::*,
    threemf::*,
};
use sc_mesh_core::{
    IndexedMesh, IndexedTriangle, MeshMetadata, Normal, Vertex,
    transform::{AffineTransform3D, Transform3D},
};
use sc_scene::{CsgPolicy, LeafPayload, Scene, SceneMetadata};
use std::{
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
};
fn mesh() -> IndexedMesh {
    IndexedMesh::new(
        Some(MeshMetadata {
            name: Some("Küche".into()),
            header_comment: None,
        }),
        vec![
            Vertex::new([-0., 0., 0.]),
            Vertex::new([1., 0., 0.]),
            Vertex::new([0., 1., 0.]),
        ],
        vec![IndexedTriangle::from_raw_indices(
            Normal::new([0., 0., 1.]),
            [0, 1, 2],
        )],
    )
}
fn leaves(g: &Scene) -> Vec<IndexedMesh> {
    let mut v = Vec::new();
    g.for_each_expanded_leaf::<sc_scene::SceneError, _>(CsgPolicy::Descend, |_, _, m| {
        v.push(m.clone());
        Ok(())
    })
    .unwrap();
    v
}
fn fixture(path: &str) -> std::fs::File {
    std::fs::File::open(format!(
        "{}/tests/fixtures/official/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn obj(s: &str) -> (Scene, Vec<LoadObjWarning>) {
    read_obj(&mut Cursor::new(s)).unwrap()
}
#[test]
#[ignore = "requires SLEPTIC_MESH_BENCH_MODEL and a local output path"]
fn local_mesh_phase_benchmark() {
    let input = std::env::var_os("SLEPTIC_MESH_BENCH_MODEL").unwrap();
    let output = std::env::var_os("SLEPTIC_MESH_BENCH_OUTPUT").unwrap();
    let start = std::time::Instant::now();
    let mut loaded = load_mesh_from_path(Path::new(&input), None, None).unwrap();
    let load = start.elapsed();
    let start = std::time::Instant::now();
    loaded
        .graph
        .repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
    let repair = start.elapsed();
    let start = std::time::Instant::now();
    write_scene(&loaded.graph, Path::new(&output), WriteFormat::StlBin).unwrap();
    eprintln!(
        "load={load:?} repair={repair:?} write={:?}",
        start.elapsed()
    );
    let start = std::time::Instant::now();
    let flat = flatten_graph_to_mesh(&loaded.graph).unwrap();
    let flatten = start.elapsed();
    let start = std::time::Instant::now();
    let mut bytes = Vec::new();
    write_stlb(&flat.mesh, &mut bytes).unwrap();
    let serialize = start.elapsed();
    let start = std::time::Instant::now();
    write_bytes_atomically(Path::new(&output), &bytes).unwrap();
    eprintln!(
        "flatten={flatten:?} serialize={serialize:?} disk={:?}",
        start.elapsed()
    );
}
#[test]
fn binary_parallel_blocks_preserve_geometry_and_first_error() {
    let mut m = mesh();
    m.faces = vec![m.faces[0]; 40000];
    let mut b = Vec::new();
    write_stlb(&m, &mut b).unwrap();
    let (g, _) = read_stlb(&mut Cursor::new(&b)).unwrap();
    assert_eq!(leaves(&g)[0].faces, m.faces);
    b[84 + 16400 * 50 + 12..84 + 16400 * 50 + 16].copy_from_slice(&f32::INFINITY.to_le_bytes());
    b[84 + 33000 * 50..84 + 33000 * 50 + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    b.truncate(b.len() - 1);
    assert!(
        matches!(read_stlb(&mut Cursor::new(b)), Err(LoadStlError::NonFiniteFloat { what: "v0.x", value }) if value == f32::INFINITY)
    );
}

fn model_package(root: &str, extra: &[(&str, &str)]) -> sc_3mf::package::Package {
    use sc_3mf::{names::*, package::*};
    let mut p = Package::new();
    for (path, body) in
        std::iter::once((DEFAULT_MODEL_PART_NAME, root)).chain(extra.iter().copied())
    {
        let xml = format!("<model xmlns=\"{CORE_NS}\" xmlns:p=\"{PRODUCTION_NS}\">{body}</model>");
        p.add_part(
            PartName::new(path).unwrap(),
            MODEL_PART_CONTENT_TYPE,
            xml.into_bytes(),
        )
        .unwrap();
    }
    p.add_root_relationship(Relationship { external: false,
        id: "start".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: DEFAULT_MODEL_PART_NAME.into(),
    });
    p
}
#[test]
fn nested_subtree_reference_export_composes_every_transform() {
    fn translation(x: f64) -> Transform3D {
        Transform3D::Affine(AffineTransform3D::from_linear_and_translation(
            [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            [x, 0., 0.],
        ))
    }
    let mut g = Scene::new();
    let leaf = g
        .add_leaf(None, translation(1.), LeafPayload::Mesh(Arc::new(mesh())))
        .unwrap();
    let first = g.register_subtree_resource(leaf).unwrap();
    let reference = g.add_reference(None, translation(2.), first).unwrap();
    let second = g.register_subtree_resource(reference).unwrap();
    let root = g.add_reference(None, translation(3.), second).unwrap();
    g.add_root(root).unwrap();
    let d = scene_to_model_document(&g).unwrap();
    assert_eq!(d.objects.len(), 1);
    assert_eq!(d.build[0].transform.unwrap().0[9], 6.);
}

#[test]
fn large_ascii_parallel_path_matches_serial_rules_and_order() {
    let mut m = mesh();
    m.faces = vec![m.faces[0]; 2100];
    let mut b = Vec::new();
    write_stla(&m, &mut b).unwrap();
    let g = read_stla(&mut Cursor::new(&b)).unwrap();
    assert_eq!(leaves(&g)[0].faces, m.faces);
    b.extend([0xff, 0xfe]);
    assert_eq!(
        leaves(&read_stla(&mut Cursor::new(&b)).unwrap())[0].faces,
        m.faces
    );
    let mut malformed = b"solid\nfacet BROKEN\n".to_vec();
    malformed.extend(b);
    assert!(matches!(
        read_stla(&mut Cursor::new(malformed)),
        Err(LoadStlError::AsciiInvalidFacetHeader { .. })
    ));
}

#[test]
fn production_import_remaps_and_shares_geometry() {
    let root = "<resources><object id=\"10\"><components><component objectid=\"2\" p:path=\"/extra.model\"/><component objectid=\"2\" p:path=\"/extra.model\"/></components></object></resources><build><item objectid=\"10\"/></build>";
    let extra = "<resources><object id=\"1\" name=\"part\"><mesh><vertices><vertex x=\"1\" y=\"2\" z=\"3\"/></vertices><triangles><triangle v1=\"0\" v2=\"0\" v3=\"0\"/></triangles></mesh></object><object id=\"2\"><components><component objectid=\"1\"/></components></object></resources>";
    let p = model_package(root, &[("/extra.model", extra)]);
    let (g, w) = read_threemf_from_package(&p).unwrap();
    assert!(w.is_empty());
    assert_eq!(leaves(&g).len(), 2);
    let d = scene_to_model_document(&g).unwrap();
    assert_eq!(
        d.objects
            .iter()
            .filter(|o| matches!(o.payload, sc_3mf::model::ObjectPayload::Mesh(_)))
            .count(),
        1
    );
    assert!(scene_shared_vertices_warning(&g).is_some());
    for target in [
        None,
        Some(
            "<resources><object id=\"2\"><components><component objectid=\"9\" p:path=\"/nested.model\"/></components></object></resources>",
        ),
    ] {
        let extras: Vec<_> = target
            .map(|s| vec![("/extra.model", s)])
            .unwrap_or_default();
        assert!(matches!(
            read_threemf_from_package(&model_package(root, &extras)),
            Err(LoadThreeMfError::ProductionReference { object_id: 2, .. })
        ));
    }
}

#[test]
fn three_mf_unit_transform_metadata_and_warnings() {
    use sc_3mf::{model::*, names::*, package::*};
    let body = "<metadata name=\"Title\">first</metadata><metadata name=\"Title\">second</metadata><metadata name=\"Description\">comment</metadata><resources><object id=\"1\" type=\"support\" name=\"local\"><mesh><vertices><vertex x=\"1\" y=\"2\" z=\"3\"/></vertices></mesh></object><object id=\"2\"><mesh/></object></resources><build><item objectid=\"1\" transform=\"1 2 0 3 1 0 0 0 1 2 3 4\"/></build>";
    let p = model_package(body, &[]);
    let n = p.start_part().unwrap();
    let mut d = parse_model(p.part_data(&n).unwrap()).unwrap();
    d.unit = Unit::Inch;
    let mut bytes = Vec::new();
    write_model(&d, &mut bytes).unwrap();
    let mut p = Package::new();
    p.add_part(n, MODEL_PART_CONTENT_TYPE, bytes).unwrap();
    p.add_root_relationship(Relationship { external: false,
        id: "start".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: DEFAULT_MODEL_PART_NAME.into(),
    });
    let (g, w) = read_threemf_from_package(&p).unwrap();
    assert_eq!(g.meta().as_ref().unwrap().name.as_deref(), Some("first"));
    assert_eq!(leaves(&g)[0].vertices[0].coords(), [25.4, 50.8, 76.2]);
    assert_eq!(
        w,
        [
            LoadThreeMfWarning::NonModelObjectTypeFlattened {
                id: 1,
                object_type: ObjectType::Support
            },
            LoadThreeMfWarning::UnbuiltObjectSkipped { id: 2, name: None }
        ]
    );
    let flattened = flatten_graph_to_mesh(&g).unwrap();
    let [x, y, z] = flattened.mesh.vertices[0].coords();
    assert!((x - 228.6).abs() < 0.001 && (y - 177.8).abs() < 0.001 && (z - 177.8).abs() < 0.001);
}
#[test]
fn ascii_official_counts() {
    for (name, n) in [
        ("flat", 3),
        ("cube", 12),
        ("block100", 12),
        ("humanoid", 96),
        ("sphere", 228),
        ("bottle", 1240),
        ("teapot", 2016),
    ] {
        let (g, w) = read_stl(&mut fixture(&format!("burkardt-stla/{name}.stl"))).unwrap();
        assert!(w.is_empty());
        assert_eq!(leaves(&g)[0].faces.len(), n, "{name}");
    }
}
#[test]
fn binary_official_names_and_colors() {
    let (g, w) = read_stl(&mut fixture("numpy-stl-stlb/CubeInvalidUnicode.stl")).unwrap();
    assert!(w.is_empty());
    let m = &leaves(&g)[0];
    assert_eq!(m.faces.len(), 12);
    assert_eq!(
        m.meta.as_ref().unwrap().name.as_deref(),
        Some("solid PRO2STL version 1.0")
    );
    let (g, w) = read_stl(&mut fixture("threejs-stlb-color/colored.stl")).unwrap();
    assert_eq!(w, vec![LoadStlWarning::HeaderNotText]);
    assert_eq!(leaves(&g)[0].faces.len(), 2156);
    assert!(leaves(&g)[0].meta.as_ref().unwrap().name.is_none());
}
#[test]
fn stl_exact_bytes_and_round_trips() {
    let m = mesh();
    let mut a = Vec::new();
    write_stla(&m, &mut a).unwrap();
    assert_eq!(
        String::from_utf8(a.clone()).unwrap(),
        "solid Kuche\n  facet normal 0e0 0e0 1e0\n    outer loop\n      vertex -0e0 0e0 0e0\n      vertex 1e0 0e0 0e0\n      vertex 0e0 1e0 0e0\n    endloop\n  endfacet\nendsolid Kuche"
    );
    let g = read_stla(&mut Cursor::new(a)).unwrap();
    assert_eq!(leaves(&g)[0].vertices[0][0].to_bits(), (-0f32).to_bits());
    let mut b = Vec::new();
    write_stlb(&m, &mut b).unwrap();
    assert_eq!(b.len(), 134);
    assert_eq!(&b[..5], b"Kuche");
    assert!(b[5..80].iter().all(|v| *v == 0));
    assert_eq!(&b[80..84], &1u32.to_le_bytes());
    assert_eq!(&b[132..], &[0, 0]);
    let (g, w) = read_stlb(&mut Cursor::new(b)).unwrap();
    assert!(w.is_empty());
    assert_eq!(leaves(&g)[0].faces, m.faces);
    assert_eq!(leaves(&g)[0].vertices, m.vertices);
}
#[test]
fn ascii_header_and_termination() {
    for s in [
        " \tSoLiD arbitrary ! name  \r\nENDSOLID\n",
        "solid\nendsolid wrong\n",
    ] {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(255);
        assert!(read_stl(&mut Cursor::new(bytes)).is_ok());
    }
    assert!(matches!(
        read_stla(&mut Cursor::new("\nsolid x\nendsolid")),
        Err(LoadStlError::AsciiHeaderMissing)
    ));
    assert!(matches!(
        read_stla(&mut Cursor::new("solidify\nendsolid")),
        Err(LoadStlError::AsciiHeaderMissing)
    ));
    assert!(matches!(
        read_stla(&mut Cursor::new("solid x")),
        Err(LoadStlError::AsciiEofWhileExpecting {
            what: "facet or endsolid"
        })
    ));
}
#[test]
fn ascii_errors_precede_later_utf8() {
    let b = b"solid\nFACET wrong\n\xff";
    let e = read_stla(&mut Cursor::new(b)).unwrap_err();
    assert!(matches!(e,LoadStlError::AsciiInvalidFacetHeader{tokens} if tokens==["facet","wrong"]));
    let e = read_stla(&mut Cursor::new(b"solid\n\xff")).unwrap_err();
    assert!(matches!(e,LoadStlError::Io(e) if e.kind()==std::io::ErrorKind::InvalidData));
}
#[test]
fn binary_limits_and_error_precedence() {
    for (n, what) in [(79, "the 80-byte STL header"), (83, "the triangle count")] {
        assert!(
            matches!(read_stlb(&mut Cursor::new(vec![0;n])),Err(LoadStlError::Truncated{what:w}) if w==what)
        );
    }
    let mut b = vec![0; 84];
    b[80..].copy_from_slice(&200000001u32.to_le_bytes());
    assert!(matches!(
        read_stlb(&mut Cursor::new(&b)),
        Err(LoadStlError::DeclaredTriangleCountExceedsCap { .. })
    ));
    b[80..].copy_from_slice(&2u32.to_le_bytes());
    b.extend([0; 50]);
    b[84..88].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(matches!(
        read_stlb(&mut Cursor::new(&b)),
        Err(LoadStlError::NonFiniteFloat {
            what: "normal.x",
            ..
        })
    ));
    b.pop();
    assert!(matches!(
        read_stlb(&mut Cursor::new(&b)),
        Err(LoadStlError::Truncated {
            what: "a triangle record"
        })
    ));
}
#[test]
fn binary_header_control_and_nul() {
    for (header, warn) in [
        (b" name\0\xff".as_slice(), false),
        (b"a\x01b".as_slice(), true),
        (b" \n \t".as_slice(), false),
    ] {
        let mut b = vec![0; 84];
        b[..header.len()].copy_from_slice(header);
        let (_, w) = read_stlb(&mut Cursor::new(b)).unwrap();
        assert_eq!(!w.is_empty(), warn);
    }
}
#[test]
fn stl_exact_welding_and_normal_rules() {
    let mut m = mesh();
    m.vertices.extend([
        Vertex::new([0., 0., 0.]),
        Vertex::new([f32::from_bits(1), 0., 0.]),
    ]);
    m.faces.push(IndexedTriangle::from_raw_indices(
        Normal::new([0.; 3]),
        [3, 1, 2],
    ));
    m.faces.push(IndexedTriangle::from_raw_indices(
        Normal::new([2., 3., 4.]),
        [4, 4, 4],
    ));
    let mut b = Vec::new();
    write_stlb(&m, &mut b).unwrap();
    let (g, _) = read_stlb(&mut Cursor::new(b)).unwrap();
    let r = &leaves(&g)[0];
    assert_eq!(r.vertices.len(), 4);
    assert_eq!(r.vertices[0][0].to_bits(), (-0f32).to_bits());
    assert_eq!(r.faces[1].vertices[0].get(), 0);
    assert_eq!(r.faces[1].normal, Normal::new([0., 0., 1.]));
    assert_eq!(r.faces[2].normal, Normal::new([2., 3., 4.]));
}
#[test]
fn obj_global_indices_fans_and_names() {
    let (g, w) = obj(
        "# first\n\n# second\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\no empty\nv 9 9 9\no  名 # keep\nf -4/a -3//bad -2 -5\no again\nf 4 2 1\0\0",
    );
    assert!(w.is_empty());
    assert_eq!(
        g.meta().as_ref().unwrap().comment.as_deref(),
        Some("first\nsecond")
    );
    let ms = leaves(&g);
    assert_eq!(ms.len(), 2);
    assert_eq!(
        ms[0].meta.as_ref().unwrap().name.as_deref(),
        Some("名 # keep")
    );
    assert_eq!(ms[0].faces.len(), 2);
    assert_eq!(ms[0].faces[0].vertices.map(|v| v.get()), [0, 1, 2]);
    assert_eq!(ms[0].faces[1].vertices.map(|v| v.get()), [0, 2, 3]);
    assert_eq!(ms[0].vertices[0], Vertex::new([1., 0., 0.]));
    assert_eq!(ms[1].vertices.len(), 3);
    assert!(scene_shared_vertices_warning(&g).is_some());
}
#[test]
fn obj_preserves_distinct_global_ids() {
    let (g, _) = obj("v 0 0 0\nv 0 0 0\nv 0 0 0\nf 1 2 3");
    assert_eq!(leaves(&g)[0].vertices.len(), 3);
    assert_eq!(leaves(&g)[0].faces[0].normal, Normal::new([0.; 3]));
}
#[test]
fn obj_w_and_arity_and_index_errors() {
    for (s, kind) in [
        ("v 1 2", 0),
        ("v x 2 3", 1),
        ("v NaN 0 0", 2),
        ("v 1 2 3 0", 3),
        ("f x", 4),
        ("f 0 1 2", 5),
        ("f -9223372036854775808 1 2", 6),
        ("f 1/a 2 3", 7),
        ("V 0 0 0", 8),
    ] {
        let e = read_obj(&mut Cursor::new(s)).unwrap_err();
        assert!(match kind {
            0 => matches!(e, LoadObjError::VertexCoordCountInvalid { .. }),
            1 => matches!(e, LoadObjError::InvalidFloat { .. }),
            2 => matches!(e, LoadObjError::NonFiniteFloat { .. }),
            3 => matches!(e, LoadObjError::VertexWZero { .. }),
            4 => matches!(e, LoadObjError::FaceTooFewVertices { .. }),
            5 => matches!(e, LoadObjError::FaceIndexZero { .. }),
            6 => matches!(e, LoadObjError::NegativeFaceIndexOutOfRange { .. }),
            7 => matches!(e, LoadObjError::FaceIndexOutOfRange { .. }),
            _ => matches!(e, LoadObjError::UnrecognizedDirective { .. }),
        });
    }
    let (g, _) = obj("v 2 4 6 2\nv 0 0 0\nv 1 1 1\nf 1 2 3");
    assert_eq!(leaves(&g)[0].vertices[0], Vertex::new([1., 2., 3.]));
}
#[test]
fn obj_warnings_and_empty_sections() {
    let (g, w) = obj(
        "vt ignored\nvn ignored\ng group name\ns off\nusemtl mat\nmtllib path\nl 1 2\no empty\n# comment\n",
    );
    assert!(leaves(&g).is_empty());
    assert_eq!(w.len(), 10);
    assert!(matches!(
        w[0],
        LoadObjWarning::SkippedTextureCoordinates { line: 1 }
    ));
    assert!(matches!(w[8], LoadObjWarning::NoVertices));
    assert!(matches!(w[9], LoadObjWarning::NoFaces));
}
#[test]
fn obj_errors_before_later_utf8() {
    assert!(matches!(
        read_obj(&mut Cursor::new(b"v nope\n\xff")),
        Err(LoadObjError::VertexCoordCountInvalid { line: 1, .. })
    ));
}
#[test]
fn reader_current_positions() {
    let mut b = Vec::from(b"prefix".as_slice());
    write_stlb(&mesh(), &mut b).unwrap();
    let mut c = Cursor::new(b);
    c.set_position(6);
    let (g, _) = read_stl(&mut c).unwrap();
    assert_eq!(leaves(&g)[0].faces.len(), 1);
    assert_eq!(c.position(), c.get_ref().len() as u64);
    let mut c = Cursor::new(b"prefixv 0 0 0\nf 1 1 1");
    c.set_position(6);
    assert_eq!(leaves(&read_obj(&mut c).unwrap().0).len(), 1);
}
#[test]
fn writer_progress_does_not_change_bytes() {
    let m = mesh();
    for binary in [false, true] {
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut ticks = Vec::new();
        if binary {
            write_stlb(&m, &mut a).unwrap();
            write_stlb_with_progress(&m, &mut b, &mut |p| ticks.push(p)).unwrap();
        } else {
            write_stla(&m, &mut a).unwrap();
            write_stla_with_progress(&m, &mut b, &mut |p| ticks.push(p)).unwrap();
        }
        assert_eq!(a, b);
        assert_eq!(
            ticks,
            vec![
                WriteSceneProgress::Serialize { done: 0, total: 1 },
                WriteSceneProgress::Serialize { done: 1, total: 1 }
            ]
        );
    }
    let g: Scene = m.into();
    let mut a = Vec::new();
    let mut b = Vec::new();
    write_obj(&g, &mut a).unwrap();
    let mut ticks = Vec::new();
    write_obj_with_progress(&g, &mut b, &mut |p| ticks.push(p)).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        ticks.last(),
        Some(&WriteSceneProgress::Serialize { done: 4, total: 4 })
    );
    write_threemf(&g, &mut a).unwrap();
    let mut p = Vec::new();
    write_threemf_with_progress(&g, &mut b, &mut |v| p.push(v)).unwrap();
    assert_eq!(a, b);
    assert!(
        p.iter()
            .any(|p| matches!(p, WriteSceneProgress::Compress { .. }))
    );
}
#[test]
fn three_mf_fixture_counts_sharing_and_round_trips() {
    for (file, nv, nt, n) in [
        ("box", 8, 12, 1),
        ("cylinder", 46, 88, 1),
        ("multiple_cylinders", 46, 88, 6),
        ("torus", 1100, 2200, 1),
    ] {
        let (g, w) = read_threemf(&mut fixture(&format!("3mf-samples/{file}.3mf"))).unwrap();
        assert!(
            w.iter()
                .all(|w| matches!(w, LoadThreeMfWarning::MaterialsDropped { .. })),
            "{file}: {w:?}"
        );
        let ms = leaves(&g);
        assert_eq!(ms.len(), n);
        assert_eq!((ms[0].vertices.len(), ms[0].faces.len()), (nv, nt));
        assert_eq!(g.mesh_resources_iter().count(), 1);
        let mut bytes = Vec::new();
        write_threemf(&g, &mut bytes).unwrap();
        let (r, _) = read_threemf(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(leaves(&r).len(), n);
        assert_eq!(r.mesh_resources_iter().count(), 1);
        assert_eq!(leaves(&r)[0].vertices, ms[0].vertices);
    }
}
#[test]
fn components_fixture_exports() {
    let (g, _) = read_threemf(&mut fixture("3mf-test-suites/P_XXX_0910_02.3mf")).unwrap();
    let ms = leaves(&g);
    assert_eq!(ms.len(), 5);
    assert_eq!((ms[0].vertices.len(), ms[0].faces.len()), (3664, 7384));
    assert_eq!(g.mesh_resources_iter().count(), 1);
    let flat = flatten_graph_to_mesh(&g).unwrap();
    assert_eq!(flat.mesh.faces.len(), 36920);
    assert_eq!(
        flat.warnings,
        vec![FlattenWarning::MultiObjectMerge { object_count: 5 }]
    );
    let mut b = Vec::new();
    write_obj(&g, &mut b).unwrap();
    let (r, _) = read_obj(&mut Cursor::new(b)).unwrap();
    assert_eq!(leaves(&r).len(), 5);
    let d = scene_to_model_document(&g).unwrap();
    assert_eq!(
        d.objects
            .iter()
            .filter(|o| matches!(o.payload, sc_3mf::model::ObjectPayload::Mesh(_)))
            .count(),
        1
    );
    assert!(scene_shared_vertices_warning(&g).is_some());
}
#[test]
fn sliced_full_resolution_fixture() {
    let (g, w) = read_threemf(&mut fixture("3mf-samples/box_sliced.3mf")).unwrap();
    assert_eq!(leaves(&g)[0].faces.len(), 12);
    assert!(
        w.iter()
            .any(|w| matches!(w, LoadThreeMfWarning::SliceDataIgnored { stack_count: 1 }))
    );
}
#[test]
fn shared_vertex_warning_ignores_world_transform() {
    let mut g = Scene::new();
    let m = Arc::new(mesh());
    for x in [0., 20.] {
        let n = g
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([x, 0., 0.])),
                LeafPayload::Mesh(m.clone()),
            )
            .unwrap();
        g.add_root(n).unwrap();
    }
    let SceneLoadWarning::SharedVertices { leaves } = scene_shared_vertices_warning(&g).unwrap();
    assert_eq!(leaves.len(), 2);
    assert!(leaves[0].node_id < leaves[1].node_id);
}
#[test]
fn names_and_obj_exact_output() {
    assert_eq!(
        crate::clean_name(" Küche\t深圳 👑 a\0b \u{fe0f}", true).as_deref(),
        Some("Kuche ShenZhen :crown: ab")
    );
    let mut g: Scene = mesh().into();
    g.set_meta(Some(SceneMetadata {
        name: Some("ignored".into()),
        comment: Some("one\ntwo\n".into()),
    }));
    let mut b = Vec::new();
    write_obj(&g, &mut b).unwrap();
    assert_eq!(
        String::from_utf8(b).unwrap(),
        "# one\n# two\n# \no Kuche\nv -0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n"
    );
}
#[test]
fn reflection_export_corrects_winding() {
    let mut g: Scene = mesh().into();
    let id = g.roots()[0];
    g.set_local_transform(
        id,
        Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([-1., 1., 1.])),
    )
    .unwrap();
    let f = flatten_graph_to_mesh(&g).unwrap();
    assert_eq!(f.mesh.faces[0].vertices.map(|i| i.get()), [0, 2, 1]);
}
#[test]
fn atomic_replace_and_failure_preserves_existing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.stl");
    std::fs::write(&p, b"original").unwrap();
    assert!(write_scene(&Scene::new(), &p, WriteFormat::StlBin).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    write_bytes_atomically(&p, b"new").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"new");
    assert!(matches!(
        write_bytes_atomically(&dir.path().join("missing/x"), b""),
        Err(WriteSceneError::CreateTempFile { .. })
    ));
}
#[test]
fn path_cap_precedes_inference() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file.unknown");
    std::fs::write(&p, b"12").unwrap();
    assert!(matches!(
        load_mesh_from_path(&p, None, Some(1)),
        Err(LoadMeshError::FileTooLarge {
            actual_bytes: 2,
            limit_bytes: 1
        })
    ));
    assert!(matches!(
        load_mesh_from_path(&p, None, Some(2)),
        Err(LoadMeshError::InferType(_))
    ));
}
#[test]
fn vocabulary() {
    for (s, t) in [
        ("sTl", MeshFileType::Stl),
        ("STL_ASCII", MeshFileType::StlAscii),
        ("STLB", MeshFileType::StlBin),
        ("3mf", MeshFileType::_3mf),
    ] {
        assert_eq!(s.parse(), Ok(t));
    }
    assert!(" stl".parse::<MeshFileType>().is_err());
    assert_eq!(
        infer_mesh_file_format_from_extension(Path::new("x.STLA")).unwrap(),
        MeshFileType::StlAscii
    );
    assert!(WriteFormat::try_from(MeshFileType::Ply).is_err());
}
#[test]
fn previews_background_and_geometry() {
    let m = mesh();
    assert_eq!(
        crate::preview::render_mesh(&m, 0, 10, [1, 2, 3]),
        Vec::<u8>::new()
    );
    let blank = IndexedMesh::new(None, Vec::new(), Vec::new());
    assert_eq!(
        crate::preview::render_mesh(&blank, 2, 2, [1, 2, 3]),
        [1, 2, 3].repeat(4)
    );
    let p = crate::preview::render_mesh(&m, 30, 30, [1, 2, 3]);
    assert_eq!(p.len(), 2700);
    assert!(p.as_chunks::<3>().0.iter().any(|p| *p != [1, 2, 3]));
}
#[test]
fn io_failures_surface() {
    struct Fail;
    impl Write for Fail {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("injected"))
        }
    }
    assert!(write_stlb(&mesh(), &mut Fail).is_err());
    assert!(write_obj(&mesh().into(), &mut Fail).is_err());
    assert!(write_threemf(&mesh().into(), &mut Fail).is_err());
}

#[test]
fn loader_explicit_format_and_missing_path_contract() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.unknown");
    std::fs::write(&path, b"solid empty\nendsolid empty\n").unwrap();
    assert!(matches!(
        load_mesh_from_path(&path, None, Some(1)),
        Err(LoadMeshError::FileTooLarge { limit_bytes: 1, .. })
    ));
    assert!(
        load_mesh_from_path(
            &path,
            Some(MeshFileType::StlAscii),
            Some(std::fs::metadata(&path).unwrap().len())
        )
        .is_ok()
    );
    let loaded = load_mesh_from_path(&path, Some(MeshFileType::StlAscii), None).unwrap();
    assert_eq!(loaded.graph.roots().len(), 1);
    assert!(matches!(
        load_mesh_from_path(&path, None, None),
        Err(LoadMeshError::InferType(_))
    ));
    assert!(load_mesh_from_path(&dir.path().join("missing.stl"), None, Some(0)).is_err());
    let mut bytes = Cursor::new(Vec::<u8>::new());
    assert!(matches!(
        load_mesh_from_reader(MeshFileType::Ply, &mut bytes),
        Err(LoadMeshError::UnsupportedFormat(MeshFileType::Ply))
    ));
}

#[test]
fn production_build_items_load_objects_from_other_parts() {
    // A build item with p:path, as multi-part projects write them: the object
    // lives in another model part.
    let root = "<resources/><build><item objectid=\"1\" p:path=\"/3D/Objects/object_1.model\" transform=\"1 0 0 0 1 0 0 0 1 5 0 0\"/></build>";
    let part = "<resources><object id=\"1\" name=\"part\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object></resources><build/>";
    let p = model_package(root, &[("/3D/Objects/object_1.model", part)]);
    let (g, _) = read_threemf_from_package(&p).unwrap();
    assert_eq!(leaves(&g).len(), 1);
    assert_eq!(leaves(&g)[0].vertices.len(), 3);
    // A path to a part that is not there, or an object it does not hold.
    for (path, id) in [("/3D/missing.model", 1), ("/3D/Objects/object_1.model", 9)] {
        let root = format!("<resources/><build><item objectid=\"{id}\" p:path=\"{path}\"/></build>");
        let p = model_package(&root, &[("/3D/Objects/object_1.model", part)]);
        assert!(matches!(read_threemf_from_package(&p), Err(LoadThreeMfError::ProductionReference { .. })));
    }
}

#[test]
fn a_model_requiring_materials_loads_its_geometry_and_warns() {
    use sc_3mf::{model::*, names::*};
    let body = "<resources><m:colorgroup id=\"5\"><m:color color=\"#FF0000\"/></m:colorgroup><object id=\"1\" pid=\"5\" pindex=\"0\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object></resources><build><item objectid=\"1\"/></build>";
    let model = format!("<model xmlns=\"{CORE_NS}\" xmlns:m=\"{MATERIALS_NS}\" requiredextensions=\"m\">{body}</model>");
    let parsed = parse_model(model.as_bytes()).unwrap();
    assert!(parsed.materials_extension_required);
    let mut package = sc_3mf::package::Package::new();
    let name = sc_3mf::package::PartName::new(DEFAULT_MODEL_PART_NAME).unwrap();
    package.add_part(name, MODEL_PART_CONTENT_TYPE, model.into_bytes()).unwrap();
    package.add_root_relationship(sc_3mf::package::Relationship {
        id: "m".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: DEFAULT_MODEL_PART_NAME.into(),
        external: false,
    });
    let (g, w) = read_threemf_from_package(&package).unwrap();
    assert_eq!(leaves(&g).len(), 1);
    assert!(w.contains(&LoadThreeMfWarning::MaterialsExtensionIgnored));
}

#[test]
fn another_parts_unit_is_ignored_for_the_roots() {
    // Conformance P_XPM_0521: a non-root part saying "inch" is read in the
    // root model's unit (millimetres here), not refused and not rescaled.
    use sc_3mf::{names::*, package::*};
    let mesh = "<object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object>";
    let root = format!("<model xmlns=\"{CORE_NS}\" xmlns:p=\"{PRODUCTION_NS}\" unit=\"millimeter\"><resources/><build><item objectid=\"1\" p:path=\"/3D/other.model\"/></build></model>");
    let other = format!("<model xmlns=\"{CORE_NS}\" unit=\"inch\"><resources>{mesh}</resources><build/></model>");
    let mut package = Package::new();
    for (path, xml) in [(DEFAULT_MODEL_PART_NAME, root), ("/3D/other.model", other)] {
        package.add_part(PartName::new(path).unwrap(), MODEL_PART_CONTENT_TYPE, xml.into_bytes()).unwrap();
    }
    package.add_root_relationship(Relationship {
        id: "m".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: DEFAULT_MODEL_PART_NAME.into(),
        external: false,
    });
    let (g, _) = read_threemf_from_package(&package).unwrap();
    assert_eq!(leaves(&g)[0].vertices[1].coords(), [1.0, 0.0, 0.0]);
}
