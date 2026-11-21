// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Decoding pictures so that one bad file costs only itself.
//!
//! Image decoders can panic on malformed files (the `png` crate has been
//! seen to). Printer pictures come from profile packages and thumbnails
//! from a cache on disk, so a damaged one must not take the others, or the
//! app, down with it: each decode runs on its own and a panic counts as a
//! picture that could not be read.
use std::path::{Path, PathBuf};

/// `decode()`'s result, or none when it panicked (`what` names the picture
/// in the warning).
pub(crate) fn decode_guarded<T>(what: &dyn std::fmt::Display, decode: impl FnOnce() -> Option<T>) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(decode)) {
        Ok(picture) => picture,
        Err(_) => {
            eprintln!("warning: the image decoder failed on {what}; that picture was skipped");
            None
        }
    }
}

/// Each of `files` decoded on its own, in order, keeping those that decode.
pub(crate) fn decode_each<T>(files: Vec<(usize, PathBuf)>, decode: impl Fn(&Path) -> Option<T>) -> Vec<(usize, T)> {
    files
        .into_iter()
        .filter_map(|(index, file)| decode_guarded(&file.display(), || decode(&file)).map(|picture| (index, picture)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decoder_panic_costs_only_that_picture() {
        let files = ["a.png", "broken.png", "c.png"].map(PathBuf::from).into_iter().enumerate().collect();
        let decoded = decode_each(files, |path| {
            if path.ends_with("broken.png") {
                panic!("malformed chunk");
            }
            Some(path.to_string_lossy().into_owned())
        });
        assert_eq!(decoded, [(0, "a.png".to_string()), (2, "c.png".to_string())]);
    }

    #[test]
    fn a_panic_reads_as_no_picture() {
        assert_eq!(decode_guarded(&"x", || -> Option<u8> { panic!("bad") }), None);
        assert_eq!(decode_guarded(&"x", || Some(7)), Some(7));
    }
}
