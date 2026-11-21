// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::*;
use crate::{job_profile::JobProfile, motion::MotionProfile};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PrinterDocument")]
pub struct PrinterProfile {
    pub identity: PrinterIdentity,
    pub hardware: PrinterHardware,
    pub capabilities: PrinterCapabilities,
    pub constraints: PrinterConstraints,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<MotionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<JobProfile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrinterDocument {
    identity: PrinterIdentity,
    hardware: PrinterHardware,
    capabilities: PrinterCapabilities,
    constraints: PrinterConstraints,
    motion: Option<MotionProfile>,
    job: Option<JobProfile>,
    instance: Option<support_settings::Retired>,
}
impl TryFrom<PrinterDocument> for PrinterProfile {
    type Error = PrinterProfileReservedInstanceSection;
    fn try_from(v: PrinterDocument) -> Result<Self, Self::Error> {
        if v.instance.is_some() {
            return Err(PrinterProfileReservedInstanceSection);
        }
        Ok(Self {
            identity: v.identity,
            hardware: v.hardware,
            capabilities: v.capabilities,
            constraints: v.constraints,
            motion: v.motion,
            job: v.job,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrinterProfileReservedInstanceSection;
impl fmt::Display for PrinterProfileReservedInstanceSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the [instance] section is reserved for a future per-device overlay and is not supported in v0")
    }
}
impl std::error::Error for PrinterProfileReservedInstanceSection {}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterIdentity {
    pub id: PrinterId,
    pub manufacturer: NonEmptyString,
    pub family: NonEmptyString,
    pub name: NonEmptyString,
    pub profile_version: semver::Version,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_asset_path: Option<ImageAssetRef>,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "HardwareDocument")]
pub struct PrinterHardware {
    build_volume: BuildVolume,
    pixel_resolution: PixelResolution,
    pixel_size_microns: PixelSizeMicrons,
    mirror: Mirror,
    #[serde(skip_serializing_if = "MachineCalibration::is_default")]
    calibration: MachineCalibration,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HardwareDocument {
    build_volume: BuildVolume,
    pixel_resolution: PixelResolution,
    pixel_size_microns: PixelSizeMicrons,
    mirror: Mirror,
    #[serde(default)]
    calibration: MachineCalibration,
}
impl TryFrom<HardwareDocument> for PrinterHardware {
    type Error = EnvelopeAxisMismatch;
    fn try_from(v: HardwareDocument) -> Result<Self, Self::Error> {
        Ok(Self::new(
            v.build_volume,
            v.pixel_resolution,
            v.pixel_size_microns,
            v.mirror,
        )?
        .with_calibration(v.calibration))
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvelopeAxisMismatch {
    pub axis: PixelAxis,
    pub declared: f64,
    pub derived: f64,
    pub difference_percent: f64,
}
impl fmt::Display for EnvelopeAxisMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "build volume {} axis ({:.4} mm) disagrees with pixel-grid-derived value ({:.4} mm); relative difference {:.4}% exceeds tolerance 0.5000%",
            self.axis, self.declared, self.derived, self.difference_percent
        )
    }
}
impl std::error::Error for EnvelopeAxisMismatch {}
impl PrinterHardware {
    pub fn new(
        build_volume: BuildVolume,
        pixel_resolution: PixelResolution,
        pixel_size_microns: PixelSizeMicrons,
        mirror: Mirror,
    ) -> Result<Self, EnvelopeAxisMismatch> {
        for (axis, declared, derived) in [
            (
                PixelAxis::X,
                build_volume.x_mm(),
                f64::from(pixel_resolution.x()) * pixel_size_microns.x.value() / 1000.0,
            ),
            (
                PixelAxis::Y,
                build_volume.y_mm(),
                f64::from(pixel_resolution.y()) * pixel_size_microns.y.value() / 1000.0,
            ),
        ] {
            let difference = (declared - derived).abs() / declared;
            if difference > 0.005 {
                return Err(EnvelopeAxisMismatch {
                    axis,
                    declared,
                    derived,
                    difference_percent: difference * 100.0,
                });
            }
        }
        Ok(Self {
            build_volume,
            pixel_resolution,
            pixel_size_microns,
            mirror,
            calibration: MachineCalibration::default(),
        })
    }
    pub fn build_volume(&self) -> BuildVolume {
        self.build_volume
    }
    pub fn pixel_resolution(&self) -> PixelResolution {
        self.pixel_resolution
    }
    pub fn pixel_size_microns(&self) -> PixelSizeMicrons {
        self.pixel_size_microns
    }
    pub fn mirror(&self) -> Mirror {
        self.mirror
    }
    pub fn calibration(&self) -> MachineCalibration {
        self.calibration
    }
    pub fn with_calibration(mut self, value: MachineCalibration) -> Self {
        self.calibration = value;
        self
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterCapabilities {
    pub output_formats: OutputFormatBindings,
    pub supports_grayscale: bool,
    pub two_stage_lift: bool,
    pub has_camera: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_layers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_protocol: Option<NetworkProtocol>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterConstraints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_step_mm: Option<PositiveFiniteMm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_height_range_mm: Option<LengthRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_speed_range_mm_min: Option<SpeedRangeMmPerMin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retract_speed_range_mm_min: Option<SpeedRangeMmPerMin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_distance_max_mm: Option<PositiveFiniteMm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_intensity_mw_per_cm2: Option<PositiveFiniteMwPerCm2>,
}
