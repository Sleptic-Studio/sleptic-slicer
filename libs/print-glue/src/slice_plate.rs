// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Slicing one plate. A [`PlateJob`] names everything a slice depends on;
//! its profile is worked out first, then the scene is sliced, all of it or
//! (when cancelled) nothing.
use crate::prepare::{
    UnsliceableSelection, slice_profile_for_selection, with_accuracy_for_selection,
};
use sc_print_primitives::{AaLevel, PositiveFiniteMm};
use sc_print_profiles::{EffectiveMaterial, PrinterProfile, SceneOverrides};
use sc_scene::Scene;
use sc_sl1cer::contour::{ContourStack, LayerAlignmentError, merge_stacks};
use sc_sl1cer::error::SliceSceneError;
use sc_sl1cer::profile::{LayerSchedule, PackingMode, SliceProfile};
use sc_sl1cer::{CancelToken, LeafSlices, SliceOutcome, SliceProgress, slice_scene_with_fallback};
use std::{error::Error, fmt};

/// The choices for one slice beyond the printer and the material.
#[derive(Debug, Clone, Copy)]
pub struct SlicingParams {
    /// The first layer's height; the normal layer height when none.
    pub first_layer_height: Option<PositiveFiniteMm>,
    /// The layer height asked for, if the selection allows it.
    pub layer_height_request: Option<PositiveFiniteMm>,
    /// Anti-aliasing for whatever rasterizes the result.
    pub aa_level: AaLevel,
    /// The scene's own accuracy and bottom/transition layer settings.
    pub process_overrides: SceneOverrides,
}

/// One plate to slice: the scene, what prints it, and the job's choices.
#[derive(Debug, Clone, Copy)]
pub struct PlateJob<'a> {
    /// The parts.
    pub scene: &'a Scene,
    /// The printer they print on.
    pub printer: &'a PrinterProfile,
    /// The resin, with its overrides applied.
    pub material: &'a EffectiveMaterial,
    /// The job's own choices.
    pub params: &'a SlicingParams,
}

impl<'a> PlateJob<'a> {
    /// A job for `scene` on `printer` with `material`.
    pub fn new(
        scene: &'a Scene,
        printer: &'a PrinterProfile,
        material: &'a EffectiveMaterial,
        params: &'a SlicingParams,
    ) -> PlateJob<'a> {
        PlateJob {
            scene,
            printer,
            material,
            params,
        }
    }

    /// The profile the job slices with: the selection's layers for this
    /// scene, with the job's accuracy settings over the material's.
    pub fn profile(&self) -> Result<SliceProfile, UnsliceableSelection> {
        let choice = self.params;
        let layers = slice_profile_for_selection(
            self.scene,
            self.printer,
            self.material,
            choice.layer_height_request,
            choice.first_layer_height,
            choice.aa_level,
            PackingMode::None,
        )?;
        Ok(with_accuracy_for_selection(
            layers,
            self.printer,
            self.material,
            &choice.process_overrides,
        ))
    }

    /// Only the layers the job would have: no slicing, no rasterizing.
    pub fn schedule(&self) -> Result<LayerSchedule, UnsliceableSelection> {
        self.profile().map(|profile| *profile.schedule())
    }

    /// Slices the plate. A part that is not a closed solid fails the slice,
    /// unless `tolerant` lets the slicer's tolerant path take it.
    pub fn slice(
        &self,
        cancel: &CancelToken,
        on_progress: impl Fn(SliceProgress) + Sync,
        tolerant: bool,
    ) -> Result<SliceRun, SlicePlateError> {
        let profile = self.profile().map_err(SlicePlateError::Prepare)?;
        let outcome =
            slice_scene_with_fallback(self.scene, &profile, cancel, on_progress, tolerant)
                .map_err(SlicePlateError::Engine)?;
        Ok(match outcome {
            SliceOutcome::Completed(leaves) => SliceRun::Completed(SlicedPlate { leaves, profile }),
            SliceOutcome::Cancelled => SliceRun::Cancelled,
        })
    }
}

/// A finished slice, or a cancelled one (which leaves nothing behind).
#[derive(Debug)]
pub enum SliceRun {
    /// Every part was sliced.
    Completed(SlicedPlate),
    /// The cancel token fired during the pass.
    Cancelled,
}

/// Every part's contours, with the profile they were sliced with.
#[derive(Debug)]
pub struct SlicedPlate {
    pub(crate) profile: SliceProfile,
    pub(crate) leaves: Vec<LeafSlices>,
}

impl SlicedPlate {
    /// The profile the plate was sliced with.
    pub fn profile(&self) -> &SliceProfile {
        &self.profile
    }

    /// The layers, the same for every part.
    pub fn schedule(&self) -> &LayerSchedule {
        self.profile.schedule()
    }

    /// Each part's contours, in the order the slicer found the parts.
    pub fn leaves(&self) -> &[LeafSlices] {
        &self.leaves
    }

    /// All parts' contours in one stack, layer by layer, side by side (not
    /// unioned: overlapping parts keep their own polygons).
    pub fn merge(self) -> Result<ContourStack, LayerAlignmentError> {
        let stacks = self
            .leaves
            .into_iter()
            .map(LeafSlices::into_stack)
            .collect();
        merge_stacks(stacks)
    }
}

/// Why a plate was not sliced. Each kind reads as its cause and names it
/// as its source.
#[derive(Debug)]
pub enum SlicePlateError {
    /// No profile: see [`UnsliceableSelection`].
    Prepare(UnsliceableSelection),
    /// The slicer refused the scene.
    Engine(SliceSceneError),
}

impl fmt::Display for SlicePlateError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SlicePlateError::Prepare(cause) => cause.fmt(out),
            SlicePlateError::Engine(cause) => cause.fmt(out),
        }
    }
}

impl Error for SlicePlateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            SlicePlateError::Prepare(cause) => cause,
            SlicePlateError::Engine(cause) => cause,
        })
    }
}
