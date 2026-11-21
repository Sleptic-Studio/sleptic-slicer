// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Reusable reversible edits and geometry-sharing snapshots over `sc_scene`.

pub mod edit;
pub mod fragment;
pub use edit::{LeafRecord, MeshRecord, SceneEdit};
pub use fragment::SceneFragment;
#[cfg(test)]
mod tests_edit;
