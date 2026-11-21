// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Why a window's 3D view has no graphics device, and what to tell the user.
//!
//! The device itself sits in `SceneLoader`; this only answers the question
//! that slot cannot: will a device still come (wait for it) or not (say so)?
//! Slint sets rendering up on a window's first paint, so "not yet" is the
//! normal state of a new window.
//!
//! The message texts are translation keys: changing a word means changing
//! every catalogue in `i18n/`.
use std::cell::RefCell;

use crate::i18n::ctr;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum NoGraphics {
    /// No device yet, or the last one was torn down; one may still arrive.
    #[default]
    NotYet,
    /// The window renders through this graphics API (as Slint names it),
    /// which the 3D view cannot use.
    ForeignApi(String),
    /// The device refused one of the 3D view's resources, for this reason;
    /// it would refuse again.
    Refused(String),
    /// The device is gone for this process (driver reset, removal).
    Gone,
}

impl NoGraphics {
    /// True only while waiting is worth it.
    pub(crate) fn worth_waiting(&self) -> bool {
        *self == Self::NotYet
    }

    /// The dialog text: retry advice only where a retry can help.
    pub(crate) fn explain(&self) -> String {
        let text = match self {
            Self::NotYet => ctr!(
                "ErrorDialog",
                "The 3D view is still starting up. Please try again in a moment."
            ),
            Self::ForeignApi(api) => ctr!(
                "ErrorDialog",
                "The 3D view could not start: this window renders through {}, which this build cannot use for 3D.",
                api
            ),
            Self::Refused(reason) => ctr!(
                "ErrorDialog",
                "{} Files cannot be opened in this window.",
                reason
            ),
            Self::Gone => ctr!(
                "ErrorDialog",
                "The graphics device was lost. Save your work and restart the application."
            ),
        };
        text.to_string()
    }
}

/// The current reason, kept by the loader next to its device slot and only
/// meaningful while that slot is empty. Publishing a device resets it, so a
/// later teardown reads as "waiting" rather than an old complaint.
#[derive(Debug, Default)]
pub(crate) struct GraphicsAbsence {
    current: RefCell<NoGraphics>,
}

impl GraphicsAbsence {
    pub(crate) fn why(&self) -> NoGraphics {
        self.current.borrow().clone()
    }

    fn record(&self, reason: NoGraphics) {
        self.current.replace(reason);
    }

    pub(crate) fn device_arrived(&self) {
        self.record(NoGraphics::NotYet);
    }

    pub(crate) fn foreign_api(&self, api: String) {
        self.record(NoGraphics::ForeignApi(api));
    }

    pub(crate) fn renderer_refused(&self, reason: String) {
        self.record(NoGraphics::Refused(reason));
    }

    pub(crate) fn device_gone(&self) {
        self.record(NoGraphics::Gone);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use NoGraphics::*;

    fn api(name: &str) -> NoGraphics {
        ForeignApi(name.to_string())
    }

    #[test]
    fn the_gap_follows_the_latest_event_and_a_device_clears_it() {
        let gap = GraphicsAbsence::default();
        assert_eq!(gap.why(), NotYet);
        gap.foreign_api("GraphicsAPI::NativeOpenGL".into());
        gap.foreign_api("GraphicsAPI::WebGL".into());
        assert_eq!(gap.why(), api("GraphicsAPI::WebGL"));
        gap.device_arrived();
        gap.device_arrived();
        assert_eq!(gap.why(), NotYet);
        gap.renderer_refused("mesh pipeline refused".into());
        assert_eq!(gap.why(), Refused("mesh pipeline refused".into()));
        gap.device_arrived();
        gap.device_gone();
        assert_eq!(gap.why(), Gone);
        gap.device_arrived();
        gap.foreign_api("GraphicsAPI::WebGL".into());
        assert_eq!(gap.why(), api("GraphicsAPI::WebGL"));
    }

    #[test]
    fn only_a_missing_setup_is_worth_waiting_for() {
        assert!(NotYet.worth_waiting());
        for gone in [api("GraphicsAPI::NativeOpenGL"), Refused("x".into()), Gone] {
            assert!(!gone.worth_waiting(), "{gone:?}");
        }
    }

    #[test]
    fn each_reason_has_its_own_message_and_only_waiting_suggests_retrying() {
        let reasons = [
            NotYet,
            api("GraphicsAPI::NativeOpenGL"),
            Refused(
                "The 3D view could not build its mesh pipeline: Validation Error: x".into(),
            ),
            Gone,
        ];
        let messages: Vec<String> = reasons.iter().map(NoGraphics::explain).collect();
        for (at, message) in messages.iter().enumerate() {
            assert_eq!(message.contains("again"), at == 0, "{message}");
            for other in &messages[at + 1..] {
                assert_ne!(message, other);
            }
        }
        assert!(messages[1].contains("GraphicsAPI::NativeOpenGL"));
        assert!(messages[2].contains("mesh pipeline") && messages[2].contains("cannot be opened"));
        assert!(messages[3].contains("lost") && messages[3].contains("restart"));
    }
}
