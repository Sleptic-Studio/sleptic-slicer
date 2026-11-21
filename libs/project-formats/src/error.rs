// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Why a project could not be read, written or opened. Most of these only
//! wrap a lower layer's error under a short prefix saying which step
//! failed; [`wrapping_error!`] writes such an enum from one line per kind.
use sc_3mf::error::{ModelWriteError, PackageReadError, PackageWriteError};
use sc_mesh_formats::error::{LoadMeshError, LoadThreeMfError, WriteThreeMfError};
use std::{error::Error, fmt, io, path::PathBuf};

/// An error enum whose every kind wraps one cause: each kind converts
/// from its cause, reads as `prefix` followed by the cause, and names the
/// cause as its source.
macro_rules! wrapping_error {
    ($(#[$doc:meta])* pub enum $name:ident { $($kind:ident($cause:ty) => $prefix:literal,)+ }) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub enum $name {
            $($kind($cause),)+
        }

        $(impl From<$cause> for $name {
            fn from(cause: $cause) -> $name {
                $name::$kind(cause)
            }
        })+

        impl fmt::Display for $name {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $($name::$kind(cause) => write!(out, "{}{cause}", $prefix),)+
                }
            }
        }

        impl Error for $name {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                match self {
                    $($name::$kind(cause) => Some(cause),)+
                }
            }
        }
    };
}

wrapping_error! {
    /// Writing a project as a 3MF package.
    pub enum WriteThreeMfProjectError {
        Scene(WriteThreeMfError) => "3MF project writer: ",
        Model(ModelWriteError) => "3MF project writer: ",
        SliceMapping(crate::threemf::SliceMappingError) => "3MF project writer: ",
        Payload(toml_edit::ser::Error) => "3MF project writer: failed to serialize the project part: ",
        Package(PackageWriteError) => "3MF project writer: ",
        Io(io::Error) => "3MF project writer I/O error: ",
    }
}

wrapping_error! {
    /// Reading a project from a 3MF package.
    pub enum ReadThreeMfProjectError {
        Package(PackageReadError) => "3MF project reader: ",
        Scene(LoadThreeMfError) => "3MF project reader: ",
        PayloadNotUtf8(std::str::Utf8Error) => "3MF project reader: the project part is not UTF-8: ",
        PayloadParse(toml_edit::de::Error) => "3MF project reader: failed to parse the project part: ",
    }
}

wrapping_error! {
    /// Reading a project, whatever its format (3MF is the only one yet).
    pub enum ReadProjectError {
        ThreeMf(ReadThreeMfProjectError) => "",
    }
}

wrapping_error! {
    /// Writing a project, whatever its format (3MF is the only one yet).
    pub enum WriteProjectError {
        ThreeMf(WriteThreeMfProjectError) => "",
    }
}

/// Opening a file as a scene: a mesh file or a project.
#[derive(Debug)]
pub enum LoadSceneError {
    /// The extension is none the app reads.
    UnsupportedExtension { path: PathBuf },
    /// The file is bigger than the app will read into memory.
    FileTooLarge { actual_bytes: u64, limit_bytes: u64 },
    Io(io::Error),
    Mesh(LoadMeshError),
    Project(ReadProjectError),
}

impl From<io::Error> for LoadSceneError {
    fn from(cause: io::Error) -> LoadSceneError {
        LoadSceneError::Io(cause)
    }
}

impl From<LoadMeshError> for LoadSceneError {
    fn from(cause: LoadMeshError) -> LoadSceneError {
        LoadSceneError::Mesh(cause)
    }
}

impl From<ReadProjectError> for LoadSceneError {
    fn from(cause: ReadProjectError) -> LoadSceneError {
        LoadSceneError::Project(cause)
    }
}

impl LoadSceneError {
    /// The lower layer's error, for the kinds that wrap one.
    fn cause(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            LoadSceneError::Io(cause) => Some(cause),
            LoadSceneError::Mesh(cause) => Some(cause),
            LoadSceneError::Project(cause) => Some(cause),
            LoadSceneError::UnsupportedExtension { .. } | LoadSceneError::FileTooLarge { .. } => None,
        }
    }
}

impl fmt::Display for LoadSceneError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadSceneError::UnsupportedExtension { path } => {
                let shown = path.display();
                write!(out, "cannot open {shown}: the extension names no supported scene format")
            }
            LoadSceneError::FileTooLarge { actual_bytes: size, limit_bytes: limit } => {
                write!(out, "file is {size} bytes, which exceeds the {limit} byte limit")
            }
            LoadSceneError::Io(cause) => write!(out, "I/O error while opening the scene file: {cause}"),
            // A mesh or project error already says what went wrong.
            LoadSceneError::Mesh(cause) => cause.fmt(out),
            LoadSceneError::Project(cause) => cause.fmt(out),
        }
    }
}

impl Error for LoadSceneError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause()
    }
}
