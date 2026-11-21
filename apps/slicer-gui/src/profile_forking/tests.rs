// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;
use sc_print_primitives::PrinterId;
use sc_print_profiles::load_material_profile_from_path;

fn source() -> MaterialProfile {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.toml");
    std::fs::write(
        &path,
        r#"
[identity]
id = "fixture-source"
brand = "Fixture maker"
family = "Fixture family"
name = "Source resin"
profile_version = "3.2.1"
[intent]
target_layer_height_mm = 0.06
target_energy_density_mj_per_cm2 = 4.0
[defaults]
bottom_layer_count = 6
transition_layers = 4
density_g_ml = 1.05
bottom_compensation_mm = 0.1
xy_compensation_mm = 0.02
aa_levels = 4
light_off_delay_s = 1.5
price_per_liter = 24.0
support_overhang_angle_deg = 40.0
[per_printer.fixture-printer]
normal_exposure_sec = 2.25
[per_printer.generic-machine]
normal_exposure_sec = 3.5
"#,
    )
    .unwrap();
    load_material_profile_from_path(&path).unwrap()
}

fn target(id: &str) -> CalibrationTarget {
    PrinterId::new(id).unwrap().into()
}

#[test]
fn all_section_choices_copy_exactly_the_selected_data() {
    let mut source = source();
    for extension in [false, true] {
        source.extension = extension;
        let before = source.clone();
        for mask in 0..8 {
            let parts = CopiedSections {
                with_intent: mask & 1 != 0,
                with_defaults: mask & 2 != 0,
                printer_blocks: if mask & 4 != 0 {
                    source.per_printer.keys().cloned().collect()
                } else {
                    vec![]
                },
            };
            let fork = forked_resin(
                &source,
                &parts,
                "  New resin  ",
                "  New maker  ",
                &BTreeSet::new(),
            )
            .unwrap();
            assert!(!fork.extension);
            assert_eq!(fork.identity.id.as_str(), "new-resin");
            assert_eq!(fork.identity.name.as_str(), "New resin");
            assert_eq!(fork.identity.brand.as_str(), "New maker");
            assert_eq!(fork.identity.family, source.identity.family);
            assert_eq!(
                fork.identity.profile_version,
                source.identity.profile_version
            );
            assert_eq!(
                fork.intent,
                if parts.with_intent {
                    source.intent
                } else {
                    Default::default()
                }
            );
            assert_eq!(
                fork.defaults,
                if parts.with_defaults {
                    source.defaults
                } else {
                    Default::default()
                }
            );
            if parts.printer_blocks.is_empty() {
                assert!(fork.per_printer.is_empty());
            } else {
                assert_eq!(fork.per_printer, source.per_printer);
            }
        }
        assert_eq!(source, before);
    }
}

#[test]
fn selected_calibrations_ignore_unknown_targets_and_duplicates() {
    let source = source();
    let parts = CopiedSections {
        with_intent: false,
        with_defaults: true,
        printer_blocks: vec![
            CalibrationTarget::Generic,
            target("missing"),
            CalibrationTarget::Generic,
        ],
    };
    let fork = forked_resin(&source, &parts, "Subset", "Maker", &BTreeSet::new()).unwrap();
    assert_eq!(fork.per_printer.len(), 1);
    assert_eq!(
        fork.per_printer.get(&CalibrationTarget::Generic),
        source.per_printer.get(&CalibrationTarget::Generic)
    );
    assert_eq!(fork.defaults, source.defaults);
    assert_eq!(fork.intent, Default::default());
}

#[test]
fn slug_generation_collapses_punctuation_without_transliterating() {
    let source = source();
    for (name, slug) in [
        ("  A—B__C !! 12_é  ", "a-b-c-12"),
        ("MIXED.case", "mixed-case"),
        ("__7__", "7"),
    ] {
        let fork = forked_resin(
            &source,
            &CopiedSections::default(),
            name,
            "Maker",
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(fork.identity.id.as_str(), slug);
        assert_eq!(fork.identity.name.as_str(), name.trim());
    }
    let occupied = [
        MaterialId::new("new-resin").unwrap(),
        MaterialId::new("new-resin-2").unwrap(),
    ]
    .into();
    assert_eq!(
        forked_resin(
            &source,
            &CopiedSections::default(),
            "New Resin",
            "Maker",
            &occupied
        )
        .unwrap()
        .identity
        .id
        .as_str(),
        "new-resin-3"
    );
}

#[test]
fn collision_search_includes_9999_and_stops_before_10000() {
    let source = source();
    let mut occupied = (2..9999)
        .map(|n| MaterialId::new(&format!("limit-{n}")).unwrap())
        .collect::<BTreeSet<_>>();
    occupied.insert(MaterialId::new("limit").unwrap());
    assert_eq!(
        forked_resin(&source, &CopiedSections::default(), "Limit", "Maker", &occupied)
            .unwrap()
            .identity
            .id
            .as_str(),
        "limit-9999"
    );
    occupied.insert(MaterialId::new("limit-9999").unwrap());
    assert_eq!(
        forked_resin(&source, &CopiedSections::default(), "Limit", "Maker", &occupied).unwrap_err(),
        ForkRefusal::NoFreeId
    );
}

#[test]
fn validation_priority_and_messages_stay_stable() {
    let source = source();
    for (name, brand, error, text) in [
        (
            " \t ",
            "",
            ForkRefusal::NameMissing,
            "Enter a name for the forked profile",
        ),
        (
            "???",
            " \n ",
            ForkRefusal::BrandMissing,
            "Enter a brand for the forked profile",
        ),
        (
            "é☃?",
            "Maker",
            ForkRefusal::NoFreeId,
            "Could not derive a free profile id from that name",
        ),
    ] {
        let result = forked_resin(
            &source,
            &CopiedSections::default(),
            name,
            brand,
            &BTreeSet::new(),
        )
        .unwrap_err();
        assert_eq!(result, error);
        assert_eq!(result.to_string(), text);
        assert_eq!(result.dialog_line().as_str(), text);
    }
}
