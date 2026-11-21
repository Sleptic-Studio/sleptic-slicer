// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! A file the user asked to open before the GPU was ready (a double-click
//! on a file, a drop during startup). Only the newest such request is kept;
//! graphics start-up loads it.
use std::cell::Cell;
use std::path::PathBuf;

#[derive(Default)]
pub(crate) struct WaitingFile(Cell<Option<PathBuf>>);

impl WaitingFile {
    pub(crate) fn new() -> Self {
        WaitingFile::default()
    }

    /// Keeps `path`, replacing any earlier request.
    pub(crate) fn hold(&self, path: PathBuf) {
        self.0.set(Some(path));
    }

    /// Hands the request over; nothing is waiting afterwards.
    pub(crate) fn take(&self) -> Option<PathBuf> {
        self.0.take()
    }

    pub(crate) fn is_queued(&self) -> bool {
        let held = self.0.take();
        let waiting = held.is_some();
        self.0.set(held);
        waiting
    }

    /// Whether adding models now would be wasted: a load is running, the
    /// project question is open, or a waiting file will replace the scene.
    pub(crate) fn blocks_import(&self, loading: bool, project_prompt: bool) -> bool {
        loading || project_prompt || self.is_queued()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::WaitingFile;

    #[test]
    fn the_newest_request_waits_until_taken_once() {
        let waiting = WaitingFile::new();
        assert!(!waiting.is_queued() && waiting.take().is_none());
        waiting.hold(PathBuf::from("first.stl"));
        let newest = PathBuf::from("a folder/../newest part.3mf");
        waiting.hold(newest.clone());
        // Looking does not use it up.
        assert!(waiting.is_queued() && waiting.is_queued());
        assert_eq!(waiting.take(), Some(newest));
        assert_eq!(waiting.take(), None);
        assert!(!WaitingFile::default().is_queued());
        // Even an empty path is a request.
        waiting.hold(PathBuf::new());
        assert_eq!(waiting.take(), Some(PathBuf::new()));
    }

    #[test]
    fn imports_wait_for_loads_prompts_and_waiting_files() {
        for (queued, loading, prompt) in (0..8).map(|bits| (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0)) {
            let waiting = WaitingFile::new();
            if queued {
                waiting.hold(PathBuf::from("startup.3mf"));
            }
            assert_eq!(waiting.blocks_import(loading, prompt), queued || loading || prompt);
            assert_eq!(waiting.is_queued(), queued, "the check leaves the request");
        }
    }
}
