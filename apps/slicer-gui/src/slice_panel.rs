// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The sidebar's Printer and Resin blocks for one window: what their dropdowns
//! list (the catalog, starred profiles and the default first, the scene's own
//! choice selected), and what a pick does (it goes into the document, which
//! then needs saving). They are rebuilt when a scene opens or closes and when
//! Preferences changes the stars or the defaults.

use std::cell::RefCell;
use std::rc::Rc;

use sc_print_primitives::{MaterialId, PrinterId};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::MainWindow;
use crate::preferences_bus::{PreferenceHub, PreferenceEdit, PreferenceWatch};
use crate::profile_catalog::InstalledProfiles;
use crate::scene_document::{SceneDocument, DocumentEvent, Watch};
use crate::slice_selection::{FirstLayerEdit, first_layer_text, read_first_layer_edit};
use crate::slice_selection::{DropdownRows, dropdown_rows, printer_choices, resin_choices};
use crate::slice_selection::{ChooserLine, chooser_brands, chooser_lines, printer_items, resin_items};

pub(crate) struct SliceProfileControls {
    profile_catalog_handle: Rc<RefCell<Rc<InstalledProfiles>>>,
    profile_preferences_bus: Rc<PreferenceHub>,
    slice_profile_document: Rc<SceneDocument>,
    slice_profile_window: slint::Weak<MainWindow>,
    // Profile ids in dropdown row order: a picked row number maps back
    // through these.
    printer_picker_profile_ids: RefCell<Vec<PrinterId>>,
    resin_picker_profile_ids: RefCell<Vec<MaterialId>>,
    // The rows behind the full-catalog chooser (R101), in display order;
    // none for a heading.
    chooser_printers: RefCell<Vec<Option<PrinterId>>>,
    chooser_resins: RefCell<Vec<Option<MaterialId>>>,
    // The brands the chooser filters by, in its order ("All" is not one).
    chooser_brands: RefCell<Vec<String>>,
    // The resin whose support angle the Supports panel last took, so a
    // refresh does not overwrite the user's own edit of it.
    support_angle_resin: RefCell<Option<MaterialId>>,
    // Held for as long as the window lives; dropping these controls ends
    // both subscriptions.
    profile_document_subscription: RefCell<Option<Watch>>,
    profile_preferences_subscription: RefCell<Option<PreferenceWatch>>,
}

impl SliceProfileControls {
    /// Connects the window's printer/resin controls to `document` and the
    /// preferences, fills them once, and returns the controls; the window keeps
    /// them, since they hold the document and preferences subscriptions.
    pub(crate) fn attach_slice_profile_controls(
        window: &MainWindow,
        catalog_cell: Rc<RefCell<Rc<InstalledProfiles>>>,
        prefs: Rc<PreferenceHub>,
        doc: Rc<SceneDocument>,
    ) -> Rc<Self> {
        let ui = window;
        ui.on_settings_match(|label, search| label.to_lowercase().contains(&search.to_lowercase()));
        let controls = Rc::new(Self {
            profile_catalog_handle: catalog_cell,
            profile_preferences_bus: prefs,
            slice_profile_document: doc,
            slice_profile_window: ui.as_weak(),
            printer_picker_profile_ids: RefCell::new(Vec::new()),
            resin_picker_profile_ids: RefCell::new(Vec::new()),
            chooser_printers: RefCell::new(Vec::new()),
            chooser_resins: RefCell::new(Vec::new()),
            chooser_brands: RefCell::new(Vec::new()),
            support_angle_resin: RefCell::new(None),
            profile_document_subscription: RefCell::new(None),
            profile_preferences_subscription: RefCell::new(None),
        });

        {
            let controls = Rc::clone(&controls);
            ui.on_printer_picked(move |index| controls.accept_slice_printer_choice(index));
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_resin_picked(move |index| controls.accept_slice_resin_choice(index));
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_resin_color_picked(move |index| controls.on_resin_color_picked(index));
        }
        ui.set_resin_swatches(ModelRc::new(VecModel::from(
            crate::resin_colors::SWATCHES
                .iter()
                .map(|(_, [r, g, b])| slint::Color::from_rgb_u8(*r, *g, *b))
                .collect::<Vec<_>>(),
        )));
        ui.set_resin_swatch_names(ModelRc::new(VecModel::from(
            crate::resin_colors::SWATCHES
                .iter()
                .map(|(name, _)| SharedString::from(crate::i18n::tr!("{} resin", *name).to_string()))
                .collect::<Vec<_>>(),
        )));
        {
            let controls = Rc::clone(&controls);
            ui.on_chooser_search_edited(move |_| controls.fill_chooser());
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_chooser_picked(move |index| controls.on_chooser_picked(index));
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_chooser_star(move |index| controls.on_chooser_star(index));
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_chooser_open_requested(move |kind| controls.open_chooser(kind));
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_chooser_brand_picked(move |index| controls.on_chooser_brand(index));
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_first_layer_edited(move |text| {
                controls.accept_first_layer_override_text(&text)
            });
        }

        {
            let controls = Rc::clone(&controls);
            ui.on_process_setting_edited(move |id, text| {
                let changed = controls
                    .slice_profile_document
                    .chosen_printer()
                    .zip(controls.slice_profile_document.chosen_resin())
                    .and_then(|(p, m)| crate::slice_projection::load_chosen_profiles(&p, &m).ok())
                    .is_some_and(|(printer, _)| {
                        crate::process_settings::edit_constrained(
                            &mut controls.slice_profile_document.process_overrides.borrow_mut(),
                            &printer.constraints,
                            id,
                            &text,
                        )
                    });
                if changed {
                    controls.slice_profile_document.process_changed();
                }
                controls.rebuild_slice_profile_options();
            });
        }
        {
            let controls = Rc::clone(&controls);
            ui.on_process_aa_picked(move |index| {
                let next = crate::aa_level::scene_aa_from_combo_index(index);
                if controls.slice_profile_document.scene_aa.replace(next) != next {
                    controls.slice_profile_document.process_changed();
                }
                controls.rebuild_slice_profile_options();
            });
        }
        // A scene opening (its saved choice is preselected), emptying or
        // changing its settings refills the dropdowns.
        {
            let weak = Rc::downgrade(&controls);
            let sub = controls.slice_profile_document.watch(move |event| {
                if matches!(
                    event,
                    DocumentEvent::Opened
                        | DocumentEvent::Emptied
                        | DocumentEvent::SettingsChanged
                        | DocumentEvent::ChoiceChanged
                        | DocumentEvent::Regrouped
                ) && let Some(controls) = weak.upgrade()
                {
                    controls.rebuild_slice_profile_options();
                }
            });
            *controls.profile_document_subscription.borrow_mut() = Some(sub);
        }

        // So do Preferences changes to stars, defaults or profiles.
        {
            let weak = Rc::downgrade(&controls);
            let sub = controls.profile_preferences_bus.watch_preferences(move |_snap, change| {
                use PreferenceEdit as Pc;
                let shown_here =
                    matches!(change, Pc::AaLevel | Pc::Favourites | Pc::DefaultPrinter | Pc::DefaultResin | Pc::Profiles);
                if shown_here && let Some(controls) = weak.upgrade() {
                    if matches!(change, PreferenceEdit::Profiles) {
                        *controls.profile_catalog_handle.borrow_mut() = Rc::new(InstalledProfiles::scan_libraries(
                            &sc_print_profiles::ProfileLibraries::platform_default(),
                        ));
                    }
                    if matches!(change, PreferenceEdit::AaLevel) {
                        controls.slice_profile_document.process_changed();
                    }
                    controls.rebuild_slice_profile_options();
                }
            });
            *controls.profile_preferences_subscription.borrow_mut() = Some(sub);
        }

        controls.rebuild_slice_profile_options();
        controls
    }

    /// Recomputes both dropdowns (catalog, stars, defaults, the scene's
    /// choice) and the panel's other fields, and writes them to the window.
    fn rebuild_slice_profile_options(&self) {
        let Some(ui) = self.slice_profile_window.upgrade() else {
            return;
        };
        let snapshot = self.profile_preferences_bus.current_values();
        ui.set_process_aa_index(crate::aa_level::aa_row_of(
            self.slice_profile_document.scene_aa.get().unwrap_or(snapshot.aa_level),
        ));
        ui.set_process_aa_modified(self.slice_profile_document.scene_aa.get().is_some());
        let catalog = self.profile_catalog_handle.borrow().clone();

        // Materialize soft defaults so rendering, slicing, saving and dumps agree.
        // Not user edits: they leave the document clean.
        if self.slice_profile_document.chosen_printer().is_none() {
            self.slice_profile_document.fill_default_printer(
                catalog.printer_in_effect(snapshot.default_printer.as_ref()),
            );
        }
        if self.slice_profile_document.chosen_resin().is_none() {
            let effective = catalog
                .usual_resin_for(self.slice_profile_document.chosen_printer().as_ref())
                .or_else(|| snapshot.default_resin.clone());
            self.slice_profile_document.fill_default_resin(effective);
        }

        let doc = &self.slice_profile_document;
        let (printers, resins) = (printer_choices(catalog.printers()), resin_choices(catalog.resin_list()));
        let stars = (&snapshot.favourite_printers, &snapshot.favourite_resins);
        let defaults = (snapshot.default_printer.as_ref(), snapshot.default_resin.as_ref());
        let printer = dropdown_rows(&printers, stars.0, defaults.0, doc.chosen_printer().as_ref());
        let resin = dropdown_rows(&resins, stars.1, defaults.1, doc.chosen_resin().as_ref());

        // Rows first, then the selected row: the selection is bound both
        // ways, and new rows may clamp it.
        ui.set_printer_choices(labels_model(&printer, crate::i18n::tr!("More printers...").to_string()));
        ui.set_printer_choice(printer.selected_row);
        ui.set_resin_choices(labels_model(&resin, crate::i18n::tr!("More resins...").to_string()));
        ui.set_resin_choice(resin.selected_row);
        if let Some(option) = usize::try_from(resin.selected_row)
            .ok()
            .and_then(|i| resin.rows.get(i))
        {
            // The models are drawn in this colour too (R121).
            ui.set_resin_color(crate::resin_colors::body_color(option.id.as_str(), &option.caption));
        }
        // Each profile's own picture (R100); none leaves the drawn one.
        use crate::product_pictures::{Kind, picture};
        ui.set_printer_picture(
            usize::try_from(printer.selected_row)
                .ok()
                .and_then(|i| printer.rows.get(i))
                .and_then(|o| picture(Kind::Printer, o.id.as_str()))
                .unwrap_or_default(),
        );
        ui.set_resin_picture(
            usize::try_from(resin.selected_row)
                .ok()
                .and_then(|i| resin.rows.get(i))
                .and_then(|o| picture(Kind::Resin, o.id.as_str()))
                .unwrap_or_default(),
        );
        ui.set_first_layer_text(
            first_layer_text(self.slice_profile_document.chosen_first_layer()).into(),
        );

        if let (Some(printer), Some(resin)) = (
            self.slice_profile_document
                .chosen_printer()
                .or(snapshot.default_printer),
            self.slice_profile_document.chosen_resin().or(snapshot.default_resin),
        ) && let Ok((printer, material)) =
            crate::slice_projection::load_chosen_profiles(&printer, &resin)
        {
            // The resin decides which overhangs need supports (R78): a
            // flexible resin sags where a rigid one holds.
            let angle = material.defaults().support_overhang_angle();
            ui.set_resin_support_summary(
                crate::i18n::tr!(
                    "Supports overhangs under {}° ({} resin)",
                    angle.round(),
                    material.identity().family.as_str()
                )
                .into(),
            );
            if self.support_angle_resin.borrow().as_ref() != Some(&resin) {
                ui.set_support_overhang_angle_deg(angle.to_string().into());
                *self.support_angle_resin.borrow_mut() = Some(resin.clone());
            }
            let h = &printer.hardware;
            let v = h.build_volume();
            let pixels = h.pixel_resolution();
            let pitch = h.pixel_size_microns();
            // Rounded (R94: "16.80003004807692 x 24.703125 um" read as noise).
            ui.set_printer_summary(crate::i18n::tr!(
                "{} x {} mm plate, {} mm tall · {} x {} px, {} µm pixels",
                short_number(v.x_mm()),
                short_number(v.y_mm()),
                short_number(v.z_mm()),
                pixels.x(),
                pixels.y(),
                if (pitch.x.value() - pitch.y.value()).abs() < 0.05 {
                    short_number(pitch.x.value())
                } else {
                    format!(
                        "{} x {}",
                        short_number(pitch.x.value()),
                        short_number(pitch.y.value())
                    )
                }
            ));
            // Only worth a line when the printer actually mirrors the image.
            let m = h.mirror();
            ui.set_mirror_summary(match (m.x, m.y) {
                (false, false) => "".into(),
                (true, false) => crate::i18n::tr!("Output image mirrored in X"),
                (false, true) => crate::i18n::tr!("Output image mirrored in Y"),
                (true, true) => crate::i18n::tr!("Output image mirrored in X and Y"),
            });
            if let Ok(config) = sc_print_profiles::resolve(
                &printer,
                &material,
                &self.slice_profile_document.process_overrides.borrow(),
            ) {
                ui.set_process_settings(crate::process_settings::rows(
                    &config,
                    &self.slice_profile_document.process_overrides.borrow(),
                ));
                let m = &config.material_resolved;
                ui.set_process_summary(crate::i18n::tr!(
                    "Transition layers: {} | Light-off: {} s",
                    m.transition_layers,
                    m.light_off_delay_s
                ));
            }
        }
        *self.printer_picker_profile_ids.borrow_mut() = printer.rows.into_iter().map(|o| o.id).collect();
        *self.resin_picker_profile_ids.borrow_mut() = resin.rows.into_iter().map(|o| o.id).collect();
    }

    fn accept_slice_printer_choice(&self, index: i32) {
        // The last row, "More printers...", opens the full chooser.
        if usize::try_from(index).ok() == Some(self.printer_picker_profile_ids.borrow().len()) {
            self.open_chooser(1);
            return;
        }
        let Some(id) = profile_id_at_picker_position(&self.printer_picker_profile_ids.borrow(), index) else {
            return;
        };
        if self.slice_profile_document.choose_printer(Some(id)) {
            self.rebuild_slice_profile_options();
        }
    }

    fn accept_slice_resin_choice(&self, index: i32) {
        if usize::try_from(index).ok() == Some(self.resin_picker_profile_ids.borrow().len()) {
            self.open_chooser(2);
            return;
        }
        let Some(id) = profile_id_at_picker_position(&self.resin_picker_profile_ids.borrow(), index) else {
            return;
        };
        if self.slice_profile_document.choose_resin(Some(id)) {
            self.rebuild_slice_profile_options();
        }
    }

    /// The colour swatch beside the Resin dropdown: palette entry `index`
    /// for the selected resin, or -1 to go back to its name's colour.
    fn on_resin_color_picked(&self, index: i32) {
        let Some(id) = self.slice_profile_document.chosen_resin() else {
            return;
        };
        let rgb = usize::try_from(index)
            .ok()
            .and_then(|i| crate::resin_colors::SWATCHES.get(i))
            .map(|(_, rgb)| *rgb);
        crate::resin_colors::set(id.as_str(), rgb);
        self.rebuild_slice_profile_options();
    }

    /// Opens the full-catalog chooser: 1 printers, 2 resins. The dropdown
    /// goes back to the current choice (its "More..." row is not a choice).
    pub(crate) fn open_chooser(&self, kind: i32) {
        let Some(ui) = self.slice_profile_window.upgrade() else {
            return;
        };
        ui.set_chooser_search("".into());
        ui.set_chooser_kind(kind);
        // Open on the brand of what is in use, so its series are in view.
        let catalog = self.profile_catalog_handle.borrow().clone();
        let current_brand = if kind == 1 {
            let current = self.slice_profile_document.chosen_printer();
            catalog.printers().iter().find(|p| Some(&p.id) == current.as_ref()).map(|p| p.printer_maker.clone())
        } else {
            let current = self.slice_profile_document.chosen_resin();
            catalog.resin_list().iter().find(|r| Some(&r.id) == current.as_ref()).map(|r| r.brand.clone())
        };
        let brands = if kind == 1 {
            chooser_brands(&printer_items(catalog.printers()))
        } else {
            chooser_brands(&resin_items(catalog.resin_list()))
        };
        let shown = current_brand
            .and_then(|brand| brands.iter().position(|b| b.eq_ignore_ascii_case(brand.trim())))
            .map_or(0, |at| at + 1);
        ui.set_chooser_brand(i32::try_from(shown).unwrap_or(0));
        self.fill_chooser();
        self.rebuild_slice_profile_options();
    }

    /// A brand in the chooser's list (0 for all of them).
    fn on_chooser_brand(&self, index: i32) {
        let Some(ui) = self.slice_profile_window.upgrade() else {
            return;
        };
        ui.set_chooser_brand(index);
        self.fill_chooser();
    }

    /// Rebuilds the chooser rows for its kind and search text.
    fn fill_chooser(&self) {
        use crate::product_pictures::{Kind, picture};
        let Some(ui) = self.slice_profile_window.upgrade() else {
            return;
        };
        let search = ui.get_chooser_search().to_lowercase();
        let snapshot = self.profile_preferences_bus.current_values();
        let catalog = self.profile_catalog_handle.borrow().clone();
        // Every typed word starts a word of the name: "saturn 4" finds the
        // Saturn 4 models, not the Saturn 2 14K.
        let matches = |label: &str| {
            let label = label.to_lowercase();
            let words: Vec<&str> = label
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .collect();
            search
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .all(|typed| words.iter().any(|w| w.starts_with(typed)))
        };
        let kind = ui.get_chooser_kind();
        let printers = printer_items(catalog.printers());
        let resins = resin_items(catalog.resin_list());
        let brands = if kind == 1 { chooser_brands(&printers) } else { chooser_brands(&resins) };
        let mut brand_labels: Vec<SharedString> = vec![crate::i18n::tr!("All").to_string().into()];
        brand_labels.extend(brands.iter().map(|b| SharedString::from(b.as_str())));
        ui.set_chooser_brands(ModelRc::new(VecModel::from(brand_labels)));
        // A search looks across every brand, so nothing it finds is hidden.
        let brand = match (search.trim().is_empty(), usize::try_from(ui.get_chooser_brand())) {
            (true, Ok(at)) if at > 0 => brands.get(at - 1).map(String::as_str),
            _ => None,
        };
        if !search.trim().is_empty() {
            ui.set_chooser_brand(0);
        }
        let heading = |text: String| crate::ChooserRow {
            heading: text.into(),
            label: SharedString::default(),
            picture: slint::Image::default(),
            in_list: false,
            current: false,
        };
        let mut rows = Vec::new();
        if kind == 1 {
            let current = self.slice_profile_document.chosen_printer();
            let lines = chooser_lines(&printers, brand, |item| {
                matches(&format!("{} {} {}", item.brand, item.group, item.label))
            });
            let mut ids = Vec::with_capacity(lines.len());
            for line in lines {
                match line {
                    ChooserLine::Heading(text) => {
                        rows.push(heading(text));
                        ids.push(None);
                    }
                    ChooserLine::Choice { id, label } => {
                        rows.push(crate::ChooserRow {
                            heading: SharedString::default(),
                            label: label.into(),
                            picture: picture(Kind::Printer, id.as_str()).unwrap_or_default(),
                            in_list: snapshot.favourite_printers.contains(&id),
                            current: current.as_ref() == Some(&id),
                        });
                        ids.push(Some(id));
                    }
                }
            }
            *self.chooser_printers.borrow_mut() = ids;
        } else {
            let current = self.slice_profile_document.chosen_resin();
            let lines = chooser_lines(&resins, brand, |item| {
                matches(&format!("{} {} {}", item.brand, item.group, item.label))
            });
            let mut ids = Vec::with_capacity(lines.len());
            for line in lines {
                match line {
                    ChooserLine::Heading(text) => {
                        rows.push(heading(text));
                        ids.push(None);
                    }
                    ChooserLine::Choice { id, label } => {
                        rows.push(crate::ChooserRow {
                            heading: SharedString::default(),
                            label: label.into(),
                            picture: picture(Kind::Resin, id.as_str()).unwrap_or_default(),
                            in_list: snapshot.favourite_resins.contains(&id),
                            current: current.as_ref() == Some(&id),
                        });
                        ids.push(Some(id));
                    }
                }
            }
            *self.chooser_resins.borrow_mut() = ids;
        }
        *self.chooser_brands.borrow_mut() = brands;
        ui.set_chooser_rows(ModelRc::new(VecModel::from(rows)));
    }

    /// A chooser row was clicked: use it, keep it in the dropdown, close.
    fn on_chooser_picked(&self, index: i32) {
        let Some(ui) = self.slice_profile_window.upgrade() else {
            return;
        };
        // A heading is not a choice: the chooser stays open.
        if ui.get_chooser_kind() == 1 {
            let Some(id) = profile_id_at_picker_position(&self.chooser_printers.borrow(), index).flatten() else {
                return;
            };
            self.set_favourite_printer(&id, true);
            self.slice_profile_document.choose_printer(Some(id));
        } else {
            let Some(id) = profile_id_at_picker_position(&self.chooser_resins.borrow(), index).flatten() else {
                return;
            };
            self.set_favourite_resin(&id, true);
            self.slice_profile_document.choose_resin(Some(id));
        }
        ui.set_chooser_kind(0);
        self.rebuild_slice_profile_options();
    }

    /// A chooser row's star: add it to or remove it from the dropdown.
    fn on_chooser_star(&self, index: i32) {
        let Some(ui) = self.slice_profile_window.upgrade() else {
            return;
        };
        let snapshot = self.profile_preferences_bus.current_values();
        if ui.get_chooser_kind() == 1 {
            if let Some(id) = profile_id_at_picker_position(&self.chooser_printers.borrow(), index).flatten() {
                let keep = !snapshot.favourite_printers.contains(&id);
                self.set_favourite_printer(&id, keep);
            }
        } else if let Some(id) = profile_id_at_picker_position(&self.chooser_resins.borrow(), index).flatten() {
            let keep = !snapshot.favourite_resins.contains(&id);
            self.set_favourite_resin(&id, keep);
        }
        self.fill_chooser();
    }

    fn set_favourite_printer(&self, id: &PrinterId, keep: bool) {
        if self.profile_preferences_bus.current_values().favourite_printers.contains(id) == keep {
            return;
        }
        let id = id.clone();
        self.profile_preferences_bus.edit_preferences(move |snap| {
            if keep {
                snap.favourite_printers.insert(id);
            } else {
                snap.favourite_printers.remove(&id);
            }
            PreferenceEdit::Favourites
        });
    }

    fn set_favourite_resin(&self, id: &MaterialId, keep: bool) {
        if self.profile_preferences_bus.current_values().favourite_resins.contains(id) == keep {
            return;
        }
        let id = id.clone();
        self.profile_preferences_bus.edit_preferences(move |snap| {
            if keep {
                snap.favourite_resins.insert(id);
            } else {
                snap.favourite_resins.remove(&id);
            }
            PreferenceEdit::Favourites
        });
    }

    /// The first-layer field was typed in: blank drops this print's own
    /// first layer height, a number in mm sets it, and anything else (a
    /// number half typed) changes nothing. The field is not rewritten here,
    /// which would move the text under the user's cursor.
    fn accept_first_layer_override_text(&self, text: &str) {
        let height = match read_first_layer_edit(text) {
            FirstLayerEdit::Ignored => return,
            FirstLayerEdit::Unset => None,
            FirstLayerEdit::Height(mm) => Some(mm),
        };
        let doc = &self.slice_profile_document;
        if doc.choose_first_layer(height) {
            doc.process_changed();
        }
    }
}

/// The id on dropdown row `index`; none for -1 (nothing picked) or a row
/// past the list (the trailing "More..." row).
fn profile_id_at_picker_position<Id: Clone>(ids: &[Id], index: i32) -> Option<Id> {
    let row = usize::try_from(index).ok()?;
    ids.get(row).cloned()
}

/// The dropdown's rows: every option's label, then `more`, the row that
/// opens the full-catalog chooser (R101).
fn labels_model<Id>(model: &DropdownRows<Id>, more: String) -> ModelRc<SharedString> {
    let mut rows: Vec<SharedString> = Vec::with_capacity(model.rows.len() + 1);
    rows.extend(model.rows.iter().map(|o| SharedString::from(o.caption.as_str())));
    rows.push(more.into());
    ModelRc::new(VecModel::from(rows))
}

/// A length or size for display: at most one decimal, none when whole
/// (223.642 -> "223.6", 230.0 -> "230").
pub(crate) fn short_number(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
}

#[cfg(test)]
mod short_number_tests {
    /// R94: printer sizes and pixel pitches read rounded.
    #[test]
    fn sizes_read_with_at_most_one_decimal() {
        assert_eq!(super::short_number(223.642), "223.6");
        assert_eq!(super::short_number(230.0), "230");
        assert_eq!(super::short_number(16.80003004807692), "16.8");
        assert_eq!(super::short_number(24.703125), "24.7");
    }
}

/// The colour of a resin for its bottle picture, from the colour named in its
/// profile name (R99): profiles have no colour field. Grey otherwise, like
/// most standard resins.
pub(crate) fn resin_color(name: &str) -> slint::Color {
    let name = name.to_lowercase();
    const COLOURS: &[(&[&str], [u8; 3])] = &[
        (&["black"], [0x4a, 0x4a, 0x4e]),
        (&["white", "ivory"], [0xf2, 0xf2, 0xee]),
        (&["red"], [0xd9, 0x44, 0x3b]),
        (&["blue", "cyan"], [0x3d, 0x7e, 0xe0]),
        (&["glow"], [0xc8, 0xf0, 0xb4]),
        (&["green"], [0x3f, 0xa3, 0x5b]),
        (&["yellow"], [0xf2, 0xc2, 0x30]),
        (&["orange"], [0xf0, 0x8a, 0x2c]),
        (&["pink"], [0xec, 0x7f, 0xb0]),
        (&["purple", "violet"], [0x8a, 0x5c, 0xd6]),
        (&["beige", "skin", "tan", "sand"], [0xe3, 0xca, 0xa5]),
        (&["clear", "transparent", "translucent"], [0xcf, 0xe8, 0xf5]),
        (&["grey", "gray"], [0x8c, 0x91, 0x96]),
    ];
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let [r, g, b] = COLOURS
        .iter()
        .find(|(keys, _)| keys.iter().any(|k| words.contains(k)))
        .map_or([0x9a, 0xa0, 0xa6], |(_, rgb)| *rgb);
    slint::Color::from_rgb_u8(r, g, b)
}

#[cfg(test)]
mod resin_color_tests {
    /// R99: the bottle takes the colour in the resin's name, grey otherwise.
    #[test]
    fn colour_comes_from_the_name() {
        let rgb = |n: &str| {
            let c = super::resin_color(n);
            [c.red(), c.green(), c.blue()]
        };
        assert_eq!(rgb("Elegoo Standard Black"), [0x4a, 0x4a, 0x4e]);
        assert_eq!(rgb("Siraya Tech Blu (Clear)"), [0xcf, 0xe8, 0xf5]);
        assert_eq!(rgb("Anycubic Craftsman Resin"), [0x9a, 0xa0, 0xa6]);
        // Whole words only: "Tough" holds no "ug", "Bread" no "red".
        assert_eq!(rgb("Bread Tough Resin"), [0x9a, 0xa0, 0xa6]);
        assert_eq!(rgb("ABS-Like Grey"), [0x8c, 0x91, 0x96]);
    }
}
