// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Behavioral pins for preferences models and callback wiring.
#[allow(unused_imports)]
use crate::panel_place::Panel;
use super::*;
use crate::app_settings::AppSettings;
use crate::{MenuLayout, ThemePreference};
use sc_print_primitives::{AaLevel, BuildVolume};

fn initial() -> UserPreferences {
    let saved = AppSettings::default();
    UserPreferences {
        orient_preset: saved.orient_preset,
        perspective_view: saved.perspective_view,
        setup_complete: saved.setup_complete,
        build_volume: crate::build_volume::FALLBACK_VOLUME,
        theme: saved.theme(),
        menu_layout: saved.menu_layout(),
        language: saved.language(),
        currency_symbol: saved.currency_symbol(),
        default_printer: saved.default_printer(),
        default_resin: saved.default_resin(),
        favourite_printers: saved.favourite_printers(),
        favourite_resins: saved.favourite_resins(),
        aa_level: saved.aa_level(),
        last_open_dir: saved.last_open_dir(),
        recent_files: saved.recent_files(),
        object_list: Panel::ObjectList.start(),
        slice_panel: Panel::SliceControls.start(),
        slice_view: Panel::SliceView.start(),
    }
}
fn machine(id: &str, name: &str) -> CatalogPrinter {
    CatalogPrinter {
        id: PrinterId::new(id).unwrap(),
        printer_maker: "Factory".into(),
        name: name.into(),
        series: String::new(),
        printer_plate_volume: BuildVolume::new(90., 70., 120.).unwrap(),
        picture_path: None,
        profile_origin: ProfileSource::Bundled,
    }
}
fn resin(id: &str, keys: &[&str]) -> CatalogResin {
    CatalogResin {
        id: MaterialId::new(id).unwrap(),
        brand: "Brand".into(),
        name: id.into(),
        resin_family: "other".into(),
        calibrated_for: keys
            .iter()
            .map(|s| CalibrationTarget::from(PrinterId::new(s).unwrap()))
            .collect(),
        profile_origin: ProfileSource::Bundled,
        overhang_limit_deg: 45.,
    }
}
fn layout() -> TagSizing {
    TagSizing {
        font: None,
        text_size: Cell::new(12.),
        row_widths: RefCell::new(BTreeMap::new()),
    }
}
fn pills(row: &ResinProfileEntry) -> Vec<(String, CalibrationTagClass)> {
    row.calibration_tag_lines
        .iter()
        .flat_map(|line| line.iter().collect::<Vec<_>>())
        .map(|p| (p.calibration_caption.to_string(), p.calibration_class))
        .collect()
}
#[test]
fn filter_and_position_boundary_table() {
    let tabs = vec!["All".into(), "Factory".into(), "Elsewhere".into()];
    for (index, expected) in [
        (-1, None),
        (0, None),
        (1, Some("Factory")),
        (2, Some("Elsewhere")),
        (3, None),
    ] {
        assert_eq!(brand_at(&tabs, index), expected);
    }
    for (label, index) in [
        (None, 0),
        (Some("All"), 0),
        (Some("Factory"), 1),
        (Some("missing"), 0),
    ] {
        assert_eq!(brand_position(&tabs, label), index);
    }
    for filter in [None, Some("Factory"), Some("Elsewhere")] {
        for query in ["", "mono", "MONO", "missing", " "] {
            assert_eq!(
                matches_filter("Factory", "mono printer", filter, query),
                filter != Some("Elsewhere") && "mono printer".contains(query)
            );
        }
    }
    let cards = vec![
        PrinterEntry {
            catalogue_index: 8,
            ..Default::default()
        },
        PrinterEntry {
            catalogue_index: 2,
            ..Default::default()
        },
    ];
    for (id, position) in [(8, 0), (2, 1), (-1, -1), (3, -1)] {
        assert_eq!(visible_selection(&cards, id), position);
    }
}
#[test]
fn catalog_rows_pin_tiers_extensions_stars_and_tag_order() {
    let machines = vec![machine("machine-a", "Zulu"), machine("machine-b", "Alpha")];
    let materials = vec![
        resin("empty", &[]),
        resin("remote", &["machine-b"]),
        resin("matching", &["machine-a", "machine-b", "generic-machine"]),
        resin("extended", &[]),
    ];
    let extensions = vec![CatalogExtension {
        id: materials[3].id.clone(),
        extra_calibrations: [CalibrationTarget::from(machines[0].id.clone())].into(),
    }];
    let favourites = [materials[3].id.clone()].into();
    let rows = material_rows(
        &materials,
        &extensions,
        &machines,
        Some(&machines[0].id),
        &favourites,
        &layout(),
    );
    assert_eq!(
        rows.iter()
            .map(|r| (r.resin_catalog_position, r.represents_extension, r.marked_as_favourite, r.allows_extension))
            .collect::<Vec<_>>(),
        vec![
            (2, false, false, false),
            (3, false, true, true),
            (3, true, true, true),
            (1, false, false, true),
            (0, false, false, true)
        ]
    );
    assert_eq!(
        pills(&rows[0]),
        vec![
            ("Zulu".into(), CalibrationTagClass::DefaultPrinterCalibration),
            ("Generic".into(), CalibrationTagClass::SharedCalibration),
            ("Alpha".into(), CalibrationTagClass::AlternatePrinterCalibration)
        ]
    );
    assert!(pills(&rows[1]).is_empty());
    assert_eq!(pills(&rows[2]), vec![("Zulu".into(), CalibrationTagClass::DefaultPrinterCalibration)]);
    assert_eq!(
        pills(&rows[4]),
        vec![("No calibration data".into(), CalibrationTagClass::AbsentCalibrationData)]
    );
    assert_eq!(rows[2].resin_display_name.as_str(), "extended (extended)");
    for row in material_rows(
        &materials,
        &extensions,
        &machines,
        None,
        &favourites,
        &layout(),
    ) {
        assert!(pills(&row).iter().all(|(_, kind)| *kind == CalibrationTagClass::AlternatePrinterCalibration));
    }
    let cards = gallery_entries(&machines, &[machines[1].id.clone()].into());
    assert_eq!(cards.len(), 2);
    assert!(!cards[0].tile.has_picture && cards[1].tile.starred);
    assert_eq!(cards[0].search_name, "zulu");
}
#[test]
fn greedy_lines_keep_oversized_pills_and_exact_fits() {
    let make = || {
        ["aa", "bb", "c"]
            .map(|s| CalibrationTagData {
                calibration_caption: s.into(),
                calibration_class: CalibrationTagClass::AlternatePrinterCalibration,
            })
            .to_vec()
    };
    let sizes = |budget| {
        wrap_tags(make(), budget, &|s| s.len() as f32)
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>()
    };
    assert_eq!(sizes(36.), vec![2, 1]);
    assert_eq!(sizes(35.9), vec![1, 2]);
    assert_eq!(sizes(-1.), vec![1, 1, 1]);
    assert_eq!(sizes(f32::NAN), vec![3]);
    assert!(wrap_tags(Vec::new(), 0., &|_| 0.).is_empty());
    assert_eq!(layout().budget(7, true), 340.);
    assert_eq!(layout().measure("x"), 7.2000003);
}
/// The gallery shows Sleptic's own render of each bundled printer (R100),
/// the picture the sidebar shows, not the profile's image file.
#[test]
fn bundled_printers_show_sleptic_pictures_in_the_gallery() {
    let tiles = gallery_entries(
        &[machine("anycubic-photon-mono-m7-pro-pwsz", "M7 Pro"), machine("one", "One")],
        &BTreeSet::new(),
    );
    assert!(tiles[0].tile.has_picture, "a bundled printer has its picture");
    assert!(tiles[0].tile.picture.size().width > 0);
    assert!(!tiles[1].tile.has_picture, "an unknown printer has none");
}
#[test]
fn thumbnail_updates_use_catalog_ids_with_filtered_rows() {
    let mut all = gallery_entries(
        &[machine("one", "One"), machine("two", "Two")],
        &BTreeSet::new(),
    );
    all[1].tile.has_picture = true;
    let visible = ModelRc::new(VecModel::from(vec![
        all[1].tile.clone(),
        all[0].tile.clone(),
    ]));
    let mut stale = visible.row_data(0).unwrap();
    stale.has_picture = false;
    visible.set_row_data(0, stale);
    publish_images(&visible, &all, &[1].into());
    assert!(visible.row_data(0).unwrap().has_picture);
    assert!(!visible.row_data(1).unwrap().has_picture);
    let folder = tempfile::tempdir().unwrap();
    assert!(read_pixels(&folder.path().join("missing.png")).is_none());
    let image_path = folder.path().join("sample.png");
    image::RgbaImage::from_pixel(2, 1, image::Rgba([3, 7, 11, 255]))
        .save(&image_path)
        .unwrap();
    assert_eq!(
        read_pixels(&image_path).unwrap().as_bytes(),
        &[3, 7, 11, 255, 3, 7, 11, 255]
    );
}
#[test]
fn wired_callbacks_publish_exact_changes_and_leave_filters_local() {
    i_slint_backend_testing::init_no_event_loop();
    let catalog = Rc::new(RefCell::new(Rc::new(InstalledProfiles::scan_libraries(
        &ProfileLibraries::new(None, Vec::new()),
    ))));
    let bus = PreferenceHub::new(initial());
    let changes = Rc::new(RefCell::new(Vec::new()));
    let _listener = bus.watch_preferences({
        let changes = changes.clone();
        move |_, change| changes.borrow_mut().push(*change)
    });
    let wired = build_preferences_window(&bus, catalog.clone()).unwrap();
    let ui = &wired.window;
    ui.invoke_theme_picked(ThemePreference::Dark);
    assert_eq!(bus.current_values().theme, ThemePreference::Dark);
    ui.invoke_language_picked(-1);
    assert!(bus.current_values().language.is_system());
    ui.invoke_currency_edited("  ".into());
    assert_eq!(bus.current_values().currency_symbol, "  ");
    assert_eq!(ui.get_currency(), "  ");
    for index in [-1, 0, 1, 2, 3, 4, 99] {
        ui.invoke_aa_picked(index);
        assert_eq!(bus.current_values().aa_level, aa_level_at_row(index));
    }
    let before = changes.borrow().len();
    for index in [-1, 0, 999] {
        ui.invoke_printer_picked(index);
        ui.invoke_resin_picked(index);
        ui.invoke_printer_star_toggled(index);
        ui.invoke_resin_star_toggled(index);
        ui.invoke_view_resin(index);
        ui.invoke_edit_resin(index, false);
        ui.invoke_fork_resin(index);
        ui.invoke_extend_resin(index);
        ui.invoke_delete_resin(index, false);
        ui.invoke_open_printer(index, false);
    }
    ui.invoke_printer_maker_picked(99);
    ui.invoke_printer_query_edited("MiXeD".into());
    ui.invoke_tag_measured(-1, false, 120.);
    ui.invoke_closing();
    assert_eq!(changes.borrow().len(), before);
    assert_eq!(
        &changes.borrow()[..3],
        &[
            PreferenceEdit::Theme,
            PreferenceEdit::Language,
            PreferenceEdit::CurrencySymbol
        ]
    );
    assert!(ui.get_printer_tiles().row_count() == 0 && ui.get_resin_list().row_count() == 0);
    assert!(!selection_was_removed(
        &catalog.borrow(),
        None,
        &MaterialId::new("gone").unwrap()
    ));
    assert!(selection_was_removed(
        &catalog.borrow(),
        Some(&MaterialId::new("gone").unwrap()),
        &MaterialId::new("gone").unwrap()
    ));
    let _ = (AaLevel::Off, MenuLayout::Bar);
}
#[test]
fn valid_selections_favourites_and_external_bus_updates_match_the_catalog() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("printers")).unwrap();
    std::fs::create_dir(directory.path().join("materials")).unwrap();
    let hardware = include_str!("../../../assets/profiles/printers/generic-machine.toml");
    for id in ["alpha-machine", "beta-machine"] {
        let document = hardware
            .replace("generic-machine", id)
            .replace("Generic Machine", id)
            .replace(
                "manufacturer = \"Generic\"",
                &format!("manufacturer = \"{id}\""),
            );
        std::fs::write(
            directory.path().join("printers").join(format!("{id}.toml")),
            document,
        )
        .unwrap();
    }
    std::fs::write(directory.path().join("materials/sample.toml"),"[identity]\nid='sample'\nbrand='Fixture'\nfamily='other'\nname='Sample'\nprofile_version='1.0.0'\n[intent]\n[defaults]\n").unwrap();
    let library = Rc::new(InstalledProfiles::scan_libraries(&ProfileLibraries::new(
        None,
        vec![directory.path().to_owned()],
    )));
    assert_eq!(
        (library.printers().len(), library.resin_list().len()),
        (2, 1)
    );
    let bus = PreferenceHub::new(initial());
    let changes = Rc::new(RefCell::new(Vec::new()));
    let _listener = bus.watch_preferences({
        let changes = changes.clone();
        move |_, change| changes.borrow_mut().push(*change)
    });
    let handle = Rc::new(RefCell::new(library.clone()));
    let wired = build_preferences_window(&bus, handle.clone()).unwrap();
    let ui = &wired.window;
    for index in [1, 0] {
        ui.invoke_printer_picked(index);
        assert_eq!(
            bus.current_values().default_printer,
            Some(library.printers()[index as usize].id.clone())
        );
        assert_eq!(
            bus.current_values().build_volume,
            library.volume_for_printer(bus.current_values().default_printer.as_ref())
        );
        assert_eq!(ui.get_chosen_printer(), index);
    }
    ui.invoke_resin_picked(0);
    assert_eq!(
        bus.current_values().default_resin,
        Some(library.resin_list()[0].id.clone())
    );
    ui.invoke_printer_maker_picked(0);
    ui.invoke_printer_star_toggled(1);
    assert!(
        bus.current_values()
            .favourite_printers
            .contains(&library.printers()[1].id)
    );
    assert!(ui.get_printer_tiles().row_data(1).unwrap().starred);
    ui.invoke_printer_query_edited("BETA".into());
    assert_eq!(ui.get_printer_tiles().row_count(), 1);
    assert_eq!(ui.get_chosen_printer_tile(), -1);
    ui.invoke_printer_star_toggled(1);
    assert!(!ui.get_printer_tiles().row_data(0).unwrap().starred);
    ui.invoke_resin_star_toggled(0);
    assert!(ui.get_resin_list().row_data(0).unwrap().marked_as_favourite);
    ui.invoke_resin_star_toggled(0);
    assert!(!ui.get_resin_list().row_data(0).unwrap().marked_as_favourite);
    ui.invoke_tag_measured(0, false, 20.);
    let rows = ui.get_resin_list();
    ui.invoke_tag_measured(0, false, 20.4);
    assert_eq!(ui.get_resin_list().row_count(), rows.row_count());
    assert_eq!(
        &*changes.borrow(),
        &[
            PreferenceEdit::DefaultPrinter,
            PreferenceEdit::DefaultPrinter,
            PreferenceEdit::DefaultResin,
            PreferenceEdit::Favourites,
            PreferenceEdit::Favourites,
            PreferenceEdit::Favourites,
            PreferenceEdit::Favourites
        ]
    );
    bus.edit_preferences(|state| {
        state.theme = ThemePreference::Light;
        PreferenceEdit::Theme
    });
    assert_eq!(ui.get_theme(), ThemePreference::Light);
    assert_eq!(ui.get_printer_tiles().row_count(), 1);
    assert!(!selection_was_removed(
        &library,
        Some(&library.resin_list()[0].id),
        &library.resin_list()[0].id
    ));
}
struct LibraryGuard;
impl Drop for LibraryGuard {
    fn drop(&mut self) {
        TEST_LIBRARIES.with(|slot| slot.replace(None));
    }
}
fn isolated_profiles(root: &Path) -> LibraryGuard {
    TEST_LIBRARIES.with(|slot| {
        slot.replace(Some(ProfileLibraries::new(
            Some(root.to_owned()),
            Vec::new(),
        )))
    });
    LibraryGuard
}
fn reload_catalog(root: &Path) -> Rc<RefCell<Rc<InstalledProfiles>>> {
    Rc::new(RefCell::new(Rc::new(InstalledProfiles::scan_libraries(
        &ProfileLibraries::new(Some(root.to_owned()), Vec::new()),
    ))))
}
fn sample_profile(root: &Path) -> MaterialProfile {
    std::fs::create_dir_all(root.join("printers")).unwrap();
    let printer = include_str!("../../../assets/profiles/printers/generic-machine.toml")
        .replace("generic-machine", "fixture-machine");
    std::fs::write(root.join("printers/fixture-machine.toml"), printer).unwrap();
    let text = "[identity]\nid='fixture-resin'\nbrand='Fixture'\nfamily='other'\nname='Resin'\nprofile_version='1.0.0'\n[intent]\n[defaults]\ntransition_layers=3\nlight_off_delay_s=2.0\ndensity_g_ml=1.1\n[per_printer.generic-machine]\nnormal_exposure_sec=2.5\ntransition_layers=4\nlight_off_delay_s=1.5\n";
    let profile: MaterialProfile = toml::from_str(text).unwrap();
    write_edited_resin(root, &profile, ResinFileKind::WholeProfile).unwrap();
    profile
}
fn bytes_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
#[test]
fn editor_callbacks_keep_unshown_fields_and_validate_before_writing() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let _guard = isolated_profiles(root);
    let original = sample_profile(root);
    let catalog = reload_catalog(root);
    let bus = PreferenceHub::new(initial());
    let slot = Rc::new(RefCell::new(None));
    let refreshes = Rc::new(Cell::new(0));
    let refresh: Rc<dyn Fn()> = {
        let refreshes = refreshes.clone();
        Rc::new(move || refreshes.set(refreshes.get() + 1))
    };
    show_material_editor(
        &bus,
        &catalog,
        &refresh,
        &slot,
        0,
        false,
        Some("fixture note".into()),
    );
    let ui = slot.borrow().as_ref().unwrap().clone_strong();
    assert!(ui.get_allow_profile_edit() && ui.get_allow_defaults_edit());
    assert_eq!(ui.get_viewer_note(), "fixture note");
    assert_eq!(ui.get_selected_calibration_target(), -1);
    ui.set_selected_calibration_target(0);
    ui.invoke_calibration_target_selected(0);
    assert_eq!(ui.get_normal_exposure_input(), "2.5");
    ui.invoke_motion_refresh();
    ui.invoke_motion_field_changed(-1, "bad".into());
    let path = sc_print_profiles::material_profile_path_in_library(root, &original.identity.id);
    let before = std::fs::read(&path).unwrap();
    ui.set_defaults_price_text("18.75".into());
    ui.set_normal_exposure_input("bad".into());
    ui.invoke_profile_save_requested();
    assert!(!ui.get_editor_error().is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(refreshes.get(), 0);
    ui.set_normal_exposure_input("3.75".into());
    ui.invoke_profile_save_requested();
    assert!(slot.borrow().is_none());
    assert_eq!(refreshes.get(), 1);
    let saved = load_material_profile_from_library(root, &original.identity.id).unwrap();
    assert_eq!(
        saved.defaults.transition_layers,
        original.defaults.transition_layers
    );
    assert_eq!(
        saved.defaults.light_off_delay_s,
        original.defaults.light_off_delay_s
    );
    assert_eq!(saved.defaults.density_g_ml, original.defaults.density_g_ml);
    assert_eq!(
        saved.per_printer.values().next().unwrap().transition_layers,
        original
            .per_printer
            .values()
            .next()
            .unwrap()
            .transition_layers
    );
    assert_eq!(
        calibration_texts(saved.per_printer.values().next().unwrap()).text(ResinField::Exposure),
        "3.75"
    );
    assert_eq!(
        10241219610633487361,
        bytes_hash(&std::fs::read(&path).unwrap())
    );
    show_material_editor(&bus, &catalog, &refresh, &slot, 0, false, None);
    let reopened = slot.borrow().as_ref().unwrap().clone_strong();
    reopened.invoke_viewer_close_requested();
    assert!(slot.borrow().is_none());
}
#[test]
fn viewer_fork_and_extension_callbacks_round_trip_their_saved_profiles() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let _guard = isolated_profiles(root);
    let source = sample_profile(root);
    let catalog = reload_catalog(root);
    let bus = PreferenceHub::new(initial());
    let refresh: Rc<dyn Fn()> = Rc::new(|| {});
    let editor = Rc::new(RefCell::new(None));
    let fork = Rc::new(RefCell::new(None));
    let extension = Rc::new(RefCell::new(None));
    show_material_view(&bus, &catalog, &editor, 0);
    let viewer = editor.borrow().as_ref().unwrap().clone_strong();
    assert_eq!(viewer.get_resin_heading(), "Fixture Resin");
    assert!(viewer.get_profile_cards().row_count() > 0);
    viewer.invoke_viewer_close_requested();
    assert!(editor.borrow().is_none());
    show_copy_dialog(&bus, &catalog, &refresh, &fork, &editor, 0);
    let ui = fork.borrow().as_ref().unwrap().clone_strong();
    assert_eq!(ui.get_new_profile_name(), "Resin (fork)");
    ui.invoke_source_block_selection_changed(-1, false);
    ui.invoke_source_block_selection_changed(0, false);
    ui.set_new_profile_name("Copied".into());
    ui.set_include_source_defaults(false);
    ui.invoke_create_fork_requested();
    assert!(fork.borrow().is_none());
    let dialog = editor.borrow().as_ref().unwrap().clone_strong();
    assert!(!dialog.get_viewer_note().is_empty());
    dialog.invoke_viewer_close_requested();
    let fork_id = catalog
        .borrow()
        .resin_list()
        .iter()
        .find(|row| row.id != source.identity.id)
        .unwrap()
        .id
        .clone();
    let saved = load_material_profile_from_library(root, &fork_id).unwrap();
    assert!(saved.per_printer.is_empty());
    assert_eq!(
        18360930898216819063,
        bytes_hash(
            &std::fs::read(sc_print_profiles::material_profile_path_in_library(
                root, &fork_id
            ))
            .unwrap()
        )
    );
    let source_index = catalog
        .borrow()
        .resin_position(&source.identity.id)
        .unwrap() as i32;
    show_calibration_dialog(&bus, &catalog, &refresh, &extension, source_index);
    let ui = extension.borrow().as_ref().unwrap().clone_strong();
    ui.set_extension_target_selection(-1);
    ui.invoke_extension_save_requested();
    assert!(extension.borrow().is_some());
    ui.set_extension_target_selection(0);
    ui.set_extension_normal_exposure_input("bad".into());
    ui.invoke_extension_save_requested();
    assert!(!ui.get_extension_validation_message().is_empty());
    ui.set_extension_normal_exposure_input("4.5".into());
    ui.invoke_extension_save_requested();
    assert!(extension.borrow().is_none());
    let extra = load_material_extension_from_library(root, &source.identity.id).unwrap();
    assert_eq!(extra.per_printer.len(), 1);
    assert_eq!(
        11706818923965652475,
        bytes_hash(
            &std::fs::read(sc_print_profiles::material_extension_path_in_library(
                root,
                &source.identity.id
            ))
            .unwrap()
        )
    );
    let current_index = catalog
        .borrow()
        .resin_position(&source.identity.id)
        .unwrap() as i32;
    show_material_editor(&bus, &catalog, &refresh, &editor, current_index, true, None);
    let ui = editor.borrow().as_ref().unwrap().clone_strong();
    assert!(!ui.get_allow_defaults_edit());
    ui.set_selected_calibration_target(0);
    ui.invoke_calibration_target_selected(0);
    assert_eq!(ui.get_normal_exposure_input(), "4.5");
    ui.set_normal_exposure_input("5.5".into());
    ui.invoke_profile_save_requested();
    assert!(editor.borrow().is_none());
    assert_eq!(
        2235634593862079538,
        bytes_hash(
            &std::fs::read(sc_print_profiles::material_extension_path_in_library(
                root,
                &source.identity.id
            ))
            .unwrap()
        )
    );
}
#[test]
fn generic_tags_unknown_ids_and_row_widths_keep_their_special_cases() {
    let generic = PrinterId::new("generic-machine").unwrap();
    let mut machines = vec![
        machine("generic-machine", "Machine"),
        machine("one", "Same"),
        machine("two", "Same"),
    ];
    machines[1].profile_origin = ProfileSource::User;
    let keys = [
        CalibrationTarget::Generic,
        CalibrationTarget::from(machines[1].id.clone()),
        CalibrationTarget::from(machines[2].id.clone()),
        CalibrationTarget::from(PrinterId::new("absent").unwrap()),
    ]
    .into();
    let tags = calibration_tags(&keys, &machines, Some(&generic));
    assert_eq!(
        tags.iter()
            .map(|tag| (tag.calibration_caption.as_str(), tag.calibration_class))
            .collect::<Vec<_>>(),
        vec![
            ("Generic", CalibrationTagClass::DefaultPrinterCalibration),
            ("Same", CalibrationTagClass::AlternatePrinterCalibration),
            ("Same", CalibrationTagClass::AlternatePrinterCalibration),
            ("absent", CalibrationTagClass::AlternatePrinterCalibration)
        ]
    );
    let cards = gallery_entries(&machines, &BTreeSet::new());
    assert!(cards[1].tile.user_made);
    assert!(cards.iter().all(|tile| !tile.tile.starred));
    let sizing = layout();
    sizing.row_widths.borrow_mut().insert((0, false), 12.);
    assert_eq!(sizing.budget(0, false), 12.);
    assert_eq!(sizing.budget(0, true), 340.);
    let mut row = resin("custom", &["generic-machine"]);
    row.profile_origin = ProfileSource::User;
    let rows = material_rows(
        &[row],
        &[],
        &machines,
        Some(&machines[1].id),
        &BTreeSet::new(),
        &sizing,
    );
    assert!(rows[0].has_custom_profile && rows[0].allows_extension);
    assert_eq!(pills(&rows[0]), vec![("Generic".into(), CalibrationTagClass::SharedCalibration)]);
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("materials")).unwrap();
    std::fs::write(folder.path().join("materials/kept.toml"), "[identity]\nid='kept'\nbrand='Test'\nfamily='other'\nname='Kept'\nprofile_version='1.0.0'\n[intent]\n[defaults]\n").unwrap();
    let catalog = InstalledProfiles::scan_libraries(&ProfileLibraries::new(
        None,
        vec![folder.path().to_owned()],
    ));
    let kept = MaterialId::new("kept").unwrap();
    let removed = MaterialId::new("removed").unwrap();
    assert!(!selection_was_removed(&catalog, Some(&kept), &kept));
    assert!(!selection_was_removed(&catalog, Some(&kept), &removed));
}
