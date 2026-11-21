// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The slice the document holds, and whether pressing Slice again would
//! change it: nothing held (slice), the same inputs (nothing to do), or
//! different ones (re-slicing replaces it, after confirming).
use sc_print_primitives::{AaLevel, MaterialId, PositiveFiniteMm, PrinterId};
use sc_sl1cer::contour::ContourStack;
use sc_sl1cer::profile::{LayerSchedule, SliceProfile};

use crate::scene_document::Revision;

/// What a slice was made from. No first-layer height means the resin's
/// layer height; `geometry` marks the scene as it was.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SliceParams {
    pub printer: PrinterId,
    pub resin: MaterialId,
    pub first_layer_height: Option<PositiveFiniteMm>,
    pub aa_level: AaLevel,
    pub geometry: Revision,
}

impl SliceParams {
    /// Everything a slice is made from except the geometry, which the
    /// document's edit history judges.
    fn choice(&self) -> (&PrinterId, &MaterialId, Option<PositiveFiniteMm>, AaLevel) {
        (&self.printer, &self.resin, self.first_layer_height, self.aa_level)
    }
}

/// A slice the document holds.
#[derive(Clone, Debug)]
pub(crate) struct SliceResult {
    /// The plate's joined contours in world coordinates (saving, the layer
    /// view).
    pub merged: ContourStack,
    /// Its layers; saving compares them with what slicing now would give.
    pub schedule: LayerSchedule,
    /// The inputs' fingerprint, saved so reopening can tell whether the
    /// slices still fit.
    pub token: String,
    pub params: SliceParams,
    /// The profile it was really made with. Export uses this one, since the
    /// profiles on disk may have changed since.
    pub profile: SliceProfile,
}

/// What pressing Slice should do about the slice held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SliceAgain {
    FirstSlice,
    Unchanged,
    Changed,
}

impl SliceAgain {
    /// `geometry_changed` comes from the document's edit history (asked
    /// only when a slice is held); the other inputs are compared here.
    pub(crate) fn compare(held: Option<&SliceParams>, now: &SliceParams, geometry_changed: bool) -> SliceAgain {
        match held {
            None => SliceAgain::FirstSlice,
            Some(before) if before.choice() == now.choice() && !geometry_changed => SliceAgain::Unchanged,
            Some(_) => SliceAgain::Changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held() -> SliceParams {
        SliceParams {
            printer: PrinterId::new("elegoo-saturn-4-ultra").unwrap(),
            resin: MaterialId::new("standard-405").unwrap(),
            first_layer_height: Some(PositiveFiniteMm::new(0.05).unwrap()),
            aa_level: AaLevel::Off,
            geometry: Revision::with_head(Some(7)),
        }
    }

    #[test]
    fn nothing_held_slices_and_identical_inputs_do_nothing() {
        assert_eq!(SliceAgain::compare(None, &held(), true), SliceAgain::FirstSlice);
        assert_eq!(SliceAgain::compare(Some(&held()), &held(), false), SliceAgain::Unchanged);
        assert_eq!(SliceAgain::compare(Some(&held()), &held(), true), SliceAgain::Changed);
    }

    #[test]
    fn any_changed_input_means_a_new_slice() {
        let changes = [
            SliceParams { printer: PrinterId::new("anycubic-photon-mono-x").unwrap(), ..held() },
            SliceParams { resin: MaterialId::new("abs-like-405").unwrap(), ..held() },
            SliceParams { first_layer_height: None, ..held() },
            SliceParams { first_layer_height: Some(PositiveFiniteMm::new(0.1).unwrap()), ..held() },
            SliceParams { aa_level: AaLevel::X4, ..held() },
        ];
        for current in changes {
            assert_eq!(SliceAgain::compare(Some(&held()), &current, false), SliceAgain::Changed, "{current:?}");
        }
        // The geometry marker itself is the document's business, not this one's.
        let edited = SliceParams { geometry: Revision::with_head(Some(9)), ..held() };
        assert_eq!(SliceAgain::compare(Some(&held()), &edited, false), SliceAgain::Unchanged);
    }
}
