// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Typed profile documents, library persistence, and deterministic process resolution.
pub use sc_print_primitives::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};
pub mod job_profile;
pub mod library;
pub mod material;
pub mod motion;
pub mod orient;
pub mod package;
pub mod printer;
pub mod recipes;
pub mod resolve;
pub mod scene_overrides;
pub mod support_settings;
pub use job_profile::{CustomValue, JobKind, JobTemplates};
pub use library::*;
pub use material::*;
pub use package::*;
pub use printer::*;
pub use resolve::*;
pub use scene_overrides::SceneOverrides;
pub use support_settings::{InvalidSupportSettings, SupportPreset};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ImageAssetRef(String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonLibraryRelativeImageRefError(pub &'static str);
impl fmt::Display for NonLibraryRelativeImageRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for NonLibraryRelativeImageRefError {}
impl ImageAssetRef {
    pub fn new(s: &str) -> Result<Self, NonLibraryRelativeImageRefError> {
        let error = if s.is_empty() {
            Some("image reference must not be empty")
        } else if s.starts_with(['/', '\\'])
            || (s.as_bytes().get(1) == Some(&b':') && s.as_bytes()[0].is_ascii_alphabetic())
        {
            Some("image reference must be a relative path")
        } else if s.split('/').any(|v| matches!(v, "" | "." | "..")) {
            Some("image reference components must be plain names (no `.`, `..`, or empty segments)")
        } else {
            None
        };
        match error {
            Some(e) => Err(NonLibraryRelativeImageRefError(e)),
            None => Ok(Self(s.to_owned())),
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn resolve(&self, root: &Path) -> PathBuf {
        self.0.split('/').fold(root.to_path_buf(), |p, s| p.join(s))
    }
    /// The file this reference names inside library `root`.
    pub fn resolve_in(&self, root: &Path) -> PathBuf {
        self.resolve(root)
    }
}
impl TryFrom<String> for ImageAssetRef {
    type Error = NonLibraryRelativeImageRefError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}
impl From<ImageAssetRef> for String {
    fn from(v: ImageAssetRef) -> Self {
        v.0
    }
}
impl std::str::FromStr for ImageAssetRef {
    type Err = NonLibraryRelativeImageRefError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}
impl fmt::Display for ImageAssetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests_spec;

pub mod material_resolved {
    pub use crate::resolve::MaterialResolved;
}
pub mod printer_profile {
    pub use crate::printer::*;
}
pub mod material_profile {
    pub use crate::material::*;
}

pub mod resolved_field {
    pub use crate::resolve::{OverrideRecord, ResolvedField, ValueSource};
}
