// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
pub mod error;
pub mod filetypes;
pub mod loader;
pub mod messages;
pub mod obj;
pub mod preview;
pub mod scene_writer;
pub mod stl;
pub mod threemf;
pub use loader::{LoadedMesh, load_mesh_from_path, load_mesh_from_reader};
pub const DEFAULT_MAX_MESH_FILE_SIZE_BYTES: u64 = 2147483648;
pub const MAX_MESH_ELEMENT_COUNT: usize = 200000000;
#[cfg(test)]
mod tests_formats;
pub(crate) fn clean_name(s: &str, ascii: bool) -> Option<String> {
    let clean: String = s
        .chars()
        .filter(|c| {
            (!c.is_control() || c.is_whitespace())
                && !matches!(*c as u32, 0xfdd0..=0xfdef)
                && (*c as u32 & 0xffff) < 0xfffe
        })
        .collect();
    let clean = if ascii {
        any_ascii::any_ascii(&clean)
    } else {
        clean
    };
    let s = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests_error;
#[cfg(test)]
mod tests_error_messages;

#[cfg(test)]
mod tests_filetypes;
