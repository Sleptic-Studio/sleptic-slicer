// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! One module per subcommand, plus what the mesh commands share.
pub(crate) mod convert;
pub(crate) mod diagnose;
pub(crate) mod fix;
pub(crate) mod inspect;
mod mesh_files;
pub(crate) mod profiles;
pub(crate) mod report;
pub(crate) mod slice;
