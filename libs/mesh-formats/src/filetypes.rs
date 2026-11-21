// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Mesh file formats by name: the ones a path's extension or a command
//! line can name, and the subset the app writes.
use crate::error::InferFileTypeError;
use std::{error::Error, fmt, path::Path, str::FromStr};

/// A mesh format by name. Plain `Stl` leaves binary or text to the file;
/// PLY and AMF are recognised but not read or written yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshFileType {
    Stl,
    StlAscii,
    StlBin,
    Obj,
    Ply,
    Amf,
    _3mf,
}

/// Every name a format answers to, in any letter case. The underscored
/// ones are only for the command line (see
/// [`infer_mesh_file_format_from_extension`]).
const NAMES: [(MeshFileType, &[&str]); 7] = [
    (MeshFileType::_3mf, &["3mf"]),
    (MeshFileType::Stl, &["stl"]),
    (MeshFileType::StlAscii, &["stla", "stl_ascii"]),
    (MeshFileType::StlBin, &["stlb", "stl_bin"]),
    (MeshFileType::Obj, &["obj"]),
    (MeshFileType::Ply, &["ply"]),
    (MeshFileType::Amf, &["amf"]),
];

/// A name that is none of [`MeshFileType`]'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidMeshFileType;

impl fmt::Display for InvalidMeshFileType {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("Invalid mesh file format type")
    }
}

impl Error for InvalidMeshFileType {}

impl FromStr for MeshFileType {
    type Err = InvalidMeshFileType;

    fn from_str(name: &str) -> Result<MeshFileType, InvalidMeshFileType> {
        let known = |(_, spellings): &&(MeshFileType, &[&str])| {
            spellings.iter().any(|spelling| spelling.eq_ignore_ascii_case(name))
        };
        NAMES.iter().find(known).map(|(kind, _)| *kind).ok_or(InvalidMeshFileType)
    }
}

/// The format `path`'s extension names. Underscored names (`stl_bin`) are
/// command-line spellings, never extensions, so they do not count here.
pub fn infer_mesh_file_format_from_extension(path: &Path) -> Result<MeshFileType, InferFileTypeError> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if !extension.contains('_') => extension.parse().ok(),
        _ => None,
    }
    .ok_or_else(|| InferFileTypeError::UnknownExtension { path: path.into() })
}

/// The formats the app writes. A plain STL is written binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteFormat {
    ThreeMf,
    Obj,
    StlBin,
    StlAscii,
}

/// A format the app reads (or recognises) but cannot write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedWriteFormat(pub MeshFileType);

impl fmt::Display for UnsupportedWriteFormat {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let UnsupportedWriteFormat(kind) = self;
        write!(out, "Writing {kind:?} files is not implemented yet")
    }
}

impl Error for UnsupportedWriteFormat {}

impl TryFrom<MeshFileType> for WriteFormat {
    type Error = UnsupportedWriteFormat;

    fn try_from(kind: MeshFileType) -> Result<WriteFormat, UnsupportedWriteFormat> {
        Ok(match kind {
            MeshFileType::_3mf => WriteFormat::ThreeMf,
            MeshFileType::Obj => WriteFormat::Obj,
            MeshFileType::Stl | MeshFileType::StlBin => WriteFormat::StlBin,
            MeshFileType::StlAscii => WriteFormat::StlAscii,
            MeshFileType::Ply | MeshFileType::Amf => return Err(UnsupportedWriteFormat(kind)),
        })
    }
}
