// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Packages that different programs could read differently, and other
//! packages the reader must refuse, or accept, on purpose.
use crate::{error::*, names::*, package::*, zip_check::*};
use std::io::{Cursor, Write};

const TYPES: &[u8] = b"<Types><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/></Types>";

fn start_rels(target: &str, extra: &str) -> Vec<u8> {
    format!("<Relationships><Relationship Id=\"m\" Type=\"{START_PART_REL_TYPE}\" Target=\"{target}\"{extra}/></Relationships>").into_bytes()
}

/// A stored (uncompressed) archive of `entries`, so tests can edit bytes.
fn stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn package() -> Vec<u8> {
    let rels = start_rels("/3D/3dmodel.model", "");
    stored(&[("[Content_Types].xml", TYPES), ("_rels/.rels", &rels), ("3D/3dmodel.model", b"<model/>")])
}

fn find(bytes: &[u8], signature: &[u8]) -> Vec<usize> {
    bytes.windows(4).enumerate().filter_map(|(i, w)| (w == signature).then_some(i)).collect()
}

fn structure(bytes: Vec<u8>) -> ZipStructureError {
    match Package::read_from(Cursor::new(bytes)) {
        Err(PackageReadError::Structure(why)) => why,
        other => panic!("expected a structure refusal, got {other:?}"),
    }
}

#[test]
fn a_plain_package_passes_and_reads() {
    let bytes = package();
    assert_eq!(check_zip_structure(&bytes).unwrap().len(), 3);
    let read = Package::read_from(Cursor::new(bytes)).unwrap();
    assert_eq!(read.start_part().unwrap().as_str(), "/3D/3dmodel.model");
}

#[test]
fn the_two_descriptions_of_an_entry_must_agree() {
    let bytes = package();
    let local = find(&bytes, b"PK\x03\x04")[2];
    let mut renamed = bytes.clone();
    renamed[local + 30] = b'4'; // "3D/..." becomes "4D/..." in the local header only
    assert!(matches!(structure(renamed), ZipStructureError::HeadersDisagree { field: "name", .. }));
    let mut recrc = bytes.clone();
    recrc[local + 14] ^= 0xFF;
    assert!(matches!(structure(recrc), ZipStructureError::HeadersDisagree { field: "CRC", .. }));
    let mut reflagged = bytes;
    reflagged[local + 6] ^= 0x02;
    assert!(matches!(structure(reflagged), ZipStructureError::HeadersDisagree { field: "set of flags", .. }));
}

#[test]
fn bytes_the_index_does_not_list_are_refused() {
    let bytes = package();
    let end = find(&bytes, b"PK\x05\x06")[0];
    let directory = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    // A whole extra local entry, slipped in before the index.
    let hidden = stored(&[("3D/other.model", b"<model>evil</model>")]);
    let hidden_entry = &hidden[..find(&hidden, b"PK\x01\x02")[0]];
    let mut smuggled = bytes[..directory].to_vec();
    smuggled.extend_from_slice(hidden_entry);
    smuggled.extend_from_slice(&bytes[directory..]);
    let moved = (directory + hidden_entry.len()) as u32;
    let new_end = end + hidden_entry.len();
    smuggled[new_end + 16..new_end + 20].copy_from_slice(&moved.to_le_bytes());
    assert!(matches!(structure(smuggled), ZipStructureError::UnlistedBytes { .. }));
}

#[test]
fn glued_and_prefixed_archives_are_refused() {
    let mut glued = package();
    glued.extend(package());
    assert!(Package::read_from(Cursor::new(glued)).is_err());
    let mut prefixed = b"junk before the archive".to_vec();
    prefixed.extend(package());
    assert!(matches!(Package::read_from(Cursor::new(prefixed)), Err(PackageReadError::Structure(_))));
}

#[test]
fn multi_disk_archives_and_double_end_records_are_refused() {
    let bytes = package();
    let end = find(&bytes, b"PK\x05\x06")[0];
    let mut split = bytes.clone();
    split[end + 4] = 1;
    assert_eq!(structure(split), ZipStructureError::MultiDisk);
    // A second end record hidden in the first one's comment: both fit.
    let mut doubled = bytes[..end + 20].to_vec();
    doubled.extend_from_slice(&22u16.to_le_bytes());
    doubled.extend_from_slice(&bytes[end..end + 20]);
    doubled.extend_from_slice(&0u16.to_le_bytes());
    assert_eq!(structure(doubled), ZipStructureError::SeveralEndRecords);
}

#[test]
fn encrypted_entries_are_refused() {
    let mut bytes = package();
    for at in find(&bytes, b"PK\x03\x04") {
        bytes[at + 6] |= 1;
    }
    for at in find(&bytes, b"PK\x01\x02") {
        bytes[at + 8] |= 1;
    }
    assert!(matches!(structure(bytes), ZipStructureError::Encrypted { .. }));
}

#[test]
fn data_descriptors_and_zip64_records_are_accepted() {
    // Streamed: sizes and CRC in a descriptor after each entry's data.
    let mut streamed = zip::ZipWriter::new_stream(Vec::new());
    let options = zip::write::SimpleFileOptions::default();
    let rels = start_rels("/3D/3dmodel.model", "");
    for (name, data) in [("[Content_Types].xml", TYPES), ("_rels/.rels", rels.as_slice()), ("3D/3dmodel.model", b"<model/>")] {
        streamed.start_file(name, options).unwrap();
        streamed.write_all(data).unwrap();
    }
    let bytes = streamed.finish().unwrap().into_inner();
    let read = Package::read_from(Cursor::new(bytes)).unwrap();
    assert_eq!(read.part_data(&read.start_part().unwrap()), Some(&b"<model/>"[..]));
    // ZIP64 extra fields on every entry.
    let mut wide = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().large_file(true);
    for (name, data) in [("[Content_Types].xml", TYPES), ("_rels/.rels", rels.as_slice()), ("3D/3dmodel.model", b"<model/>")] {
        wide.start_file(name, options).unwrap();
        wide.write_all(data).unwrap();
    }
    let bytes = wide.finish().unwrap().into_inner();
    Package::read_from(Cursor::new(bytes)).unwrap();
}

#[test]
fn part_names_ignore_ascii_case() {
    // The override, the start part and the part itself are spelled three ways.
    let types = b"<Types><Default Extension=\"RELS\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Override PartName=\"/3D/3DMODEL.MODEL\" ContentType=\"m\"/></Types>";
    let rels = start_rels("/3d/3dModel.model", "");
    let bytes = stored(&[("[Content_Types].xml", types), ("_RELS/.RELS", &rels), ("3D/3dmodel.model", b"<model/>")]);
    let read = Package::read_from(Cursor::new(bytes)).unwrap();
    let start = read.start_part().unwrap();
    assert_eq!(read.part_content_type(&start), Some("m"));
    // Two entries that only differ in case are one part named twice.
    let twice = stored(&[("[Content_Types].xml", TYPES), ("a.model", b"1"), ("A.MODEL", b"2")]);
    assert!(matches!(
        Package::read_from(Cursor::new(twice)),
        Err(PackageReadError::EquivalentPartNames { .. })
    ));
}

#[test]
fn interleaved_parts_are_joined_and_broken_ones_refused() {
    let rels = start_rels("/3D/3dmodel.model", "");
    let pieces = stored(&[
        ("[Content_Types].xml", TYPES),
        ("_rels/.rels", &rels),
        ("3D/3dmodel.model/[1].piece", b"del/"),
        ("3D/3dmodel.model/[0].piece", b"<mo"),
        ("3D/3dmodel.model/[2].last.piece", b">"),
    ]);
    let read = Package::read_from(Cursor::new(pieces)).unwrap();
    assert_eq!(read.part_data(&read.start_part().unwrap()), Some(&b"<model/>"[..]));
    let gap = stored(&[
        ("[Content_Types].xml", TYPES),
        ("3D/3dmodel.model/[0].piece", b"<mo"),
        ("3D/3dmodel.model/[2].last.piece", b">"),
    ]);
    assert!(matches!(Package::read_from(Cursor::new(gap)), Err(PackageReadError::BrokenPieces { .. })));
    let no_last = stored(&[("[Content_Types].xml", TYPES), ("3D/3dmodel.model/[0].piece", b"<model/>")]);
    assert!(matches!(Package::read_from(Cursor::new(no_last)), Err(PackageReadError::BrokenPieces { .. })));
    let both = stored(&[
        ("[Content_Types].xml", TYPES),
        ("3D/3dmodel.model", b"<model/>"),
        ("3D/3dmodel.model/[0].last.piece", b"<model/>"),
    ]);
    assert!(matches!(Package::read_from(Cursor::new(both)), Err(PackageReadError::EquivalentPartNames { .. })));
}

#[test]
fn the_start_part_is_one_internal_relationship() {
    let external = start_rels("https://example.com/3dmodel.model", " TargetMode=\"External\"");
    let bytes = stored(&[("[Content_Types].xml", TYPES), ("_rels/.rels", &external), ("3D/3dmodel.model", b"<model/>")]);
    let read = Package::read_from(Cursor::new(bytes)).unwrap();
    assert!(matches!(read.start_part(), Err(PackageReadError::ExternalStartPart { .. })));
    let two = format!(
        "<Relationships><Relationship Id=\"a\" Type=\"{START_PART_REL_TYPE}\" Target=\"/3D/3dmodel.model\"/><Relationship Id=\"b\" Type=\"{START_PART_REL_TYPE}\" Target=\"/3D/other.model\"/></Relationships>"
    );
    let bytes = stored(&[
        ("[Content_Types].xml", TYPES),
        ("_rels/.rels", two.as_bytes()),
        ("3D/3dmodel.model", b"<model/>"),
        ("3D/other.model", b"<model/>"),
    ]);
    let read = Package::read_from(Cursor::new(bytes)).unwrap();
    assert!(matches!(read.start_part(), Err(PackageReadError::SeveralStartParts)));
}

#[test]
fn oversized_index_parts_are_refused_before_parsing() {
    // 17 MB of whitespace in [Content_Types].xml deflates to a few kB.
    let mut huge = b"<Types>".to_vec();
    huge.resize(17 * 1024 * 1024, b' ');
    huge.extend_from_slice(b"</Types>");
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("[Content_Types].xml", options).unwrap();
    writer.write_all(&huge).unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert!(bytes.len() < 1024 * 1024);
    assert!(matches!(
        Package::read_from(Cursor::new(bytes)),
        Err(PackageReadError::MetadataPartTooLarge { .. })
    ));
}

#[test]
fn package_writer_output_passes_its_own_checks() {
    let mut package = Package::new();
    let name = PartName::new("/3D/3dmodel.model").unwrap();
    package.add_part(name, "application/vnd.ms-package.3dmanufacturing-3dmodel+xml", vec![b'x'; 5 * 1024 * 1024]).unwrap();
    package.add_root_relationship(Relationship {
        id: "m".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: "/3D/3dmodel.model".into(),
        external: false,
    });
    let mut bytes = Cursor::new(Vec::new());
    package.write_to(&mut bytes).unwrap();
    check_zip_structure(bytes.get_ref()).unwrap();
    Package::read_from(bytes).unwrap();
}

#[test]
fn content_types_are_declared_once_and_relationships_belong_to_parts() {
    // Conformance N_XXX_0205: a second Default for the same extension, and a
    // second Override for the same part.
    let twice = b"<Types><Default Extension=\"model\" ContentType=\"a\"/><Default Extension=\"MODEL\" ContentType=\"b\"/></Types>";
    let bytes = stored(&[("[Content_Types].xml", twice), ("3D/3dmodel.model", b"<model/>")]);
    assert!(matches!(Package::read_from(Cursor::new(bytes)), Err(PackageReadError::DuplicateContentType { .. })));
    let twice = b"<Types><Override PartName=\"/3D/3dmodel.model\" ContentType=\"a\"/><Override PartName=\"/3d/3DMODEL.model\" ContentType=\"a\"/></Types>";
    let bytes = stored(&[("[Content_Types].xml", twice), ("3D/3dmodel.model", b"<model/>")]);
    assert!(matches!(Package::read_from(Cursor::new(bytes)), Err(PackageReadError::DuplicateContentType { .. })));
    // Conformance N_XXX_0407_02: a .rels part named for a part that is absent.
    let rels = start_rels("/3D/3dmodel.model", "");
    let bytes = stored(&[
        ("[Content_Types].xml", TYPES),
        ("_rels/.rels", &rels),
        ("3D/_rels/wrong3dmodel.model.rels", &rels),
        ("3D/3dmodel.model", b"<model/>"),
    ]);
    assert!(matches!(Package::read_from(Cursor::new(bytes)), Err(PackageReadError::OrphanRelationships { .. })));
}

const RELS_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

fn with_types(types: &str, rels: &[u8], extra: &[(&str, &[u8])]) -> Result<Package, PackageReadError> {
    let mut entries: Vec<(&str, &[u8])> = vec![("[Content_Types].xml", types.as_bytes()), ("_rels/.rels", rels), ("3D/3dmodel.model", b"<model/>")];
    entries.extend_from_slice(extra);
    Package::read_from(Cursor::new(stored(&entries)))
}

#[test]
fn packaging_rules_from_the_conformance_suite() {
    let model = "<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>";
    let rels = start_rels("/3D/3dmodel.model", "");
    // N_*_0206: a Default with an empty extension.
    let types = format!("<Types>{model}<Default Extension=\"rels\" ContentType=\"{RELS_TYPE}\"/><Default Extension=\"\" ContentType=\"x\"/></Types>");
    assert!(matches!(with_types(&types, &rels, &[]), Err(PackageReadError::InvalidContentTypes { .. })));
    // N_*_0207: an Override with an empty part name.
    let types = format!("<Types>{model}<Default Extension=\"rels\" ContentType=\"{RELS_TYPE}\"/><Override PartName=\"\" ContentType=\"image/png\"/></Types>");
    assert!(matches!(with_types(&types, &rels, &[]), Err(PackageReadError::InvalidContentTypes { .. })));
    // N_*_0204_01, N_*_0404_03: relationship parts must have exactly the relationships type.
    for wrong in [format!("{RELS_TYPE};cow=\"Moo\""), "application/xml".to_owned()] {
        let types = format!("<Types>{model}<Default Extension=\"rels\" ContentType='{wrong}'/></Types>");
        assert!(matches!(with_types(&types, &rels, &[]), Err(PackageReadError::InvalidContentTypes { .. })), "{wrong}");
    }
    let good_types = format!("<Types>{model}<Default Extension=\"rels\" ContentType=\"{RELS_TYPE}\"/><Default Extension=\"png\" ContentType=\"image/png\"/></Types>");
    with_types(&good_types, &rels, &[]).unwrap();
    // N_*_0405_04: an Id that is not an XML ID; and two relationships with one Id.
    let numeric = format!("<Relationships><Relationship Id=\"8rel\" Type=\"{START_PART_REL_TYPE}\" Target=\"/3D/3dmodel.model\"/></Relationships>");
    assert!(matches!(with_types(&good_types, numeric.as_bytes(), &[]), Err(PackageReadError::InvalidRelationship { .. })));
    let thumbnail = |target: &str, mode: &str| {
        format!("<Relationships><Relationship Id=\"m\" Type=\"{START_PART_REL_TYPE}\" Target=\"/3D/3dmodel.model\"/><Relationship Id=\"t\" Type=\"{THUMBNAIL_REL_TYPE}\" Target=\"{target}\"{mode}/></Relationships>")
    };
    let png: &[u8] = b"\x89PNG";
    // A thumbnail that is there, as a PNG: fine, also relative to the root.
    with_types(&good_types, thumbnail("/Metadata/t.png", "").as_bytes(), &[("Metadata/t.png", png)]).unwrap();
    with_types(&good_types, thumbnail("Metadata/t.png", "").as_bytes(), &[("Metadata/t.png", png)]).unwrap();
    // N_*_0405_01: missing; N_*_0403: external; N_*_0404_04: not an image.
    for (rels, types) in [
        (thumbnail("/Metadata/missing.png", ""), good_types.clone()),
        (thumbnail("http://example.com/t.png", " TargetMode=\"External\""), good_types.clone()),
        (thumbnail("/Metadata/t.png", ""), good_types.replace("image/png", "image/xxxpng")),
    ] {
        let refused = with_types(&types, rels.as_bytes(), &[("Metadata/t.png", png)]);
        assert!(matches!(refused, Err(PackageReadError::InvalidRelationship { .. })), "{rels}");
    }
}
