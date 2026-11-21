// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Every error message of this crate, word for word: the GUI shows them and
//! bug reports quote them, so rewording one is a change to review, not a
//! side effect.
use crate::error::*;

fn says(error: impl std::fmt::Display, text: &str) {
    assert_eq!(error.to_string(), text);
}

fn disk() -> std::io::Error {
    std::io::Error::other("disk")
}

fn bad_xml() -> quick_xml::Error {
    quick_xml::Error::Io(std::sync::Arc::new(disk()))
}

#[test]
fn part_name_and_package_write_messages() {
    says(MalformedPartNameError::Empty, "part name is empty");
    says(MalformedPartNameError::NotAbsolute, "part name must start with '/'");
    says(MalformedPartNameError::EmptySegment, "part name contains an empty segment");
    says(MalformedPartNameError::TrailingDot, "part name segment ends with a dot");
    let zip = zip::result::ZipError::FileNotFound;
    let zip_text = zip.to_string();
    says(PackageWriteError::Zip(zip), &format!("could not write the package archive: {zip_text}"));
    says(PackageWriteError::Io(disk()), "could not write package data: disk");
    says(
        PackageWriteError::DuplicatePartName { name: "/a".into() },
        "package already contains a part named \"/a\"",
    );
}

#[test]
fn package_read_messages() {
    let xml_text = bad_xml().to_string();
    let zip_text = zip::result::ZipError::FileNotFound.to_string();
    says(
        PackageReadError::Zip(zip::result::ZipError::FileNotFound),
        &format!("the package archive is damaged or is not a ZIP file ({zip_text})"),
    );
    says(PackageReadError::PartRead { part: "/a".into(), source: disk() }, "the package archive is damaged: part \"/a\" does not decompress correctly (disk)");
    says(PackageReadError::MissingContentTypes, "package has no [Content_Types].xml part");
    says(PackageReadError::ContentTypesXml(bad_xml()), &format!("[Content_Types].xml is not valid XML: {xml_text}"));
    says(
        PackageReadError::ContentTypeMissingAttribute { attribute: "Extension" },
        "a content-type entry is missing its \"Extension\" attribute",
    );
    says(
        PackageReadError::RelationshipsXml { part: "/_rels/.rels".into(), source: bad_xml() },
        &format!("relationships part \"/_rels/.rels\" is not valid XML: {xml_text}"),
    );
    says(
        PackageReadError::RelationshipMissingAttribute { part: "/r".into(), attribute: "Target" },
        "a relationship in \"/r\" is missing its \"Target\" attribute",
    );
    says(PackageReadError::PartWithoutContentType { part: "/a".into() }, "part \"/a\" has no content type");
    says(
        PackageReadError::MissingStartPartRelationship,
        "package has no StartPart relationship pointing at a 3D Model part",
    );
    says(
        PackageReadError::StartPartNotFound { target: "/3D/x".into() },
        "the StartPart relationship points at \"/3D/x\", which is not in the package",
    );
    says(
        PackageReadError::MalformedPartName { name: "a".into(), source: MalformedPartNameError::NotAbsolute },
        "entry \"a\" is not a valid part name: part name must start with '/'",
    );
    says(
        PackageReadError::OversizedPart { part: "/a".into(), cap_bytes: 9 },
        "part \"/a\" decompresses past the 9-byte per-part cap; the package was refused",
    );
    says(
        PackageReadError::OversizedPackage { cap_bytes: 9 },
        "the package decompresses past the 9-byte total cap and was refused",
    );
}

#[test]
fn model_parse_messages() {
    use ModelParseError as E;
    let xml_text = bad_xml().to_string();
    says(E::Xml(bad_xml()), &format!("model XML is unreadable: {xml_text}"));
    let attr = quick_xml::events::attributes::AttrError::ExpectedEq(3);
    let attr_text = attr.to_string();
    says(E::AttrSyntax(attr), &format!("model XML has a malformed attribute: {attr_text}"));
    says(E::DtdForbidden, "model XML contains DTD content, which 3MF forbids");
    says(E::UndeclaredEntity { name: "x".into() }, "model XML references undeclared entity \"&x;\"");
    says(
        E::UnexpectedRootElement { found: "svg".into() },
        "expected a core-namespace <model> root element, found <svg>",
    );
    says(E::MissingModelElement, "document contains no <model> element");
    says(
        E::MissingRequiredAttribute { element: "vertex", attribute: "x" },
        "<vertex> is missing its \"x\" attribute",
    );
    says(E::InvalidUnit { value: "km".into() }, "\"km\" is not a valid model unit");
    says(
        E::InvalidNumber { element: "vertex", attribute: "y", value: "q".into() },
        "<vertex> attribute \"y\" is not a number: \"q\"",
    );
    says(E::InvalidMatrix { value: "1 2".into() }, "transform is not 12 whitespace-separated numbers: \"1 2\"");
    says(
        E::InvalidResourceId { value: "-1".into() },
        "\"-1\" is not a valid resource id (positive 32-bit integer)",
    );
    says(E::InvalidObjectType { value: "blob".into() }, "\"blob\" is not a valid object type");
    says(E::DuplicateResourceId { id: 4 }, "resource id 4 is defined more than once");
    says(E::UnknownObjectReference { id: 4 }, "reference to object id 4, which is not defined at this point");
    says(
        E::TriangleIndexOutOfRange { object_id: 1, index: 7, vertex_count: 3 },
        "object 1 has a triangle referencing vertex 7, but only 3 vertices exist",
    );
    says(E::ObjectWithoutGeometry { id: 4 }, "object 4 has neither a mesh nor components");
    says(E::DuplicateGeometry { id: 4 }, "object 4 has more than one geometry element");
    says(
        E::BuildItemReferencesOtherType { id: 4 },
        "build item references object 4 of type \"other\", which 3MF forbids",
    );
    says(
        E::UnsupportedRequiredExtension { namespace: "urn:x".into() },
        "model requires the unsupported 3MF extension \"urn:x\"",
    );
    says(
        E::UnknownRequiredExtensionPrefix { prefix: "p".into() },
        "requiredextensions names prefix \"p\" with no xmlns declaration",
    );
    says(
        E::ProductionPathUnsupported,
        "model splits objects across parts via the production extension (p:path), which is not yet supported",
    );
    says(
        E::SliceStackMixesSlicesAndRefs { id: 2 },
        "slice stack 2 mixes inline slices with slicerefs, which the slice extension forbids",
    );
    says(
        E::UnknownSliceStackReference { object_id: 1, stack_id: 2 },
        "object 1 references slice stack 2, which is not defined at this point",
    );
    says(E::InvalidMeshResolution { value: "mid".into() }, "\"mid\" is not a valid mesh resolution");
    says(E::PolygonWithoutSegments { stack_id: 2 }, "slice stack 2 has a polygon with no segments");
    says(
        E::SliceIndexOutOfRange { stack_id: 2, index: 9, vertex_count: 4 },
        "slice stack 2 references slice vertex 9, but only 4 vertices exist",
    );
}

#[test]
fn model_write_messages() {
    use ModelWriteError as E;
    says(E::Io(disk()), "could not write model XML: disk");
    says(
        E::SliceZNotIncreasing { stack_id: 2, index: 5 },
        "slice stack 2: slice 5 does not increase the stack's z",
    );
    says(E::SliceZTopNotFinite { stack_id: 2, index: 5 }, "slice stack 2: slice 5 has a non-finite ztop");
    says(E::SliceZBottomNotFinite { stack_id: 2 }, "slice stack 2: the zbottom is not finite");
    says(E::PolygonWithoutSegments { stack_id: 2 }, "slice stack 2 has a polygon with no segments");
    says(
        E::SliceIndexOutOfRange { stack_id: 2, index: 9, vertex_count: 4 },
        "slice stack 2 references slice vertex 9, but only 4 vertices exist",
    );
    says(
        E::OpenPolygonOnModelObject { object_id: 1, stack_id: 2 },
        "object 1 requires closed slice contours, but stack 2 has an open polygon",
    );
    says(
        E::UnknownSliceStackReference { object_id: 1, stack_id: 2 },
        "object 1 references slice stack 2, which the document does not define",
    );
    says(E::NonPlanarSlicedTransform { object_id: 1 }, "a transform applied to sliced object 1 is not planar");
}
