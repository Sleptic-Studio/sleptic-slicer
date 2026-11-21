// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::*;
use sc_3mf::{
    model::*,
    package::{Package, PartName, Relationship},
};
use sc_mesh_core::{MeshMetadata, transform::Transform3D};
use sc_scene::{CsgPolicy, LeafPayload, ReferencePolicy, Scene, SceneMetadata};
use sc_sl1cer::{contour::*, geom2d::Point2D};
use std::{io::Cursor, sync::Arc};
const MODEL: &str = "/3D/3dmodel.model";
const PROJECT: &str = "/Metadata/sleptic-project.toml";
const OLD_PROJECT: &str = "/Metadata/soulcrafted-project.toml";
fn part(s: &str) -> PartName {
    PartName::new(s).unwrap()
}
fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut p = Package::new();
    p.add_part(
        part(MODEL),
        "application/vnd.ms-package.3dmanufacturing-3dmodel+xml",
        include_bytes!("threemf/fixtures/tetrahedron.model").to_vec(),
    )
    .unwrap();
    p.add_root_relationship(Relationship { external: false,
        id: "start".into(),
        rel_type: "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel".into(),
        target: MODEL.into(),
    });
    for &(name, bytes) in parts {
        p.add_part(
            part(name),
            if name.ends_with(".model") {
                "application/vnd.ms-package.3dmanufacturing-3dmodel+xml"
            } else {
                "application/toml"
            },
            bytes.to_vec(),
        )
        .unwrap();
    }
    let mut out = Cursor::new(vec![]);
    p.write_to(&mut out).unwrap();
    out.into_inner()
}
fn load(b: Vec<u8>) -> LoadedProject {
    read_project(&mut Cursor::new(b), ProjectFileType::ThreeMf).unwrap()
}
fn scene() -> Scene {
    load(package(&[])).scene
}
fn leaves(s: &Scene) -> Vec<sc_scene::NodeId> {
    s.reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Refuse)
        .unwrap()
}
fn save(s: &Scene, p: &ProjectPayload, slices: Option<&PlateSlices>, fp: Option<&str>) -> Vec<u8> {
    let mut v = vec![];
    write_project(
        s,
        p,
        EmbedProfiles::Yes,
        slices,
        fp,
        ProjectFileType::ThreeMf,
        &mut v,
    )
    .unwrap();
    v
}
fn payload() -> ProjectPayload {
    ProjectPayload {
        printer: Some("test-printer".parse().unwrap()),
        resin: Some("test-resin".parse().unwrap()),
        first_layer_height: Some(sc_print_primitives::PositiveFiniteMm::new(0.07).unwrap()),
        process_overrides: Some(" opaque = \"value\"\n".into()),
        aa_level: Some(sc_print_primitives::AaLevel::X4),
        embedded: vec![
            EmbeddedProfile { profile: NamedProfile::Printer("test-printer".parse().unwrap()), extension: false, bytes: include_bytes!("../tests/fixtures/test-printer.toml").to_vec() },
            EmbeddedProfile { profile: NamedProfile::Resin("test-resin".parse().unwrap()), extension: false, bytes: include_bytes!("../tests/fixtures/test-resin.toml").to_vec() },
            EmbeddedProfile { profile: NamedProfile::Resin("test-resin".parse().unwrap()), extension: true, bytes: include_bytes!("../tests/fixtures/test-resin-extension.toml").to_vec() },
        ],
    }
}
fn slices() -> PlateSlices {
    let points = vec![
        Point2D::new(0.0, 0.0),
        Point2D::new(1.0, 0.0),
        Point2D::new(1.0, 1.0),
        Point2D::new(0.0, 1.0),
    ];
    let p = Polygon::new(ClosedRing::new(points), vec![]).unwrap();
    PlateSlices {
        contours: ContourStack::new(vec![
            ContourLayer::new(0.025, vec![p]),
            ContourLayer::new(0.075, vec![]),
        ])
        .unwrap(),
        slab_tops: vec![0.05, 0.1],
    }
}
#[test]
fn retained_payload_fixtures_and_plain_model() {
    let plain = load(package(&[]));
    assert!(plain.payload.is_none());
    assert!(plain.slices.is_none());
    assert_eq!(leaves(&plain.scene).len(), 1);
    for name in [
        include_bytes!("threemf/fixtures/payload-v1-full.toml").as_slice(),
        include_bytes!("threemf/fixtures/payload-v1-unknown-keys.toml").as_slice(),
    ] {
        let p = load(package(&[(PROJECT, name)])).payload.unwrap();
        assert_eq!(p.printer.unwrap().as_str(), "test-printer");
        assert_eq!(p.resin.unwrap().as_str(), "test-resin");
    }
    let p = load(package(&[(
        PROJECT,
        include_bytes!("threemf/fixtures/payload-v2.toml"),
    )]));
    assert!(p.payload.is_none());
    assert!(matches!(
        p.warnings.last(),
        Some(ReadProjectWarning::NewerProjectData { version: 2 })
    ));
}
#[test]
fn settings_validation_and_unsupported_version_short_circuit() {
    for s in [
        "version=1\n[profiles]\nprinter='BAD'",
        "version=1\n[print]\nfirst_layer_height_mm=0",
        "version=1\n[print]\naa_level='X4'",
        "version='1'",
        "[profiles]",
        "version = [",
        "version=1\n[print]\nprocess_overrides=2",
    ] {
        assert!(
            read_project(
                &mut Cursor::new(package(&[(PROJECT, s.as_bytes())])),
                ProjectFileType::ThreeMf
            )
            .is_err(),
            "{s}"
        );
    }
    assert!(
        read_project(
            &mut Cursor::new(package(&[(PROJECT, &[0xff])])),
            ProjectFileType::ThreeMf
        )
        .is_err()
    );
    for v in [-4, 2] {
        let s = format!("version={v}\n[profiles.printer]\nunrelated=[1,2]");
        let l = load(package(&[(PROJECT, s.as_bytes())]));
        assert!(l.payload.is_none());
        assert!(
            matches!(l.warnings.last(),Some(ReadProjectWarning::NewerProjectData{version}) if *version==v)
        );
    }
    let p = load(package(&[(PROJECT, b"version=1")]));
    assert_eq!(p.payload, Some(ProjectPayload::default()));
}
#[test]
fn complete_project_roundtrip_and_opc_contents() {
    let s = scene();
    let p = payload();
    let bytes = save(&s, &p, Some(&slices()), Some(" opaque \r\n fingerprint"));
    let loaded = load(bytes.clone());
    assert_eq!(loaded.payload, Some(p));
    assert_eq!(
        loaded.slice_fingerprint.as_deref(),
        Some(" opaque \r\n fingerprint")
    );
    let c = loaded.slices.unwrap();
    assert_eq!(c.layers().len(), 2);
    assert_eq!(c.layers()[0].z_mm(), 0.05);
    assert!(c.layers()[1].polygons().is_empty());
    let package = Package::read_from(Cursor::new(bytes)).unwrap();
    let names: Vec<_> = package.part_names().map(|n| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "/Metadata/thumbnail.png",
            MODEL,
            PROJECT,
            "/Metadata/sleptic-plates.toml",
            "/3D/slices.model",
            "/Metadata/profiles/printers/test-printer.toml",
            "/Metadata/profiles/materials/test-resin.toml",
            "/Metadata/profiles/materials/extensions/test-resin.toml"
        ]
    );
    assert_eq!(
        package.part_content_type(&part(PROJECT)),
        Some("application/vnd.sleptic.project+toml")
    );
    assert_eq!(
        package.part_content_type(&part("/Metadata/profiles/materials/test-resin.toml")),
        Some("application/toml")
    );
    let mut ids = std::collections::HashSet::new();
    for r in package.root_relationships() {
        assert!(ids.insert(&r.id));
    }
    let without = Package::read_from(Cursor::new(save(&s, &payload(), None, None))).unwrap();
    assert_eq!(
        package.part_data(&part(MODEL)),
        without.part_data(&part(MODEL))
    );
    assert!(
        !std::str::from_utf8(package.part_data(&part(MODEL)).unwrap())
            .unwrap()
            .contains("s:slicestack")
    );
}
#[test]
fn embedding_opaque_bytes_order_classification_and_duplicates() {
    let mut p = payload();
    p.embedded.push(EmbeddedProfile { profile: NamedProfile::Printer("unrelated-".parse().unwrap()), extension: false, bytes: vec![0xff, 0, 0xfe] });
    let bytes = save(&scene(), &p, None, None);
    assert_eq!(load(bytes).payload.unwrap(), p);
    let mut out = vec![];
    write_project(
        &scene(),
        &p,
        EmbedProfiles::No,
        None,
        None,
        ProjectFileType::ThreeMf,
        &mut out,
    )
    .unwrap();
    assert!(load(out).payload.unwrap().embedded.is_empty());
    p.embedded.push(p.embedded[0].clone());
    assert!(matches!(
        write_project(
            &scene(),
            &p,
            EmbedProfiles::Yes,
            None,
            None,
            ProjectFileType::ThreeMf,
            &mut vec![]
        ),
        Err(WriteProjectError::ThreeMf(
            error::WriteThreeMfProjectError::Package(
                sc_3mf::error::PackageWriteError::DuplicatePartName { .. }
            )
        ))
    ));
    let names = [
        "/Metadata/profiles/printers/a.toml",
        "/Metadata/profiles/materials/extensions/a.toml",
        "/Metadata/profiles/materials/a.toml",
        "/Metadata/profiles/printers/b.TOML",
        "/Metadata/profiles/printers/nested/a.toml",
        "/Metadata/profiles/materials/-a.toml",
        "/Metadata/profiles/materials/B.toml",
        "/Metadata/other.toml",
    ];
    let mut parts = vec![(PROJECT, b"version=1".as_slice())];
    parts.extend(names.iter().map(|&n| (n, b"opaque".as_slice())));
    let l = load(package(&parts));
    assert_eq!(l.payload.unwrap().embedded.len(), 3);
    assert_eq!(
        l.warnings
            .iter()
            .filter(|w| matches!(w, ReadProjectWarning::UnknownProfileFile { .. }))
            .count(),
        4
    );
    let l = load(package(&parts[1..]));
    assert!(l.warnings.is_empty());
}
#[test]
fn profile_reference_warning_semantics() {
    let mut p = payload();
    assert!(p.profile_ref_warnings(|_| true, |_| true).is_empty());
    assert!(matches!(
        &p.profile_ref_warnings(|_| false, |_| false)[..],
        [
            ProfileRefWarning { profile: NamedProfile::Printer(_), embedded: true },
            ProfileRefWarning { profile: NamedProfile::Resin(_), embedded: true }
        ]
    ));
    p.embedded
        .retain(|e| matches!(e, EmbeddedProfile { profile: NamedProfile::Resin(_), extension: true, .. }));
    assert!(matches!(
        &p.profile_ref_warnings(|_| false, |_| false)[..],
        [
            ProfileRefWarning { profile: NamedProfile::Printer(_), embedded: false },
            ProfileRefWarning { profile: NamedProfile::Resin(_), embedded: false }
        ]
    ));
    ProjectPayload::default().profile_ref_warnings(|_| panic!("absent"), |_| panic!("absent"));
}
#[test]
fn foreign_warning_order_and_unique_dialects() {
    let l = load(package(&[
        ("/Cura/a", b""),
        ("/Metadata/project_settings.config", b""),
        ("/Cura/b", b""),
        ("/Metadata/Slic3r_PE.config", b""),
        (PROJECT, b"version=99"),
    ]));
    assert_eq!(
        l.warnings,
        vec![
            ReadProjectWarning::OtherSlicersData {
                dialect: ForeignDialect::Cura
            },
            ReadProjectWarning::OtherSlicersData {
                dialect: ForeignDialect::BambuStudioOrca
            },
            ReadProjectWarning::OtherSlicersData {
                dialect: ForeignDialect::PrusaSlicer
            },
            ReadProjectWarning::NewerProjectData { version: 99 }
        ]
    );
}
#[test]
fn ordinary_hollow_paint_support_and_plates_roundtrip() {
    let mut s = scene();
    let model = leaves(&s)[0];
    s.set_node_meta(
        model,
        Some(SceneMetadata {
            name: Some(" Full name & < > ".into()),
            comment: Some("notes\nsc-support-paint 1 2 3".into()),
        }),
    )
    .unwrap();
    let mesh = s.mesh_at_leaf(model).unwrap().clone();
    let hollow = "sleptic-hollow-v1\n{ \"opaque\": true }\n";
    let mut m = mesh.clone();
    m.meta = Some(MeshMetadata {
        name: Some("mesh".into()),
        header_comment: Some(hollow.into()),
    });
    let h = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(m)))
        .unwrap();
    s.add_root(h).unwrap();
    let comment = format!(
        "sc-generated-support\nposition = [1.0, 2.0, 3.0]\nnormal = [0.0, 0.0, 1.0]\nowner = {}\nlift_mm = 2.5\nsmall = false\ngeometry_stale = true\nowner_linear = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]",
        model.get()
    );
    let support = s
        .add_leaf(
            Some(SceneMetadata {
                name: Some("contact".into()),
                comment: Some(comment),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    s.add_root(support).unwrap();
    let order = vec![
        sc_scene::plates::Plate {
            id: 7,
            locked: true,
            settings: Some(" raw\n setting ".into()),
        },
        sc_scene::plates::Plate {
            id: 2,
            locked: false,
            settings: None,
        },
    ];
    let plates = sc_scene::plates::Plates::from_parts(
        order.clone(),
        [(model, 7), (h, 2), (support, 7)].into_iter().collect(),
    )
    .unwrap();
    assert!(s.set_plates(plates));
    let loaded = load(save(&s, &ProjectPayload::default(), None, None));
    let ids = leaves(&loaded.scene);
    assert_eq!(ids.len(), 3);
    assert_eq!(
        loaded.scene.node(ids[0]).unwrap().meta,
        s.node(model).unwrap().meta
    );
    assert_eq!(
        loaded
            .scene
            .mesh_at_leaf(ids[1])
            .unwrap()
            .meta
            .as_ref()
            .unwrap()
            .header_comment
            .as_deref(),
        Some(hollow)
    );
    let c = loaded
        .scene
        .node(ids[2])
        .unwrap()
        .meta
        .as_ref()
        .unwrap()
        .comment
        .as_ref()
        .unwrap();
    assert!(c.contains(&format!("owner = {}", ids[0].get())));
    assert!(c.contains("small = false\ngeometry_stale = true\nowner_linear"));
    assert_eq!(loaded.scene.plates().order, order);
    assert_eq!(loaded.scene.plate_of(ids[0]), 7);
    assert_eq!(loaded.scene.plate_of(ids[1]), 2);
    assert_eq!(loaded.scene.plate_of(ids[2]), 7);
}
#[test]
fn malformed_optional_metadata_and_plate_validation_are_silent() {
    for name in [
        "/Metadata/sleptic-objects.toml",
        "/Metadata/sleptic-supports.toml",
        "/Metadata/soulcrafted-supports.toml",
        "/Metadata/sleptic-hollow.toml",
        "/Metadata/sleptic-plates.toml",
    ] {
        for bytes in [b"bad = [".as_slice(), &[0xff], b"version=8"] {
            assert!(load(package(&[(name, bytes)])).warnings.is_empty());
        }
    }
    for text in [
        "version=1\nplates=[]\nassignments=[1]",
        "version=1\nplates=[{id=0,locked=false}]\nassignments=[0]",
        "version=1\nplates=[{id=4294967295,locked=false}]\nassignments=[4294967295]",
        "version=1\nplates=[{id=2,locked=false},{id=2,locked=true}]\nassignments=[2]",
        "version=1\nplates=[{id=2,locked=false}]\nassignments=[]",
        "version=1\nplates=[{id=2,locked=false}]\nassignments=[3]",
    ] {
        let l = load(package(&[(
            "/Metadata/sleptic-plates.toml",
            text.as_bytes(),
        )]));
        assert_eq!(l.scene.plates().order[0].id, 1);
        assert_eq!(l.scene.plates().order.len(), 1);
    }
}
#[test]
fn legacy_contacts_and_raft_protocol() {
    for name in [
        "/Metadata/sleptic-supports.toml",
        "/Metadata/soulcrafted-supports.toml",
    ] {
        contacts_and_raft_under(name);
    }
}
fn contacts_and_raft_under(name: &str) {
    let mut s = scene();
    let mesh = s.mesh_at_leaf(leaves(&s)[0]).unwrap().clone();
    for _ in 0..2 {
        let id = s
            .add_leaf(
                None,
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(mesh.clone())),
            )
            .unwrap();
        s.add_root(id).unwrap();
    }
    let b = save(&s, &ProjectPayload::default(), None, None);
    let base = Package::read_from(Cursor::new(b)).unwrap();
    let support=b"version=1\n[[supports]]\nobject_name='legacy'\nleaf_index=1\nposition=[1.0,2.0,3.0]\nnormal=[0.0,0.0,1.0]\nowner=999\nlift_mm=-2\n[[supports]]\nobject_name='raft'\nleaf_index=2\nraft=true\n";
    let mut package = base;
    package
        .add_part(
            part(name),
            "application/toml",
            support.to_vec(),
        )
        .unwrap();
    let mut out = Cursor::new(vec![]);
    package.write_to(&mut out).unwrap();
    let l = load(out.into_inner());
    let ids = leaves(&l.scene);
    assert_eq!(
        l.scene
            .node(ids[1])
            .unwrap()
            .meta
            .as_ref()
            .unwrap()
            .comment
            .as_deref(),
        Some("sc-generated-support\nposition = [1.0, 2.0, 3.0]\nnormal = [0.0, 0.0, 1.0]")
    );
    assert_eq!(
        l.scene
            .node(ids[2])
            .unwrap()
            .meta
            .as_ref()
            .unwrap()
            .comment
            .as_deref(),
        Some(format!("sc-generated-support-raft\nowner = {}", ids[0].get()).as_str())
    );
}
#[test]
fn slice_mapping_dedup_signed_zero_and_independent_holes() {
    let outer = ClosedRing::new(vec![
        Point2D::new(0.0, 0.0),
        Point2D::new(0.0, 0.0),
        Point2D::new(2.0, 0.0),
        Point2D::new(2.0, 2.0),
        Point2D::new(0.0, 0.0),
    ]);
    let hole = ClosedRing::new(vec![
        Point2D::new(-0.0, 0.0),
        Point2D::new(1.0, 1.0),
        Point2D::new(1.0, 0.0),
    ]);
    let s = PlateSlices {
        contours: ContourStack::new(vec![ContourLayer::new(
            0.1,
            vec![Polygon::new_unchecked(outer, vec![hole])],
        )])
        .unwrap(),
        slab_tops: vec![0.2],
    };
    let m = slices::to_model(&s).unwrap();
    let SliceStackContent::Slices(ref slices) = m.slice_stacks[0].content else {
        panic!()
    };
    assert_eq!(slices[0].vertices.len(), 6);
    let c = slices::from_model(&m).unwrap();
    assert_eq!(c.layers()[0].polygons().len(), 2);
    assert_eq!(c.layers()[0].polygons()[0].outer().len(), 3);
    assert!(
        c.layers()[0]
            .polygons()
            .iter()
            .all(|p| p.holes().is_empty())
    );
    let mut bad = s.clone();
    bad.slab_tops.clear();
    assert!(matches!(
        slices::to_model(&bad),
        Err(threemf::SliceMappingError::LayerCountMismatch { layers: 1, tops: 0 })
    ));
    bad = s;
    bad.slab_tops[0] = 0.0;
    assert!(
        write_project(
            &scene(),
            &ProjectPayload::default(),
            EmbedProfiles::No,
            Some(&bad),
            None,
            ProjectFileType::ThreeMf,
            &mut vec![]
        )
        .is_err()
    );
}
#[test]
fn progress_is_ordered_fixed_and_does_not_change_bytes() {
    let s = scene();
    let p = payload();
    let slices = slices();
    let expected = save(&s, &p, Some(&slices), None);
    let mut bytes = vec![];
    let mut reports = vec![];
    write_project_with_progress(
        &s,
        &p,
        EmbedProfiles::Yes,
        Some(&slices),
        None,
        ProjectFileType::ThreeMf,
        &mut bytes,
        &mut |r| reports.push(r),
    )
    .unwrap();
    assert_eq!(bytes, expected);
    let split = reports
        .iter()
        .position(|r| matches!(r, WriteProgress::Compress { .. }))
        .unwrap();
    assert_eq!(split, 2);
    assert!(
        reports[split..]
            .iter()
            .all(|r| matches!(r, WriteProgress::Compress { .. }))
    );
    let totals: Vec<_> = reports[..split]
        .iter()
        .map(|r| match r {
            WriteProgress::Serialize { total, .. } => *total,
            _ => 0,
        })
        .collect();
    assert!(totals.iter().all(|t| *t == totals[0]));
    assert!(matches!(reports.last(),Some(WriteProgress::Compress{done,total}) if done==total));
    let mut read_progress = vec![];
    threemf::read_threemf_project_with_progress(&mut Cursor::new(bytes), |p| {
        read_progress.push((p.decompressed_bytes, p.total_bytes))
    })
    .unwrap();
    assert!(
        read_progress
            .windows(2)
            .all(|v| v[0].0 < v[1].0 && v[0].1 == v[1].1)
    );
    let (done, total) = read_progress.last().unwrap();
    assert_eq!(done, total);
}
#[test]
fn loader_extensions_size_and_completion() {
    let d = tempfile::tempdir().unwrap();
    for ext in ["3MF", "OBJ", "STL", "stla", "stlb"] {
        assert!(is_supported_scene_file_extension(
            &d.path().join(format!("a.{ext}"))
        ))
    }
    assert!(!is_supported_scene_file_extension(std::path::Path::new(
        "a.3mf.bak"
    )));
    assert!(matches!(
        load_scene(&d.path().join("absent.ply"), None, |_| {}),
        Err(LoadSceneError::UnsupportedExtension { .. })
    ));
    let p = d.path().join("a.3mf");
    let b = save(&scene(), &payload(), None, Some("fp"));
    std::fs::write(&p, &b).unwrap();
    assert!(matches!(
        load_scene(&p, Some(b.len() as u64 - 1), |_| {}),
        Err(LoadSceneError::FileTooLarge { .. })
    ));
    let mut progress = vec![];
    let l = load_scene(&p, Some(b.len() as u64), |p| progress.push(p)).unwrap();
    assert_eq!(l.slice_fingerprint.as_deref(), Some("fp"));
    let p = progress.last().unwrap();
    assert_eq!(p.done_bytes, p.total_bytes);
    let p = d.path().join("a.obj");
    std::fs::write(&p, b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    let mut progress = vec![];
    let l = load_scene(&p, None, |p| progress.push(p)).unwrap();
    assert!(l.payload.is_none());
    assert!(l.project_warnings.is_empty());
    let p = progress.last().unwrap();
    assert_eq!(p.done_bytes, p.total_bytes);
}
#[test]
fn fingerprint_and_slices_are_independent() {
    let s = scene();
    let p = ProjectPayload::default();
    let l = load(save(&s, &p, None, Some("fp")));
    assert!(l.slices.is_none());
    assert_eq!(l.slice_fingerprint.as_deref(), Some("fp"));
    let l = load(save(&s, &p, Some(&slices()), None));
    assert!(l.slices.is_some());
    assert!(l.slice_fingerprint.is_none());
    let doc = slices::to_model(&slices()).unwrap();
    let mut bytes = vec![];
    write_model(&doc, &mut bytes).unwrap();
    let l = load(package(&[("/3D/slices.model", &bytes)]));
    assert!(l.payload.is_none());
    assert!(l.slices.is_some());
}

#[test]
fn large_persisted_slices_report_intermediate_serialization_and_keep_scene_xml_independent() {
    let scene = scene();
    let one = slices();
    let n = 4000;
    let contours = ContourStack::new(
        (0..n)
            .map(|i| {
                ContourLayer::new(
                    i as f64 * 0.05 + 0.025,
                    one.contours.layers()[0].polygons().to_vec(),
                )
            })
            .collect(),
    )
    .unwrap();
    let slices = PlateSlices {
        contours,
        slab_tops: (0..n).map(|i| (i + 1) as f64 * 0.05).collect(),
    };
    let mut out = vec![];
    let mut progress = vec![];
    write_project_with_progress(
        &scene,
        &ProjectPayload::default(),
        EmbedProfiles::No,
        Some(&slices),
        None,
        ProjectFileType::ThreeMf,
        &mut out,
        &mut |p| progress.push(p),
    )
    .unwrap();
    assert!(
        progress
            .iter()
            .filter(|p| matches!(p, WriteProgress::Serialize { .. }))
            .count()
            > 3
    );
    let loaded = load(out);
    assert_eq!(loaded.slices.unwrap().layers().len(), n);
}
#[test]
fn repeated_subtrees_keep_geometry_sharing_and_expanded_metadata() {
    let mut s = Scene::new();
    let mesh = s.add_mesh_resource(sc_mesh_fixtures::unit_cube_mesh());
    let root = s.add_group(None, Transform3D::Identity);
    let leaf = s
        .add_leaf(
            Some(SceneMetadata {
                name: Some("exact leaf".into()),
                comment: Some("opaque note".into()),
            }),
            Transform3D::Identity,
            LeafPayload::MeshResource(mesh),
        )
        .unwrap();
    s.add_child(root, leaf).unwrap();
    let resource = s.register_subtree_resource(root).unwrap();
    for _ in 0..3 {
        let id = s
            .add_reference(None, Transform3D::Identity, resource)
            .unwrap();
        s.add_root(id).unwrap();
    }
    let b = save(&s, &ProjectPayload::default(), None, None);
    let p = Package::read_from(Cursor::new(b.clone())).unwrap();
    let model = parse_model(p.part_data(&part(MODEL)).unwrap()).unwrap();
    assert_eq!(
        model
            .objects
            .iter()
            .filter(|o| matches!(o.payload, ObjectPayload::Mesh(_)))
            .count(),
        1
    );
    let loaded = load(b);
    let ids = leaves(&loaded.scene);
    assert_eq!(ids.len(), 3);
    for id in ids {
        assert_eq!(
            loaded
                .scene
                .node(id)
                .unwrap()
                .meta
                .as_ref()
                .unwrap()
                .comment
                .as_deref(),
            Some("opaque note")
        );
    }
}
#[test]
fn bad_slice_parts_are_ignored_and_mapping_errors_are_exact() {
    for bytes in [b"not xml".as_slice(), b"<model/>"] {
        let l = load(package(&[("/3D/slices.model", bytes)]));
        assert!(l.slices.is_none());
    }
    let mut model = slices::to_model(&slices()).unwrap();
    let SliceStackContent::Slices(ref mut slices) = model.slice_stacks[0].content else {
        panic!()
    };
    slices[0].polygons[0].start = ResourceIndex::new(99).unwrap();
    assert_eq!(
        slices::from_model(&model).unwrap_err().to_string(),
        "a slice polygon referenced vertex index 99, which is out of range"
    );
    let mut model = ModelDocument {
        slice_stacks: vec![SliceStack {
            id: ResourceId::MIN,
            z_bottom: 0.0,
            content: SliceStackContent::Slices(vec![Slice {
                z_top: f64::NAN,
                vertices: vec![],
                polygons: vec![],
            }]),
        }],
        ..Default::default()
    };
    assert_eq!(
        slices::from_model(&model).unwrap_err().to_string(),
        "a reconstructed slice layer has a non-finite Z"
    );
    model.slice_stacks[0].content = SliceStackContent::Refs(vec![]);
    assert_eq!(
        slices::from_model(&model).unwrap_err().to_string(),
        "the slice stack uses slicerefs to other parts, which are not supported"
    );
}

/// Reproducible new-writer baseline; compare external measurements on the same machine.
#[test]
#[ignore]
fn project_measurements() {
    let mut s = Scene::new();
    let resource = s.add_mesh_resource(sc_mesh_fixtures::unit_cube_mesh());
    for i in 0..1000 {
        let id = s
            .add_leaf(
                Some(SceneMetadata {
                    name: Some(format!("model {i}")),
                    comment: None,
                }),
                Transform3D::Identity,
                LeafPayload::MeshResource(resource),
            )
            .unwrap();
        s.add_root(id).unwrap();
    }
    let one = slices();
    let n = 10000;
    let data = PlateSlices {
        contours: ContourStack::new(
            (0..n)
                .map(|i| {
                    ContourLayer::new(
                        i as f64 * 0.05 + 0.025,
                        one.contours.layers()[0].polygons().to_vec(),
                    )
                })
                .collect(),
        )
        .unwrap(),
        slab_tops: (0..n).map(|i| (i + 1) as f64 * 0.05).collect(),
    };
    let start = std::time::Instant::now();
    let bytes = save(
        &s,
        &ProjectPayload::default(),
        Some(&data),
        Some("benchmark"),
    );
    println!(
        "project,shared-1000-slices-10000,write,{:.6},{}",
        start.elapsed().as_secs_f64(),
        bytes.len()
    );
    let start = std::time::Instant::now();
    let loaded = load(bytes);
    println!(
        "project,shared-1000-slices-10000,read,{:.6},{}",
        start.elapsed().as_secs_f64(),
        loaded.slices.unwrap().layers().len()
    );
}
#[test]
fn projects_saved_under_the_old_names_still_open() {
    let bytes = include_bytes!("threemf/fixtures/payload-v1-full.toml");
    let old = load(package(&[(OLD_PROJECT, bytes)])).payload.unwrap();
    assert_eq!(old.printer.unwrap().as_str(), "test-printer");
    assert_eq!(old.resin.unwrap().as_str(), "test-resin");
    // A file carrying both is read from the current name.
    let both = load(package(&[(PROJECT, b"version=99"), (OLD_PROJECT, bytes)]));
    assert!(both.payload.is_none());
}
#[test]
fn saved_projects_carry_no_old_project_name() {
    let mut s = scene();
    let id = leaves(&s)[0];
    s.set_node_meta(
        id,
        Some(SceneMetadata {
            name: Some("part".into()),
            comment: Some("sc-generated-support
position = [1.0, 2.0, 3.0]
normal = [0.0, 0.0, 1.0]".into()),
        }),
    )
    .unwrap();
    let package = Package::read_from(Cursor::new(save(&s, &payload(), None, None))).unwrap();
    let names: Vec<_> = package.part_names().map(|n| n.as_str().to_owned()).collect();
    assert!(names.iter().any(|n| n == PROJECT), "{names:?}");
    let mut text = names.join("
");
    for n in package.part_names() {
        text.push_str(package.part_content_type(n).unwrap_or_default());
        text.push_str(&String::from_utf8_lossy(package.part_data(n).unwrap_or_default()));
    }
    for r in package.root_relationships() {
        text.push_str(&r.rel_type);
        text.push_str(&r.target);
    }
    assert!(!text.to_lowercase().contains("soulcrafted"), "{text}");
}

#[test]
fn loader_size_limit_reports_exact_values_before_reading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("part.obj");
    let data = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    std::fs::write(&path, data).unwrap();
    let size = data.len() as u64;
    let mut calls = 0;
    match load_scene(&path, Some(size - 1), |_| calls += 1) {
        Err(LoadSceneError::FileTooLarge {
            actual_bytes,
            limit_bytes,
        }) => {
            assert_eq!((actual_bytes, limit_bytes), (size, size - 1));
        }
        other => panic!("unexpected result: {other:?}"),
    }
    assert_eq!(calls, 0);
    assert!(load_scene(&path, Some(size), |_| {}).is_ok());
    assert!(load_scene(&path, None, |_| {}).is_ok());
}

#[test]
fn project_writer_error_messages_cover_each_variant() {
    use crate::error::WriteThreeMfProjectError as E;
    use std::error::Error;
    fn io() -> std::io::Error { std::io::Error::other("disk") }
    let cases = [
        E::Scene(sc_mesh_formats::error::WriteThreeMfError::Io(io())),
        E::Model(sc_3mf::error::ModelWriteError::Io(io())),
        E::SliceMapping(threemf::SliceMappingError::NonFiniteLayerZ),
        E::Package(sc_3mf::error::PackageWriteError::Io(io())),
        E::Io(io()),
    ];
    for (index, error) in cases.iter().enumerate() {
        let prefix = if index == 4 { "3MF project writer I/O error: " } else { "3MF project writer: " };
        assert_eq!(error.to_string(), format!("{prefix}{}", error.source().unwrap()));
    }
    struct Refuse;
    impl serde::Serialize for Refuse {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("refused"))
        }
    }
    let error = E::Payload(toml_edit::ser::to_string(&Refuse).unwrap_err());
    assert_eq!(error.to_string(), format!("3MF project writer: failed to serialize the project part: {}", error.source().unwrap()));
}
