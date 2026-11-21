// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Every load warning in one place. Each reader defines its own, next to
//! the code that raises it; this module gathers them for callers.
pub use crate::loader::{LoadMeshWarning, SceneLoadWarning, SharedVertexLeaf};
pub use crate::obj::LoadObjWarning;
pub use crate::stl::LoadStlWarning;
pub use crate::threemf::LoadThreeMfWarning;
