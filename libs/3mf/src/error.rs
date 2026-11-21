// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

// Keep wrapper conversions together so each source has one explicit destination.
macro_rules! error_conversions {
    ($( $source:ty => $target:ident :: $variant:ident ),* $(,)?) => {
        $(impl From<$source> for $target {
            fn from(value: $source) -> Self { Self::$variant(value) }
        })*
    };
}

error_conversions! {
    zip::result::ZipError => PackageWriteError::Zip,
    std::io::Error => PackageWriteError::Io,
    zip::result::ZipError => PackageReadError::Zip,
    quick_xml::Error => PackageReadError::ContentTypesXml,
    quick_xml::Error => ModelParseError::Xml,
    quick_xml::events::attributes::AttrError => ModelParseError::AttrSyntax,
    std::io::Error => ModelWriteError::Io,
}

// A wrapper reports its immediate cause, while validation failures terminate the chain.
macro_rules! error_sources {
    ($kind:ty, $cause:ident; $( $pattern:pat_param )|+) => {
        impl std::error::Error for $kind {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    $($pattern => Some($cause),)+
                    _ => None,
                }
            }
        }
    };
}
error_sources!(PackageWriteError, cause; Self::Zip(cause) | Self::Io(cause));
error_sources!(PackageReadError, cause; Self::Zip(cause) | Self::ArchiveRead(cause) | Self::Structure(cause) | Self::PartRead { source: cause, .. } | Self::ContentTypesXml(cause) | Self::RelationshipsXml { source: cause, .. } | Self::MalformedPartName { source: cause, .. });
error_sources!(ModelParseError, cause; Self::Xml(cause) | Self::AttrSyntax(cause));
error_sources!(ModelWriteError, cause; Self::Io(cause));

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MalformedPartNameError {
    Empty,
    NotAbsolute,
    EmptySegment,
    TrailingDot,
}
impl std::error::Error for MalformedPartNameError {}
#[derive(Debug)]
pub enum PackageWriteError {
    Zip(zip::result::ZipError),
    Io(std::io::Error),
    DuplicatePartName { name: String },
}

#[derive(Debug)]
pub enum PackageReadError {
    Zip(zip::result::ZipError),
    PartRead {
        part: String,
        source: std::io::Error,
    },
    MissingContentTypes,
    ContentTypesXml(quick_xml::Error),
    ContentTypeMissingAttribute {
        attribute: &'static str,
    },
    RelationshipsXml {
        part: String,
        source: quick_xml::Error,
    },
    RelationshipMissingAttribute {
        part: String,
        attribute: &'static str,
    },
    PartWithoutContentType {
        part: String,
    },
    MissingStartPartRelationship,
    StartPartNotFound {
        target: String,
    },
    MalformedPartName {
        name: String,
        source: MalformedPartNameError,
    },
    OversizedPart {
        part: String,
        cap_bytes: u64,
    },
    OversizedPackage {
        cap_bytes: u64,
    },
    /// The package file itself could not be read.
    ArchiveRead(std::io::Error),
    /// The ZIP archive's structure was refused (see `zip_check`).
    Structure(crate::zip_check::ZipStructureError),
    /// `[Content_Types].xml` or a relationship part is larger than any
    /// real package needs.
    MetadataPartTooLarge {
        part: String,
        cap_bytes: u64,
    },
    /// Two parts whose names differ only in ASCII case, which OPC treats
    /// as the same part.
    EquivalentPartNames {
        first: String,
        second: String,
    },
    /// An interleaved part's pieces are missing, repeated or out of order.
    BrokenPieces {
        part: String,
    },
    /// More than one StartPart relationship.
    SeveralStartParts,
    /// `[Content_Types].xml` declares an extension or a part twice.
    DuplicateContentType {
        key: String,
    },
    /// A relationship part for a part the package does not hold.
    OrphanRelationships {
        part: String,
    },
    /// `[Content_Types].xml` breaks a packaging rule, described.
    InvalidContentTypes {
        reason: String,
    },
    /// A relationship breaks a packaging rule, described.
    InvalidRelationship {
        part: String,
        reason: String,
    },
    /// The StartPart relationship points outside the package.
    ExternalStartPart {
        target: String,
    },
}

#[derive(Debug)]
pub enum ModelParseError {
    Xml(quick_xml::Error),
    AttrSyntax(quick_xml::events::attributes::AttrError),
    DtdForbidden,
    /// Something the core specification rules out, described.
    Forbidden {
        what: String,
    },
    UndeclaredEntity {
        name: String,
    },
    UnexpectedRootElement {
        found: String,
    },
    MissingModelElement,
    MissingRequiredAttribute {
        element: &'static str,
        attribute: &'static str,
    },
    InvalidUnit {
        value: String,
    },
    InvalidNumber {
        element: &'static str,
        attribute: &'static str,
        value: String,
    },
    InvalidMatrix {
        value: String,
    },
    InvalidResourceId {
        value: String,
    },
    InvalidObjectType {
        value: String,
    },
    DuplicateResourceId {
        id: u32,
    },
    UnknownObjectReference {
        id: u32,
    },
    TriangleIndexOutOfRange {
        object_id: u32,
        index: u32,
        vertex_count: usize,
    },
    ObjectWithoutGeometry {
        id: u32,
    },
    DuplicateGeometry {
        id: u32,
    },
    BuildItemReferencesOtherType {
        id: u32,
    },
    UnsupportedRequiredExtension {
        namespace: String,
    },
    UnknownRequiredExtensionPrefix {
        prefix: String,
    },
    ProductionPathUnsupported,
    SliceStackMixesSlicesAndRefs {
        id: u32,
    },
    UnknownSliceStackReference {
        object_id: u32,
        stack_id: u32,
    },
    InvalidMeshResolution {
        value: String,
    },
    PolygonWithoutSegments {
        stack_id: u32,
    },
    SliceIndexOutOfRange {
        stack_id: u32,
        index: u32,
        vertex_count: usize,
    },
}

impl From<quick_xml::encoding::EncodingError> for ModelParseError {
    fn from(error: quick_xml::encoding::EncodingError) -> Self {
        Self::Xml(error.into())
    }
}

#[derive(Debug)]
pub enum ModelWriteError {
    Io(std::io::Error),
    SliceZNotIncreasing {
        stack_id: u32,
        index: usize,
    },
    SliceZTopNotFinite {
        stack_id: u32,
        index: usize,
    },
    SliceZBottomNotFinite {
        stack_id: u32,
    },
    PolygonWithoutSegments {
        stack_id: u32,
    },
    SliceIndexOutOfRange {
        stack_id: u32,
        index: u32,
        vertex_count: usize,
    },
    OpenPolygonOnModelObject {
        object_id: u32,
        stack_id: u32,
    },
    UnknownSliceStackReference {
        object_id: u32,
        stack_id: u32,
    },
    NonPlanarSlicedTransform {
        object_id: u32,
    },
}

/// `before`, then `inner`'s message.
fn after(out: &mut fmt::Formatter<'_>, before: &str, inner: &dyn fmt::Display) -> fmt::Result {
    out.write_str(before)?;
    fmt::Display::fmt(inner, out)
}

// Said the same way by the reader and the writer of slice stacks.
fn empty_polygon(out: &mut fmt::Formatter<'_>, stack: u32) -> fmt::Result {
    write!(out, "slice stack {stack} has a polygon with no segments")
}

fn slice_vertex_past_end(out: &mut fmt::Formatter<'_>, stack: u32, vertex: u32, count: usize) -> fmt::Result {
    write!(out, "slice stack {stack} references slice vertex {vertex}, but only {count} vertices exist")
}

impl fmt::Display for MalformedPartNameError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let problem = match self {
            Self::Empty => "is empty",
            Self::NotAbsolute => "must start with '/'",
            Self::EmptySegment => "contains an empty segment",
            Self::TrailingDot => "segment ends with a dot",
        };
        write!(out, "part name {problem}")
    }
}

impl fmt::Display for PackageWriteError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zip(inner) => after(out, "could not write the package archive: ", inner),
            Self::Io(inner) => after(out, "could not write package data: ", inner),
            Self::DuplicatePartName { name: taken } => {
                write!(out, "package already contains a part named \"{taken}\"")
            }
        }
    }
}

impl fmt::Display for PackageReadError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zip(inner) => {
                out.write_str("the package archive is damaged or is not a ZIP file (")?;
                fmt::Display::fmt(inner, out)?;
                out.write_str(")")
            }
            Self::ArchiveRead(why) => write!(out, "could not read the package file: {why}"),
            Self::Structure(why) => write!(out, "the package was refused: {why}"),
            Self::MetadataPartTooLarge { part, cap_bytes: cap } => write!(
                out,
                "part \"{part}\" is larger than {cap} bytes, far beyond what any package needs; the package was refused"
            ),
            Self::EquivalentPartNames { first, second } => write!(
                out,
                "the package holds \"{first}\" and \"{second}\", names that only differ in case and so mean the same part"
            ),
            Self::BrokenPieces { part } => {
                write!(out, "part \"{part}\" is stored in pieces, and some are missing, repeated or out of order")
            }
            Self::DuplicateContentType { key } => write!(
                out,
                "[Content_Types].xml declares \"{key}\" twice, so programs could give it different types"
            ),
            Self::InvalidContentTypes { reason } => write!(out, "[Content_Types].xml is invalid: {reason}"),
            Self::InvalidRelationship { part, reason } => write!(out, "a relationship in \"{part}\" is invalid: {reason}"),
            Self::OrphanRelationships { part } => {
                write!(out, "relationship part \"{part}\" belongs to a part the package does not hold")
            }
            Self::SeveralStartParts => out.write_str(
                "the package has more than one StartPart relationship, so programs could open different models",
            ),
            Self::ExternalStartPart { target } => write!(
                out,
                "the package's StartPart relationship points outside the package, at \"{target}\""
            ),
            Self::ContentTypesXml(inner) => after(out, "[Content_Types].xml is not valid XML: ", inner),
            Self::MissingContentTypes => out.write_str("package has no [Content_Types].xml part"),
            Self::MissingStartPartRelationship => {
                out.write_str("package has no StartPart relationship pointing at a 3D Model part")
            }
            Self::PartRead { part: name, source: why } => write!(
                out,
                "the package archive is damaged: part \"{name}\" does not decompress correctly ({why})"
            ),
            Self::ContentTypeMissingAttribute { attribute: wanted } => {
                write!(out, "a content-type entry is missing its \"{wanted}\" attribute")
            }
            Self::RelationshipsXml { part: name, source: why } => {
                write!(out, "relationships part \"{name}\" is not valid XML: {why}")
            }
            Self::RelationshipMissingAttribute { part: name, attribute: wanted } => {
                write!(out, "a relationship in \"{name}\" is missing its \"{wanted}\" attribute")
            }
            Self::PartWithoutContentType { part: name } => write!(out, "part \"{name}\" has no content type"),
            Self::StartPartNotFound { target: aim } => {
                write!(out, "the StartPart relationship points at \"{aim}\", which is not in the package")
            }
            Self::MalformedPartName { name: entry, source: why } => {
                write!(out, "entry \"{entry}\" is not a valid part name: {why}")
            }
            Self::OversizedPart { part: name, cap_bytes: cap } => write!(
                out,
                "part \"{name}\" decompresses past the {cap}-byte per-part cap; the package was refused"
            ),
            Self::OversizedPackage { cap_bytes: cap } => {
                write!(out, "the package decompresses past the {cap}-byte total cap and was refused")
            }
        }
    }
}

impl fmt::Display for ModelParseError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(inner) => after(out, "model XML is unreadable: ", inner),
            Self::AttrSyntax(inner) => after(out, "model XML has a malformed attribute: ", inner),
            Self::DtdForbidden => out.write_str("model XML contains DTD content, which 3MF forbids"),
            Self::Forbidden { what } => write!(out, "the model breaks the 3MF core specification: {what}"),
            Self::MissingModelElement => out.write_str("document contains no <model> element"),
            Self::ProductionPathUnsupported => out.write_str(
                "model splits objects across parts via the production extension (p:path), \
                 which is not yet supported",
            ),
            &Self::PolygonWithoutSegments { stack_id } => empty_polygon(out, stack_id),
            &Self::SliceIndexOutOfRange { stack_id, index, vertex_count } => {
                slice_vertex_past_end(out, stack_id, index, vertex_count)
            }
            Self::UndeclaredEntity { name: entity } => {
                write!(out, "model XML references undeclared entity \"&{entity};\"")
            }
            Self::UnexpectedRootElement { found: tag } => {
                write!(out, "expected a core-namespace <model> root element, found <{tag}>")
            }
            Self::MissingRequiredAttribute { element: tag, attribute: wanted } => {
                write!(out, "<{tag}> is missing its \"{wanted}\" attribute")
            }
            Self::InvalidNumber { element: tag, attribute: name, value: text } => {
                write!(out, "<{tag}> attribute \"{name}\" is not a number: \"{text}\"")
            }
            Self::InvalidMatrix { value: text } => {
                write!(out, "transform is not 12 whitespace-separated numbers: \"{text}\"")
            }
            Self::InvalidResourceId { value: text } => {
                write!(out, "\"{text}\" is not a valid resource id (positive 32-bit integer)")
            }
            Self::InvalidUnit { value: text } => write!(out, "\"{text}\" is not a valid model unit"),
            Self::InvalidObjectType { value: text } => write!(out, "\"{text}\" is not a valid object type"),
            Self::InvalidMeshResolution { value: text } => {
                write!(out, "\"{text}\" is not a valid mesh resolution")
            }
            Self::DuplicateResourceId { id } => write!(out, "resource id {id} is defined more than once"),
            Self::UnknownObjectReference { id } => {
                write!(out, "reference to object id {id}, which is not defined at this point")
            }
            Self::TriangleIndexOutOfRange { object_id: object, index: vertex, vertex_count: count } => write!(
                out,
                "object {object} has a triangle referencing vertex {vertex}, but only {count} vertices exist"
            ),
            Self::ObjectWithoutGeometry { id } => write!(out, "object {id} has neither a mesh nor components"),
            Self::DuplicateGeometry { id } => write!(out, "object {id} has more than one geometry element"),
            Self::BuildItemReferencesOtherType { id } => {
                write!(out, "build item references object {id} of type \"other\", which 3MF forbids")
            }
            Self::UnsupportedRequiredExtension { namespace: uri } => {
                write!(out, "model requires the unsupported 3MF extension \"{uri}\"")
            }
            Self::UnknownRequiredExtensionPrefix { prefix: short } => {
                write!(out, "requiredextensions names prefix \"{short}\" with no xmlns declaration")
            }
            Self::SliceStackMixesSlicesAndRefs { id } => write!(
                out,
                "slice stack {id} mixes inline slices with slicerefs, which the slice extension forbids"
            ),
            Self::UnknownSliceStackReference { object_id: object, stack_id: stack } => write!(
                out,
                "object {object} references slice stack {stack}, which is not defined at this point"
            ),
        }
    }
}

impl fmt::Display for ModelWriteError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(inner) => after(out, "could not write model XML: ", inner),
            &Self::PolygonWithoutSegments { stack_id } => empty_polygon(out, stack_id),
            &Self::SliceIndexOutOfRange { stack_id, index, vertex_count } => {
                slice_vertex_past_end(out, stack_id, index, vertex_count)
            }
            Self::SliceZNotIncreasing { stack_id: stack, index: slice } => {
                write!(out, "slice stack {stack}: slice {slice} does not increase the stack's z")
            }
            Self::SliceZTopNotFinite { stack_id: stack, index: slice } => {
                write!(out, "slice stack {stack}: slice {slice} has a non-finite ztop")
            }
            Self::SliceZBottomNotFinite { stack_id: stack } => {
                write!(out, "slice stack {stack}: the zbottom is not finite")
            }
            Self::OpenPolygonOnModelObject { object_id: object, stack_id: stack } => write!(
                out,
                "object {object} requires closed slice contours, but stack {stack} has an open polygon"
            ),
            Self::UnknownSliceStackReference { object_id: object, stack_id: stack } => write!(
                out,
                "object {object} references slice stack {stack}, which the document does not define"
            ),
            Self::NonPlanarSlicedTransform { object_id: object } => {
                write!(out, "a transform applied to sliced object {object} is not planar")
            }
        }
    }
}
