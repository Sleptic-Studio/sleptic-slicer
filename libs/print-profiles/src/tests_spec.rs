// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use std::{
    fs,
    io::{Cursor, Write},
};
fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles")
}
fn printer() -> PrinterProfile {
    load_printer_profile_from_library(&assets(), &generic_printer_id()).unwrap()
}
fn material() -> MaterialProfile {
    let id = list_material_ids(&assets()).unwrap().remove(0);
    load_material_profile_from_library(&assets(), &id).unwrap()
}
fn mm(v: f64) -> PositiveFiniteMm {
    PositiveFiniteMm::new(v).unwrap()
}
#[test]
fn every_bundled_profile_loads_roundtrips_and_resolves() {
    let root = assets();
    let printers = list_printer_ids(&root).unwrap();
    let materials = list_material_ids(&root).unwrap();
    assert_eq!((printers.len(), materials.len()), (97, 43));
    let generic = printer();
    for id in printers {
        let p = load_printer_profile_from_library(&root, &id).unwrap();
        assert_eq!(p.identity.id, id);
        let text = toml_edit::ser::to_string_pretty(&p).unwrap();
        assert_eq!(p, toml_edit::de::from_str(&text).unwrap());
    }
    for id in materials {
        let m = load_material_profile_from_library(&root, &id).unwrap();
        assert_eq!(m.identity.id, id);
        let text = toml_edit::ser::to_string_pretty(&m).unwrap();
        assert_eq!(m, toml_edit::de::from_str(&text).unwrap());
        let resolved = resolve(&generic, &m.into(), &SceneOverrides::default()).unwrap();
        assert!(resolved.material_resolved.normal_exposure_sec.value() > 0.0);
    }
}
#[test]
fn profile_optional_sections_stay_absent() {
    let p = printer();
    let m = material();
    let text = toml_edit::ser::to_string_pretty(&p).unwrap();
    assert!(!text.contains("[motion"));
    let text = toml_edit::ser::to_string_pretty(&m).unwrap();
    assert!(!text.contains("[defaults.motion"));
    assert!(!text.contains("extension = false"));
}
#[test]
fn printer_rejects_reserved_and_unknown_fields() {
    let text = toml_edit::ser::to_string_pretty(&printer()).unwrap();
    for suffix in ["\n[instance]", "\ninstance = {}"] {
        let doc = if suffix.contains("[instance]") {
            format!("{text}{suffix}")
        } else {
            format!("{suffix}\n{text}")
        };
        assert!(
            toml_edit::de::from_str::<PrinterProfile>(&doc)
                .unwrap_err()
                .to_string()
                .contains("reserved")
        );
    }
    assert!(toml_edit::de::from_str::<PrinterProfile>(&format!("extra = true\n{text}")).is_err());
}
#[test]
fn material_reserved_sections_and_unknowns() {
    let text = toml_edit::ser::to_string_pretty(&material()).unwrap();
    for (section, message) in [
        ("per_environment", "per_environment"),
        ("per_session_state", "per_session_state"),
        ("per_printer.generic-machine.constraints", "constraints"),
    ] {
        let err = toml_edit::de::from_str::<MaterialProfile>(&format!("{text}\n[{section}]\n"))
            .unwrap_err();
        assert!(err.to_string().contains(message), "{err}");
    }
    assert!(toml_edit::de::from_str::<MaterialProfile>(&format!("future = true\n{text}")).is_ok());
}
#[test]
fn material_numeric_validation() {
    let base = material();
    for (field, value, msg) in [
        ("density_g_ml", "0.0", "density"),
        ("transition_layers", "1001", "transition"),
        ("light_off_delay_s", "-1.0", "light_off"),
        ("xy_compensation_mm", "nan", "XY compensation"),
        ("aa_levels", "1", "aa_levels"),
        ("bottom_compensation_mm", "-0.1", "bottom_compensation"),
        ("support_overhang_angle_deg", "90.0", "support_overhang"),
    ] {
        let mut doc = toml_edit::ser::to_string_pretty(&base)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        doc["defaults"][field] = value.parse::<toml_edit::Value>().unwrap().into();
        let err = toml_edit::de::from_str::<MaterialProfile>(&doc.to_string()).unwrap_err();
        assert!(err.to_string().contains(msg), "{err}");
    }
}
#[test]
fn hardware_checks_each_axis_and_preserves_calibration() {
    let p = printer();
    let h = &p.hardware;
    let bad = PrinterHardware::new(
        BuildVolume::new(100.0, 89.0, 175.0).unwrap(),
        h.pixel_resolution(),
        h.pixel_size_microns(),
        h.mirror(),
    )
    .unwrap_err();
    assert_eq!(bad.axis, PixelAxis::X);
    assert!(bad.to_string().contains("tolerance 0.5000%"));
    let calibration = MachineCalibration::new(LcdRotation::Deg90, [1.01, 0.99]).unwrap();
    assert_eq!(
        (*h).with_calibration(calibration).calibration(),
        calibration
    );
}
#[test]
fn images_are_lexically_validated() {
    for s in ["", "/a", "\\a", "C:foo", "a//b", "a/../b", "./a", "a/"] {
        assert!(ImageAssetRef::new(s).is_err(), "{s}");
    }
    for s in ["images/a.png", "a\\b", "?/image.png"] {
        let r = ImageAssetRef::new(s).unwrap();
        assert_eq!(r.as_str(), s);
        assert_eq!(
            r.resolve(Path::new("root")),
            s.split('/').fold(PathBuf::from("root"), |p, s| p.join(s))
        );
    }
}
#[test]
fn target_ordering_and_coverage() {
    let mut m = material();
    m.per_printer.clear();
    assert_eq!(
        CalibrationCoverage::of_material(&m, None),
        CalibrationCoverage::None
    );
    m.per_printer
        .insert(CalibrationTarget::Generic, Default::default());
    assert_eq!(
        CalibrationCoverage::of_material(&m, None),
        CalibrationCoverage::GenericFallback
    );
    assert_eq!(
        CalibrationCoverage::of_material(&m, Some(&generic_printer_id())),
        CalibrationCoverage::Specific
    );
    assert!(SpecificPrinterId::try_from(generic_printer_id()).is_err());
    let target: CalibrationTarget = PrinterId::new("aaa").unwrap().into();
    assert!(CalibrationTarget::Generic < target);
}
#[test]
fn coverage_prefers_exact_targets_regardless_of_iteration_order() {
    let selected = PrinterId::new("selected-printer").unwrap();
    let other = PrinterId::new("other-printer").unwrap();
    let exact = CalibrationTarget::from_printer(&selected);
    let unrelated = CalibrationTarget::from_printer(&other);
    let generic = generic_printer_id();
    for targets in [
        vec![],
        vec![CalibrationTarget::Generic],
        vec![unrelated.clone()],
        vec![CalibrationTarget::Generic, exact.clone()],
        vec![exact.clone(), CalibrationTarget::Generic],
        vec![unrelated, exact],
    ] {
        for printer in [None, Some(&generic), Some(&selected), Some(&other)] {
            let expected = if printer
                .is_some_and(|id| targets.contains(&CalibrationTarget::from_printer(id)))
            {
                CalibrationCoverage::Specific
            } else if targets.contains(&CalibrationTarget::Generic) {
                CalibrationCoverage::GenericFallback
            } else {
                CalibrationCoverage::None
            };
            assert_eq!(CalibrationCoverage::of(&targets, printer), expected);
        }
    }
}
#[test]
fn bundled_material_serialization_preserves_the_canonical_dialect() {
    let root = assets();
    for id in list_material_ids(&root).unwrap() {
        let path = material_profile_path_in_library(&root, &id);
        let profile = load_material_profile_from_path(&path).unwrap();
        assert_eq!(
            toml_edit::ser::to_string_pretty(&profile).unwrap(),
            fs::read_to_string(&path).unwrap(),
            "material {id} changed its saved representation"
        );
    }
}
/// Saving a bundled profile through the library writes the file it came
/// from, so a user who saves without changes sees no difference. The DIY
/// printer is the exception: it is hand-commented to explain placeholders,
/// and saving keeps its values, not its comments.
#[test]
fn saving_a_bundled_profile_reproduces_its_file() {
    let root = assets();
    let scratch = tempfile::tempdir().unwrap();
    for id in list_printer_ids(&root).unwrap() {
        let path = printer_profile_path_in_library(&root, &id);
        let profile = load_printer_profile_from_path(&path).unwrap();
        let saved = scratch.path().join(format!("{id}.toml"));
        save_printer_profile_to_path(&saved, &profile).unwrap();
        assert_eq!(load_printer_profile_from_path(&saved).unwrap(), profile);
        let original = fs::read_to_string(&path).unwrap();
        if id.as_str() == "sleptic-diy-msla" {
            assert!(original.contains("HARDWARE PLACEHOLDERS"));
            continue;
        }
        assert_eq!(fs::read_to_string(&saved).unwrap(), original, "printer {id}");
    }
    for id in list_material_ids(&root).unwrap() {
        let path = material_profile_path_in_library(&root, &id);
        let saved = scratch.path().join(format!("{id}.toml"));
        save_material_profile_to_path(&saved, &load_material_profile_from_path(&path).unwrap())
            .unwrap();
        assert_eq!(
            fs::read_to_string(&saved).unwrap(),
            fs::read_to_string(&path).unwrap(),
            "material {id}"
        );
    }
}
#[test]
fn extension_is_additive_and_reports_in_order() {
    let base = material();
    let mut extension = base.clone();
    extension.extension = true;
    let other: CalibrationTarget = PrinterId::new("new-printer").unwrap().into();
    extension
        .per_printer
        .insert(other.clone(), Default::default());
    let (effective, issues) = EffectiveMaterial::compose(base.clone(), Some(extension));
    assert_eq!(effective.identity, base.identity);
    assert_eq!(effective.per_printer.len(), base.per_printer.len() + 1);
    assert!(matches!(
        issues[0],
        MaterialExtensionIssue::InheritedIntentCarriesValues
    ));
    assert!(matches!(
        issues[1],
        MaterialExtensionIssue::InheritedDefaultsCarryValues
    ));
    assert_eq!(
        effective.per_printer[&CalibrationTarget::Generic],
        base.per_printer[&CalibrationTarget::Generic]
    );
    assert!(effective.per_printer.contains_key(&other));
}
#[test]
fn extension_rejection_precedes_inherited_values() {
    let base = material();
    let (_, issues) = EffectiveMaterial::compose(base.clone(), Some(base.clone()));
    assert_eq!(issues.len(), 1);
    assert!(matches!(
        issues[0],
        MaterialExtensionIssue::ExtensionNotFlagged
    ));
    let mut extension = base.clone();
    extension.extension = true;
    extension.identity.id = MaterialId::new("other").unwrap();
    let (_, issues) = EffectiveMaterial::compose(base, Some(extension));
    assert!(matches!(
        issues[0],
        MaterialExtensionIssue::ExtensionIdentityMismatch { .. }
    ));
}
#[test]
fn extension_price_alone_is_silent() {
    let base = material();
    let mut e = base.clone();
    e.extension = true;
    e.intent = Default::default();
    e.defaults = Default::default();
    e.defaults.price_per_liter = Some(PositiveFiniteCurrencyPerLiter::new(50.0).unwrap());
    e.per_printer.clear();
    assert!(EffectiveMaterial::compose(base, Some(e)).1.is_empty());
}
#[test]
fn exact_calibration_does_not_fill_from_generic() {
    let mut m = material();
    let specific = PrinterId::new("specific").unwrap();
    m.per_printer
        .insert(specific.clone().into(), Default::default());
    assert!(
        EffectiveMaterial::from(m)
            .calibration_for(&specific)
            .unwrap()
            .1
            .normal_exposure_sec
            .is_none()
    );
}
#[test]
fn listing_uses_names_without_parsing() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("materials/dir.toml")).unwrap();
    for file in [
        "b.toml",
        "a.toml",
        "UPPER.toml",
        "c.TOML",
        "-bad.toml",
        "bad_name.toml",
    ] {
        fs::write(d.path().join("materials").join(file), "invalid").unwrap();
    }
    let ids = list_material_ids(d.path())
        .unwrap()
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["a", "b", "dir"]);
    assert!(list_printer_ids(d.path()).unwrap().is_empty());
}
#[test]
fn lookup_stops_at_malformed_user_and_observes_changes() {
    let d = tempfile::tempdir().unwrap();
    let libs = ProfileLibraries::new(Some(d.path().into()), vec![assets()]);
    let id = generic_printer_id();
    assert_eq!(libs.printer_by_id(&id).unwrap().1, ProfileSource::Bundled);
    fs::create_dir_all(d.path().join("printers")).unwrap();
    fs::write(printer_profile_path_in_library(d.path(), &id), "invalid").unwrap();
    assert!(matches!(
        libs.printer_by_id(&id),
        Err(LoadPrinterProfileByIdError::Source(_))
    ));
    save_printer_profile_to_library(d.path(), &printer()).unwrap();
    assert_eq!(libs.printer_by_id(&id).unwrap().1, ProfileSource::User);
}
#[test]
fn lookup_does_not_enforce_filename_identity() {
    let d = tempfile::tempdir().unwrap();
    let id = PrinterId::new("different").unwrap();
    save_printer_profile_to_path(&printer_profile_path_in_library(d.path(), &id), &printer())
        .unwrap();
    assert_eq!(
        load_printer_profile_from_library(d.path(), &id)
            .unwrap()
            .identity
            .id,
        generic_printer_id()
    );
}
#[test]
fn extension_persistence_comments_and_delete_scope() {
    let d = tempfile::tempdir().unwrap();
    let mut m = material();
    save_material_profile_to_library(d.path(), &m).unwrap();
    let base = material_profile_path_in_library(d.path(), &m.identity.id);
    assert!(save_material_extension_to_library(d.path(), &m).is_err());
    m.extension = true;
    m.intent = Default::default();
    m.defaults = Default::default();
    save_material_extension_to_library(d.path(), &m).unwrap();
    let path = material_extension_path_in_library(d.path(), &m.identity.id);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.matches("# Values are inherited from the parent profile.")
            .count(),
        2
    );
    assert_eq!(load_material_extension_from_path(&path).unwrap(), m);
    delete_material_extension_from_library(d.path(), &m.identity.id).unwrap();
    assert!(base.is_file());
    assert!(delete_material_extension_from_library(d.path(), &m.identity.id).is_err());
    delete_material_profile_from_library(d.path(), &m.identity.id).unwrap();
}
#[test]
fn unreadable_extension_is_an_issue() {
    let d = tempfile::tempdir().unwrap();
    let m = material();
    let id = m.identity.id.clone();
    let path = material_extension_path_in_library(d.path(), &id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "bad toml").unwrap();
    let libs = ProfileLibraries::new(Some(d.path().into()), vec![assets()]);
    let (effective, issues) = libs.effective_material_by_id(&id).unwrap();
    assert_eq!(effective.identity, m.identity);
    assert!(matches!(
        issues[0],
        MaterialExtensionIssue::ExtensionUnreadable { .. }
    ));
}
#[test]
fn motion_partial_overlay_interpolation_and_duration() {
    let mut a = motion::Motion {
        lift_slow_distance_mm: Some(3.0),
        lift_slow_speed_mm_min: Some(60.0),
        ..Default::default()
    };
    assert!(a.validate().is_err());
    a = a.overlay(motion::Motion {
        retract_slow_distance_mm: Some(3.0),
        ..Default::default()
    });
    a.validate().unwrap();
    assert_eq!(a.duration_s(), 183.0);
    let b = motion::Motion {
        lift_slow_distance_mm: Some(5.0),
        ..Default::default()
    };
    assert_eq!(a.interpolate(b, 0.5).lift_slow_distance_mm, Some(4.0));
    assert_eq!(a.interpolate(b, 2.0).lift_slow_distance_mm, Some(5.0));
    assert_eq!(a.interpolate(b, 0.5).retract_slow_distance_mm, Some(3.0));
    assert!(toml_edit::de::from_str::<motion::Motion>("lift_fast_distance_mm = -1.0").is_err());
    assert!(toml_edit::de::from_str::<motion::Motion>("lift_slow_distance_mm = 1.0").is_ok());
}
#[test]
fn supports_defaults_retired_and_unknown_fields() {
    let settings: support_settings::SupportSettings =
        toml_edit::de::from_str("min_island_area_mm2 = { arbitrary = [1, 2] }").unwrap();
    assert_eq!(settings, support_settings::SupportSettings::default());
    assert!(
        !toml_edit::ser::to_string_pretty(&settings)
            .unwrap()
            .contains("min_island")
    );
    assert!(
        toml_edit::de::from_str::<support_settings::SupportSettings>("unknown = true").is_err()
    );
    settings.validate().unwrap();
}
#[test]
fn supports_validation_and_presets() {
    let mut s = support_settings::SupportSettings {
        spacing_mm: 0.0,
        tip_length_mm: 0.0,
        ..Default::default()
    };
    assert_eq!(s.validate().unwrap_err().0, "spacing_mm");
    s = support_settings::SupportSettings::default();
    let spacing = s.spacing_mm;
    s.apply_preset(SupportPreset::Heavy);
    assert_eq!(
        (
            s.tip_contact_diameter_mm,
            s.tip_length_mm,
            s.shaft_diameter_mm,
            s.contact_depth_mm
        ),
        (0.7, 2.5, 2.0, 0.2)
    );
    assert_eq!(s.spacing_mm, spacing);
    assert_eq!(s.max_column_diameter_mm, 3.2);
    assert_eq!(s.lowest_contact_mm(), 1.8);
    s.validate().unwrap();
}
#[test]
fn orient_alias_and_weights() {
    let old: recipes::Recipe = recipes::Recipe::parse(
        "version = 1\nname = 'old'\n[[steps]]\nkind = 'auto_orient'\npreset = 'smallest_footprint'",
    )
    .unwrap();
    assert!(old.serialize().unwrap().contains("most_per_plate"));
    assert_eq!(orient::Preset::Balanced.weights(), [1.0, 1.0, 1.0, 0.02]);
}
#[test]
fn recipes_validate_without_restricting_step_order() {
    for r in recipes::defaults() {
        r.validate().unwrap();
        assert_eq!(recipes::Recipe::parse(&r.serialize().unwrap()).unwrap(), r);
    }
    let mut r = recipes::defaults().remove(0);
    r.steps = vec![recipes::Step::Slice, recipes::Step::Slice];
    r.validate().unwrap();
    r.name = "  ".into();
    assert!(r.validate().is_err());
    r.name = " name ".into();
    assert_eq!(
        recipes::Recipe::parse(&r.serialize().unwrap())
            .unwrap()
            .name,
        " name "
    );
}
#[test]
fn recipe_export_tokens_and_formats() {
    let mut r = recipes::defaults().remove(0);
    for pattern in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "a:b",
        "{unknown}",
        "{name",
        "name}",
    ] {
        r.steps = vec![recipes::Step::Export {
            format: "gcode-zip".into(),
            folder: ".".into(),
            name_pattern: pattern.into(),
        }];
        assert!(r.validate().is_err(), "{pattern}");
    }
    r.steps = vec![recipes::Step::Export {
        format: "gcode-zip".into(),
        folder: ".".into(),
        name_pattern: " {name}_{plate} ".into(),
    }];
    r.validate().unwrap();
}
#[test]
fn plate_roundtrip_and_scene_validation() {
    let text = "name='plate'\naa='x4'\nfirst_layer_height=0.1\n[process]\ntransition_layers=1001\nlight_off_delay_s=-2.0\nfuture=1";
    let p = scene_overrides::PlateSettings::parse(text).unwrap();
    assert_eq!(
        scene_overrides::PlateSettings::parse(&p.serialize().unwrap()).unwrap(),
        p
    );
    assert_eq!(p.process.transition_layers, Some(1001));
    assert!(scene_overrides::PlateSettings::parse("[process]\naa_levels=1").is_err());
}
#[test]
fn layer_quantisation_can_escape_range_and_overrides_do_not_hide_errors() {
    let mut p = printer();
    p.constraints.z_step_mm = Some(mm(0.03));
    p.constraints.layer_height_range_mm = Some(LengthRange::new(mm(0.04), mm(0.05)).unwrap());
    let m: EffectiveMaterial = material().into();
    assert_eq!(
        resolve_layer_height(&p, &m, Some(mm(0.05)))
            .unwrap()
            .value(),
        0.06
    );
    assert_eq!(snap_to_z_step(&p.constraints, mm(0.045)).value(), 0.06);
    p.constraints.lift_speed_range_mm_min = Some(
        SpeedRangeMmPerMin::new(
            PositiveFiniteMmPerMin::new(10000.0).unwrap(),
            PositiveFiniteMmPerMin::new(20000.0).unwrap(),
        )
        .unwrap(),
    );
    assert!(matches!(
        resolve_layer_height(&p, &m, Some(mm(0.05))),
        Err(ResolveError::RejectedBelowMinimum { .. })
    ));
}
#[test]
fn resolution_provenance_and_invalid_delay_override() {
    let p = printer();
    let m: EffectiveMaterial = material().into();
    let original = resolve(&p, &m, &SceneOverrides::default()).unwrap();
    let o = SceneOverrides {
        light_off_delay_s: Some(f64::NAN),
        normal_exposure_sec: Some(PositiveFiniteSec::new(8.0).unwrap()),
        ..Default::default()
    };
    let resolved = resolve(&p, &m, &o).unwrap();
    assert_eq!(
        resolved.material_resolved.light_off_delay_s,
        original.material_resolved.light_off_delay_s
    );
    assert_eq!(resolved.material_resolved.normal_exposure_sec.value(), 8.0);
    let record = resolved.overrides_applied.last().unwrap();
    assert_eq!(record.source, ValueSource::Override);
    assert_eq!(record.final_value, 8.0);
    assert!(record.prior_value.is_some());
}
#[test]
fn accuracy_resolves_independently() {
    let p = printer();
    let mut m = material();
    m.per_printer.clear();
    m.defaults.bottom_compensation_mm = Some(0.1);
    let mut effective: EffectiveMaterial = m.into();
    assert_eq!(
        resolve_accuracy(&p, &effective, &Default::default()),
        (0.1, 0.0, 256)
    );
    effective.per_printer.insert(
        CalibrationTarget::Generic,
        MaterialPerPrinter {
            xy_compensation_mm: Some(-0.02),
            ..Default::default()
        },
    );
    assert_eq!(
        resolve_accuracy(
            &p,
            &effective,
            &SceneOverrides {
                aa_levels: Some(8),
                ..Default::default()
            }
        ),
        (0.1, -0.02, 8)
    );
}
#[test]
fn partial_motion_is_returned_and_full_cycle_is_checked() {
    let p = printer();
    let mut m = material();
    m.defaults.motion = Some(motion::MotionProfile {
        normal: motion::Motion {
            wait_before_cure_s: Some(2.0),
            ..Default::default()
        },
        ..Default::default()
    });
    let resolved = resolve(&p, &m.clone().into(), &Default::default()).unwrap();
    assert_eq!(
        resolved.material_resolved.motion.normal.wait_before_cure_s,
        Some(2.0)
    );
    assert!(
        resolved
            .material_resolved
            .motion
            .normal
            .lift_slow_distance_mm
            .is_none()
    );
    m.defaults
        .motion
        .as_mut()
        .unwrap()
        .normal
        .lift_fast_distance_mm = Some(2.0);
    assert!(matches!(
        resolve(&p, &m.into(), &Default::default()),
        Err(ResolveError::InvalidMotion(_))
    ));
}
fn archive(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap()
}
#[test]
fn package_raw_roundtrip_and_manifest_order() {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir_all(source.path().join("materials/extensions")).unwrap();
    fs::create_dir_all(source.path().join("printers")).unwrap();
    for path in [
        "printers/same.toml",
        "materials/same.toml",
        "materials/extensions/same.toml",
    ] {
        fs::write(source.path().join(path), b"# raw bytes\ninvalid TOML").unwrap();
    }
    let mut bytes = Cursor::new(Vec::new());
    export_package(
        source.path(),
        &ExportSelection::Ids(vec!["same".into(), "same".into()]),
        &ExportManifest::stamped_now("test"),
        &mut bytes,
    )
    .unwrap();
    bytes.set_position(0);
    let mut zip = zip::ZipArchive::new(bytes.clone()).unwrap();
    let names = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "manifest.toml",
            "printers/same.toml",
            "materials/same.toml",
            "materials/extensions/same.toml"
        ]
    );
    let dest = tempfile::tempdir().unwrap();
    import_package(bytes, dest.path(), false).unwrap();
    assert_eq!(
        fs::read(dest.path().join("materials/same.toml")).unwrap(),
        b"# raw bytes\ninvalid TOML"
    );
}
#[test]
fn package_collisions_are_detected_before_writes() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join("printers")).unwrap();
    fs::write(d.path().join("printers/z.toml"), b"old").unwrap();
    let bytes = archive(&[("printers/a.toml", b"new"), ("printers/z.toml", b"new")]);
    let err = import_package(bytes.clone(), d.path(), false).unwrap_err();
    assert!(matches!(err, ImportError::Collisions(_)));
    assert!(!d.path().join("printers/a.toml").exists());
    import_package(bytes, d.path(), true).unwrap();
    assert_eq!(fs::read(d.path().join("printers/z.toml")).unwrap(), b"new");
}
#[test]
fn package_ignores_unrecognized_paths_and_accepts_empty() {
    let d = tempfile::tempdir().unwrap();
    let bytes = archive(&[
        ("printers/../evil.toml", b"x"),
        ("printers/A.toml", b"x"),
        ("printers/ok.TOML", b"x"),
        ("materials/nested/a.toml", b"x"),
        ("manifest.toml", b"bad"),
    ]);
    import_package(bytes, d.path(), false).unwrap();
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    import_package(archive(&[]), d.path(), false).unwrap();
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
}
#[test]
fn package_selection_errors_follow_input_order() {
    let d = tempfile::tempdir().unwrap();
    let e = export_package(
        d.path(),
        &ExportSelection::Ids(vec!["missing".into(), "INVALID".into()]),
        &ExportManifest::stamped_now("v"),
        Cursor::new(Vec::new()),
    )
    .unwrap_err();
    assert!(matches!(e, ExportError::UnknownId { .. }));
}
#[test]
fn printer_image_copies_without_decoding() {
    let d = tempfile::tempdir().unwrap();
    let source = d.path().join("photo.JPG");
    fs::write(&source, b"not an image").unwrap();
    let r = save_printer_image_to_library(d.path(), &generic_printer_id(), &source).unwrap();
    assert_eq!(r.as_str(), "images/generic-machine.jpg");
    assert_eq!(fs::read(r.resolve(d.path())).unwrap(), b"not an image");
}
#[test]
#[ignore = "manual timing; no historical implementation is consulted"]
fn bundled_load_timing() {
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let root = assets();
        for id in list_printer_ids(&root).unwrap() {
            std::hint::black_box(load_printer_profile_from_library(&root, &id).unwrap());
        }
        for id in list_material_ids(&root).unwrap() {
            std::hint::black_box(load_material_profile_from_library(&root, &id).unwrap());
        }
    }
    eprintln!("100 complete library loads: {:?}", start.elapsed());
}
#[test]
fn package_collision_report_is_per_kind_and_sorted() {
    let d = tempfile::tempdir().unwrap();
    for dir in ["printers", "materials/extensions"] {
        fs::create_dir_all(d.path().join(dir)).unwrap();
    }
    for file in ["printers/b.toml", "printers/a.toml", "materials/m.toml", "materials/extensions/x.toml"] {
        fs::write(d.path().join(file), b"old").unwrap();
    }
    let bytes = archive(&[
        ("printers/b.toml", b"1"),
        ("printers/a.toml", b"1"),
        ("materials/m.toml", b"1"),
        ("materials/extensions/x.toml", b"1"),
        ("printers/new.toml", b"1"),
    ]);
    let Err(ImportError::Collisions(report)) = import_package(bytes, d.path(), false) else {
        panic!("collisions expected");
    };
    let ids = |list: &[Collision]| list.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&report.printers), ["a", "b"]);
    assert_eq!(ids(&report.materials), ["m"]);
    assert_eq!(ids(&report.material_extensions), ["x"]);
    assert_eq!(report.printers[0].library_path, d.path().join("printers/a.toml"));
    assert_eq!(report.len(), 4);
    assert!(!report.is_empty());
    assert!(!d.path().join("printers/new.toml").exists());
}
#[test]
fn package_selections_pick_their_kinds() {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir_all(source.path().join("materials/extensions")).unwrap();
    fs::create_dir_all(source.path().join("printers")).unwrap();
    for path in ["printers/p.toml", "materials/m.toml", "materials/extensions/e.toml"] {
        fs::write(source.path().join(path), b"x").unwrap();
    }
    let names = |selection: ExportSelection| {
        let mut bytes = Cursor::new(Vec::new());
        export_package(source.path(), &selection, &ExportManifest::stamped_now("t"), &mut bytes).unwrap();
        let mut zip = zip::ZipArchive::new(bytes).unwrap();
        (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned()).collect::<Vec<_>>()
    };
    assert_eq!(names(ExportSelection::All), ["manifest.toml", "printers/p.toml", "materials/m.toml", "materials/extensions/e.toml"]);
    assert_eq!(names(ExportSelection::Printers), ["manifest.toml", "printers/p.toml"]);
    assert_eq!(names(ExportSelection::Materials), ["manifest.toml", "materials/m.toml", "materials/extensions/e.toml"]);
    let invalid = export_package(
        source.path(),
        &ExportSelection::Ids(vec!["NOT VALID".into()]),
        &ExportManifest::stamped_now("t"),
        Cursor::new(Vec::new()),
    );
    assert!(matches!(invalid, Err(ExportError::InvalidId { .. })));
}
#[test]
fn package_error_messages_are_stable() {
    use std::error::Error as _;
    let io = || std::io::Error::other("disk");
    let unknown = ExportError::UnknownId { id: "x".into() };
    assert_eq!(unknown.to_string(), "profile id \"x\" matched no printer or material in the library");
    assert!(unknown.source().is_none());
    let read = ExportError::ReadProfile { path: "printers/p.toml".into(), cause: io() };
    assert_eq!(read.to_string(), "failed to read profile printers/p.toml for export: disk");
    let zip = ExportError::Zip(zip::result::ZipError::FileNotFound);
    assert_eq!(zip.to_string(), format!("failed to write the package archive: {}", zip::result::ZipError::FileNotFound));
    let invalid = PrinterId::new("NOT VALID").unwrap_err();
    let bad_id = ExportError::InvalidId { id: "NOT VALID".into(), cause: invalid.clone() };
    assert_eq!(bad_id.to_string(), format!("invalid profile id \"NOT VALID\": {invalid}"));
    assert!(bad_id.source().is_some());
    let archive_error = ImportError::Archive(zip::result::ZipError::FileNotFound);
    assert_eq!(archive_error.to_string(), format!("failed to read the package archive: {}", zip::result::ZipError::FileNotFound));
    let entry = ImportError::ReadEntry { name: "printers/p.toml".into(), cause: io() };
    assert_eq!(entry.to_string(), "failed to read package entry \"printers/p.toml\": disk");
    let report = CollisionReport { printers: vec![Collision { id: "p".into(), library_path: "p".into() }], ..Default::default() };
    let clash = ImportError::Collisions(report);
    assert_eq!(clash.to_string(), "import would overwrite 1 existing profile(s)");
    assert!(clash.source().is_none());
    let dir = ImportError::CreateSubdir { path: "printers".into(), cause: io() };
    assert_eq!(dir.to_string(), "failed to create the destination directory printers: disk");
    assert!(dir.source().is_some());
    let write = ImportError::WriteProfile { path: "printers/p.toml".into(), cause: io() };
    assert_eq!(write.to_string(), "failed to write profile printers/p.toml: disk");
}
#[test]
fn package_import_checks_the_archive_structure_first() {
    let d = tempfile::tempdir().unwrap();
    // Junk before the archive: other programs could read it differently.
    let mut prefixed = b"not part of the archive".to_vec();
    prefixed.extend(archive(&[("printers/p.toml", b"x")]).into_inner());
    let refused = import_package(Cursor::new(prefixed), d.path(), false);
    assert!(matches!(refused, Err(ImportError::Structure(_))), "{refused:?}");
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    // Both headers claim 17 MB for an empty stored entry: consistent, but
    // larger than any profile, and refused before anything is written.
    let mut bytes = {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("printers/p.toml", stored).unwrap();
        zip.finish().unwrap().into_inner()
    };
    let claimed = (17u32 * 1024 * 1024).to_le_bytes();
    let at = |bytes: &[u8], signature: &[u8]| bytes.windows(4).position(|w| w == signature).unwrap();
    let local = at(&bytes, b"PK\x03\x04");
    bytes[local + 22..local + 26].copy_from_slice(&claimed);
    let central = at(&bytes, b"PK\x01\x02");
    bytes[central + 24..central + 28].copy_from_slice(&claimed);
    let refused = import_package(Cursor::new(bytes), d.path(), false);
    assert!(matches!(refused, Err(ImportError::ProfileTooLarge { .. })), "{refused:?}");
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    // A normal package still imports.
    import_package(archive(&[("printers/p.toml", b"x")]), d.path(), false).unwrap();
    assert_eq!(fs::read(d.path().join("printers/p.toml")).unwrap(), b"x");
}

#[test]
fn library_messages_name_the_profile_kind() {
    let empty = tempfile::tempdir().unwrap();
    let libraries = ProfileLibraries::new(Some(empty.path().to_path_buf()), vec![]);
    let printer_id = generic_printer_id();
    let resin_id: MaterialId = "absent-resin".parse().unwrap();
    let printer_missing = libraries.printer_by_id(&printer_id).unwrap_err().to_string();
    let resin_missing = libraries.material_by_id(&resin_id).unwrap_err().to_string();
    assert!(printer_missing.starts_with("printer profile not found in any of the configured sources: "));
    assert!(resin_missing.starts_with("material profile not found in any of the configured sources: "));
    let none = ProfileLibraries::new(None, vec![]).material_by_id(&resin_id).unwrap_err();
    assert_eq!(none.to_string(), "material profile not found in any of the configured sources (no profile library roots configured)");

    let broken = empty.path().join("broken.toml");
    fs::write(&broken, "not = [toml").unwrap();
    let printer_error = load_printer_profile_from_path(&broken).unwrap_err();
    let resin_error = load_material_profile_from_path(&broken).unwrap_err();
    assert!(matches!(printer_error.kind, LoadPrinterProfileErrorKind::TomlParse(_)));
    assert!(printer_error.to_string().starts_with(&format!("failed to load printer profile from {}: ", broken.display())));
    assert!(resin_error.to_string().starts_with(&format!("failed to load material profile from {}: ", broken.display())));
    assert!(std::error::Error::source(&resin_error).is_some());

    // A library whose copy will not read stops the search there.
    fs::create_dir_all(empty.path().join("materials")).unwrap();
    fs::write(empty.path().join("materials/absent-resin.toml"), "not = [toml").unwrap();
    assert!(matches!(libraries.material_by_id(&resin_id), Err(LoadMaterialProfileByIdError::Source(_))));
}
