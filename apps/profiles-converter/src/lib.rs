// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Conversion of external profile libraries into checked Sleptic documents.
pub mod args;
pub mod documents;
pub mod json_catalog;
pub mod library;
pub mod options;
pub mod pipeline;
#[cfg(test)]
mod tests_spec;
