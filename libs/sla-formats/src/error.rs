// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What can go wrong writing sliced output.
use crate::Format;
use std::{fmt, path::PathBuf};
#[derive(Debug)]
pub enum WriterError {
    Io(std::io::Error),
    OutputPathInvalid {
        reason: String,
    },
    OutputPathExistsAndIsNotADirectory {
        path: PathBuf,
    },
    OutputPathExistsAndIsNotEmpty {
        path: PathBuf,
    },
    EncodingFailed {
        format: Format,
        reason: String,
    },
    SceneContextRequired {
        format: Format,
    },
    NoModelsToWrite,
    LayerGridMismatch {
        leaf: sc_scene::NodeId,
        schedule_layers: usize,
        stack_layers: usize,
    },
    SceneBakeFailed(sc_scene::SceneError),
    ModelSerializationFailed(sc_3mf::error::ModelWriteError),
    PackageWriteFailed(sc_3mf::error::PackageWriteError),
}
impl fmt::Display for WriterError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoModelsToWrite => out.write_str("the per-leaf slices list is empty; nothing to write"),
            Self::Io(why) => write!(out, "filesystem operation failed: {why}"),
            Self::SceneBakeFailed(why) => write!(out, "baking a leaf mesh into world space failed: {why}"),
            Self::ModelSerializationFailed(why) => write!(out, "serializing a 3MF model document failed: {why}"),
            Self::PackageWriteFailed(why) => write!(out, "writing the 3MF package failed: {why}"),
            Self::OutputPathInvalid { reason } => write!(out, "output path is invalid: {reason}"),
            Self::OutputPathExistsAndIsNotADirectory { path } => {
                write!(out, "output path \"{}\" exists and is not a directory", path.display())
            }
            Self::OutputPathExistsAndIsNotEmpty { path } => {
                write!(out, "output directory \"{}\" exists and is not empty", path.display())
            }
            Self::EncodingFailed { format, reason } => {
                write!(out, "encoder for format \"{format}\" rejected the request: {reason}")
            }
            Self::SceneContextRequired { format } => write!(
                out,
                "format \"{format}\" stores per-model slice stacks; \
                 it cannot be written from a merged contour stack alone"
            ),
            Self::LayerGridMismatch { leaf, schedule_layers: planned, stack_layers: found } => write!(
                out,
                "leaf #{} carries {found} layer(s) but the layer schedule defines {planned}",
                leaf.get()
            ),
        }
    }
}
impl std::error::Error for WriterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Io(why) => why,
            Self::SceneBakeFailed(why) => why,
            Self::ModelSerializationFailed(why) => why,
            Self::PackageWriteFailed(why) => why,
            _ => return None,
        })
    }
}
macro_rules! wraps {
    ($($from:ty => $variant:ident),* $(,)?) => {$(
        impl From<$from> for WriterError {
            fn from(inner: $from) -> Self {
                Self::$variant(inner)
            }
        }
    )*};
}
wraps! {
    std::io::Error => Io,
    sc_3mf::error::ModelWriteError => ModelSerializationFailed,
    sc_3mf::error::PackageWriteError => PackageWriteFailed,
}
/// An encoder's refusal of `format`, with its message as the reason.
pub(crate) fn encoding(format: Format, refusal: impl fmt::Display) -> WriterError {
    let reason = refusal.to_string();
    WriterError::EncodingFailed { format, reason }
}
