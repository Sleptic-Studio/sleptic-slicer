// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::error::{MalformedPartNameError as Name, PackageWriteError};
use std::error::Error;

#[test]
fn part_name_errors_have_stable_messages_and_no_source() {
    for (error, expected) in [
        (Name::Empty, "part name is empty"),
        (Name::NotAbsolute, "part name must start with '/'"),
        (Name::EmptySegment, "part name contains an empty segment"),
        (Name::TrailingDot, "part name segment ends with a dot"),
    ] {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_none());
    }
}

#[test]
fn package_write_errors_keep_causes_and_conversions() {
    let error = PackageWriteError::from(std::io::Error::other("disk"));
    assert!(matches!(error, PackageWriteError::Io(_)));
    assert_eq!(error.to_string(), "could not write package data: disk");
    assert_eq!(error.source().unwrap().to_string(), "disk");
    let error = PackageWriteError::from(zip::result::ZipError::FileNotFound);
    assert!(matches!(error, PackageWriteError::Zip(_)));
    assert_eq!(error.to_string(), format!("could not write the package archive: {}", zip::result::ZipError::FileNotFound));
    assert!(error.source().is_some());
    let error = PackageWriteError::DuplicatePartName { name: "a".into() };
    assert_eq!(error.to_string(), "package already contains a part named \"a\"");
    assert!(error.source().is_none());
}

#[test]
fn model_write_messages_and_sources() {
    use crate::error::ModelWriteError as E;
    let cases = [
        (E::SliceZNotIncreasing { stack_id: 2, index: 3 }, "slice stack 2: slice 3 does not increase the stack's z"),
        (E::SliceZTopNotFinite { stack_id: 2, index: 3 }, "slice stack 2: slice 3 has a non-finite ztop"),
        (E::SliceZBottomNotFinite { stack_id: 2 }, "slice stack 2: the zbottom is not finite"),
        (E::PolygonWithoutSegments { stack_id: 2 }, "slice stack 2 has a polygon with no segments"),
        (E::SliceIndexOutOfRange { stack_id: 2, index: 3, vertex_count: 4 }, "slice stack 2 references slice vertex 3, but only 4 vertices exist"),
        (E::OpenPolygonOnModelObject { object_id: 1, stack_id: 2 }, "object 1 requires closed slice contours, but stack 2 has an open polygon"),
        (E::UnknownSliceStackReference { object_id: 1, stack_id: 2 }, "object 1 references slice stack 2, which the document does not define"),
        (E::NonPlanarSlicedTransform { object_id: 1 }, "a transform applied to sliced object 1 is not planar"),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_none());
    }
    let error = E::from(std::io::Error::other("disk"));
    assert!(matches!(error, E::Io(_)));
    assert_eq!(error.to_string(), "could not write model XML: disk");
    assert_eq!(error.source().unwrap().to_string(), "disk");
}

#[test]
fn package_read_messages_without_causes() {
    use crate::error::PackageReadError as E;
    let cases = [
        (E::MissingContentTypes, "package has no [Content_Types].xml part"),
        (E::ContentTypeMissingAttribute { attribute: "Type" }, "a content-type entry is missing its \"Type\" attribute"),
        (E::RelationshipMissingAttribute { part: "p".into(), attribute: "Id" }, "a relationship in \"p\" is missing its \"Id\" attribute"),
        (E::PartWithoutContentType { part: "p".into() }, "part \"p\" has no content type"),
        (E::MissingStartPartRelationship, "package has no StartPart relationship pointing at a 3D Model part"),
        (E::StartPartNotFound { target: "p".into() }, "the StartPart relationship points at \"p\", which is not in the package"),
        (E::OversizedPart { part: "p".into(), cap_bytes: 42 }, "part \"p\" decompresses past the 42-byte per-part cap; the package was refused"),
        (E::OversizedPackage { cap_bytes: 42 }, "the package decompresses past the 42-byte total cap and was refused"),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_none());
    }
}

#[test]
fn package_read_causes_and_conversions() {
    use crate::error::PackageReadError as E;
    let error = E::from(zip::result::ZipError::FileNotFound);
    assert!(matches!(error, E::Zip(_)));
    assert_eq!(error.to_string(), format!("the package archive is damaged or is not a ZIP file ({})", zip::result::ZipError::FileNotFound));
    assert_eq!(error.source().unwrap().to_string(), zip::result::ZipError::FileNotFound.to_string());

    let xml = || quick_xml::Error::from(std::io::Error::other("bad xml"));
    let error = E::from(xml());
    assert!(matches!(error, E::ContentTypesXml(_)));
    assert_eq!(error.to_string(), format!("[Content_Types].xml is not valid XML: {}", xml()));
    assert_eq!(error.source().unwrap().to_string(), xml().to_string());

    let error = E::PartRead { part: "p".into(), source: std::io::Error::other("disk") };
    assert_eq!(error.to_string(), "the package archive is damaged: part \"p\" does not decompress correctly (disk)");
    assert_eq!(error.source().unwrap().to_string(), "disk");
    let error = E::RelationshipsXml { part: "p".into(), source: xml() };
    assert_eq!(error.to_string(), format!("relationships part \"p\" is not valid XML: {}", xml()));
    assert_eq!(error.source().unwrap().to_string(), xml().to_string());
    let error = E::MalformedPartName { name: "p".into(), source: Name::NotAbsolute };
    assert_eq!(error.to_string(), "entry \"p\" is not a valid part name: part name must start with '/'");
    assert_eq!(error.source().unwrap().to_string(), "part name must start with '/'");
}

#[test]
fn model_parse_messages_without_causes() {
    use crate::error::ModelParseError as E;
    let cases = [
        (E::DtdForbidden, "model XML contains DTD content, which 3MF forbids"),
        (E::UndeclaredEntity { name: "x".into() }, "model XML references undeclared entity \"&x;\""),
        (E::UnexpectedRootElement { found: "x".into() }, "expected a core-namespace <model> root element, found <x>"),
        (E::MissingModelElement, "document contains no <model> element"),
        (E::MissingRequiredAttribute { element: "e", attribute: "a" }, "<e> is missing its \"a\" attribute"),
        (E::InvalidUnit { value: "x".into() }, "\"x\" is not a valid model unit"),
        (E::InvalidNumber { element: "e", attribute: "a", value: "x".into() }, "<e> attribute \"a\" is not a number: \"x\""),
        (E::InvalidMatrix { value: "x".into() }, "transform is not 12 whitespace-separated numbers: \"x\""),
        (E::InvalidResourceId { value: "x".into() }, "\"x\" is not a valid resource id (positive 32-bit integer)"),
        (E::InvalidObjectType { value: "x".into() }, "\"x\" is not a valid object type"),
        (E::DuplicateResourceId { id: 2 }, "resource id 2 is defined more than once"),
        (E::UnknownObjectReference { id: 2 }, "reference to object id 2, which is not defined at this point"),
        (E::TriangleIndexOutOfRange { object_id: 1, index: 2, vertex_count: 3 }, "object 1 has a triangle referencing vertex 2, but only 3 vertices exist"),
        (E::ObjectWithoutGeometry { id: 2 }, "object 2 has neither a mesh nor components"),
        (E::DuplicateGeometry { id: 2 }, "object 2 has more than one geometry element"),
        (E::BuildItemReferencesOtherType { id: 2 }, "build item references object 2 of type \"other\", which 3MF forbids"),
        (E::UnsupportedRequiredExtension { namespace: "x".into() }, "model requires the unsupported 3MF extension \"x\""),
        (E::UnknownRequiredExtensionPrefix { prefix: "x".into() }, "requiredextensions names prefix \"x\" with no xmlns declaration"),
        (E::ProductionPathUnsupported, "model splits objects across parts via the production extension (p:path), which is not yet supported"),
        (E::SliceStackMixesSlicesAndRefs { id: 2 }, "slice stack 2 mixes inline slices with slicerefs, which the slice extension forbids"),
        (E::UnknownSliceStackReference { object_id: 1, stack_id: 2 }, "object 1 references slice stack 2, which is not defined at this point"),
        (E::InvalidMeshResolution { value: "x".into() }, "\"x\" is not a valid mesh resolution"),
        (E::PolygonWithoutSegments { stack_id: 2 }, "slice stack 2 has a polygon with no segments"),
        (E::SliceIndexOutOfRange { stack_id: 2, index: 3, vertex_count: 4 }, "slice stack 2 references slice vertex 3, but only 4 vertices exist"),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_none());
    }
}

#[test]
fn model_parse_causes_and_conversions() {
    use crate::error::ModelParseError as E;
    let xml = || quick_xml::Error::from(std::io::Error::other("bad xml"));
    let error = E::from(xml());
    assert!(matches!(error, E::Xml(_)));
    assert_eq!(error.to_string(), format!("model XML is unreadable: {}", xml()));
    assert_eq!(error.source().unwrap().to_string(), xml().to_string());

    let attr = || quick_xml::events::attributes::AttrError::ExpectedEq(7);
    let error = E::from(attr());
    assert!(matches!(error, E::AttrSyntax(_)));
    assert_eq!(error.to_string(), format!("model XML has a malformed attribute: {}", attr()));
    assert_eq!(error.source().unwrap().to_string(), attr().to_string());

    let invalid = std::hint::black_box([255_u8]);
    let encoding = quick_xml::encoding::EncodingError::from(std::str::from_utf8(&invalid).unwrap_err());
    let expected = quick_xml::Error::from(encoding.clone()).to_string();
    let error = E::from(encoding);
    assert!(matches!(error, E::Xml(_)));
    assert_eq!(error.to_string(), format!("model XML is unreadable: {expected}"));
    assert_eq!(error.source().unwrap().to_string(), expected);
}


#[test]
fn package_cause_preserves_original_error_identity() {
    assert_eq!(Name::Empty.to_string(), "part name is empty");
    let wrapped = PackageWriteError::from(std::io::Error::other("cause"));
    let PackageWriteError::Io(inner) = &wrapped else { unreachable!() };
    assert!(std::ptr::eq(inner, wrapped.source().unwrap().downcast_ref::<std::io::Error>().unwrap()));
}
