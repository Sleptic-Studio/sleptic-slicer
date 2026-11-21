// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Slices saved in a project, checked again when it reopens.
//!
//! A project saves its slices with a token: a fingerprint of the scene's
//! geometry and of the layer schedule they were cut with. On opening, the
//! profiles are resolved again from what is installed and the fingerprint
//! recomputed for the reopened scene; only a match keeps the slices. Any
//! change since the save (a profile, the geometry) drops them and the scene
//! opens unsliced. Both steps read disk or hash geometry, so they run on the
//! loading thread; the UI thread turns the result into the held slice.
use sc_print_glue::{PersistedSliceValidity, validate_persisted_slices};
use sc_print_primitives::AaLevel;
use sc_project_formats::ProjectPayload;
use sc_scene::Scene;
use sc_sl1cer::contour::ContourStack;
use sc_sl1cer::profile::LayerSchedule;

use crate::scene_document::Revision;
use crate::slice_projection::resolve_current_profile;
use crate::slice_result::{SliceParams, SliceResult};

/// Saved slices that still fit the reopened scene, as the held slice they
/// become once the scene is installed and has a revision to stamp them
/// with (they then read as current and save again without slicing).
pub(crate) struct SavedSlices(Box<dyn FnOnce(Revision) -> SliceResult + Send>);

impl SavedSlices {
    pub(crate) fn into_slice_result(self, geometry: Revision) -> SliceResult {
        (self.0)(geometry)
    }
}

/// Whether `token` is the scene's fingerprint under `schedule`.
fn still_matches(token: &str, scene: &Scene, schedule: &LayerSchedule) -> bool {
    validate_persisted_slices(token, scene, schedule) == PersistedSliceValidity::InSync
}

/// The saved slices, if they still fit the reopened scene: the profile is
/// resolved the way slicing would resolve it, then the token has to be the
/// scene's fingerprint under its layers. Anything missing (slices, token,
/// printer, resin, a profile that no longer resolves) or a different
/// fingerprint gives `None`. Anti-aliasing is neither saved nor part of the
/// token, so the slice counts as made with it Off.
pub(crate) fn check_saved_slices(
    payload: Option<&ProjectPayload>,
    scene: &Scene,
    slices: Option<ContourStack>,
    fingerprint: Option<String>,
) -> Option<SavedSlices> {
    let payload = payload?;
    let (printer, resin) = (payload.printer.clone()?, payload.resin.clone()?);
    let first_layer_height = payload.first_layer_height;
    let profile = resolve_current_profile(&printer, &resin, first_layer_height, AaLevel::Off, scene).ok()?;
    let schedule = *profile.layer_schedule();
    let token = fingerprint?;
    let merged = slices?;
    if !still_matches(&token, scene, &schedule) {
        return None;
    }
    Some(SavedSlices(Box::new(move |geometry| SliceResult {
        merged,
        schedule,
        token,
        params: SliceParams { printer, resin, first_layer_height, aa_level: AaLevel::Off, geometry },
        profile,
    })))
}

#[cfg(test)]
#[path = "tests_reloaded_slices.rs"]
mod tests;
