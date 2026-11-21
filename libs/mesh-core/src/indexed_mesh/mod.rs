// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Triangle meshes as a vertex list and faces that index into it, and what
//! a loaded mesh goes through before slicing. Everything is re-exported
//! here; the modules follow the order a mesh meets them.

// The data.
mod mesh;
mod topology;
pub use mesh::*;
pub use topology::*;

// Checking it: plain checks, then views that hold a checked mesh.
mod validate;
pub(crate) mod validated;
pub use validate::*;
pub use validated::*;

// Saying what is wrong, per face and grouped for a report.
mod diagnose;
mod error_summary;
pub use diagnose::*;
pub use error_summary::*;

// Putting it right.
mod fix;
mod repair;
pub use fix::*;
pub use repair::*;
