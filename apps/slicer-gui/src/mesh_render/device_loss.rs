// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Remembers, for the whole process, that the graphics device is gone.
//!
//! A lost device is reported only once, to its device-lost callback.
//! Error scopes and the uncaptured-error handler never see it; whatever is
//! made afterwards is silently invalid, and the next buffer mapping
//! panics. The callback writes the loss here and every user of the device
//! looks here first.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// A handle on the one record; clones share it across threads.
#[derive(Clone, Debug, Default)]
pub struct DeviceLoss {
    /// Why the device went, from the first report only.
    why: Arc<OnceLock<String>>,
    /// Whether the user has been told.
    announced: Arc<AtomicBool>,
}

impl DeviceLoss {
    /// Records that the device is lost. Only the first reason is kept: a
    /// lost device reports again as it is torn down.
    pub fn mark(&self, reason: String) {
        self.why.get_or_init(|| reason);
    }

    pub fn is_lost(&self) -> bool {
        self.why.get().is_some()
    }

    pub fn reason(&self) -> Option<String> {
        self.why.get().map(String::clone)
    }

    /// Whether the caller is the one to tell the user: true once, then
    /// false for everyone.
    pub fn claim_report(&self) -> bool {
        self.announced
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::DeviceLoss;

    #[test]
    fn a_loss_on_another_thread_is_seen_here_with_its_first_reason() {
        let here = DeviceLoss::default();
        assert!(!here.is_lost() && here.reason().is_none());
        let there = here.clone();
        std::thread::spawn(move || {
            there.mark("Destroyed: on the render thread".to_owned());
            there.mark("Unknown: while tearing down".to_owned());
        })
        .join()
        .unwrap();
        here.mark("ignored".to_owned());
        assert!(here.is_lost());
        assert_eq!(here.reason(), Some("Destroyed: on the render thread".to_owned()));
    }

    #[test]
    fn the_user_is_told_once_whoever_asks() {
        let first = DeviceLoss::default();
        let second = first.clone();
        let told: Vec<bool> = [&second, &first, &second].iter().map(|handle| handle.claim_report()).collect();
        assert_eq!(told, [true, false, false]);
    }
}
