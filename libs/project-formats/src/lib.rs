// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
pub mod error;
pub mod filetypes;
pub mod loader;
pub mod payload;
pub mod read;
pub mod slices;
pub mod threemf;
pub mod write;
pub use error::{LoadSceneError, ReadProjectError, WriteProjectError};
pub use filetypes::{
    InferProjectFileTypeError, ProjectFileType, infer_project_file_type_from_extension,
};
pub use loader::{
    LoadProgress, LoadedScene, SUPPORTED_SCENE_FILE_EXTENSIONS, is_supported_scene_file_extension,
    load_scene,
};
pub use payload::*;
pub use read::{ForeignDialect, LoadedProject, ReadProjectWarning, read_project};
pub use slices::PlateSlices;
pub use write::{EmbedProfiles, WriteProgress, write_project, write_project_with_progress};

#[cfg(test)]
mod tests_project;
