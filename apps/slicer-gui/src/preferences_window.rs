// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Preferences callbacks, profile dialogs and catalog projections.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

use sc_print_primitives::{MaterialId, PrinterId};
use sc_print_profiles::{
    CalibrationTarget, EffectiveMaterial, MaterialProfile, ProfileLibraries, delete_material_extension_from_library,
    delete_material_profile_from_library, load_material_extension_from_library, load_material_profile_from_library,
    user_library_dir as system_user_library_dir,
};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel,
};

use crate::aa_level::aa_level_at_row;
use crate::i18n::ctr;
use crate::language::LanguagePreference;
use crate::material_form::{
    FieldTexts, ResinField, ResinFileKind, calibration_from_texts, calibration_texts, defaults_from_texts,
    defaults_texts, write_edited_resin,
};
use crate::preferences_bus::{
    PreferenceHub, PreferenceEdit, UserPreferences, PreferenceWatch,
};
use crate::profile_catalog::{
    CatalogResin, CatalogExtension, CatalogPrinter, InstalledProfiles,
    ProfileSource, has_calibration_for,
};
use crate::profile_extending::{extension_with_calibration, printers_without_calibration};
use crate::profile_forking::{CopiedSections, forked_resin};
use crate::profile_transfer::{
    PackageKind, ImportResult, overwrite_warning, chose_overwrite, write_package,
    install_package,
};
use crate::profile_view::{
    profile_view_rows, identity_section, intent_section, ordered_calibration_targets,
    resin_title,
};
use crate::product_pictures::{self, Kind};
use crate::text_metrics::TextWidths;
use crate::window_prefs::TakesPreferences;
use crate::{
    ExtendDialog, ForkDialog, CalibrationTagClass, PreferencesWindow, PrinterEntry, ProfileViewDialog,
    CalibrationTagData, ResinProfileEntry,
};

type SharedCatalog = Rc<RefCell<Rc<InstalledProfiles>>>;
type PreferenceStore = Rc<PreferenceHub>;
type Redraw = Rc<dyn Fn()>;
type DialogSlot<T> = Rc<RefCell<Option<T>>>;

struct GalleryEntry {
    brand_key: String,
    search_name: String,
    tile: PrinterEntry,
}
pub(crate) struct PreferencesWindowBinding {
    pub(crate) window: PreferencesWindow,
    _updates: PreferenceWatch,
    _pictures: Rc<Timer>,
}

// Callbacks share the session but hold only a weak reference to the window.
struct PreferencesSession {
    ui: slint::Weak<PreferencesWindow>,
    bus: PreferenceStore,
    catalog: SharedCatalog,
    cards: Rc<RefCell<Vec<GalleryEntry>>>,
    manufacturers: RefCell<Vec<String>>,
    tab: Cell<i32>,
    query: RefCell<String>,
    pills: Rc<TagSizing>,
    thumbnails: Rc<Timer>,
    printer_editor: RefCell<Option<crate::PrinterEditor>>,
    profile: DialogSlot<ProfileViewDialog>,
    fork: DialogSlot<ForkDialog>,
    extension: DialogSlot<ExtendDialog>,
}
impl PreferencesSession {
    fn filter(&self, ui: &PreferencesWindow) {
        publish_gallery(
            ui,
            &self.cards.borrow(),
            brand_at(&self.manufacturers.borrow(), self.tab.get()),
            &self.query.borrow(),
        );
    }
    fn refresh(&self) {
        if let Some(ui) = self.ui.upgrade() {
            let catalog = self.catalog.borrow().clone();
            *self.cards.borrow_mut() =
                gallery_entries(catalog.printers(), &self.bus.current_values().favourite_printers);
            *self.manufacturers.borrow_mut() = brand_labels(&catalog);
            publish_materials(&ui, &self.bus.current_values(), &catalog, &self.pills);
            publish_brands(&ui, &self.manufacturers.borrow());
            self.filter(&ui);
            fetch_gallery_images(
                &self.thumbnails,
                &ui,
                self.cards.clone(),
                image_jobs(&catalog),
            );
        }
    }
    fn select_printer(&self, index: i32) {
        let catalog = self.catalog.borrow().clone();
        if let Some(entry) = catalog.printers().get(index as usize) {
            let id = entry.id.clone();
            let volume = catalog.volume_for_printer(Some(&id));
            self.bus.edit_preferences(move |state| {
                state.default_printer = Some(id);
                state.build_volume = volume;
                PreferenceEdit::DefaultPrinter
            });
        }
    }
    fn select_resin(&self, index: i32) {
        let catalog = self.catalog.borrow().clone();
        if let Some(entry) = catalog.resin_list().get(index as usize) {
            let id = entry.id.clone();
            self.bus.edit_preferences(move |state| {
                state.default_resin = Some(id);
                PreferenceEdit::DefaultResin
            });
        }
    }
    fn star_printer(&self, index: i32) {
        let catalog = self.catalog.borrow().clone();
        let Some(entry) = catalog.printers().get(index as usize) else {
            return;
        };
        let id = entry.id.clone();
        let enabled = !self.bus.current_values().favourite_printers.contains(&id);
        self.bus.edit_preferences(move |state| {
            if enabled {
                state.favourite_printers.insert(id);
            } else {
                state.favourite_printers.remove(&id);
            }
            PreferenceEdit::Favourites
        });
        if let Some(cached) = self.cards.borrow_mut().get_mut(index as usize) {
            cached.tile.starred = enabled;
        }
        // Replacing the whole model here would discard the loaded pictures and scroll position.
        if let Some(ui) = self.ui.upgrade() {
            let model = ui.get_printer_tiles();
            for row in 0..model.row_count() {
                if let Some(mut tile) = model.row_data(row).filter(|tile| tile.catalogue_index == index) {
                    tile.starred = enabled;
                    model.set_row_data(row, tile);
                }
            }
        }
    }
    fn star_resin(&self, index: i32) {
        let catalog = self.catalog.borrow().clone();
        if let Some(entry) = catalog.resin_list().get(index as usize) {
            let id = entry.id.clone();
            let enabled = !self.bus.current_values().favourite_resins.contains(&id);
            self.bus.edit_preferences(move |state| {
                if enabled {
                    state.favourite_resins.insert(id);
                } else {
                    state.favourite_resins.remove(&id);
                }
                PreferenceEdit::Favourites
            });
        }
    }
}

pub(crate) fn build_preferences_window(
    bus: &Rc<PreferenceHub>,
    library_handle: Rc<RefCell<Rc<InstalledProfiles>>>,
) -> Result<PreferencesWindowBinding, slint::PlatformError> {
    let ui = PreferencesWindow::new()?;
    let catalog = library_handle.borrow().clone();
    let state = bus.current_values();
    let manufacturers = brand_labels(&catalog);
    let initial = brand_position(&manufacturers, preferred_brand(&catalog, &state).as_deref());
    let session = Rc::new(PreferencesSession {
        ui: ui.as_weak(),
        bus: bus.clone(),
        catalog: library_handle,
        cards: Rc::new(RefCell::new(gallery_entries(
            catalog.printers(),
            &state.favourite_printers,
        ))),
        manufacturers: RefCell::new(manufacturers),
        tab: Cell::new(initial),
        query: RefCell::new(String::new()),
        pills: Rc::new(TagSizing::new()),
        thumbnails: Rc::new(Timer::default()),
        printer_editor: RefCell::new(None),
        profile: Rc::new(RefCell::new(None)),
        fork: Rc::new(RefCell::new(None)),
        extension: Rc::new(RefCell::new(None)),
    });
    ui.take_preferences(&state);
    publish_materials(&ui, &state, &catalog, &session.pills);
    ui.set_maker_shown(initial);
    publish_brands(&ui, &session.manufacturers.borrow());
    session.filter(&ui);
    fetch_gallery_images(
        &session.thumbnails,
        &ui,
        session.cards.clone(),
        image_jobs(&catalog),
    );
    let refresh: Redraw = {
        let weak = Rc::downgrade(&session);
        Rc::new(move || {
            if let Some(session) = weak.upgrade() {
                session.refresh();
            }
        })
    };
    macro_rules! callback {
        ($on:ident, $s:ident, || $body:block) => {
            ui.$on({ let $s = session.clone(); move || $body });
        };
        ($on:ident, $s:ident, |$($arg:ident),*| $body:block) => {
            ui.$on({ let $s = session.clone(); move |$($arg),*| $body });
        };
    }
    callback!(on_closing, s, || {
        if let Some(window) = s.ui.upgrade() {
            let _ = window.hide();
        }
    });
    callback!(on_theme_picked, s, |value| {
        s.bus.edit_preferences(move |state| {
            state.theme = value;
            PreferenceEdit::Theme
        });
    });
    callback!(on_language_picked, s, |index| {
        let language = LanguagePreference::from_combo_index(index);
        s.bus.edit_preferences(move |state| {
            state.language = language;
            PreferenceEdit::Language
        });
    });
    callback!(on_currency_edited, s, |value| {
        let symbol = value.to_string();
        s.bus.edit_preferences(move |state| {
            state.currency_symbol = symbol;
            PreferenceEdit::CurrencySymbol
        });
    });
    callback!(on_aa_picked, s, |index| {
        let level = aa_level_at_row(index);
        s.bus.edit_preferences(move |state| {
            state.aa_level = level;
            PreferenceEdit::AaLevel
        });
    });
    callback!(on_printer_maker_picked, s, |index| {
        s.tab.set(index);
        if let Some(window) = s.ui.upgrade() {
            s.filter(&window);
        }
    });
    callback!(on_printer_query_edited, s, |text| {
        *s.query.borrow_mut() = text.to_lowercase();
        if let Some(window) = s.ui.upgrade() {
            s.filter(&window);
        }
    });
    callback!(on_printer_picked, s, |index| {
        s.select_printer(index);
    });
    callback!(on_resin_picked, s, |index| {
        s.select_resin(index);
    });
    callback!(on_printer_star_toggled, s, |index| {
        s.star_printer(index);
    });
    callback!(on_resin_star_toggled, s, |index| {
        s.star_resin(index);
    });
    callback!(on_tag_measured, s, |index, extension, width| {
        resize_tags(&s.ui, &s.catalog, &s.bus, &s.pills, index, extension, width);
    });
    ui.on_export_printers(|| export_package(PackageKind::PrinterProfiles));
    ui.on_export_resins(|| export_package(PackageKind::ResinProfiles));
    ui.on_import_package({
        let catalog = session.catalog.clone();
        let refresh = refresh.clone();
        move || import_package(catalog.clone(), refresh.clone())
    });
    callback!(on_view_resin, s, |index| {
        show_material_view(&s.bus, &s.catalog, &s.profile, index);
    });
    ui.on_edit_resin({
        let s = session.clone();
        let refresh = refresh.clone();
        move |index, extension| {
            show_material_editor(
                &s.bus, &s.catalog, &refresh, &s.profile, index, extension, None,
            );
        }
    });
    ui.on_delete_resin({
        let s = session.clone();
        let refresh = refresh.clone();
        move |index, extension| {
            delete_material(
                s.bus.clone(),
                s.catalog.clone(),
                refresh.clone(),
                index,
                extension,
            );
        }
    });
    ui.on_fork_resin({
        let s = session.clone();
        let refresh = refresh.clone();
        move |index| {
            show_copy_dialog(&s.bus, &s.catalog, &refresh, &s.fork, &s.profile, index);
        }
    });
    ui.on_extend_resin({
        let s = session.clone();
        let refresh = refresh.clone();
        move |index| {
            show_calibration_dialog(&s.bus, &s.catalog, &refresh, &s.extension, index);
        }
    });
    ui.on_open_printer({
        let s = session.clone();
        let refresh = refresh.clone();
        move |index, duplicate| {
            let catalog = s.catalog.borrow().clone();
            let Some(entry) = catalog.printers().get(index as usize) else {
                return;
            };
            let Ok(profile) = profile_libraries().load_printer_by_id(&entry.id) else {
                return;
            };
            let handle = s.catalog.clone();
            let bus = s.bus.clone();
            let refresh = refresh.clone();
            let saved = Rc::new(move |profile: &sc_print_profiles::PrinterProfile| {
                reload_library(&handle);
                let id = profile.identity.id.clone();
                let volume = handle.borrow().volume_for_printer(Some(&id));
                bus.edit_preferences(move |state| {
                    state.default_printer = Some(id);
                    state.build_volume = volume;
                    PreferenceEdit::DefaultPrinter
                });
                refresh();
            });
            let theme = s.bus.current_values().theme;
            match crate::printer_editor::open(profile, duplicate, saved) {
                Ok(editor) => {
                    editor.set_theme(theme);
                    *s.printer_editor.borrow_mut() = Some(editor);
                }
                Err(error) => eprintln!("Printer editor: {error}"),
            }
        }
    });
    let listener = bus.watch_preferences({
        let weak = session.ui.clone();
        let catalog = session.catalog.clone();
        let pills = session.pills.clone();
        move |state, _| {
            if let Some(window) = weak.upgrade() {
                window.take_preferences(state);
                let current = catalog.borrow().clone();
                publish_materials(&window, state, &current, &pills);
            }
        }
    });
    Ok(PreferencesWindowBinding {
        window: ui,
        _updates: listener,
        _pictures: session.thumbnails.clone(),
    })
}
fn publish_brands(ui: &PreferencesWindow, labels: &[String]) {
    let values: Vec<SharedString> = labels.iter().map(|label| label.as_str().into()).collect();
    ui.set_printer_makers(model(values));
}
fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}
fn reload_library(slot: &SharedCatalog) {
    let current = InstalledProfiles::scan_libraries(&profile_libraries());
    slot.replace(Rc::new(current));
}
fn export_package(scope: PackageKind) {
    let task = async move {
        let Ok(root) = user_library_dir() else {
            eprintln!("cannot resolve the user profile library directory");
            return;
        };
        let picker = rfd::AsyncFileDialog::new()
            .set_file_name(scope.suggested_file_name())
            .add_filter(ctr!("FileDialog", "Profile package").to_string(), &["zip"]);
        if let Some(destination) = picker.save_file().await
            && let Err(error) = write_package(&root, &scope.as_export_selection(), destination.path())
        {
            eprintln!("{error}");
        }
    };
    if let Err(error) = slint::spawn_local(task) {
        eprintln!("failed to spawn export task: {error}");
    }
}
fn import_package(catalog: SharedCatalog, refresh: Redraw) {
    let task = async move {
        let Ok(root) = user_library_dir() else {
            eprintln!("cannot resolve the user profile library directory");
            return;
        };
        let picker = rfd::AsyncFileDialog::new()
            .add_filter(ctr!("FileDialog", "Profile package").to_string(), &["zip"]);
        let Some(package) = picker.pick_file().await else {
            return;
        };
        let source = package.path().to_path_buf();
        let result = install_package(&root, &source, false);
        let installed = match result {
            Err(error) => {
                eprintln!("{error}");
                false
            }
            Ok(ImportResult::Added) => true,
            Ok(ImportResult::WouldOverwrite(collisions)) => {
                let action = ctr!("PreferencesPage", "Overwrite").to_string();
                let response = rfd::AsyncMessageDialog::new()
                    .set_title(ctr!("PreferencesPage", "Overwrite existing profiles?").to_string())
                    .set_description(overwrite_warning(&collisions))
                    .set_buttons(rfd::MessageButtons::OkCancelCustom(
                        action.clone(),
                        ctr!("PreferencesPage", "Cancel").to_string(),
                    ))
                    .show()
                    .await;
                if chose_overwrite(&response, &action) {
                    match install_package(&root, &source, true) {
                        Ok(_) => true,
                        Err(error) => {
                            eprintln!("{error}");
                            false
                        }
                    }
                } else {
                    false
                }
            }
        };
        if installed {
            reload_library(&catalog);
            refresh();
        }
    };
    if let Err(error) = slint::spawn_local(task) {
        eprintln!("failed to spawn import task: {error}");
    }
}
fn resolve_material(
    catalog: &InstalledProfiles,
    index: i32,
) -> Option<(MaterialProfile, Option<MaterialProfile>, ProfileSource)> {
    let row = catalog.resin_list().get(index as usize)?;
    let libraries = profile_libraries();
    let base = libraries.load_material_by_id(&row.id).or_else(|error| {
        libraries
            .user_library_dir()
            .and_then(|root| load_material_extension_from_library(root, &row.id).ok())
            .ok_or(error)
    });
    let base = match base {
        Ok(profile) => profile,
        Err(error) => {
            eprintln!("{error}");
            return None;
        }
    };
    let mut extension = None;
    if catalog.extension_of(&row.id).is_some()
        && let Some(root) = libraries.user_library_dir()
    {
        match load_material_extension_from_library(root, &row.id) {
            Ok(profile) => extension = Some(profile),
            Err(error) => eprintln!("{error}"),
        }
    }
    Some((base, extension, row.profile_origin))
}
fn show_material_view(
    bus: &PreferenceStore,
    catalog: &SharedCatalog,
    slot: &DialogSlot<ProfileViewDialog>,
    index: i32,
) {
    let catalog = catalog.borrow().clone();
    let Some((base, extension, origin)) = resolve_material(&catalog, index) else {
        return;
    };
    let (material, warnings) = EffectiveMaterial::compose(base, extension);
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    let snapshot = bus.current_values();
    let printer = catalog.printer_in_effect(snapshot.default_printer.as_ref());
    let sections =
        profile_view_rows(&material, catalog.printers(), printer.as_ref(), origin);
    let Ok(dialog) = ProfileViewDialog::new()
        .map_err(|error| eprintln!("failed to create the profile viewer: {error}"))
    else {
        return;
    };
    crate::theme_sync::track(&dialog, ProfileViewDialog::set_viewer_theme);
    dialog.set_viewer_theme(snapshot.theme);
    dialog.set_resin_heading(resin_title(material.identity()).into());
    dialog.set_profile_cards(model(sections));
    let weak = dialog.as_weak();
    let retained = slot.clone();
    dialog.on_viewer_close_requested(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
        retained.take();
    });
    match dialog.show() {
        Ok(()) => {
            slot.replace(Some(dialog));
        }
        Err(error) => eprintln!("failed to show the profile viewer: {error}"),
    }
}
fn selection_was_removed(
    catalog: &InstalledProfiles,
    selected: Option<&MaterialId>,
    removed: &MaterialId,
) -> bool {
    selected.is_some_and(|id| id == removed && catalog.resin_position(id).is_none())
}
fn delete_material(
    bus: PreferenceStore,
    catalog: SharedCatalog,
    refresh: Redraw,
    index: i32,
    extension: bool,
) {
    let (id, label) = {
        let library = catalog.borrow();
        let Some(row) = library.resin_list().get(index as usize) else {
            return;
        };
        (row.id.clone(), format!("{} {}", row.brand, row.name))
    };
    let task = async move {
        let Ok(root) = user_library_dir() else {
            eprintln!("cannot resolve the user profile library directory");
            return;
        };
        let description = if extension {
            ctr!(
                "PreferencesPage",
                "Deleting the extension of {} removes every calibration pair it adds.",
                label,
            )
        } else {
            ctr!(
                "PreferencesPage",
                "Deleting {} removes the custom profile file.",
                label,
            )
        };
        let action = ctr!("PreferencesPage", "Delete").to_string();
        let response = rfd::AsyncMessageDialog::new()
            .set_title(ctr!("PreferencesPage", "Delete profile?").to_string())
            .set_description(description.to_string())
            .set_buttons(rfd::MessageButtons::OkCancelCustom(
                action.clone(),
                ctr!("PreferencesPage", "Cancel").to_string(),
            ))
            .show()
            .await;
        if !chose_overwrite(&response, &action) {
            return;
        }
        let result = if extension {
            delete_material_extension_from_library(&root, &id)
        } else {
            delete_material_profile_from_library(&root, &id)
        };
        if let Err(error) = result {
            eprintln!("{error}");
            return;
        }
        reload_library(&catalog);
        if selection_was_removed(
            &catalog.borrow(),
            bus.current_values().default_resin.as_ref(),
            &id,
        ) {
            bus.edit_preferences(|state| {
                state.default_resin = None;
                PreferenceEdit::DefaultResin
            });
        }
        refresh();
    };
    if let Err(error) = slint::spawn_local(task) {
        eprintln!("failed to spawn delete task: {error}");
    }
}
fn publish_materials(
    ui: &PreferencesWindow,
    state: &UserPreferences,
    catalog: &InstalledProfiles,
    layout: &TagSizing,
) {
    layout.text_size.set(ui.get_tag_font_size());
    let printer = catalog.printer_in_effect(state.default_printer.as_ref());
    ui.set_chosen_printer(
        printer
            .as_ref()
            .and_then(|id| catalog.printer_position(id))
            .map_or(-1, |index| index as i32),
    );
    ui.set_chosen_resin(
        state
            .default_resin
            .as_ref()
            .and_then(|id| catalog.resin_position(id))
            .map_or(-1, |index| index as i32),
    );
    ui.set_resin_list(model(material_rows(
        catalog.resin_list(),
        catalog.extensions(),
        catalog.printers(),
        printer.as_ref(),
        &state.favourite_resins,
        layout,
    )));
}
fn gallery_entries(
    printers: &[CatalogPrinter],
    stars: &BTreeSet<PrinterId>,
) -> Vec<GalleryEntry> {
    let mut tiles = Vec::with_capacity(printers.len());
    for (index, printer) in printers.iter().enumerate() {
        // Sleptic's render of the printer (or the user's own photo of it),
        // the same picture the sidebar shows beside the printer picker.
        let picture = product_pictures::picture(Kind::Printer, printer.id.as_str());
        tiles.push(GalleryEntry {
            brand_key: printer.printer_maker.clone(),
            search_name: printer.name.to_lowercase(),
            tile: PrinterEntry {
                label: printer.name.as_str().into(),
                catalogue_index: index as i32,
                user_made: printer.profile_origin == ProfileSource::User,
                starred: stars.contains(&printer.id),
                has_picture: picture.is_some(),
                picture: picture.unwrap_or_default(),
            },
        });
    }
    tiles
}
/// Profile images to load in the background: only for printers Sleptic has
/// no picture of (a profile package may bring its own).
fn image_jobs(catalog: &InstalledProfiles) -> Vec<(usize, PathBuf)> {
    let mut files = Vec::new();
    for (index, entry) in catalog.printers().iter().enumerate() {
        if product_pictures::picture(Kind::Printer, entry.id.as_str()).is_some() {
            continue;
        }
        if let Some(path) = &entry.picture_path {
            files.push((index, path.clone()));
        }
    }
    files
}
fn read_pixels(path: &Path) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    match image::open(path) {
        Err(error) => {
            eprintln!(
                "warning: failed to load printer thumbnail {}: {error}",
                path.display()
            );
            None
        }
        Ok(decoded) => {
            let pixels = decoded.into_rgba8();
            let mut output = SharedPixelBuffer::new(pixels.width(), pixels.height());
            output.make_mut_bytes().copy_from_slice(pixels.as_raw());
            Some(output)
        }
    }
}
fn fetch_gallery_images(
    timer: &Rc<Timer>,
    ui: &PreferencesWindow,
    cards: Rc<RefCell<Vec<GalleryEntry>>>,
    files: Vec<(usize, PathBuf)>,
) {
    timer.stop();
    if files.is_empty() {
        return;
    }
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        // One by one, so a picture that crashes the decoder costs only itself.
        let images = crate::safe_decode::decode_each(files, read_pixels);
        let _ = send.send(images);
    });
    let weak = ui.as_weak();
    let clock = Rc::downgrade(timer);
    timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
        match receive.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => (),
            Ok(images) => {
                if let Some(ui) = weak.upgrade() {
                    let mut changed = BTreeSet::new();
                    {
                        let mut cache = cards.borrow_mut();
                        for (index, pixels) in images {
                            if let Some(tile) = cache.get_mut(index) {
                                tile.tile.picture = Image::from_rgba8(pixels);
                                tile.tile.has_picture = true;
                                changed.insert(index as i32);
                            }
                        }
                    }
                    publish_images(&ui.get_printer_tiles(), &cards.borrow(), &changed);
                }
            }
        }
        if let Some(timer) = clock.upgrade() {
            timer.stop();
        }
    });
}
fn brand_labels(catalog: &InstalledProfiles) -> Vec<String> {
    std::iter::once(ctr!("PreferencesPage", "All").to_string())
        .chain(catalog.maker_names())
        .collect()
}
fn preferred_brand(catalog: &InstalledProfiles, state: &UserPreferences) -> Option<String> {
    let selected = catalog.printer_in_effect(state.default_printer.as_ref())?;
    catalog
        .printers()
        .get(catalog.printer_position(&selected)?)
        .map(|printer| printer.printer_maker.clone())
}
fn brand_position(tabs: &[String], selected: Option<&str>) -> i32 {
    for (index, label) in tabs.iter().enumerate().skip(1) {
        if Some(label.as_str()) == selected {
            return index as i32;
        }
    }
    0
}
fn brand_at(tabs: &[String], index: i32) -> Option<&str> {
    (index > 0)
        .then(|| tabs.get(index as usize))
        .flatten()
        .map(String::as_str)
}
fn matches_filter(
    brand: &str,
    lower_name: &str,
    selected_brand: Option<&str>,
    query: &str,
) -> bool {
    if selected_brand.is_some_and(|selected| selected != brand) {
        return false;
    }
    query.is_empty() || lower_name.contains(query)
}
fn visible_selection(cards: &[PrinterEntry], id: i32) -> i32 {
    for (position, card) in cards.iter().enumerate() {
        if card.catalogue_index == id {
            return position as i32;
        }
    }
    -1
}
fn publish_images(visible: &ModelRc<PrinterEntry>, cache: &[GalleryEntry], changed: &BTreeSet<i32>) {
    for position in 0..visible.row_count() {
        if let Some(current) = visible
            .row_data(position)
            .filter(|card| changed.contains(&card.catalogue_index))
        {
            visible.set_row_data(position, cache[current.catalogue_index as usize].tile.clone());
        }
    }
}
fn publish_gallery(
    ui: &PreferencesWindow,
    cache: &[GalleryEntry],
    brand: Option<&str>,
    query: &str,
) {
    let mut shown = Vec::new();
    for data in cache {
        if matches_filter(&data.brand_key, &data.search_name, brand, query) {
            shown.push(data.tile.clone());
        }
    }
    ui.set_chosen_printer_tile(visible_selection(
        &shown,
        ui.get_chosen_printer(),
    ));
    ui.set_printer_tiles(model(shown));
}
const TAG_INSET: f32 = 14.0;
const TAG_SPACING: f32 = 4.0;
const INITIAL_TAG_WIDTH: f32 = 340.0;
struct TagSizing {
    font: Option<TextWidths>,
    text_size: Cell<f32>,
    row_widths: RefCell<BTreeMap<(i32, bool), f32>>,
}
impl TagSizing {
    fn new() -> Self {
        Self {
            font: TextWidths::bundled(),
            text_size: Cell::new(12.),
            row_widths: RefCell::new(BTreeMap::new()),
        }
    }
    fn measure(&self, label: &str) -> f32 {
        self.font.as_ref().map_or_else(
            || label.chars().count() as f32 * 0.6 * self.text_size.get(),
            |font| font.pixel_width(label, self.text_size.get()),
        )
    }
    fn budget(&self, index: i32, extension: bool) -> f32 {
        self.row_widths
            .borrow()
            .get(&(index, extension))
            .copied()
            .unwrap_or(INITIAL_TAG_WIDTH)
    }
}
fn material_rows(
    materials: &[CatalogResin],
    extensions: &[CatalogExtension],
    printers: &[CatalogPrinter],
    selected: Option<&PrinterId>,
    stars: &BTreeSet<MaterialId>,
    layout: &TagSizing,
) -> Vec<ResinProfileEntry> {
    let mut tiers: [Vec<ResinProfileEntry>; 3] = Default::default();
    for (index, resin) in materials.iter().enumerate() {
        let extension = extensions.iter().find(|extra| extra.id == resin.id);
        let tier = if has_calibration_for(resin, extension, selected) {
            0
        } else if resin.calibrated_for.is_empty()
            && extension.is_none_or(|extra| extra.extra_calibrations.is_empty())
        {
            2
        } else {
            1
        };
        let picture = product_pictures::picture(Kind::Resin, resin.id.as_str());
        for is_extension in [false, true] {
            if is_extension && extension.is_none() {
                break;
            }
            let shown = picture.as_ref().filter(|_| !is_extension);
            let tags = tags_for_row(resin, extension, printers, selected, is_extension);
            tiers[tier].push(ResinProfileEntry {
                resin_display_name: if is_extension {
                    ctr!("PreferencesPage", "{} (extended)", resin.name)
                } else {
                    resin.name.as_str().into()
                },
                resin_brand_name: resin.brand.as_str().into(),
                resin_family_name: crate::profile_catalog::resin_family_text(
                    &resin.resin_family,
                    resin.overhang_limit_deg,
                )
                .into(),
                calibration_tag_lines: tag_model(wrap_tags(
                    tags,
                    layout.budget(index as i32, is_extension),
                    &|text| layout.measure(text),
                )),
                has_custom_profile: is_extension || resin.profile_origin == ProfileSource::User,
                marked_as_favourite: stars.contains(&resin.id),
                has_picture: shown.is_some(),
                picture: shown.cloned().unwrap_or_default(),
                represents_extension: is_extension,
                allows_extension: calibration_missing(resin, extension, printers),
                resin_catalog_position: index as i32,
            });
        }
    }
    tiers.into_iter().flatten().collect()
}
fn resize_tags(
    weak: &slint::Weak<PreferencesWindow>,
    catalog: &SharedCatalog,
    bus: &PreferenceStore,
    layout: &Rc<TagSizing>,
    index: i32,
    extension_row: bool,
    width: f32,
) {
    let key = (index, extension_row);
    let mut row_widths = layout.row_widths.borrow_mut();
    if row_widths
        .get(&key)
        .is_some_and(|prior| (prior - width).abs() < 0.5)
    {
        return;
    }
    row_widths.insert(key, width);
    drop(row_widths);
    let Some(ui) = weak.upgrade() else {
        return;
    };
    layout.text_size.set(ui.get_tag_font_size());
    let catalog = catalog.borrow().clone();
    let Some(resin) = catalog.resin_list().get(index as usize) else {
        return;
    };
    let selected = catalog.printer_in_effect(bus.current_values().default_printer.as_ref());
    let tags = tags_for_row(
        resin,
        catalog.extension_of(&resin.id),
        catalog.printers(),
        selected.as_ref(),
        extension_row,
    );
    let wrapped = tag_model(wrap_tags(tags, width, &|text| layout.measure(text)));
    let rows = ui.get_resin_list();
    for position in 0..rows.row_count() {
        if let Some(mut row) = rows
            .row_data(position)
            .filter(|row| row.resin_catalog_position == index && row.represents_extension == extension_row)
        {
            row.calibration_tag_lines = wrapped;
            rows.set_row_data(position, row);
            break;
        }
    }
}
fn tags_for_row(
    resin: &CatalogResin,
    extra: Option<&CatalogExtension>,
    printers: &[CatalogPrinter],
    selected: Option<&PrinterId>,
    extension_row: bool,
) -> Vec<CalibrationTagData> {
    match (extension_row, extra) {
        (true, Some(extra)) => calibration_tags(&extra.extra_calibrations, printers, selected),
        (true, None) => Vec::new(),
        (false, _)
            if resin.calibrated_for.is_empty()
                && extra.is_none_or(|extra| extra.extra_calibrations.is_empty()) =>
        {
            vec![missing_calibration(selected)]
        }
        _ => calibration_tags(&resin.calibrated_for, printers, selected),
    }
}
fn calibration_missing(
    resin: &CatalogResin,
    extra: Option<&CatalogExtension>,
    printers: &[CatalogPrinter],
) -> bool {
    printers
        .iter()
        .map(|printer| CalibrationTarget::from_printer(&printer.id))
        .any(|key| {
            !(resin.calibrated_for.contains(&key)
                || extra.is_some_and(|extra| extra.extra_calibrations.contains(&key)))
        })
}
fn machine_label(printers: &[CatalogPrinter], id: &PrinterId) -> SharedString {
    match printers.iter().find(|printer| printer.id == *id) {
        Some(printer) => printer.name.as_str().into(),
        None => id.as_str().into(),
    }
}
fn calibration_tags(
    keys: &BTreeSet<CalibrationTarget>,
    printers: &[CatalogPrinter],
    selected: Option<&PrinterId>,
) -> Vec<CalibrationTagData> {
    let active = selected.map(CalibrationTarget::from_printer);
    let mut ranked = Vec::new();
    for key in keys {
        let rank = if active.as_ref() == Some(key) {
            0
        } else if matches!(key, CalibrationTarget::Generic) {
            1
        } else {
            2
        };
        let label = match key {
            CalibrationTarget::Generic => ctr!("PreferencesPage", "Generic"),
            CalibrationTarget::Printer(id) => machine_label(printers, id.as_printer_id()),
        };
        let kind = match (selected.is_some(), rank) {
            (true, 0) => CalibrationTagClass::DefaultPrinterCalibration,
            (true, 1) => CalibrationTagClass::SharedCalibration,
            _ => CalibrationTagClass::AlternatePrinterCalibration,
        };
        ranked.push((rank, CalibrationTagData { calibration_caption: label, calibration_class: kind }));
    }
    ranked.sort_by(|(a_rank, a), (b_rank, b)| a_rank.cmp(b_rank).then(a.calibration_caption.cmp(&b.calibration_caption)));
    ranked.into_iter().map(|(_, pill)| pill).collect()
}
fn missing_calibration(selected: Option<&PrinterId>) -> CalibrationTagData {
    let kind = selected.map_or(CalibrationTagClass::AlternatePrinterCalibration, |_| CalibrationTagClass::AbsentCalibrationData);
    CalibrationTagData {
        calibration_caption: ctr!("PreferencesPage", "No calibration data"),
        calibration_class: kind,
    }
}
fn wrap_tags(
    tags: Vec<CalibrationTagData>,
    budget: f32,
    text_width: &dyn Fn(&str) -> f32,
) -> Vec<Vec<CalibrationTagData>> {
    let mut result: Vec<Vec<CalibrationTagData>> = Vec::new();
    let mut line_width = 0.;
    for tag in tags {
        let tag_width = text_width(&tag.calibration_caption) + TAG_INSET;
        let occupied = result.last().is_some_and(|line| !line.is_empty());
        let next = if occupied {
            line_width + TAG_SPACING + tag_width
        } else {
            tag_width
        };
        if !occupied || next > budget {
            result.push(Vec::new());
            line_width = tag_width;
        } else {
            line_width = next;
        }
        result.last_mut().unwrap().push(tag);
    }
    result
}
fn tag_model(lines: Vec<Vec<CalibrationTagData>>) -> ModelRc<ModelRc<CalibrationTagData>> {
    model(lines.into_iter().map(model).collect())
}
/// Which text box of the profile viewer edits which field, as the dialog's
/// getter and setter: one table per block, read for saving and written to
/// fill the boxes in.
type BoxBinding = (ResinField, fn(&ProfileViewDialog) -> SharedString, fn(&ProfileViewDialog, SharedString));

const DEFAULTS_BOXES: [BoxBinding; 6] = [
    (ResinField::ScaleX, ProfileViewDialog::get_shrink_x_input, ProfileViewDialog::set_shrink_x_input),
    (ResinField::ScaleY, ProfileViewDialog::get_shrink_y_input, ProfileViewDialog::set_shrink_y_input),
    (ResinField::ScaleZ, ProfileViewDialog::get_shrink_z_input, ProfileViewDialog::set_shrink_z_input),
    (ResinField::BottomLayers, ProfileViewDialog::get_default_bottom_layers_input, ProfileViewDialog::set_default_bottom_layers_input),
    (ResinField::Price, ProfileViewDialog::get_defaults_price_text, ProfileViewDialog::set_defaults_price_text),
    (ResinField::OverhangAngle, ProfileViewDialog::get_defaults_support_angle_text, ProfileViewDialog::set_defaults_support_angle_text),
];

const CALIBRATION_BOXES: [BoxBinding; 6] = [
    (ResinField::Exposure, ProfileViewDialog::get_normal_exposure_input, ProfileViewDialog::set_normal_exposure_input),
    (ResinField::BottomLayersExposure, ProfileViewDialog::get_bottom_exposure_input, ProfileViewDialog::set_bottom_exposure_input),
    (ResinField::BottomLayers, ProfileViewDialog::get_calibration_bottom_layers_input, ProfileViewDialog::set_calibration_bottom_layers_input),
    (ResinField::LiftHeight, ProfileViewDialog::get_lift_distance_text, ProfileViewDialog::set_lift_distance_text),
    (ResinField::LiftRate, ProfileViewDialog::get_lift_speed_text, ProfileViewDialog::set_lift_speed_text),
    (ResinField::RetractRate, ProfileViewDialog::get_retract_speed_text, ProfileViewDialog::set_retract_speed_text),
];

fn read_boxes(dialog: &ProfileViewDialog, boxes: &[BoxBinding]) -> FieldTexts {
    boxes.iter().map(|(field, get, _)| (*field, get(dialog).to_string())).collect()
}

fn fill_boxes(dialog: &ProfileViewDialog, boxes: &[BoxBinding], texts: &FieldTexts) {
    for (field, _, set) in boxes {
        set(dialog, texts.text(*field).into());
    }
}

fn update_edited_values(
    dialog: &ProfileViewDialog,
    profile: &mut MaterialProfile,
    kind: ResinFileKind,
    targets: &[(CalibrationTarget, SharedString)],
) -> Result<bool, SharedString> {
    if kind == ResinFileKind::WholeProfile {
        let mut defaults =
            defaults_from_texts(&read_boxes(dialog, &DEFAULTS_BOXES)).map_err(|error| error.rejection_line())?;
        // Fields absent from this editor must survive saving it.
        defaults.transition_layers = profile.defaults.transition_layers;
        defaults.bottom_compensation_mm = profile.defaults.bottom_compensation_mm;
        defaults.xy_compensation_mm = profile.defaults.xy_compensation_mm;
        defaults.aa_levels = profile.defaults.aa_levels;
        defaults.light_off_delay_s = profile.defaults.light_off_delay_s;
        defaults.density_g_ml = profile.defaults.density_g_ml;
        defaults.motion = profile.defaults.motion;
        profile.defaults = defaults;
    }
    let index = dialog.get_selected_calibration_target();
    if index >= 0 {
        let Some((target, _)) = targets.get(index as usize) else {
            return Ok(false);
        };
        let mut block =
            calibration_from_texts(&read_boxes(dialog, &CALIBRATION_BOXES))
                .map_err(|error| error.rejection_line())?;
        if let Some(previous) = profile.per_printer.get(target) {
            block.transition_layers = previous.transition_layers;
            block.bottom_compensation_mm = previous.bottom_compensation_mm;
            block.xy_compensation_mm = previous.xy_compensation_mm;
            block.aa_levels = previous.aa_levels;
            block.light_off_delay_s = previous.light_off_delay_s;
        }
        let motion = crate::motion_form::parse(dialog).map_err(SharedString::from)?;
        block.motion = (motion != Default::default()).then_some(motion);
        profile.per_printer.insert(target.clone(), block);
    }
    let root = user_library_dir().map_err(|error| SharedString::from(error.to_string()))?;
    write_edited_resin(&root, profile, kind).map_err(SharedString::from)?;
    Ok(true)
}
fn show_material_editor(
    bus: &PreferenceStore,
    library_handle: &SharedCatalog,
    redraw: &Redraw,
    dialog_slot: &DialogSlot<ProfileViewDialog>,
    index: i32,
    is_extension: bool,
    note: Option<SharedString>,
) {
    let catalog = library_handle.borrow().clone();
    let Some(row) = catalog.resin_list().get(index as usize) else {
        return;
    };
    let Ok(root) = user_library_dir()
        .map_err(|error| eprintln!("cannot resolve the user profile library directory: {error}"))
    else {
        return;
    };
    let loaded = if is_extension {
        load_material_extension_from_library(&root, &row.id)
            .map(|profile| (profile, ResinFileKind::ExtensionOnly))
            .map_err(|error| error.to_string())
    } else {
        load_material_profile_from_library(&root, &row.id)
            .map(|profile| (profile, ResinFileKind::WholeProfile))
            .or_else(|original| {
                load_material_extension_from_library(&root, &row.id)
                    .map(|profile| (profile, ResinFileKind::ExtensionOnly))
                    .map_err(|_| original)
            })
            .map_err(|error| error.to_string())
    };
    let (profile, kind) = match loaded {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };
    let state = bus.current_values();
    let printer = catalog.printer_in_effect(state.default_printer.as_ref());
    let mut sections = vec![identity_section(&profile.identity, ProfileSource::User)];
    if kind == ResinFileKind::WholeProfile {
        sections.push(intent_section(&profile.intent));
    }
    let targets = Rc::new(ordered_calibration_targets(
        &profile,
        catalog.printers(),
        printer.as_ref(),
    ));
    let Ok(dialog) = ProfileViewDialog::new()
        .map_err(|error| eprintln!("failed to create the profile editor: {error}"))
    else {
        return;
    };
    crate::theme_sync::track(&dialog, ProfileViewDialog::set_viewer_theme);
    dialog.set_viewer_theme(state.theme);
    dialog.set_resin_heading(if kind == ResinFileKind::WholeProfile {
        resin_title(&profile.identity).into()
    } else {
        ctr!(
            "PreferencesPage",
            "{} (extended)",
            resin_title(&profile.identity)
        )
    });
    dialog.set_viewer_note(note.unwrap_or_default());
    dialog.set_profile_cards(model(sections));
    dialog.set_allow_profile_edit(true);
    dialog.set_allow_defaults_edit(kind == ResinFileKind::WholeProfile);
    if kind == ResinFileKind::WholeProfile {
        fill_boxes(&dialog, &DEFAULTS_BOXES, &defaults_texts(&profile.defaults));
    }
    dialog.set_calibration_target_captions(model(
        targets.iter().map(|(_, label)| label.clone()).collect(),
    ));
    dialog.set_selected_calibration_target(-1);
    let inherited_motion = resolve_material(&catalog, index)
        .map(|(base, extension, _)| EffectiveMaterial::compose(base, extension).0)
        .and_then(|material| material.defaults().motion)
        .unwrap_or_default();
    let working = Rc::new(RefCell::new(profile));
    let weak = dialog.as_weak();
    dialog.on_calibration_target_selected({
        let weak = weak.clone(); let working = working.clone(); let targets = targets.clone();
        move |index| {
            let Some(ui) = weak.upgrade() else { return; };
            let Some((key, _)) = targets.get(index as usize) else { return; };
            let profile = working.borrow();
            let Some(block) = profile.per_printer.get(key) else { return; };
            let printer_id = if matches!(key, CalibrationTarget::Generic) { printer.as_ref().unwrap_or_else(|| key.printer_id()) } else { key.printer_id() };
            let motion = profile_libraries().load_printer_by_id(printer_id).ok().and_then(|profile| profile.motion).unwrap_or_default().normal.overlay(inherited_motion.normal);
            ui.set_motion_inherited(toml::to_string(&motion).unwrap_or_default().into());
            crate::motion_form::seed(&ui, block.motion.unwrap_or_default());
            fill_boxes(&ui, &CALIBRATION_BOXES, &calibration_texts(block));
        }
    });
    dialog.on_motion_refresh({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                crate::motion_form::refresh(&ui);
            }
        }
    });
    dialog.on_motion_field_changed({
        let weak = weak.clone();
        move |index, value| {
            if let Some(ui) = weak.upgrade() {
                let fields = ui.get_motion_fields();
                if let Some(mut field) = fields.row_data(index as usize) {
                    field.value = value;
                    fields.set_row_data(index as usize, field);
                    crate::motion_form::refresh(&ui);
                }
            }
        }
    });
    dialog.on_profile_save_requested({
        let weak = weak.clone();
        let slot = dialog_slot.clone();
        let catalog = library_handle.clone();
        let refresh = redraw.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let outcome = update_edited_values(&ui, &mut working.borrow_mut(), kind, &targets);
            match outcome {
                Err(message) => ui.set_editor_error(message),
                Ok(false) => (),
                Ok(true) => {
                    reload_library(&catalog);
                    refresh();
                    let _ = ui.hide();
                    slot.take();
                }
            }
        }
    });
    dialog.on_viewer_close_requested({
        let slot = dialog_slot.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
            slot.take();
        }
    });
    match dialog.show() {
        Ok(()) => {
            dialog_slot.replace(Some(dialog));
        }
        Err(error) => eprintln!("failed to show the profile editor: {error}"),
    }
}
fn write_profile(profile: &MaterialProfile, kind: ResinFileKind) -> Result<(), SharedString> {
    let folder = user_library_dir().map_err(|failure| SharedString::from(failure.to_string()))?;
    write_edited_resin(&folder, profile, kind).map_err(SharedString::from)
}
fn dismiss_dialog<T: ComponentHandle>(weak: &slint::Weak<T>, destination: &RefCell<Option<T>>) {
    if let Some(view) = weak.upgrade() {
        let _ = view.hide();
    }
    destination.take();
}
fn show_copy_dialog(
    bus: &PreferenceStore,
    library_handle: &SharedCatalog,
    redraw: &Redraw,
    fork_slot: &DialogSlot<ForkDialog>,
    editor_slot: &DialogSlot<ProfileViewDialog>,
    index: i32,
) {
    let catalog = library_handle.borrow().clone();
    let Some((source, _, _)) = resolve_material(&catalog, index) else {
        return;
    };
    let state = bus.current_values();
    let printer = catalog.printer_in_effect(state.default_printer.as_ref());
    let targets = Rc::new(ordered_calibration_targets(
        &source,
        catalog.printers(),
        printer.as_ref(),
    ));
    let Ok(dialog) =
        ForkDialog::new().map_err(|error| eprintln!("failed to create the fork dialog: {error}"))
    else {
        return;
    };
    crate::theme_sync::track(&dialog, ForkDialog::set_fork_dialog_theme);
    dialog.set_fork_dialog_theme(state.theme);
    dialog.set_fork_source_caption(resin_title(&source.identity).into());
    dialog.set_new_profile_name(ctr!(
        "ForkDialog",
        "{} (fork)",
        source.identity.name.as_str()
    ));
    dialog.set_new_profile_brand(source.identity.brand.as_str().into());
    dialog.set_source_block_captions(model(
        targets.iter().map(|(_, label)| label.clone()).collect(),
    ));
    let selected = Rc::new(RefCell::new(vec![true; targets.len()]));
    dialog.on_source_block_selection_changed({
        let selected = selected.clone();
        move |index, checked| {
            if let Some(value) = selected.borrow_mut().get_mut(index as usize) {
                *value = checked;
            }
        }
    });
    let weak = dialog.as_weak();
    dialog.on_create_fork_requested({
        let weak = weak.clone();
        let destination = fork_slot.clone();
        let edit_destination = editor_slot.clone();
        let preferences = bus.clone();
        let library = library_handle.clone();
        let redraw = redraw.clone();
        move || {
            if let Some(view) = weak.upgrade() {
                let choice = CopiedSections {
                    with_intent: view.get_include_source_intent(),
                    with_defaults: view.get_include_source_defaults(),
                    printer_blocks: targets.iter().enumerate()
                        .filter(|(position, _)| selected.borrow()[*position])
                        .map(|(_, (key, _))| key.clone())
                        .collect(),
                };
                let occupied = library.borrow().resin_list().iter().map(|item| item.id.clone()).collect();
                let result = forked_resin(&source, &choice, &view.get_new_profile_name(), &view.get_new_profile_brand(), &occupied)
                    .map_err(|failure| failure.dialog_line())
                    .and_then(|copy| {
                        write_profile(&copy, ResinFileKind::WholeProfile)?;
                        Ok(copy.identity.id)
                    });
                match result {
                    Err(message) => view.set_fork_validation_message(message),
                    Ok(id) => {
                        reload_library(&library);
                        redraw();
                        let _ = view.hide();
                        destination.take();
                        let position = library.borrow().resin_position(&id);
                        if let Some(position) = position {
                            show_material_editor(&preferences, &library, &redraw, &edit_destination,
                                position as i32, false, Some(ctr!("ProfileViewDialog",
                                    "The forked profile is already in your library. Close keeps it unchanged.",)));
                        }
                    }
                }
            }
        }
    });
    dialog.on_dismiss_fork_requested({
        let destination = fork_slot.clone();
        move || dismiss_dialog(&weak, &destination)
    });
    match dialog.show() {
        Ok(()) => {
            fork_slot.replace(Some(dialog));
        }
        Err(error) => eprintln!("failed to show the fork dialog: {error}"),
    }
}
fn show_calibration_dialog(
    bus: &PreferenceStore,
    library_handle: &SharedCatalog,
    redraw: &Redraw,
    extend_slot: &DialogSlot<ExtendDialog>,
    index: i32,
) {
    let catalog = library_handle.borrow().clone();
    let Some((base, extension, _)) = resolve_material(&catalog, index) else {
        return;
    };
    let state = bus.current_values();
    let printer = catalog.printer_in_effect(state.default_printer.as_ref());
    let (material, warnings) = EffectiveMaterial::compose(base.clone(), extension.clone());
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    let targets = printers_without_calibration(&material, catalog.printers(), printer.as_ref());
    if targets.is_empty() {
        return;
    }
    let Ok(dialog) = ExtendDialog::new()
        .map_err(|error| eprintln!("failed to create the extend dialog: {error}"))
    else {
        return;
    };
    crate::theme_sync::track(&dialog, ExtendDialog::set_extension_dialog_theme);
    dialog.set_extension_dialog_theme(state.theme);
    dialog.set_extension_source_title(resin_title(&base.identity).into());
    dialog.set_extension_target_choices(model(
        targets.iter().map(|(_, label)| label.clone()).collect(),
    ));
    dialog.set_extension_target_selection(-1);
    let weak = dialog.as_weak();
    dialog.on_extension_save_requested({
        let weak = weak.clone();
        let destination = extend_slot.clone();
        let library = library_handle.clone();
        let redraw = redraw.clone();
        move || {
            if let Some(view) = weak.upgrade()
                && let Some((target, _)) = targets.get(view.get_extension_target_selection() as usize)
            {
                let typed = FieldTexts::default()
                    .with(ResinField::Exposure, view.get_extension_normal_exposure_input())
                    .with(ResinField::BottomLayersExposure, view.get_extension_bottom_exposure_input())
                    .with(ResinField::BottomLayers, view.get_extension_bottom_layer_input())
                    .with(ResinField::LiftHeight, view.get_extension_lift_distance_input())
                    .with(ResinField::LiftRate, view.get_extension_lift_speed_input())
                    .with(ResinField::RetractRate, view.get_extension_retract_speed_input());
                let outcome =
                    calibration_from_texts(&typed)
                        .map_err(|failure| failure.rejection_line())
                        .and_then(|values| {
                            write_profile(
                                &extension_with_calibration(&base, extension.as_ref(), target, values),
                                ResinFileKind::ExtensionOnly,
                            )
                        });
                match outcome {
                    Err(message) => view.set_extension_validation_message(message),
                    Ok(()) => {
                        reload_library(&library);
                        redraw();
                        dismiss_dialog(&weak, &destination);
                    }
                }
            }
        }
    });
    dialog.on_extension_cancel_requested({
        let destination = extend_slot.clone();
        move || dismiss_dialog(&weak, &destination)
    });
    match dialog.show() {
        Ok(()) => {
            extend_slot.replace(Some(dialog));
        }
        Err(error) => eprintln!("failed to show the extend dialog: {error}"),
    }
}
#[cfg(test)]
#[path = "tests_preferences_window.rs"]
mod tests;

// Tests route profile I/O to a thread-local library instead of the real user directory.
#[cfg(test)]
thread_local! {
    static TEST_LIBRARIES: RefCell<Option<ProfileLibraries>> = const { RefCell::new(None) };
}
fn profile_libraries() -> ProfileLibraries {
    #[cfg(test)]
    if let Some(library) = TEST_LIBRARIES.with(|slot| slot.borrow().clone()) {
        return library;
    }
    ProfileLibraries::platform_default()
}
fn user_library_dir() -> Result<PathBuf, sc_print_profiles::UserLibraryDirError> {
    #[cfg(test)]
    if let Some(root) = TEST_LIBRARIES.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|library| library.user.clone())
    }) {
        return Ok(root);
    }
    system_user_library_dir()
}

#[cfg(test)]
#[path = "tests_resin_page_controls.rs"]
mod tests_resin_page_controls;

#[cfg(test)]
#[path = "tests_preferences_sidebar.rs"]
mod tests_preferences_sidebar;

#[cfg(test)]
#[path = "tests_preference_pages.rs"]
mod tests_preference_pages;
