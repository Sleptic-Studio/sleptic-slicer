// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The profile catalog from outside the crate, on profile folders written
//! for each test: merging, sorting, pictures, extensions and coverage.
use std::fs;
use std::path::{Path, PathBuf};

use sc_print_primitives::{MaterialId, PrinterId};
use sc_print_profiles::{CalibrationTarget, ProfileLibraries};
use sc_slicer_gui::build_volume::{BuildVolume, FALLBACK_VOLUME};
use sc_slicer_gui::profile_catalog::{
    CalibrationCoverage, HOUSE_PRINTER_ID, CatalogResin, InstalledProfiles, ProfileSource,
    calibration_coverage, has_calibration_for,
};
use tempfile::TempDir;

/// A profile folder for one test.
struct Folder(TempDir);

impl Folder {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn file(&self, path: &str, text: &str) -> &Self {
        let path = self.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self
    }

    /// A printer of this size in mm; 40 µm pixels, so the resolution
    /// matches the size.
    fn printer_sized(
        &self,
        id: &str,
        maker: &str,
        name: &str,
        size: [f64; 3],
        picture: Option<&str>,
    ) -> &Self {
        let [x, y, z] = size;
        let picture = picture
            .map(|path| format!("image_asset_path = \"{path}\"\n"))
            .unwrap_or_default();
        let text = format!(
            r#"[identity]
id = "{id}"
manufacturer = "{maker}"
family = "Test Family"
name = "{name}"
profile_version = "1.0.0"
{picture}
[hardware.build_volume]
x_mm = {x:?}
y_mm = {y:?}
z_mm = {z:?}

[hardware.pixel_resolution]
x = {px}
y = {py}

[hardware.pixel_size_microns]
x = 40.0
y = 40.0

[hardware.mirror]
x = false
y = false

[capabilities]
output_formats = [{{ format = "aff", version = "pm3" }}]
supports_grayscale = true
two_stage_lift = false
has_camera = false

[constraints]
"#,
            px = (x * 25.0).round() as u32,
            py = (y * 25.0).round() as u32,
        );
        self.file(&format!("printers/{id}.toml"), &text)
    }

    fn printer(&self, id: &str, maker: &str, name: &str) -> &Self {
        self.printer_sized(id, maker, name, [100.0, 60.0, 80.0], None)
    }

    /// A resin calibrated for these printers.
    fn resin(&self, id: &str, brand: &str, name: &str, family: &str, printers: &[&str]) -> &Self {
        let text = format!(
            "[identity]\nid = \"{id}\"\nbrand = \"{brand}\"\nfamily = \"{family}\"\nname = \"{name}\"\n\
             profile_version = \"1.0.0\"\n\n[intent]\ntarget_layer_height_mm = 0.05\n\n[defaults]\n\
             bottom_layer_count = 5\nscale_compensation_pct = {{ x = 0.0, y = 0.0, z = 0.0 }}\n{}",
            calibrations(printers)
        );
        self.file(&format!("materials/{id}.toml"), &text)
    }

    /// An extension adding calibrations for these printers to resin `id`.
    fn extension(&self, id: &str, printers: &[&str]) -> &Self {
        let text = format!(
            "extension = true\n\n[identity]\nid = \"{id}\"\nbrand = \"Brand\"\nfamily = \"other\"\n\
             name = \"Extended Resin\"\nprofile_version = \"1.0.0\"\n\n[intent]\n\n[defaults]\n{}",
            calibrations(printers)
        );
        self.file(&format!("materials/extensions/{id}.toml"), &text)
    }
}

fn calibrations(printers: &[&str]) -> String {
    let block = |id: &&str| {
        format!("\n[per_printer.{id}]\nnormal_exposure_sec = 2.5\nbottom_exposure_sec = 30.0\n")
    };
    printers.iter().map(block).collect()
}

fn catalog(user: Option<&Folder>, bundled: &[&Folder]) -> InstalledProfiles {
    let bundled: Vec<PathBuf> = bundled
        .iter()
        .map(|folder| folder.path().to_owned())
        .collect();
    InstalledProfiles::scan_libraries(&ProfileLibraries::new(
        user.map(|folder| folder.path().to_owned()),
        bundled,
    ))
}

fn printer(id: &str) -> PrinterId {
    PrinterId::new(id).unwrap()
}

fn resin(id: &str) -> MaterialId {
    MaterialId::new(id).unwrap()
}

fn targets(ids: &[&str]) -> std::collections::BTreeSet<CalibrationTarget> {
    ids.iter()
        .map(|id| CalibrationTarget::from(printer(id)))
        .collect()
}

fn entry<'a>(catalog: &'a InstalledProfiles, id: &str) -> &'a CatalogResin {
    &catalog.resin_list()[catalog.resin_position(&resin(id)).unwrap()]
}

#[test]
fn missing_folders_give_an_empty_catalog() {
    let gone = Folder::new();
    let catalog = InstalledProfiles::scan_libraries(&ProfileLibraries::new(
        None,
        vec![gone.path().join("none")],
    ));
    assert!(catalog.printers().is_empty() && catalog.resin_list().is_empty());
    assert_eq!(catalog.first_launch_printer(), None);
    assert_eq!(
        catalog.volume_for_printer(None),
        FALLBACK_VOLUME
    );
}

#[test]
fn printers_and_resins_are_sorted_by_maker_and_name() {
    let bundled = Folder::new();
    bundled
        .printer("z-one", "Zeta", "One")
        .printer("a-two", "Alpha", "Two")
        .printer("a-one", "Alpha", "One");
    bundled
        .resin("z", "Zeta", "Alpha Name", "other", &[])
        .resin("ab", "Alpha", "B", "other", &[]);
    bundled.resin("aa", "Alpha", "A", "other", &[]);
    let catalog = catalog(None, &[&bundled]);
    let printers: Vec<_> = catalog
        .printers()
        .iter()
        .map(|p| (p.printer_maker.as_str(), p.name.as_str()))
        .collect();
    assert_eq!(
        printers,
        [("Alpha", "One"), ("Alpha", "Two"), ("Zeta", "One")]
    );
    let resins: Vec<_> = catalog
        .resin_list()
        .iter()
        .map(|r| (r.brand.as_str(), r.name.as_str()))
        .collect();
    assert_eq!(
        resins,
        [("Alpha", "A"), ("Alpha", "B"), ("Zeta", "Alpha Name")]
    );
    assert_eq!(catalog.maker_names(), ["Alpha", "Zeta"]);
    assert_eq!(catalog.printer_position(&printer("z-one")), Some(2));
    assert_eq!(catalog.printer_position(&printer("none")), None);
    assert_eq!(catalog.resin_position(&resin("aa")), Some(0));
    assert_eq!(catalog.resin_position(&resin("none")), None);
}

#[test]
fn a_printers_size_sets_the_build_volume() {
    let bundled = Folder::new();
    bundled.printer_sized("vol", "Maker", "Model", [192.0, 120.0, 200.0], None);
    let catalog = catalog(None, &[&bundled]);
    let volume = catalog.plate_volume_of(&printer("vol")).unwrap();
    assert_eq!(
        (volume.x_mm(), volume.y_mm(), volume.z_mm()),
        (192.0, 120.0, 200.0)
    );
    assert_eq!(
        catalog.volume_for_printer(Some(&printer("vol"))),
        BuildVolume::new(192.0, 120.0, 200.0)
    );
    assert_eq!(
        catalog.volume_for_printer(None),
        FALLBACK_VOLUME
    );
    assert_eq!(
        catalog.volume_for_printer(Some(&printer("other"))),
        FALLBACK_VOLUME
    );
}

#[test]
fn a_picture_is_kept_only_when_its_file_exists() {
    let bundled = Folder::new();
    bundled.file("images/model.png", "png");
    bundled.printer_sized(
        "a",
        "Maker",
        "A",
        [100.0, 60.0, 80.0],
        Some("images/model.png"),
    );
    bundled.printer_sized(
        "b",
        "Maker",
        "B",
        [100.0, 60.0, 80.0],
        Some("images/gone.png"),
    );
    bundled.printer("c", "Maker", "C");
    let catalog = catalog(None, &[&bundled]);
    let pictures: Vec<_> = catalog
        .printers()
        .iter()
        .map(|p| p.picture_path.clone())
        .collect();
    assert_eq!(
        pictures,
        [Some(bundled.path().join("images/model.png")), None, None]
    );
}

#[test]
fn the_users_copy_wins_and_each_entry_knows_its_folder() {
    let (user, bundled) = (Folder::new(), Folder::new());
    bundled
        .printer("shared", "Bundled Maker", "Bundled")
        .printer("b-only", "Maker", "Bundled Only");
    user.printer_sized("shared", "User Maker", "User", [150.0, 90.0, 120.0], None)
        .printer("u-only", "Maker", "Own");
    let catalog = catalog(Some(&user), &[&bundled]);
    let found: Vec<_> = catalog
        .printers()
        .iter()
        .map(|p| (p.id.as_str(), p.printer_maker.as_str(), p.profile_origin))
        .collect();
    assert_eq!(
        found,
        [
            ("b-only", "Maker", ProfileSource::Bundled),
            ("u-only", "Maker", ProfileSource::User),
            ("shared", "User Maker", ProfileSource::User),
        ]
    );
    assert_eq!(
        catalog
            .plate_volume_of(&printer("shared"))
            .unwrap()
            .x_mm(),
        150.0
    );
}

#[test]
fn a_broken_profile_is_skipped_and_hides_lower_copies() {
    let (user, bundled) = (Folder::new(), Folder::new());
    bundled
        .printer("good", "Maker", "Good")
        .printer("hidden", "Maker", "Bundled");
    bundled.file("printers/broken.toml", "this is not a printer profile");
    user.file("printers/hidden.toml", "neither is this");
    let catalog = catalog(Some(&user), &[&bundled]);
    let ids: Vec<_> = catalog.printers().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["good"]);
}

#[test]
fn every_bundled_folder_is_read_and_its_profiles_load() {
    let (first, second) = (Folder::new(), Folder::new());
    second.printer("second", "Maker", "Second");
    let libraries = ProfileLibraries::new(
        None,
        vec![first.path().to_owned(), second.path().to_owned()],
    );
    let catalog = InstalledProfiles::scan_libraries(&libraries);
    assert!(catalog.printer_position(&printer("second")).is_some());
    assert_eq!(
        libraries
            .load_printer_by_id(&printer("second"))
            .unwrap()
            .identity
            .id,
        printer("second")
    );
}

#[test]
fn a_new_user_profile_appears_when_the_catalog_is_rebuilt() {
    let (user, bundled) = (Folder::new(), Folder::new());
    bundled.resin("shipped", "Brand", "Shipped", "other", &[]);
    assert_eq!(
        catalog(Some(&user), &[&bundled]).resin_position(&resin("made")),
        None
    );
    user.resin("made", "Brand", "Made", "other", &[]);
    assert_eq!(
        entry(&catalog(Some(&user), &[&bundled]), "made").profile_origin,
        ProfileSource::User
    );
}

#[test]
fn an_extension_adds_calibrations_to_its_resin() {
    let (user, bundled) = (Folder::new(), Folder::new());
    bundled.resin("resin", "Brand", "Resin", "other", &["printer-a"]);
    user.extension("resin", &["printer-b"]);
    let catalog = catalog(Some(&user), &[&bundled]);
    assert_eq!(catalog.resin_list().len(), 1);
    assert_eq!(entry(&catalog, "resin").profile_origin, ProfileSource::Bundled);
    assert_eq!(
        entry(&catalog, "resin").calibrated_for,
        targets(&["printer-a"])
    );
    assert_eq!(
        catalog.extension_of(&resin("resin")).unwrap().extra_calibrations,
        targets(&["printer-b"])
    );
    assert_eq!(catalog.extensions().len(), 1);
    assert_eq!(catalog.extension_of(&resin("other")), None);
}

#[test]
fn an_extension_without_its_resin_is_listed_as_the_users_own() {
    let (user, bundled) = (Folder::new(), Folder::new());
    bundled
        .resin("other", "Aaa", "Other", "other", &[])
        .resin("last", "Zzz", "Last", "other", &[]);
    user.extension("orphan", &["printer-b"]);
    let catalog = catalog(Some(&user), &[&bundled]);
    assert_eq!(catalog.extension_of(&resin("orphan")), None);
    let orphan = entry(&catalog, "orphan");
    assert_eq!(
        (orphan.profile_origin, orphan.name.as_str()),
        (ProfileSource::User, "Extended Resin")
    );
    assert_eq!(orphan.calibrated_for, targets(&["printer-b"]));
    // Sorted in by brand: Aaa, Brand, Zzz.
    let order: Vec<_> = catalog.resin_list().iter().map(|r| r.id.as_str()).collect();
    assert_eq!(order, ["other", "orphan", "last"]);
}

#[test]
fn only_flagged_files_in_the_users_extension_folder_count() {
    let (user, bundled) = (Folder::new(), Folder::new());
    bundled
        .resin("resin", "Brand", "Resin", "other", &[])
        .extension("resin", &["printer-b"]);
    // A whole profile left in the extension folder, without the flag.
    user.file(
        "materials/extensions/resin.toml",
        "[identity]\nid = \"resin\"\nbrand = \"Brand\"\nfamily = \"other\"\nname = \"Resin\"\n\
         profile_version = \"1.0.0\"\n\n[intent]\n\n[defaults]\n",
    );
    let catalog = catalog(Some(&user), &[&bundled]);
    assert_eq!(catalog.extension_of(&resin("resin")), None);
    assert_eq!(catalog.resin_list().len(), 1);
}

#[test]
fn coverage_counts_own_calibrations_first_then_the_generic_one() {
    let (user, bundled) = (Folder::new(), Folder::new());
    bundled.resin("own", "Brand", "Own", "other", &["printer-a"]);
    bundled.resin("generic", "Brand", "Generic", "other", &["generic-machine"]);
    bundled.resin("extended", "Brand", "Extended", "other", &["printer-a"]);
    bundled.resin(
        "extended-generic",
        "Brand",
        "Extended Generic",
        "other",
        &[],
    );
    user.extension("extended", &["printer-b"])
        .extension("extended-generic", &["generic-machine"]);
    let catalog = catalog(Some(&user), &[&bundled]);
    let of = |id: &str, on: Option<&str>| {
        let extension = catalog.extension_of(&resin(id));
        let on = on.map(printer);
        (
            calibration_coverage(entry(&catalog, id), extension, on.as_ref()),
            has_calibration_for(entry(&catalog, id), extension, on.as_ref()),
        )
    };
    use CalibrationCoverage::{GenericFallback, None as Uncovered, Specific};
    assert_eq!(of("own", Some("printer-a")), (Specific, true));
    assert_eq!(of("own", Some("printer-b")), (Uncovered, false));
    assert_eq!(of("own", None), (Uncovered, false));
    assert_eq!(of("generic", Some("printer-z")), (GenericFallback, true));
    assert_eq!(of("generic", Some("generic-machine")), (Specific, true));
    assert_eq!(of("generic", None), (GenericFallback, true));
    assert_eq!(of("extended", Some("printer-b")), (Specific, true));
    assert_eq!(
        of("extended-generic", Some("printer-z")),
        (GenericFallback, true)
    );
    // Without its extension the resin would not match.
    assert_eq!(
        calibration_coverage(
            entry(&catalog, "extended"),
            None,
            Some(&printer("printer-b"))
        ),
        Uncovered
    );
}

#[test]
fn the_first_printer_is_sleptics_then_generic_then_the_first_listed() {
    let bundled = Folder::new();
    bundled
        .printer("z-printer", "Zeta", "Z")
        .printer("a-printer", "Alpha", "A");
    assert_eq!(
        catalog(None, &[&bundled]).first_launch_printer(),
        Some(printer("a-printer"))
    );
    // Generic sorts after "Alpha A" and still wins.
    bundled.printer_sized(
        "generic-machine",
        "Generic",
        "Generic Machine",
        [120.0, 68.0, 130.0],
        None,
    );
    let with_generic = catalog(None, &[&bundled]);
    assert_eq!(
        with_generic.first_launch_printer(),
        Some(printer("generic-machine"))
    );
    assert_eq!(
        with_generic.printer_in_effect(None),
        Some(printer("generic-machine"))
    );
    assert_eq!(
        with_generic.printer_in_effect(Some(&printer("a-printer"))),
        Some(printer("a-printer"))
    );
    bundled.printer_sized(
        HOUSE_PRINTER_ID,
        "Sleptic",
        "DIY MSLA",
        [192.0, 120.0, 200.0],
        None,
    );
    assert_eq!(
        catalog(None, &[&bundled]).first_launch_printer(),
        Some(printer(HOUSE_PRINTER_ID))
    );
}

#[test]
fn the_fallback_resin_is_a_calibrated_standard_one_by_id() {
    let bundled = Folder::new();
    bundled.resin("aaa-abs", "A", "ABS", "abs-like", &["generic-machine"]);
    bundled.resin(
        "zzz-standard",
        "Z",
        "Standard",
        "standard",
        &["generic-machine"],
    );
    bundled.resin(
        "bbb-uncalibrated",
        "B",
        "Standard",
        "standard",
        &["other-machine"],
    );
    let house = printer(HOUSE_PRINTER_ID);
    assert_eq!(
        catalog(None, &[&bundled]).usual_resin_for(Some(&house)),
        Some(resin("zzz-standard"))
    );
    bundled.resin(
        "ccc-standard",
        "C",
        "Standard",
        "Standard",
        &["generic-machine"],
    );
    assert_eq!(
        catalog(None, &[&bundled]).usual_resin_for(Some(&house)),
        Some(resin("ccc-standard"))
    );
    // With nothing calibrated for it, there is none.
    let other = Folder::new();
    other.resin("x", "X", "X", "standard", &["printer-a"]);
    assert_eq!(
        catalog(None, &[&other]).usual_resin_for(Some(&house)),
        None
    );
}
