// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The printers and resins the app can offer, read from the user's profile
//! folder and the bundled ones.
//!
//! The pickers (printer gallery, manufacturer filter, resin list) all read
//! from here. When two folders hold the same id, the earlier folder wins, as
//! in the loaders that open a profile by id; each entry remembers which
//! folder it came from, so the user's own profiles can be marked. A resin
//! extension in the user folder adds calibrations to the resin of the same
//! id; with no such resin it is listed on its own as a custom resin.
//!
//! The catalog is never edited: it is built again whenever a profile folder
//! changes (a profile made or imported).
use std::collections::BTreeSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use sc_print_primitives::{BuildVolume, MaterialId, PrinterId};

pub use sc_print_profiles::{CalibrationCoverage, ProfileSource};
use sc_print_profiles::{
    CalibrationTarget, MaterialProfile, PrinterProfile, ProfileLibraries, generic_printer_id,
    list_material_extension_ids, list_material_ids, list_printer_ids,
    load_material_extension_from_library, load_material_profile_from_library,
    load_printer_profile_from_library,
};

/// Sleptic's own printer, bundled: the default before the user picks one.
pub const HOUSE_PRINTER_ID: &str = "sleptic-diy-msla";

/// What the printer picker shows of a printer profile.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogPrinter {
    pub id: PrinterId,
    pub printer_maker: String,
    pub name: String,
    /// The printer's series ("Mars", "Photon Mono X"), its profile's family.
    pub series: String,
    pub printer_plate_volume: BuildVolume,
    /// The printer's picture, if the profile names one and the file exists.
    pub picture_path: Option<PathBuf>,
    pub profile_origin: ProfileSource,
}

/// What the resin picker shows of a resin profile.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogResin {
    pub id: MaterialId,
    pub brand: String,
    pub name: String,
    pub resin_family: String,
    /// The printers this resin has a `[per_printer.<id>]` calibration for.
    pub calibrated_for: BTreeSet<CalibrationTarget>,
    pub profile_origin: ProfileSource,
    /// Overhangs flatter than this, in degrees, need supports in this resin.
    pub overhang_limit_deg: f64,
}

/// A user's extension to a resin: calibrations for more printers, under
/// the resin's own id.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogExtension {
    /// The resin it extends.
    pub id: MaterialId,
    /// The printers it adds a calibration for.
    pub extra_calibrations: BTreeSet<CalibrationTarget>,
}

/// How well the resin, with its extension, is calibrated for `printer`.
pub fn calibration_coverage(
    entry: &CatalogResin,
    extension: Option<&CatalogExtension>,
    printer: Option<&PrinterId>,
) -> CalibrationCoverage {
    let added = extension
        .into_iter()
        .flat_map(|extension| extension.extra_calibrations.iter());
    CalibrationCoverage::of(entry.calibrated_for.iter().chain(added), printer)
}

/// Whether the resin has any calibration that applies to `printer`, its
/// own or the generic one; such resins head the resin list.
pub fn has_calibration_for(
    entry: &CatalogResin,
    extension: Option<&CatalogExtension>,
    printer: Option<&PrinterId>,
) -> bool {
    calibration_coverage(entry, extension, printer) != CalibrationCoverage::None
}

/// Every printer and resin on offer. Printers are sorted by manufacturer
/// then name, resins by brand then name; the resin list reorders its own
/// copy for the chosen printer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstalledProfiles {
    printers: Vec<CatalogPrinter>,
    materials: Vec<CatalogResin>,
    /// Extensions whose resin is in `materials`, sorted by id.
    extensions: Vec<CatalogExtension>,
}

type Roots = [(PathBuf, ProfileSource)];

/// Visits each id once, in the first folder that lists it. A folder that
/// cannot be listed is skipped with a warning.
fn first_load_listing<I: Ord + Clone, E: Display>(
    roots: &Roots,
    list: impl Fn(&Path) -> Result<Vec<I>, E>,
    mut visit: impl FnMut(&Path, ProfileSource, I),
) {
    let mut seen = BTreeSet::new();
    for (root, source) in roots {
        match list(root) {
            Ok(ids) => {
                for id in ids {
                    if seen.insert(id.clone()) {
                        visit(root, *source, id);
                    }
                }
            }
            Err(err) => eprintln!("warning: {err}"),
        }
    }
}

/// Adds the loaded profile, or warns. A broken copy still hides the same id
/// in later folders: that entry is just missing.
fn keep_loaded<T, P, E: Display>(entries: &mut Vec<T>, loaded: Result<P, E>, entry: impl FnOnce(P) -> T) {
    match loaded {
        Ok(profile) => entries.push(entry(profile)),
        Err(err) => eprintln!("warning: {err}"),
    }
}

fn order_resins(materials: &mut [CatalogResin]) {
    materials.sort_by(|a, b| (&a.brand, &a.name).cmp(&(&b.brand, &b.name)));
}

impl InstalledProfiles {
    /// Reads every folder of `libraries`; a folder that does not exist adds
    /// nothing.
    pub fn scan_libraries(libraries: &ProfileLibraries) -> Self {
        let roots: Vec<_> = libraries
            .roots()
            .map(|(root, source)| (root.to_owned(), source))
            .collect();
        let mut materials = Vec::new();
        first_load_listing(&roots, list_material_ids, |root, source, id| {
            keep_loaded(
                &mut materials,
                load_material_profile_from_library(root, &id),
                |profile| catalog_resin(source, profile),
            );
        });
        order_resins(&mut materials);
        let extensions = merge_extensions(&roots, &mut materials);
        let mut printers = Vec::new();
        first_load_listing(&roots, list_printer_ids, |root, source, id| {
            keep_loaded(
                &mut printers,
                load_printer_profile_from_library(root, &id),
                |profile| catalog_printer(root, source, profile),
            );
        });
        printers.sort_by(|a: &CatalogPrinter, b| {
            (&a.printer_maker, &a.name).cmp(&(&b.printer_maker, &b.name))
        });
        Self {
            printers,
            materials,
            extensions,
        }
    }

    pub fn printers(&self) -> &[CatalogPrinter] {
        &self.printers
    }

    pub fn resin_list(&self) -> &[CatalogResin] {
        &self.materials
    }

    /// The extensions, each attached to its resin, sorted by id.
    pub fn extensions(&self) -> &[CatalogExtension] {
        &self.extensions
    }

    pub fn extension_of(&self, id: &MaterialId) -> Option<&CatalogExtension> {
        self.extensions.iter().find(|extension| &extension.id == id)
    }

    pub fn printer_position(&self, id: &PrinterId) -> Option<usize> {
        self.printers.iter().position(|entry| &entry.id == id)
    }

    pub fn resin_position(&self, id: &MaterialId) -> Option<usize> {
        self.materials.iter().position(|entry| &entry.id == id)
    }

    /// The printer shown before the user has chosen one: Sleptic's own if
    /// present, else the generic machine, else the first listed. Only for
    /// display and the build volume; never saved as the user's choice.
    pub fn first_launch_printer(&self) -> Option<PrinterId> {
        let listed = |id: &str| self.printers.iter().find(|entry| entry.id.as_str() == id);
        let generic = generic_printer_id();
        let entry = listed(HOUSE_PRINTER_ID)
            .or_else(|| listed(generic.as_str()))
            .or(self.printers.first());
        entry.map(|entry| entry.id.clone())
    }

    /// The user's chosen printer, or the first-run default.
    pub fn printer_in_effect(&self, chosen: Option<&PrinterId>) -> Option<PrinterId> {
        chosen.cloned().or_else(|| self.first_launch_printer())
    }

    /// The resin to fall back on for `printer`: among those calibrated for
    /// it (an extension or the generic calibration counts), a standard
    /// resin first, then the lowest id.
    pub fn usual_resin_for(&self, printer: Option<&PrinterId>) -> Option<MaterialId> {
        let fits = |entry: &&CatalogResin| {
            has_calibration_for(entry, self.extension_of(&entry.id), printer)
        };
        let best = self.materials.iter().filter(fits).min_by_key(|entry| {
            (
                !entry.resin_family.eq_ignore_ascii_case("standard"),
                entry.id.clone(),
            )
        });
        best.map(|entry| entry.id.clone())
    }

    pub fn plate_volume_of(&self, id: &PrinterId) -> Option<BuildVolume> {
        self.printers
            .iter()
            .find(|entry| &entry.id == id)
            .map(|entry| entry.printer_plate_volume)
    }

    /// Each manufacturer once, sorted, for the manufacturer filter.
    pub fn maker_names(&self) -> Vec<String> {
        let names: BTreeSet<&String> = self
            .printers
            .iter()
            .map(|entry| &entry.printer_maker)
            .collect();
        names.into_iter().cloned().collect()
    }

    /// The build volume the 3D view draws for `printer` (its x, y, z as
    /// width, depth, height), or the built-in default when there is none.
    pub fn volume_for_printer(
        &self,
        printer: Option<&PrinterId>,
    ) -> crate::build_volume::BuildVolume {
        match printer.and_then(|id| self.plate_volume_of(id)) {
            Some(volume) => crate::build_volume::BuildVolume::new(
                volume.x_mm() as f32,
                volume.y_mm() as f32,
                volume.z_mm() as f32,
            ),
            None => crate::build_volume::FALLBACK_VOLUME,
        }
    }
}

/// Reads the extensions in the user folders (the bundled ones have none).
/// One whose resin is listed is attached to it; one without is added to the
/// resins as the user's own, so its settings stay visible.
fn merge_extensions(
    roots: &Roots,
    materials: &mut Vec<CatalogResin>,
) -> Vec<CatalogExtension> {
    let user: Vec<_> = roots
        .iter()
        .filter(|(_, source)| *source == ProfileSource::User)
        .cloned()
        .collect();
    let mut extensions = Vec::new();
    let mut orphans = Vec::new();
    first_load_listing(&user, list_material_extension_ids, |root, _, id| {
        match load_material_extension_from_library(root, &id) {
            Ok(profile) if materials.iter().chain(&orphans).any(|entry| entry.id == id) => {
                let printers = profile.per_printer.keys().cloned().collect();
                extensions.push(CatalogExtension { id, extra_calibrations: printers });
            }
            Ok(profile) => orphans.push(catalog_resin(ProfileSource::User, profile)),
            Err(err) => eprintln!("warning: {err}"),
        }
    });
    if !orphans.is_empty() {
        materials.append(&mut orphans);
        order_resins(materials);
    }
    extensions.sort_by(|a, b| a.id.cmp(&b.id));
    extensions
}

fn catalog_printer(
    root: &Path,
    source: ProfileSource,
    profile: PrinterProfile,
) -> CatalogPrinter {
    let build_volume = profile.hardware.build_volume();
    let identity = profile.identity;
    // A picture that is named but missing is treated as none.
    let image_path = identity.image_asset_path.and_then(|picture| {
        let path = picture.resolve_in(root);
        path.is_file().then_some(path)
    });
    CatalogPrinter {
        id: identity.id,
        printer_maker: identity.manufacturer.as_str().to_owned(),
        name: identity.name.as_str().to_owned(),
        series: identity.family.as_str().to_owned(),
        printer_plate_volume: build_volume,
        picture_path: image_path,
        profile_origin: source,
    }
}

fn catalog_resin(source: ProfileSource, profile: MaterialProfile) -> CatalogResin {
    let support_overhang_angle_deg = profile.defaults.support_overhang_angle();
    let identity = profile.identity;
    CatalogResin {
        id: identity.id,
        brand: identity.brand.as_str().to_owned(),
        name: identity.name.as_str().to_owned(),
        resin_family: identity.family.as_str().to_owned(),
        calibrated_for: profile.per_printer.keys().cloned().collect(),
        profile_origin: source,
        overhang_limit_deg: support_overhang_angle_deg,
    }
}

/// A resin's type and when it needs supports, for the resin lists (R78:
/// flexible resins always need them).
pub fn resin_family_text(family: &str, support_overhang_angle_deg: f64) -> String {
    crate::i18n::tr!(
        "{} · supports overhangs under {}°",
        family,
        support_overhang_angle_deg.round()
    )
    .to_string()
}
