// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Checked documents and any external assets needed when saving them.
use sc_print_profiles::{MaterialProfile, PrinterProfile};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Document {
    Printer {
        profile: PrinterProfile,
        image_source: Option<PathBuf>,
    },
    Material(MaterialProfile),
}
