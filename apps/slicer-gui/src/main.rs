// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Sleptic Slicer's desktop app. Everything lives in the library; this
//! binary only starts it.

// A release build on Windows is a GUI program, so starting it from Explorer
// opens no console window beside it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    sc_slicer_gui::run()
}
