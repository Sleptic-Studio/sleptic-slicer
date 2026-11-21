// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! From the chosen printer, resin and print settings to what slicing
//! needs: the loaded profiles, the layer schedule and the slice profile.
//! The slicing itself is `sc_print_glue::slice_plate`.
use std::fmt;

use sc_print_glue::{
    SlicePlateError, SlicingParams, UnsliceableScene, UnsliceableSelection, PlateJob,
    slice_profile_for_selection,
};
use sc_print_primitives::{AaLevel, MaterialId, PositiveFiniteMm, PrinterId};
use sc_print_profiles::{EffectiveMaterial, PrinterProfile, ProfileLibraries};
use sc_scene::Scene;
use sc_sl1cer::profile::{LayerSchedule, PackingMode, SliceProfile};

/// What stopped a slice, without the details (those are the cause).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reason {
    PrinterProfile,
    ResinProfile,
    SceneBounds,
    NoGeometry,
    /// Nothing is above the plate: the scene's top is at `top_mm` mm.
    BelowPlate { top_mm: f64 },
    PrintSettings,
    /// The scene's height gives no usable layer count.
    LayerCount,
    Slicer,
    /// The parts' layer grids disagreed when joined. Parts sliced on one
    /// schedule cannot cause it; it is reported instead of panicking.
    LayerGrids,
    InputKey,
    /// The slicing thread panicked with this message.
    Panic(String),
}

type Cause = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Why a slice could not be prepared or run, worded for the error dialog:
/// a reason, and the library error behind it when there is one.
#[derive(Debug)]
pub(crate) struct SliceFailure {
    reason: Reason,
    cause: Option<Cause>,
}

impl SliceFailure {
    pub(crate) fn plain(reason: Reason) -> Self {
        SliceFailure { reason, cause: None }
    }

    pub(crate) fn because(reason: Reason, cause: impl Into<Cause>) -> Self {
        SliceFailure { reason, cause: Some(cause.into()) }
    }

    #[cfg(test)]
    pub(crate) fn reason(&self) -> &Reason {
        &self.reason
    }
}

impl fmt::Display for SliceFailure {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        // What failed, then the library's own words when it gave any.
        let lead = match &self.reason {
            Reason::PrinterProfile => "Could not load the printer profile",
            Reason::ResinProfile => "Could not load the resin profile",
            Reason::SceneBounds => "Could not measure the scene bounds",
            Reason::PrintSettings => "Could not resolve the print configuration",
            Reason::LayerCount => "Could not derive the layer count",
            Reason::Slicer => "Slicing failed",
            Reason::InputKey => "Could not fingerprint the slice inputs",
            Reason::NoGeometry => return out.write_str("The scene has no geometry to slice."),
            Reason::LayerGrids => {
                return out.write_str("The slice layers could not be merged (internal layer-grid mismatch).");
            }
            Reason::Panic(message) => return write!(out, "The slicer panicked: {message}"),
            Reason::BelowPlate { top_mm } => {
                return write!(
                    out,
                    "The scene's top is at z = {top_mm} mm; nothing sits above the build plate (z = 0). Move the model above the plate before slicing.",
                );
            }
        };
        match &self.cause {
            Some(cause) => write!(out, "{lead}: {cause}"),
            None => out.write_str(lead),
        }
    }
}

impl std::error::Error for SliceFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_deref().map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}

impl From<UnsliceableScene> for SliceFailure {
    fn from(why: UnsliceableScene) -> Self {
        match why {
            UnsliceableScene::Aabb(e) => Self::because(Reason::SceneBounds, e),
            UnsliceableScene::NoGeometry => Self::plain(Reason::NoGeometry),
            UnsliceableScene::TopAtOrBelowPlate { z_max_mm } => Self::plain(Reason::BelowPlate { top_mm: z_max_mm }),
            UnsliceableScene::Unschedulable(e) => Self::because(Reason::LayerCount, e),
        }
    }
}

impl From<UnsliceableSelection> for SliceFailure {
    fn from(why: UnsliceableSelection) -> Self {
        match why {
            UnsliceableSelection::LayerHeight(e) => Self::because(Reason::PrintSettings, e),
            UnsliceableSelection::Scene(e) => e.into(),
        }
    }
}

impl From<SlicePlateError> for SliceFailure {
    fn from(why: SlicePlateError) -> Self {
        match why {
            SlicePlateError::Prepare(e) => e.into(),
            SlicePlateError::Engine(e) => Self::because(Reason::Slicer, e),
        }
    }
}

/// The printer and the resin as it applies (with any user extension) from
/// the installed libraries. A faulty extension is only warned about on
/// stderr; its bad parts are left out.
pub(crate) fn load_chosen_profiles(
    printer: &PrinterId,
    resin: &MaterialId,
) -> Result<(PrinterProfile, EffectiveMaterial), SliceFailure> {
    let libraries = ProfileLibraries::platform_default();
    let printer = libraries.load_printer_by_id(printer).map_err(|e| SliceFailure::because(Reason::PrinterProfile, e))?;
    let (resin, warnings) =
        libraries.effective_material_by_id(resin).map_err(|e| SliceFailure::because(Reason::ResinProfile, e))?;
    warnings.iter().for_each(|warning| eprintln!("warning: {warning}"));
    Ok((printer, resin))
}

/// The layers slicing `scene` with this choice would use (compared on save
/// against a slice already held). Anti-aliasing does not change layers.
pub(crate) fn layers_for_choice(
    printer: &PrinterId,
    resin: &MaterialId,
    first_layer_height: Option<PositiveFiniteMm>,
    scene: &Scene,
) -> Result<LayerSchedule, SliceFailure> {
    let (printer, resin) = load_chosen_profiles(printer, resin)?;
    let settings = SlicingParams {
        aa_level: AaLevel::Off,
        first_layer_height,
        layer_height_request: None,
        process_overrides: Default::default(),
    };
    PlateJob::new(scene, &printer, &resin, &settings).schedule().map_err(SliceFailure::from)
}

/// The full slice profile of this choice, without slicing: a project keeps
/// its slices with the printer and resin ids only, so reopening it rebuilds
/// the profile here (unpacked, as every slice the app runs is).
pub(crate) fn resolve_current_profile(
    printer: &PrinterId,
    resin: &MaterialId,
    first_layer_height: Option<PositiveFiniteMm>,
    aa_level: AaLevel,
    scene: &Scene,
) -> Result<SliceProfile, SliceFailure> {
    let (printer, resin) = load_chosen_profiles(printer, resin)?;
    slice_profile_for_selection(scene, &printer, &resin, None, first_layer_height, aa_level, PackingMode::None)
        .map_err(SliceFailure::from)
}

#[cfg(test)]
#[path = "tests_slice_projection.rs"]
mod tests;
