// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{error::*, model::*, names::*, package::*};
use std::io::Cursor;
#[test]
fn attribute_line_end_normalization_and_geometry_containers() {
    let d = parse("<metadata name=\"a\r\nb&#10;c\rd\te\">value</metadata><resources><object id=\"1\"><mesh><vertices><triangle/><vertex x=\"-0\" y=\"0\" z=\"1\"/></vertices><triangles><vertex/><triangle v1=\"0\" v2=\"0\" v3=\"0\"/></triangles></mesh></object></resources>").unwrap();
    assert_eq!(d.metadata[0].name, "a b\nc d e");
    let ObjectPayload::Mesh(m) = &d.objects[0].payload else {
        panic!()
    };
    assert_eq!(m.vertices.len(), 1);
    assert_eq!(m.triangles.len(), 1);
    assert_eq!(m.vertices[0].x.to_bits(), (-0.0f64).to_bits());
}
fn doc(body: &str) -> Vec<u8> {
    format!("<model xmlns=\"{CORE_NS}\" xmlns:s=\"{SLICE_NS}\" xmlns:p=\"{PRODUCTION_NS}\">{body}</model>").into_bytes()
}
fn parse(body: &str) -> Result<ModelDocument, ModelParseError> {
    parse_model(&doc(body))
}
fn fixture(name: &str) -> Package {
    Package::read_from(
        std::fs::File::open(format!(
            "{}/tests/fixtures/official/{name}.3mf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn vocabulary_boundaries() {
    assert!(ResourceId::new(0).is_none());
    assert!(ResourceId::new(2147483647).is_some());
    assert!(ResourceId::new(2147483648).is_none());
    assert!(ResourceIndex::new(0).is_some());
    assert!(ResourceIndex::new(2147483648).is_none());
    for (u, s, n) in [
        (Unit::Micron, "micron", 0.001),
        (Unit::Millimeter, "millimeter", 1.),
        (Unit::Centimeter, "centimeter", 10.),
        (Unit::Inch, "inch", 25.4),
        (Unit::Foot, "foot", 304.8),
        (Unit::Meter, "meter", 1000.),
    ] {
        assert_eq!(u.as_str(), s);
        assert_eq!(u.scale_to_mm(), n);
        assert_eq!(Unit::parse(s), Some(u));
    }
    assert!(Unit::parse(" inch").is_none());
    assert_eq!(
        Matrix3D::parse(" 1 0 0 0 1 0 0 0 1 +0 -0 1e-8 ").unwrap().0[11],
        1e-8
    );
    assert!(Matrix3D::parse("1 0 0").is_none());
    assert!(Matrix3D::parse("NaN 0 0 0 1 0 0 0 1 0 0 0").is_none());
}
#[test]
fn official_meshes_round_trip() {
    for (name, id, nv, nt, nb) in [
        ("box", 1, 8, 12, 1),
        ("cylinder", 1, 46, 88, 1),
        ("multiple_cylinders", 2, 46, 88, 6),
        ("torus", 1, 1100, 2200, 1),
    ] {
        let p = fixture(name);
        let d = parse_model(p.part_data(&p.start_part().unwrap()).unwrap()).unwrap();
        assert_eq!(d.unit, Unit::Millimeter);
        assert_eq!(d.objects.len(), 1);
        assert_eq!(d.objects[0].id.get(), id);
        let ObjectPayload::Mesh(m) = &d.objects[0].payload else {
            panic!()
        };
        assert_eq!(
            (m.vertices.len(), m.triangles.len(), d.build.len()),
            (nv, nt, nb)
        );
        if nb == 6 {
            assert_eq!(d.build[1].transform.unwrap().0[9], 21.)
        }
        let mut bytes = Vec::new();
        write_model(&d, &mut bytes).unwrap();
        assert_eq!(parse_model(&bytes).unwrap(), d);
    }
}
#[test]
fn sliced_fixture_round_trip() {
    let p = fixture("box_sliced");
    let d = parse_model(p.part_data(&p.start_part().unwrap()).unwrap()).unwrap();
    assert_eq!(d.objects[0].slice_stack.unwrap().get(), 1);
    let SliceStackContent::Refs(rs) = &d.slice_stacks[0].content else {
        panic!()
    };
    let name = PartName::new(&rs[0].slice_path).unwrap();
    let slices = parse_model(p.part_data(&name).unwrap()).unwrap();
    let SliceStackContent::Slices(ss) = &slices.slice_stacks[0].content else {
        panic!()
    };
    assert_eq!(ss.len(), 379);
    assert_eq!(ss[0].z_top, 0.08);
    assert_eq!(ss[0].vertices.len(), 4);
    assert!(ss[0].polygons[0].is_closed());
    assert_eq!(ss[378].z_top, 30.32);
    assert!(ss[378].vertices.is_empty());
    for d in [d, slices] {
        let mut b = Vec::new();
        write_model(&d, &mut b).unwrap();
        assert_eq!(parse_model(&b).unwrap(), d);
    }
}
#[test]
fn metadata_fixture() {
    let bytes = include_bytes!("../tests/fixtures/spec/metadata_example.model");
    let d = parse_model(bytes).unwrap();
    assert_eq!(d.metadata.len(), 9);
    assert_eq!(d.base_materials.len(), 1);
    assert_eq!(d.objects.len(), 2);
    assert!(matches!(d.objects[1].payload, ObjectPayload::Components(_)));
    let mut b = Vec::new();
    write_model(&d, &mut b).unwrap();
    assert_eq!(parse_model(&b).unwrap(), d);
}
#[test]
fn metadata_entities_and_normalization() {
    let d=parse("<metadata name=\"a&#9;b\t c\" preserve=\"true\" type=\"x\"> a&amp;<![CDATA[<b>]]><ignored>gone</ignored>&#10;z </metadata>").unwrap();
    assert_eq!(d.metadata[0].name, "a\tb  c");
    assert_eq!(d.metadata[0].value, " a&<b>\nz ");
    assert!(d.metadata[0].preserve);
    assert!(matches!(
        parse("<metadata name=\"x\">&no;</metadata>"),
        Err(ModelParseError::UndeclaredEntity { .. })
    ));
}
#[test]
fn roots_extensions_and_trailing_bytes() {
    assert!(matches!(
        parse_model(b"<model/>"),
        Err(ModelParseError::UnexpectedRootElement { .. })
    ));
    assert!(matches!(
        parse_model(b"<!DOCTYPE model><model/>"),
        Err(ModelParseError::DtdForbidden)
    ));
    assert!(parse_model(format!("<c:model xmlns:c=\"{CORE_NS}\"/>garbage").as_bytes()).is_ok());
    assert!(matches!(
        parse_model(format!("<model xmlns=\"{CORE_NS}\" requiredextensions=\"q\"/>").as_bytes()),
        Err(ModelParseError::UnknownRequiredExtensionPrefix { .. })
    ));
    assert!(matches!(
        parse_model(
            format!("<model xmlns=\"{CORE_NS}\" xmlns:q=\"urn:no\" requiredextensions=\"q\"/>")
                .as_bytes()
        ),
        Err(ModelParseError::UnsupportedRequiredExtension { .. })
    ));
}
#[test]
fn geometry_and_reference_validation() {
    for (body, kind) in [
        ("<resources><object id=\"1\"/></resources>", 0),
        (
            "<resources><object id=\"1\"><mesh/><components/></object></resources>",
            1,
        ),
        (
            "<resources><object id=\"1\"><components><component objectid=\"1\"/></components></object></resources>",
            2,
        ),
        (
            "<resources><object id=\"1\"><mesh><triangles><triangle v1=\"0\" v2=\"0\" v3=\"0\"/></triangles></mesh></object></resources>",
            3,
        ),
        (
            "<resources><object id=\"1\"><mesh/></object><object id=\"1\"><mesh/></object></resources>",
            4,
        ),
    ] {
        let e = parse(body).unwrap_err();
        assert!(match kind {
            0 => matches!(e, ModelParseError::ObjectWithoutGeometry { .. }),
            1 => matches!(e, ModelParseError::DuplicateGeometry { .. }),
            2 => matches!(e, ModelParseError::UnknownObjectReference { .. }),
            3 => matches!(e, ModelParseError::TriangleIndexOutOfRange { .. }),
            _ => matches!(e, ModelParseError::DuplicateResourceId { .. }),
        });
    }
}
#[test]
fn production_path_is_read_but_omitted() {
    let d=parse("<resources><object id=\"1\"><components><component objectid=\"99\" p:path=\"/other.model\"/></components></object></resources>").unwrap();
    let mut b = Vec::new();
    write_model(&d, &mut b).unwrap();
    assert!(!String::from_utf8(b).unwrap().contains("path="));
    // A build item may name its part too; the reader resolves it later.
    let item = parse("<build><item objectid=\"1\" p:path=\"/x\"/></build>").unwrap();
    assert_eq!(item.build[0].path.as_deref(), Some("/x"));
    // Anywhere else, p:path is not something this reader knows.
    assert!(matches!(
        parse("<resources><object id=\"1\" p:path=\"/x\"><mesh/></object></resources>"),
        Err(ModelParseError::ProductionPathUnsupported)
    ));
}
#[test]
fn exact_empty_xml() {
    let mut b = Vec::new();
    write_model(&ModelDocument::default(), &mut b).unwrap();
    assert_eq!(
        String::from_utf8(b).unwrap(),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><model unit=\"millimeter\" xmlns=\"{CORE_NS}\"><resources></resources><build></build></model>"
        )
    );
}
#[test]
fn bit_exact_coordinates() {
    for value in [
        -0.,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
        1.0000000000000002,
    ] {
        let mut d = parse("<resources><object id=\"1\"><mesh/></object></resources>").unwrap();
        if let ObjectPayload::Mesh(m) = &mut d.objects[0].payload {
            m.vertices.push(Vertex3 {
                x: value,
                y: 0.,
                z: 0.,
            });
        }
        let mut b = Vec::new();
        write_model(&d, &mut b).unwrap();
        let r = parse_model(&b).unwrap();
        let ObjectPayload::Mesh(out) = &r.objects[0].payload else {
            panic!()
        };
        assert_eq!(out.vertices[0].x.to_bits(), value.to_bits());
    }
}
#[test]
fn part_name_precedence() {
    for (s, e) in [
        ("", MalformedPartNameError::Empty),
        ("x", MalformedPartNameError::NotAbsolute),
        ("/", MalformedPartNameError::EmptySegment),
        ("/a//b.", MalformedPartNameError::EmptySegment),
        ("/a.", MalformedPartNameError::TrailingDot),
    ] {
        assert_eq!(PartName::new(s), Err(e));
    }
    for s in ["/a./b", "/../x", "/%20", "/A\\b"] {
        assert_eq!(PartName::new(s).unwrap().as_str(), s);
    }
}
#[test]
fn package_order_content_and_progress() {
    let mut p = Package::new();
    let n = PartName::new("/x.model").unwrap();
    p.add_part(n.clone(), MODEL_PART_CONTENT_TYPE, vec![b'x'; 600001])
        .unwrap();
    assert!(p.add_part(n.clone(), "x", vec![]).is_err());
    p.add_root_relationship(Relationship { external: false,
        id: "r".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: "x.model".into(),
    });
    let mut a = Cursor::new(Vec::new());
    let mut ticks = Vec::new();
    p.write_to_with_progress(&mut a, &mut |d, t| ticks.push((d, t)))
        .unwrap();
    assert_eq!(ticks.last(), Some(&(600001, 600001)));
    assert!(ticks.windows(2).all(|v| v[1].0 - v[0].0 <= 262144));
    let mut b = Cursor::new(Vec::new());
    p.write_to(&mut b).unwrap();
    assert_eq!(a.get_ref(), b.get_ref());
    a.set_position(0);
    let mut read = Vec::new();
    let q = Package::read_from_with_progress(a, &mut |v| read.push(v)).unwrap();
    assert_eq!(q.start_part().unwrap(), n);
    assert_eq!(q.part_data(&n).unwrap().len(), 600001);
    let last = read.last().unwrap();
    assert_eq!(last.decompressed_bytes, last.total_bytes);
}

#[test]
fn slice_validation_precedes_output_and_normalizes_planar_cells() {
    let mut d = parse("<resources><s:slicestack id=\"1\"><s:slice ztop=\"1\"><s:vertices><s:vertex x=\"0\" y=\"0\"/></s:vertices><s:polygon startv=\"0\"><s:segment v2=\"0\"/></s:polygon></s:slice></s:slicestack><object id=\"2\" s:slicestackid=\"1\"><mesh/></object><object id=\"3\"><components><component objectid=\"2\"/></components></object></resources><build><item objectid=\"3\"/></build>").unwrap();
    let mut out = Vec::new();
    let mut m = Matrix3D::IDENTITY;
    m.0[2] = 1.;
    d.build[0].transform = Some(m);
    assert!(matches!(
        write_model(&d, &mut out),
        Err(ModelWriteError::NonPlanarSlicedTransform { object_id: 3 })
    ));
    assert!(out.is_empty());
    m.0[2] = -0.;
    m.0[5] = -0.;
    m.0[6] = -0.;
    m.0[7] = -0.;
    m.0[9] = -0.;
    d.build[0].transform = Some(m);
    write_model(&d, &mut out).unwrap();
    let r = parse_model(&out).unwrap().build[0].transform.unwrap();
    for i in [2, 5, 6, 7] {
        assert_eq!(r.0[i].to_bits(), 0.0f64.to_bits());
    }
    assert_eq!(r.0[9].to_bits(), (-0.0f64).to_bits());
    for (kind, expected) in [
        (0, "zbottom"),
        (1, "ztop"),
        (2, "increase"),
        (3, "no segments"),
        (4, "slice vertex"),
        (5, "open polygon"),
    ] {
        let mut bad = d.clone();
        let SliceStackContent::Slices(ss) = &mut bad.slice_stacks[0].content else {
            panic!()
        };
        match kind {
            0 => bad.slice_stacks[0].z_bottom = f64::NAN,
            1 => ss[0].z_top = f64::INFINITY,
            2 => ss[0].z_top = 0.,
            3 => ss[0].polygons[0].segments.clear(),
            4 => ss[0].polygons[0].start = ResourceIndex::new(1).unwrap(),
            _ => {
                ss[0].vertices.push(SliceVertex { x: 1., y: 1. });
                ss[0].polygons[0].segments[0].end = ResourceIndex::new(1).unwrap();
            }
        }
        let mut b = Vec::new();
        assert!(
            write_model(&bad, &mut b)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
        assert!(b.is_empty());
    }
}

#[test]
fn package_content_types_relationship_order_and_start_selection() {
    use std::io::{Read, Write};
    let mut p = Package::new();
    for (name, ty) in [
        ("/z.X", "type/a"),
        ("/a.x", "type/b"),
        ("/b.Y", "type/c"),
        ("/noext", "type/d"),
    ] {
        p.add_part(PartName::new(name).unwrap(), ty, vec![])
            .unwrap();
    }
    for name in ["/z.X", "/a.x"] {
        p.add_part_relationship(
            &PartName::new(name).unwrap(),
            Relationship { external: false,
                id: format!("rel{}", name.len()),
                rel_type: "a&b".into(),
                target: "x<y".into(),
            },
        );
    }
    let mut b = Cursor::new(Vec::new());
    p.write_to(&mut b).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(b.get_ref())).unwrap();
    let names: Vec<_> = (0..z.len())
        .map(|i| z.by_index(i).unwrap().name().unwrap().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "_rels/a.x.rels",
            "_rels/z.X.rels",
            "z.X",
            "a.x",
            "b.Y",
            "noext"
        ]
    );
    let mut ct = String::new();
    z.by_index(0).unwrap().read_to_string(&mut ct).unwrap();
    assert!(ct.contains("<Default Extension=\"y\" ContentType=\"type/c\"/>"));
    assert!(!ct.contains("Extension=\"x\""));
    assert!(ct.find("PartName=\"/z.X\"").unwrap() < ct.find("PartName=\"/a.x\"").unwrap());
    let q = Package::read_from(Cursor::new(b.into_inner())).unwrap();
    assert_eq!(
        q.part_names().map(PartName::as_str).collect::<Vec<_>>(),
        ["/z.X", "/a.x", "/b.Y", "/noext"]
    );
    assert_eq!(
        q.part_relationships(&PartName::new("/z.X").unwrap())[0].rel_type,
        "a&b"
    );
    let mut b = Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(&mut b);
    let o = zip::write::SimpleFileOptions::default();
    z.start_file("[Content_Types].xml", o).unwrap();
    z.write_all(b"<Types><Default Extension=\"MoDeL\" ContentType=\"new\"/><Override PartName=\"/x.MODEL\" ContentType=\"override\"/></Types>").unwrap();
    z.start_file("x.MODEL", o).unwrap();
    z.start_file("y.model", o).unwrap();
    z.start_file("opaque.bin", o).unwrap();
    z.finish().unwrap();
    let q = Package::read_from(Cursor::new(b.into_inner())).unwrap();
    for (n, t) in [
        ("/x.MODEL", "override"),
        ("/y.model", "new"),
    ] {
        assert_eq!(q.part_content_type(&PartName::new(n).unwrap()), Some(t));
    }
    assert_eq!(q.part_content_type(&PartName::new("/opaque.bin").unwrap()), None);
    assert_eq!(q.warnings(), [PackageWarning::UnlistedPart { part: "/opaque.bin".into() }]);
    p.add_root_relationship(Relationship { external: false,
        id: "first".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: "missing".into(),
    });
    p.add_root_relationship(Relationship { external: false,
        id: "second".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: "z.X".into(),
    });
    // Two StartPart relationships: programs could take either, so neither.
    assert!(matches!(p.start_part(), Err(PackageReadError::SeveralStartParts)));
}

#[test]
fn large_parallel_package_is_valid_deterministic_and_progress_neutral() {
    use std::io::Read;
    let data: Vec<_> = (0..4 * 1024 * 1024 + 17)
        .map(|i| ((i * 37 + i / 17) % 251) as u8)
        .collect();
    let name = PartName::new("/模型.bin").unwrap();
    let mut p = Package::new();
    p.add_part(name.clone(), "type/opaque", data.clone())
        .unwrap();
    let mut a = Cursor::new(Vec::new());
    let mut ticks = Vec::new();
    p.write_to_with_progress(&mut a, |done, total| ticks.push((done, total)))
        .unwrap();
    let mut b = Cursor::new(Vec::new());
    p.write_to(&mut b).unwrap();
    assert_eq!(a.get_ref(), b.get_ref());
    assert_eq!(ticks.last(), Some(&(data.len() as u64, data.len() as u64)));
    assert!(
        ticks
            .windows(2)
            .all(|w| w[1].0 > w[0].0 && w[1].0 - w[0].0 <= 262144)
    );
    let mut z = zip::ZipArchive::new(Cursor::new(a.get_ref())).unwrap();
    let mut f = z.by_index(2).unwrap();
    assert_eq!(f.name().unwrap(), "模型.bin");
    assert_eq!(f.compression(), zip::CompressionMethod::Deflated);
    let mut decoded = Vec::new();
    f.read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, data);
    let q = Package::read_from(Cursor::new(b.into_inner())).unwrap();
    assert_eq!(q.part_data(&name), Some(data.as_slice()));
}

#[test]
fn parallel_xml_geometry_preserves_order_and_bits() {
    let mut d = parse("<resources><object id=\"1\"><mesh/></object></resources>").unwrap();
    let ObjectPayload::Mesh(m) = &mut d.objects[0].payload else {
        panic!()
    };
    m.vertices = (0..9000)
        .map(|i| Vertex3 {
            x: if i % 2 == 0 { -0. } else { i as f64 * 1e-12 },
            y: i as f64,
            z: -1.,
        })
        .collect();
    m.triangles = (0..9000)
        .map(|i| TriangleRef {
            v1: ResourceIndex::new(i).unwrap(),
            v2: ResourceIndex::new((i + 1) % 9000).unwrap(),
            v3: ResourceIndex::new(0).unwrap(),
        })
        .collect();
    let mut b = Vec::new();
    write_model(&d, &mut b).unwrap();
    assert_eq!(parse_model(&b).unwrap(), d);
    let ObjectPayload::Mesh(expected) = &d.objects[0].payload else {
        panic!()
    };
    let parsed = parse_model(&b).unwrap();
    let ObjectPayload::Mesh(actual) = &parsed.objects[0].payload else {
        panic!()
    };
    for (a, b) in expected.vertices.iter().zip(&actual.vertices) {
        assert_eq!(a.x.to_bits(), b.x.to_bits());
    }
}

#[test]
fn claimed_zip_caps_are_checked_before_extraction() {
    fn archive(count: usize, claimed: u32) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..count {
            w.start_file(
                format!("part{i}.bin"),
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        }
        let mut bytes = w.finish().unwrap().into_inner();
        // Both descriptions claim the size (the archive is consistent, so
        // the structure check passes and the caps are what refuse it).
        fn at(bytes: &[u8], signature: &[u8]) -> Vec<usize> {
            bytes.windows(4).enumerate().filter_map(|(i, b)| (b == signature).then_some(i)).collect()
        }
        for i in at(&bytes, b"PK\x01\x02") {
            bytes[i + 24..i + 28].copy_from_slice(&claimed.to_le_bytes());
        }
        for i in at(&bytes, b"PK\x03\x04") {
            bytes[i + 22..i + 26].copy_from_slice(&claimed.to_le_bytes());
        }
        bytes
    }
    assert!(matches!(
        Package::read_from(Cursor::new(archive(1, 2147483649))),
        Err(PackageReadError::OversizedPart {
            cap_bytes: 2147483648,
            ..
        })
    ));
    assert!(matches!(
        Package::read_from(Cursor::new(archive(3, 1500000000))),
        Err(PackageReadError::OversizedPackage {
            cap_bytes: 4294967296
        })
    ));
}

#[test]
fn model_writer_does_not_flush_the_sink() {
    struct Sink(Vec<u8>);
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            panic!("model writer must not flush")
        }
    }
    let mut sink = Sink(Vec::new());
    write_model(&ModelDocument::default(), &mut sink).unwrap();
    assert!(!sink.0.is_empty());
}

proptest::proptest! {
    #[test]
    fn finite_coordinate_bits_round_trip(bits in proptest::prelude::any::<u64>()) {
        let value = f64::from_bits(bits);
        proptest::prop_assume!(value.is_finite());
        let mut d = ModelDocument::default();
        d.objects.push(ObjectResource { id: ResourceId::MIN, object_type: ObjectType::Model, name: None, partnumber: None, property_ref: None, slice_stack: None, mesh_resolution: MeshResolution::Fullres,
            payload: ObjectPayload::Mesh(MeshData { vertices: vec![Vertex3 { x:value,y:value,z:value }], triangles: vec![] }) });
        let mut bytes = Vec::new(); write_model(&d, &mut bytes).unwrap();
        let r = parse_model(&bytes).unwrap();
        let ObjectPayload::Mesh(m) = &r.objects[0].payload else { panic!() };
        proptest::prop_assert_eq!(m.vertices[0].x.to_bits(), bits);
    }
}

#[test]
fn start_part_relationship_contract() {
    fn rel(kind: &str, target: &str) -> Relationship {
        Relationship { external: false, id: "id".into(), rel_type: kind.into(), target: target.into() }
    }
    let mut p = Package::new();
    assert!(matches!(p.start_part(), Err(PackageReadError::MissingStartPartRelationship)));
    p.add_root_relationship(rel("other", "ignored"));
    p.add_root_relationship(rel(START_PART_REL_TYPE, "bad//name"));
    assert!(matches!(p.start_part(), Err(PackageReadError::MalformedPartName { name, source: MalformedPartNameError::EmptySegment }) if name == "/bad//name"));
    let mut p = Package::new();
    p.add_root_relationship(rel(START_PART_REL_TYPE, "missing.model"));
    assert!(matches!(p.start_part(), Err(PackageReadError::StartPartNotFound { target }) if target == "missing.model"));
    let name = PartName::new("/present.model").unwrap();
    p.add_part(name.clone(), "type", vec![]).unwrap();
    let mut q = Package::new();
    q.add_part(name.clone(), "type", vec![]).unwrap();
    q.add_root_relationship(rel(START_PART_REL_TYPE, "/present.model"));
    assert_eq!(q.start_part().unwrap(), name);
    let mut q = Package::new();
    q.add_part(name.clone(), "type", vec![]).unwrap();
    q.add_root_relationship(rel(START_PART_REL_TYPE, "present.model"));
    assert_eq!(q.start_part().unwrap(), name);
}

#[test]
fn archive_member_selection_contract() {
    use std::io::Write;
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            if name.ends_with('/') {
                writer.add_directory(*name, options).unwrap();
            } else {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
        }
        writer.finish().unwrap().into_inner()
    }
    let types = b"<Types><Default Extension=\"bin\" ContentType=\"binary\"/><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/></Types>";
    let data = archive(&[("[Content_Types].xml", types), ("folder/", b""), ("folder/a.BIN", b"payload"), ("/plain", b"x")]);
    let package = Package::read_from(Cursor::new(data)).unwrap();
    // A part with no content type is another program's extra file: the
    // formats say to ignore it, and the reader warns.
    assert_eq!(package.part_names().map(PartName::as_str).collect::<Vec<_>>(), ["/folder/a.BIN"]);
    assert_eq!(package.warnings(), [PackageWarning::UnlistedPart { part: "/plain".into() }]);
    let name = PartName::new("/folder/a.BIN").unwrap();
    assert_eq!(package.part_data(&name), Some(&b"payload"[..]));
    assert_eq!(package.part_content_type(&name), Some("binary"));
    // An untyped model is ignored too, but naming it as the model fails.
    let rels = format!("<Relationships><Relationship Id=\"m\" Type=\"{START_PART_REL_TYPE}\" Target=\"/missing.MODEL\"/></Relationships>");
    let data = archive(&[("[Content_Types].xml", types), ("_rels/.rels", rels.as_bytes()), ("missing.MODEL", b"")]);
    let package = Package::read_from(Cursor::new(data)).unwrap();
    assert!(matches!(package.start_part(), Err(PackageReadError::PartWithoutContentType { part }) if part == "/missing.MODEL"));
    let data = archive(&[("[Content_Types].xml", types), ("bad//name", b"")]);
    assert!(matches!(Package::read_from(Cursor::new(data)), Err(PackageReadError::MalformedPartName { name, source: MalformedPartNameError::EmptySegment }) if name == "/bad//name"));
}

#[test]
fn model_rules_from_the_conformance_suite() {
    let model = |attrs: &str, body: &str| {
        format!("<model xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\"{attrs}>{body}<resources/><build/></model>")
    };
    // N_*_0409: xml:space on the model element.
    let spaced = model(" xml:space=\"preserve\"", "");
    assert!(matches!(parse_model(spaced.as_bytes()), Err(ModelParseError::Forbidden { .. })));
    // N_*_0410_01: a metadata prefix with no declaration; declared, it reads
    // and is written back with its declaration.
    let undeclared = model("", "<metadata name=\"x:anyname\">test</metadata>");
    let lenient = parse_model(undeclared.as_bytes()).unwrap();
    assert!(lenient.undeclared_metadata_prefixes.contains("x"));
    let mut rewritten = Vec::new();
    write_model(&lenient, &mut rewritten).unwrap();
    assert!(String::from_utf8_lossy(&rewritten).contains("xmlns:x=\"urn:sleptic:undeclared:x\""));
    let declared = model(" xmlns:x=\"urn:vendor\"", "<metadata name=\"x:anyname\">test</metadata>");
    let read = parse_model(declared.as_bytes()).unwrap();
    let mut written = Vec::new();
    write_model(&read, &mut written).unwrap();
    assert_eq!(parse_model(&written).unwrap(), read);
    // A repeated name is let through, the first counting.
    let twice = model("", "<metadata name=\"Title\">a</metadata><metadata name=\"Title\">b</metadata>");
    assert_eq!(parse_model(twice.as_bytes()).unwrap().metadata[0].value, "a");
    // N_*_0424: a property on an object made of components.
    let body = "<resources><basematerials id=\"5\"><base name=\"red\" displaycolor=\"#FF0000\"/></basematerials><object id=\"1\"><mesh><vertices/><triangles/></mesh></object><object id=\"2\" pid=\"5\" pindex=\"0\"><components><component objectid=\"1\"/></components></object></resources><build/>";
    let doc = format!("<model xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">{body}</model>");
    assert!(matches!(parse_model(doc.as_bytes()), Err(ModelParseError::Forbidden { .. })));
}

#[test]
fn matrix_attribute_requires_exactly_twelve_finite_values() {
    let matrix = Matrix3D([1., -2., 3.5, 4., 5., 6., 7., 8., 9., 10., 11., 12.]);
    assert_eq!(Matrix3D::parse(&matrix.to_attribute()), Some(matrix));
    assert!(Matrix3D::parse("1 2 3 4 5 6 7 8 9 10 11 12 13").is_none());
    assert!(Matrix3D::parse("1 2 3 4 5 6 7 8 9 10 11 inf").is_none());
    assert!(Matrix3D::parse("1 2 3 4 5 6 7 8 9 10 11 nope").is_none());
}
