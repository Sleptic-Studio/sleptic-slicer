// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Reading and writing 3MF files at the format's level: the ZIP package
//! with its parts and relationships (`package`, guarded by `zip_check`),
//! the XML model part (`model`), and the namespaces and names both use
//! (`names`). Meshes and scenes are built from this elsewhere.
pub mod package;
pub mod zip_check;

pub mod model;
pub mod names;

pub mod error;

#[cfg(test)]
mod tests_error;
#[cfg(test)]
mod tests_error_messages;
#[cfg(test)]
mod tests_hardening;
#[cfg(test)]
mod tests_model;
