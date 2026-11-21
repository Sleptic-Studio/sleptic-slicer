// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
pub mod debug;
pub mod error;
pub mod format;
pub mod gcode_zip;
pub mod options;
pub mod threemf;
pub mod write;
pub use error::WriterError;
pub use format::Format;
pub use gcode_zip::{PreparedJob, write_gcode_zip};
pub use options::WriterOptions;
pub use threemf::write_scene_slices;
pub use write::write_slice;

#[cfg(test)]
mod tests_formats;
#[cfg(test)]
mod tests_error_messages;
