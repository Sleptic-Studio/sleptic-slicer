// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::*;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "PrinterId", into = "PrinterId")]
pub enum CalibrationTarget {
    Generic,
    Printer(SpecificPrinterId),
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "PrinterId", into = "PrinterId")]
pub struct SpecificPrinterId(PrinterId);
pub fn generic_printer_id() -> PrinterId {
    // This literal is valid by construction.
    match PrinterId::new("generic-machine") {
        Ok(id) => id,
        Err(_) => unreachable!("valid built-in id"),
    }
}
impl TryFrom<PrinterId> for SpecificPrinterId {
    type Error = &'static str;
    fn try_from(id: PrinterId) -> Result<Self, Self::Error> {
        if id.as_str() == "generic-machine" {
            Err("a specific printer id cannot be generic-machine")
        } else {
            Ok(Self(id))
        }
    }
}
impl From<SpecificPrinterId> for PrinterId {
    fn from(id: SpecificPrinterId) -> Self {
        id.0
    }
}
impl From<PrinterId> for CalibrationTarget {
    fn from(id: PrinterId) -> Self {
        if id.as_str() == "generic-machine" {
            Self::Generic
        } else {
            Self::Printer(SpecificPrinterId(id))
        }
    }
}
impl From<CalibrationTarget> for PrinterId {
    fn from(t: CalibrationTarget) -> Self {
        match t {
            CalibrationTarget::Generic => generic_printer_id(),
            CalibrationTarget::Printer(id) => id.0,
        }
    }
}
impl SpecificPrinterId {
    /// The printer this calibration block belongs to.
    pub fn as_printer_id(&self) -> &PrinterId {
        &self.0
    }
}
impl CalibrationTarget {
    /// The target a printer's calibration is stored under: the generic
    /// block for the generic machine, its own block otherwise.
    pub fn from_printer(id: &PrinterId) -> Self {
        id.clone().into()
    }
    /// The printer id this target is written as in a profile.
    pub fn printer_id(&self) -> &PrinterId {
        static GENERIC: std::sync::OnceLock<PrinterId> = std::sync::OnceLock::new();
        match self {
            Self::Generic => GENERIC.get_or_init(generic_printer_id),
            Self::Printer(id) => &id.0,
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            Self::Generic => "generic-machine",
            Self::Printer(id) => id.0.as_str(),
        }
    }
}
impl fmt::Display for CalibrationTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl fmt::Display for SpecificPrinterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationCoverage {
    Specific,
    GenericFallback,
    None,
}
impl CalibrationCoverage {
    /// How `printer` is covered by a set of calibration targets (a
    /// material's own blocks, possibly chained with an extension's).
    pub fn of<'a>(
        targets: impl IntoIterator<Item = &'a CalibrationTarget>,
        printer: Option<&PrinterId>,
    ) -> Self {
        let wanted = printer.map(CalibrationTarget::from_printer);
        let mut generic = false;
        for target in targets {
            if wanted.as_ref() == Some(target) {
                return Self::Specific;
            }
            generic |= *target == CalibrationTarget::Generic;
        }
        if generic {
            Self::GenericFallback
        } else {
            Self::None
        }
    }
    /// Coverage of `printer` by one material profile's own blocks.
    pub fn of_material(material: &MaterialProfile, printer: Option<&PrinterId>) -> Self {
        Self::of(material.per_printer.keys(), printer)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MaterialDocument")]
pub struct MaterialProfile {
    #[serde(default, skip_serializing_if = "is_false")]
    pub extension: bool,
    pub identity: MaterialIdentity,
    pub intent: MaterialIntent,
    pub defaults: MaterialDefaults,
    #[serde(default)]
    pub per_printer: BTreeMap<CalibrationTarget, MaterialPerPrinter>,
}
fn is_false(v: &bool) -> bool {
    !*v
}
#[derive(Deserialize)]
struct MaterialDocument {
    #[serde(default)]
    extension: bool,
    identity: MaterialIdentity,
    intent: MaterialIntent,
    defaults: MaterialDefaults,
    #[serde(default)]
    per_printer: BTreeMap<CalibrationTarget, CalibrationDocument>,
    per_environment: Option<support_settings::Retired>,
    per_session_state: Option<support_settings::Retired>,
}
#[derive(Deserialize)]
#[serde(try_from = "MaterialPerPrinterDocument")]
struct CalibrationDocument {
    values: MaterialPerPrinter,
    constraints: Option<support_settings::Retired>,
}
impl TryFrom<MaterialPerPrinterDocument> for CalibrationDocument {
    type Error = &'static str;
    fn try_from(mut document: MaterialPerPrinterDocument) -> Result<Self, Self::Error> {
        // Decode fields once; flattening a validated struct buffers and replays
        // every calibration value through an intermediate serde content map.
        let constraints = document.constraints.take();
        Ok(Self {
            values: document.try_into()?,
            constraints,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialProfileReservedSection {
    PerEnvironment,
    PerSessionState,
    Constraints { target: CalibrationTarget },
}
impl fmt::Display for MaterialProfileReservedSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self{Self::PerEnvironment=>f.write_str("the [per_environment.*] sections are reserved for future conditional overrides and are not supported in v0"),Self::PerSessionState=>f.write_str("the [per_session_state.*] sections are reserved for future conditional overrides and are not supported in v0"),Self::Constraints{target}=>write!(f,"[per_printer.\"{target}\".constraints] is reserved for material tightening of printer constraints and is not supported in v0")}
    }
}
impl std::error::Error for MaterialProfileReservedSection {}
impl TryFrom<MaterialDocument> for MaterialProfile {
    type Error = MaterialProfileReservedSection;
    fn try_from(v: MaterialDocument) -> Result<Self, Self::Error> {
        if v.per_environment.is_some() {
            return Err(Self::Error::PerEnvironment);
        }
        if v.per_session_state.is_some() {
            return Err(Self::Error::PerSessionState);
        }
        let mut per_printer = BTreeMap::new();
        for (target, entry) in v.per_printer {
            if entry.constraints.is_some() {
                return Err(Self::Error::Constraints { target });
            }
            per_printer.insert(target, entry.values);
        }
        Ok(Self {
            extension: v.extension,
            identity: v.identity,
            intent: v.intent,
            defaults: v.defaults,
            per_printer,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialIdentity {
    pub id: MaterialId,
    pub brand: NonEmptyString,
    pub family: NonEmptyString,
    pub name: NonEmptyString,
    pub profile_version: semver::Version,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MaterialIntent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_energy_density_mj_per_cm2: Option<PositiveFiniteMjPerCm2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_layer_height_mm: Option<PositiveFiniteMm>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MaterialDefaultsDocument")]
pub struct MaterialDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<motion::MotionProfile>,
    // Field order is part of the canonical profile writer's representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density_g_ml: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_layers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_compensation_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xy_compensation_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aa_levels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_off_delay_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_layer_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale_compensation_pct: Option<ScaleCompensation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_per_liter: Option<PositiveFiniteCurrencyPerLiter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_overhang_angle_deg: Option<f64>,
}
#[derive(Deserialize)]
struct MaterialDefaultsDocument {
    motion: Option<motion::MotionProfile>,
    transition_layers: Option<u32>,
    bottom_compensation_mm: Option<f64>,
    xy_compensation_mm: Option<f64>,
    aa_levels: Option<u16>,
    light_off_delay_s: Option<f64>,
    bottom_layer_count: Option<u32>,
    density_g_ml: Option<f64>,
    scale_compensation_pct: Option<ScaleCompensation>,
    price_per_liter: Option<PositiveFiniteCurrencyPerLiter>,
    support_overhang_angle_deg: Option<f64>,
}
impl TryFrom<MaterialDefaultsDocument> for MaterialDefaults {
    type Error = &'static str;
    fn try_from(v: MaterialDefaultsDocument) -> Result<Self, Self::Error> {
        if let Some(x) = v.density_g_ml
            && (!x.is_finite() || x <= 0.0)
        {
            return Err("density_g_ml must be positive and finite");
        }
        if let Some(x) = v.transition_layers
            && x > 1000
        {
            return Err("transition_layers must be <= 1000");
        }
        if let Some(x) = v.light_off_delay_s
            && (!x.is_finite() || x < 0.0)
        {
            return Err("light_off_delay_s must be finite and non-negative");
        }
        if let Some(x) = v.xy_compensation_mm
            && !x.is_finite()
        {
            return Err("XY compensation must be finite");
        }
        if let Some(x) = v.aa_levels
            && !(2..=256).contains(&x)
        {
            return Err("aa_levels must be 2..=256");
        }
        if let Some(x) = v.bottom_compensation_mm
            && (!x.is_finite() || x < 0.0)
        {
            return Err("bottom_compensation_mm must be finite and non-negative");
        }
        if let Some(x) = v.support_overhang_angle_deg
            && (!x.is_finite() || x <= 0.0 || x >= 90.0)
        {
            return Err("support_overhang_angle_deg must be between 0 and 90 degrees");
        }
        Ok(Self {
            motion: v.motion,
            transition_layers: v.transition_layers,
            bottom_compensation_mm: v.bottom_compensation_mm,
            xy_compensation_mm: v.xy_compensation_mm,
            aa_levels: v.aa_levels,
            light_off_delay_s: v.light_off_delay_s,
            bottom_layer_count: v.bottom_layer_count,
            density_g_ml: v.density_g_ml,
            scale_compensation_pct: v.scale_compensation_pct,
            price_per_liter: v.price_per_liter,
            support_overhang_angle_deg: v.support_overhang_angle_deg,
        })
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MaterialPerPrinterDocument")]
pub struct MaterialPerPrinter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<motion::MotionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_layers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_compensation_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xy_compensation_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aa_levels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_off_delay_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_exposure_sec: Option<PositiveFiniteSec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_exposure_sec: Option<PositiveFiniteSec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_layer_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_distance_mm: Option<PositiveFiniteMm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_speed_mm_min: Option<PositiveFiniteMmPerMin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retract_speed_mm_min: Option<PositiveFiniteMmPerMin>,
}
#[derive(Deserialize)]
struct MaterialPerPrinterDocument {
    motion: Option<motion::MotionProfile>,
    transition_layers: Option<u32>,
    bottom_compensation_mm: Option<f64>,
    xy_compensation_mm: Option<f64>,
    aa_levels: Option<u16>,
    light_off_delay_s: Option<f64>,
    bottom_layer_count: Option<u32>,
    normal_exposure_sec: Option<PositiveFiniteSec>,
    bottom_exposure_sec: Option<PositiveFiniteSec>,
    lift_distance_mm: Option<PositiveFiniteMm>,
    lift_speed_mm_min: Option<PositiveFiniteMmPerMin>,
    retract_speed_mm_min: Option<PositiveFiniteMmPerMin>,
    constraints: Option<support_settings::Retired>,
}
impl TryFrom<MaterialPerPrinterDocument> for MaterialPerPrinter {
    type Error = &'static str;
    fn try_from(v: MaterialPerPrinterDocument) -> Result<Self, Self::Error> {
        if let Some(x) = v.transition_layers
            && x > 1000
        {
            return Err("transition_layers must be <= 1000");
        }
        if let Some(x) = v.light_off_delay_s
            && (!x.is_finite() || x < 0.0)
        {
            return Err("light_off_delay_s must be finite and non-negative");
        }
        if let Some(x) = v.xy_compensation_mm
            && !x.is_finite()
        {
            return Err("XY compensation must be finite");
        }
        if let Some(x) = v.aa_levels
            && !(2..=256).contains(&x)
        {
            return Err("aa_levels must be 2..=256");
        }
        if let Some(x) = v.bottom_compensation_mm
            && (!x.is_finite() || x < 0.0)
        {
            return Err("bottom_compensation_mm must be finite and non-negative");
        }
        Ok(Self {
            motion: v.motion,
            transition_layers: v.transition_layers,
            bottom_compensation_mm: v.bottom_compensation_mm,
            xy_compensation_mm: v.xy_compensation_mm,
            aa_levels: v.aa_levels,
            light_off_delay_s: v.light_off_delay_s,
            bottom_layer_count: v.bottom_layer_count,
            normal_exposure_sec: v.normal_exposure_sec,
            bottom_exposure_sec: v.bottom_exposure_sec,
            lift_distance_mm: v.lift_distance_mm,
            lift_speed_mm_min: v.lift_speed_mm_min,
            retract_speed_mm_min: v.retract_speed_mm_min,
        })
    }
}
impl MaterialDefaults {
    pub fn support_overhang_angle(&self) -> f64 {
        self.support_overhang_angle_deg.unwrap_or(45.0)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveMaterial {
    pub identity: MaterialIdentity,
    pub intent: MaterialIntent,
    pub defaults: MaterialDefaults,
    pub per_printer: BTreeMap<CalibrationTarget, MaterialPerPrinter>,
}
impl From<MaterialProfile> for EffectiveMaterial {
    fn from(m: MaterialProfile) -> Self {
        Self {
            identity: m.identity,
            intent: m.intent,
            defaults: m.defaults,
            per_printer: m.per_printer,
        }
    }
}
impl From<&MaterialProfile> for EffectiveMaterial {
    fn from(m: &MaterialProfile) -> Self {
        Self::from(m.clone())
    }
}
/// Issues compare by what they report (their debug rendering): one variant
/// carries a load error, which has no equality of its own.
impl PartialEq for MaterialExtensionIssue {
    fn eq(&self, other: &Self) -> bool {
        format!("{self:?}") == format!("{other:?}")
    }
}
#[derive(Debug)]
pub enum MaterialExtensionIssue {
    ExtensionUnreadable {
        path: PathBuf,
        cause: LoadMaterialProfileError,
    },
    ExtensionNotFlagged,
    ExtensionIdentityMismatch {
        extension: MaterialId,
        base: MaterialId,
    },
    InheritedIntentCarriesValues,
    InheritedDefaultsCarryValues,
    OverlappingCalibrationPair {
        target: CalibrationTarget,
    },
}
impl fmt::Display for MaterialExtensionIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self{
Self::ExtensionUnreadable{path,cause}=>write!(f,"the extension file {} could not be loaded and was ignored: {cause}",path.display()),
Self::ExtensionNotFlagged=>f.write_str("the extension file is not flagged as an extension and was ignored"),
Self::ExtensionIdentityMismatch{extension,base}=>write!(f,"the extension file names material '{extension}' but the base profile is '{base}'; the file was ignored"),
Self::InheritedIntentCarriesValues=>f.write_str("the extension's [intent] section carries values; intent is inherited from the base profile, so they were ignored"),
Self::InheritedDefaultsCarryValues=>f.write_str("the extension's [defaults] section carries values; defaults are inherited from the base profile, so they were ignored"),
Self::OverlappingCalibrationPair{target}=>write!(f,"the extension adds a calibration block for printer '{target}', which the base profile already covers; the base profile's block stays in effect")}
    }
}
impl EffectiveMaterial {
    pub fn compose(
        base: MaterialProfile,
        extension: Option<MaterialProfile>,
    ) -> (Self, Vec<MaterialExtensionIssue>) {
        let mut result = Self::from(base);
        let mut issues = Vec::new();
        let Some(extension) = extension else {
            return (result, issues);
        };
        if !extension.extension {
            issues.push(MaterialExtensionIssue::ExtensionNotFlagged);
            return (result, issues);
        }
        if extension.identity.id != result.identity.id {
            issues.push(MaterialExtensionIssue::ExtensionIdentityMismatch {
                extension: extension.identity.id,
                base: result.identity.id.clone(),
            });
            return (result, issues);
        }
        if extension.intent != MaterialIntent::default() {
            issues.push(MaterialExtensionIssue::InheritedIntentCarriesValues);
        }
        let mut inherited = extension.defaults;
        inherited.price_per_liter = None;
        if inherited != MaterialDefaults::default() {
            issues.push(MaterialExtensionIssue::InheritedDefaultsCarryValues);
        }
        for (target, entry) in extension.per_printer {
            match result.per_printer.entry(target) {
                std::collections::btree_map::Entry::Vacant(v) => {
                    v.insert(entry);
                }
                std::collections::btree_map::Entry::Occupied(o) => {
                    issues.push(MaterialExtensionIssue::OverlappingCalibrationPair {
                        target: o.key().clone(),
                    })
                }
            }
        }
        (result, issues)
    }
    pub fn identity(&self) -> &MaterialIdentity {
        &self.identity
    }
    pub fn intent(&self) -> &MaterialIntent {
        &self.intent
    }
    pub fn defaults(&self) -> &MaterialDefaults {
        &self.defaults
    }
    pub fn per_printer(&self) -> &BTreeMap<CalibrationTarget, MaterialPerPrinter> {
        &self.per_printer
    }
    pub fn calibration_for(
        &self,
        id: &PrinterId,
    ) -> Option<(&CalibrationTarget, &MaterialPerPrinter)> {
        self.per_printer
            .get_key_value(&id.clone().into())
            .or_else(|| self.per_printer.get_key_value(&CalibrationTarget::Generic))
    }
}
