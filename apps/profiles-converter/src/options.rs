// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
#[derive(Debug, Clone)]
pub struct RawCliArgs {
    pub format: String,
    pub inputs: Vec<PathBuf>,
    pub out: PathBuf,
    pub with_images: bool,
}

use std::path::PathBuf;
use std::{fmt, path::Path};

/// A validated set of command-line choices.
#[derive(Debug, Clone)]
pub struct ConversionRequest(RawCliArgs);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidOptions {
    NoInputs,
    UnsupportedFormat(String),
}

impl fmt::Display for InvalidOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputs => f.write_str("At least one source file or directory is required"),
            Self::UnsupportedFormat(value) => write!(
                f,
                "Unsupported profile format {value:?}; choose json"
            ),
        }
    }
}
impl std::error::Error for InvalidOptions {}

impl ConversionRequest {
    pub fn from_raw(raw: RawCliArgs) -> Result<Self, InvalidOptions> {
        if raw.format != "json" {
            return Err(InvalidOptions::UnsupportedFormat(raw.format));
        }
        if raw.inputs.is_empty() {
            return Err(InvalidOptions::NoInputs);
        }
        Ok(Self(raw))
    }

    pub fn format(&self) -> &str {
        &self.0.format
    }
    pub fn inputs(&self) -> &[PathBuf] {
        &self.0.inputs
    }
    pub fn out(&self) -> &Path {
        &self.0.out
    }
    pub fn with_images(&self) -> bool {
        self.0.with_images
    }

    pub fn summary(&self) -> String {
        let images = if self.0.with_images { "on" } else { "off" };
        format!(
            "format: {}; inputs: {}; output: {}; images: {}",
            self.0.format,
            self.0.inputs.len(),
            self.0.out.display(),
            images
        )
    }
}
