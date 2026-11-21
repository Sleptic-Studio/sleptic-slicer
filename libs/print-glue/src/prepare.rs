// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The slicing profile for a scene: the layer height the selection allows,
//! the scene's height measured, and from both the number of layers.
use crate::projection::{
    UnschedulableHeight, LayerPlan, hardware_profile, total_layers_for_height,
};
use sc_print_primitives::{AaLevel, PositiveFiniteMm};
use sc_print_profiles::{
    EffectiveMaterial, PrinterHardware, PrinterProfile, ResolveError, SceneOverrides,
    resolve_accuracy, resolve_layer_height, snap_to_z_step,
};
use sc_scene::{Scene, SceneError};
use sc_sl1cer::{
    profile::{PackingMode, SliceProfile},
    raster::accuracy::Accuracy,
};
use std::fmt;
/// Why a scene gets no slicing profile.
#[derive(Debug, Clone, PartialEq)]
pub enum UnsliceableScene {
    /// The scene's bounds could not be worked out.
    Aabb(SceneError),
    /// Nothing in the scene has a size.
    NoGeometry,
    /// The whole scene is at or under the plate.
    TopAtOrBelowPlate {
        /// The scene's highest point, in mm.
        z_max_mm: f64,
    },
    /// No layer count reaches the top (see [`UnschedulableHeight`]).
    Unschedulable(UnschedulableHeight),
}
impl fmt::Display for UnsliceableScene {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unschedulable(inner) => fmt::Display::fmt(inner, out),
            Self::NoGeometry => out.write_str("The scene has no geometry to slice"),
            Self::Aabb(why) => write!(out, "Cannot measure scene bounds: {why}"),
            Self::TopAtOrBelowPlate { z_max_mm: top } => {
                write!(out, "Scene top {top} mm is at or below the build plate")
            }
        }
    }
}
impl std::error::Error for UnsliceableScene {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Aabb(why) => Some(why),
            Self::Unschedulable(why) => Some(why),
            Self::NoGeometry | Self::TopAtOrBelowPlate { .. } => None,
        }
    }
}
/// Why a printer, material and scene give no slicing profile.
#[derive(Debug, Clone, PartialEq)]
pub enum UnsliceableSelection {
    /// No layer height the printer and material both allow.
    LayerHeight(ResolveError),
    /// The scene itself (see [`UnsliceableScene`]).
    Scene(UnsliceableScene),
}
impl fmt::Display for UnsliceableSelection {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(inner) => fmt::Display::fmt(inner, out),
            Self::LayerHeight(why) => write!(out, "Cannot select layer height: {why}"),
        }
    }
}
impl std::error::Error for UnsliceableSelection {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scene(why) => Some(why),
            Self::LayerHeight(why) => Some(why),
        }
    }
}

/// The profile for slicing `scene` with this printer and material: the
/// layer height asked for if they allow it (else theirs), the first layer
/// snapped to the printer's Z step, the layer count from the scene's top,
/// and the material's accuracy settings. Exposure is not needed.
pub fn slice_profile_for_selection(
    scene: &Scene,
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
    layer_height_request: Option<PositiveFiniteMm>,
    first_layer_height: Option<PositiveFiniteMm>,
    aa_level: AaLevel,
    packing_mode: PackingMode,
) -> Result<SliceProfile, UnsliceableSelection> {
    let height = match resolve_layer_height(printer, material, layer_height_request) {
        Ok(height) => height,
        Err(why) => return Err(UnsliceableSelection::LayerHeight(why)),
    };
    let snapped_first = first_layer_height.map(|asked| snap_to_z_step(&printer.constraints, asked));
    let geometry = slice_profile_for_scene(scene, &printer.hardware, height, snapped_first, aa_level, packing_mode)
        .map_err(UnsliceableSelection::Scene)?;
    Ok(with_accuracy_for_selection(geometry, printer, material, &SceneOverrides::default()))
}

/// The profile whose layers just cover `scene`: its top measured, then the
/// first layer (`layer_height` when not given) and as many more as it
/// takes. A scene with nothing above the plate has none.
pub fn slice_profile_for_scene(
    scene: &Scene,
    hardware: &PrinterHardware,
    layer_height: PositiveFiniteMm,
    first_layer_height: Option<PositiveFiniteMm>,
    aa_level: AaLevel,
    packing_mode: PackingMode,
) -> Result<SliceProfile, UnsliceableScene> {
    let Some(extent) = scene.aabb().map_err(UnsliceableScene::Aabb)? else {
        return Err(UnsliceableScene::NoGeometry);
    };
    let [_, _, highest] = extent.max().coords();
    let top = f64::from(highest);
    if top <= 0.0 {
        return Err(UnsliceableScene::TopAtOrBelowPlate { z_max_mm: top });
    }
    let first = first_layer_height.unwrap_or(layer_height);
    let layers = total_layers_for_height(top, layer_height.value(), first.value())
        .map_err(UnsliceableScene::Unschedulable)?;
    Ok(hardware_profile(hardware, LayerPlan { layer_height: layer_height, first_layer_height: first, layers: layers }, aa_level, packing_mode))
}

/// `profile` with the material's accuracy settings and support angle (no
/// motion or exposure). Bottom and transition layer counts come from the
/// scene's overrides, else the material's calibration for this printer,
/// else the material's defaults, else none.
pub fn with_accuracy_for_selection(
    profile: SliceProfile,
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
    o: &SceneOverrides,
) -> SliceProfile {
    let (bottom_compensation_mm, xy_compensation_mm, aa_levels) = resolve_accuracy(printer, material, o);
    let calibrated = material.calibration_for(&printer.identity.id).map(|(_, values)| values);
    let defaults = &material.defaults;
    let bottom_layers = o
        .bottom_layer_count
        .or_else(|| calibrated?.bottom_layer_count)
        .or(defaults.bottom_layer_count)
        .unwrap_or_default();
    let transition_layers = o
        .transition_layers
        .or_else(|| calibrated?.transition_layers)
        .or(defaults.transition_layers)
        .unwrap_or_default();
    let accuracy = Accuracy { bottom_compensation_mm, xy_compensation_mm, aa_levels, bottom_layers, transition_layers };
    profile.with_accuracy(accuracy).with_support_overhang_angle(defaults.support_overhang_angle())
}
