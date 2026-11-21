// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Launch automation uses the same document and callbacks as interactive actions.
use crate::{
    MainWindow, WorkspaceSection, launch_options::LaunchOptions, scene_document::SceneDocument,
};
use slint::{ComponentHandle, Model};
use std::{cell::Cell, error::Error, rc::Rc, time::Duration};

pub(crate) fn install(
    ui: &MainWindow,
    document: Rc<SceneDocument>,
    options: LaunchOptions,
    preferences: crate::preferences_bus::UserPreferences,
) -> Result<(), Box<dyn Error>> {
    if let Some((w, h)) = options.window {
        ui.window()
            .set_size(slint::LogicalSize::new(w as f32, h as f32));
    }
    if let Some((x, y)) = options.window_at {
        ui.window().set_position(slint::PhysicalPosition::new(x, y));
    }
    let printer = options.printer.as_deref().map(str::parse).transpose()?;
    let material = options.material.as_deref().map(str::parse).transpose()?;
    // Placement captures the printer envelope when loading is dispatched.
    // Apply launch overrides before opening, as well as after project install.
    if let Some(id) = printer.clone() {
        document.choose_printer(Some(id));
    }
    if let Some(id) = material.clone() {
        document.choose_resin(Some(id));
    }
    let weak = ui.as_weak();
    let timer = Rc::new(slint::Timer::default());
    let stop = timer.clone();
    let mut editor = None;
    let mut motion_editor = None;
    let mut audit_dialog = None;
    let mut configured = false;
    let mut requested_slice = false;
    let mut requested_supports = false;
    let mut last = Vec::new();
    let mut stable = 0;
    let mut input_verified = false;
    let polls = Cell::new(0u32);
    let mut idle_without_scene = 0u32;
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(150), move || {
        let Some(ui) = weak.upgrade() else { stop.stop(); return; };
        polls.set(polls.get()+1);
        if polls.get() > 2400 { eprintln!("UI verification timed out"); std::process::exit(1); }
        match scene_wait(ui.get_busy_opening() || ui.get_opening(), options.open.is_empty() || ui.get_scene_present(), &mut idle_without_scene) {
            SceneWait::Ready => {}
            SceneWait::Waiting => { stable = 0; return; }
            SceneWait::Failed => { eprintln!("UI verification: the model could not be opened"); std::process::exit(2); }
        }
        if !configured {
            if let Some(id) = printer.clone() { document.choose_printer(Some(id)); }
            if let Some(id) = material.clone() { document.choose_resin(Some(id)); }
            if let Some(mode) = &options.settings_mode { ui.set_settings_level(match mode.as_str() { "simple"=>0,"expert"=>2,_=>1 }); }
            if let Some(search) = &options.search { ui.set_settings_search(search.into()); }
            if let Some(theme) = &options.theme { ui.set_theme(if theme=="light" { crate::ThemePreference::Light } else { crate::ThemePreference::Dark }); }
            ui.set_preview_3d(options.preview_view.as_deref()==Some("3d"));
            if let Some(mode)=options.verify_input.as_deref().and_then(|v|v.strip_prefix("audit-")) {
                audit_dialog=Some(AuditDialog::open(mode,ui.get_theme(),options.window,&preferences).unwrap());
                if let Some((x,y))=options.window_at {audit_dialog.as_ref().unwrap().window().set_position(slint::PhysicalPosition::new(x,y));}
            }
            if options.verify_printer_error {
                let profile = sc_print_profiles::ProfileLibraries::platform_default().load_printer_by_id(&"sleptic-diy-msla".parse().unwrap()).unwrap();
                let dialog = crate::printer_editor::open(profile, false, Rc::new(|_| {})).unwrap();
                dialog.set_layer("G1 Z{unknown_height}".into()); dialog.invoke_validate(); editor = Some(dialog);
            }
            if options.verify_measure { ui.set_measure_tool_active(true); ui.invoke_measure_evidence_requested(); }
            configured = true;
            if let Some(plate) = options.plate { ui.invoke_plate_selected(plate as i32); }
            if let Some(mode) = &options.batch_mode {
                ui.invoke_batch_prepare_requested();
                for path in &options.batch_sources { ui.invoke_batch_add_path(path.to_string_lossy().as_ref().into()); }
                ui.set_batch_orient(false);
                if mode == "summary" { ui.invoke_batch_start(); }
            }
            if let Some(mode)=options.verify_input.as_deref()
                && ["machine-orientation","machine-xy-ruler","machine-z-tower"].contains(&mode) {
                    ui.set_machine_tool(match mode {"machine-orientation"=>0,"machine-xy-ruler"=>1,_=>2});ui.invoke_machine_create();ui.set_machine_tool(-1);
            }
            if matches!(options.verify_input.as_deref(),Some("calibration-plate"|"calibration-preview")) {ui.invoke_calibration_create();}
            if options.verify_input.as_deref()==Some("motion-editor") {
                use sc_print_profiles::motion::{Motion,MotionProfile};
                let d=crate::motion_form::open(MotionProfile {normal:Motion {lift_slow_distance_mm:Some(2.0),lift_slow_speed_mm_min:Some(60.0),lift_fast_distance_mm:Some(4.0),lift_fast_speed_mm_min:Some(240.0),retract_slow_distance_mm:Some(2.0),retract_slow_speed_mm_min:Some(120.0),retract_fast_distance_mm:Some(4.0),retract_fast_speed_mm_min:Some(240.0),wait_before_cure_s:Some(0.3),wait_after_lift_s:Some(0.4),wait_after_retract_s:Some(0.5),light_off_delay_s:Some(0.6)},..Default::default()},Rc::new(|_|{})).unwrap();
                d.set_viewer_theme(ui.get_theme());
                if let Some((w,h))=options.window {d.window().set_size(slint::LogicalSize::new(w as f32,h as f32));}
                if let Some((x,y))=options.window_at {d.window().set_position(slint::PhysicalPosition::new(x,y));}
                motion_editor=Some(d);
            }
            if let Some(height) = &options.layer_height { ui.invoke_process_setting_edited(0, height.into()); }
            if options.auto_orient { ui.invoke_auto_orient_requested(); }
            if options.arrange { ui.invoke_arrange_requested(); }
            if let Some(axis) = options.mirror { ui.invoke_mirror_requested(axis); }
            if options.hollow || options.auto_drains { ui.invoke_hollow_requested(options.auto_drains); }
            if options.fill_plate { ui.invoke_fill_plate_requested(); }
            if options.auto_prepare { requested_supports = true; ui.invoke_auto_prepare_requested(); return; }
            if options.supports { requested_supports = true; ui.set_supports_enabled(true); ui.set_supports_tool_active(true); ui.invoke_auto_supports_requested(); return; }
            if options.slice_all { requested_slice = true; ui.invoke_slice_all_requested(); return; }
            if options.slice { requested_slice = true; ui.invoke_start_slice(); return; }
        }
        if ui.get_batch_busy() { stable = 0; return; }
        if let Some(plate) = options.plate && document.active_plate() != plate && !ui.get_slicing() { ui.invoke_plate_selected(plate as i32); }
        if ui.get_making_supports() { stable = 0; return; }
        if requested_supports && !requested_slice && options.slice { requested_slice = true; ui.invoke_start_slice(); return; }
        if ui.get_slicing() { stable = 0; return; }
        if requested_slice && ui.get_layer_total() == 0 && !ui.get_slice_failed() { return; }
        if let Some(workspace) = &options.workspace {
            let section = if workspace == "preview" { WorkspaceSection::Slice } else { WorkspaceSection::Transform };
            if ui.get_section() != section { ui.invoke_section_picked(section); stable = 0; return; }
        }
        if let Some(index)=options.risk_jump && ui.get_selected_risk()!=index as i32 && ui.get_print_risks().row_count()>index { ui.invoke_risk_jump_requested(index as i32); stable=0;return; }
        if let Some(layer) = options.preview_layer {
            let index = layer.saturating_sub(1).min(ui.get_layer_total().saturating_sub(1).max(0) as u32) as f32;
            if ui.get_layer_shown() != index { ui.set_layer_shown(index); ui.invoke_layer_picked(); stable = 0; return; }
        }
        if let Some(input) = &options.verify_input && !input_verified {
                input_verified = true;
                if let Some(page) = match input.as_str() { "hub-home" => Some(1), "hub-device" => Some(2), "hub-project" => Some(3), _ => None } {
                    ui.set_hub_page(page);
                    ui.invoke_hub_page_opened(page);
                }
                // The in-window confirmation (opening a project over unsaved work).
                if input == "confirm" {
                    crate::confirm::ask(
                        &ui,
                        crate::confirm::Question {
                            title: "Save changes to Benchy fleet.3mf?",
                            body: "Opening calibration.3mf closes the current project. Unsaved changes are lost if you don't save.",
                            primary: "Save",
                            secondary: "Don't save",
                            destructive: false,
                        },
                        |_| {},
                    );
                }
                // First-run wizard at a step (setup-0 .. setup-3).
                if let Some(step) = input.strip_prefix("setup-").and_then(|n| n.parse::<i32>().ok()) {
                    ui.invoke_show_setup_requested();
                    ui.set_setup_step(step);
                }
                if input.starts_with("popup-") {ui.set_verification_popup(match input.as_str() {"popup-mirror"=>1,"popup-plate"=>2,"popup-slice"=>3,"popup-export"=>4,"popup-more"=>6,_=>5});}
                else if input=="panel-hollow" {ui.set_hollow_tool_active(true);}
                else if input=="panel-supports" {ui.set_supports_tool_active(true);}
                // The dropdown's last row, as a user opens the chooser (R101).
                else if input=="object-panel-hidden" {ui.set_parts_panel_shown(false);}
                else if input=="chooser-printer" {ui.invoke_printer_picked(ui.get_printer_choices().row_count() as i32 - 1);}
                else if input=="panel-batch" {ui.invoke_batch_prepare_requested();}
                else if input=="orient-options" {ui.set_orient_advanced(true);ui.set_orient_options_open(true);}
                else if input=="calibration-dialog" {ui.set_calibration_open(true);}
                else if input=="calibration-menu" {ui.set_calibration_menu_open(true);}
                else if input=="overhang-test" {ui.invoke_overhang_test_create();ui.set_overhang_test_open(true);}
                else if input=="machine-xy-dialog" {ui.set_machine_tool(1);}
                else if input=="machine-orientation-dialog" {ui.set_machine_tool(0);}
                else if input=="machine-z-dialog" {ui.set_machine_tool(2);}
                else if input.starts_with("audit-") || input.starts_with("machine-") || ["motion-editor","calibration-plate","calibration-preview"].contains(&input.as_str()) {}
                else if input.starts_with("recipe-") {
                    ui.invoke_recipes_requested();
                    ui.set_recipe_scope(1);
                    if input != "recipe-editor" { ui.invoke_recipe_run(); }
                }
                else if input == "supports-comparison" { ui.invoke_model_tool_evidence(4); }
                else if input=="model-simplify" { ui.set_model_tools_active(true); ui.set_model_tool_mode(3); }
                else if input=="toolbar-move" { ui.invoke_gizmo_toolbar(0); }
                else if input=="toolbar-scale" { ui.invoke_gizmo_toolbar(2); }
                else if input=="toolbar-rotate" { ui.invoke_gizmo_toolbar(1); }
                else if input.starts_with("model-") {
                    ui.invoke_model_tool_evidence(match input.as_str() {"model-cut"=>0,"model-split"=>1,"model-label"=>2,_=>3});
                }
                else if input == "supports" {
                    ui.set_supports_tool_active(true);
                    ui.set_supports_enabled(true);
                    document.choose(crate::selection::Selection::None);
                    ui.invoke_input_evidence(2);
                }
                else if input == "place-face" { ui.invoke_input_evidence(6); }
                else if let Some(k) = ["cube-face", "cube-edge", "cube-corner", "layer-closeup", "support-closeup", "plate-label"].iter().position(|m| *m == input) { ui.invoke_input_evidence(7 + k as i32); }
                else if input == "multi" { ui.invoke_duplicate_requested(); document.select_all(); }
                else { ui.invoke_input_evidence(match input.as_str() { "object-menu" | "move-menu" => 1, "stack" => 2, "global-menu" => 3, "orbit" => 4, "drag" => 5, _ => 0 }); }
                if input == "move-menu" { ui.invoke_move_menu_evidence(); }
                stable = 0; return;
        }
        if ui.get_recipe_busy() {
            if options.verify_input.as_deref()==Some("recipe-progress")
                && let Some(path)=&options.screenshot
                && let Ok(capture)=ui.window().take_snapshot() {
                        if let Some((w,h)) = options.window
                            && (capture.width() != w || capture.height() != h) {
                            let size = ui.window().size().to_logical(ui.window().scale_factor());
                            ui.window().set_size(slint::LogicalSize::new(size.width + w as f32 - capture.width() as f32, size.height + h as f32 - capture.height() as f32));
                            return;
                        }
                        image::save_buffer(path,capture.as_bytes(),capture.width(),capture.height(),image::ColorType::Rgba8).unwrap();
                        if let Some(state)=&options.dump_state { std::fs::write(state,serde_json::json!({"recipe_busy":true,"summary":ui.get_recipe_summary().as_str(),"progress":ui.get_recipe_progress().iter().map(|s|s.to_string()).collect::<Vec<_>>(),"width":capture.width(),"height":capture.height()}).to_string()).unwrap(); }
                        std::process::exit(0);
            }
            stable=0; return;
        }
        if options.benchmark && ui.get_benchmark_frames() < 120 {
            ui.set_benchmark_preview(true); ui.invoke_layer_picked(); return;
        }
        ui.set_benchmark_preview(false);
        if options.screenshot.is_none() && options.dump_state.is_none() { stop.stop(); return; }
        let capture_window=if let Some(d)=&audit_dialog {d.window()} else if let Some(d)=&motion_editor {d.window()} else {editor.as_ref().map_or(ui.window(),|w|w.window())};
        let capture = match capture_window.take_snapshot() { Ok(c) => c, Err(e) => { eprintln!("Snapshot failed: {e}"); std::process::exit(1); } };
        if let Some((w,h)) = options.window.filter(|_| editor.is_none())
            && (capture.width() != w || capture.height() != h) {
                let size = capture_window.size().to_logical(capture_window.scale_factor());
                capture_window.set_size(slint::LogicalSize::new(size.width + w as f32 - capture.width() as f32, size.height + h as f32 - capture.height() as f32));
                stable = 0; return;
            }
        // Five identical rendered frames after the final state change. This also
        // waits for asynchronous GPU uploads and layout, rather than a fixed delay.
        if frames_settled(capture.as_bytes(), &last) { stable += 1; } else { last = capture.as_bytes().to_vec(); stable = 0; }
        ui.window().request_redraw();
        if stable < 5 { return; }
        let write = || -> Result<(), Box<dyn Error>> {
            if let Some(path) = &options.screenshot {
                image::save_buffer(path, capture.as_bytes(), capture.width(), capture.height(), image::ColorType::Rgba8)?;
            }
            if let Some(path) = &options.dump_state {
                let mut state = serde_json::json!({
                    "printer_editor_error": editor.as_ref().map(|w| w.get_error().to_string()),
                    "measure": ui.get_measure_text().as_str(),
                    "recovery_prompt": ui.get_recovery_prompt(),
                    "workspace": if ui.get_section() == WorkspaceSection::Slice { "preview" } else { "prepare" },
                    "slice_state": match &*document.slice_state.borrow() {
                        crate::slice_state::SliceState::Idle => "Idle", crate::slice_state::SliceState::Slicing { .. } => "Slicing", crate::slice_state::SliceState::Sliced => "Sliced", crate::slice_state::SliceState::Stale => "Stale", crate::slice_state::SliceState::Failed { .. } => "Failed",
                    },
                    "printer": document.chosen_printer().map(|id| id.to_string()),
                    "material": document.chosen_resin().map(|id| id.to_string()),
                    "visible_panels": if ui.get_section() == WorkspaceSection::Slice { vec!["settings", "preview", "layer_slider", "stats", "status"] } else if ui.get_chosen_part_row() >= 0 { vec!["settings", "objects", "transforms", "viewport", "status"] } else { vec!["settings", "objects", "object_placeholder", "viewport", "status"] },
                    "settings_mode": ui.get_settings_level(),
                    "search": ui.get_settings_search().as_str(),
                    "preview_view": if ui.get_preview_3d() { "3d" } else { "2d" },
                    "selected_object": ui.get_chosen_part_row(),
                    "selection_count": document.chosen().ids().len(),
                    "object_scale": (0..3).filter_map(|i|ui.get_object_scale().row_data(i).map(|s|s.to_string())).collect::<Vec<_>>(),
                    "islands": ui.get_island_layers().row_count(),
                    "hollow_saved": ui.get_hollow_saved_text().as_str(),
                    "suction_cups": ui.get_suction_cup_count(),
                    "plate_locked": ui.get_plate_locked(),
                    "plate_toolbar": [ui.get_plate_toolbar_x(),ui.get_plate_toolbar_y()],
                    "camera_angles": [ui.get_nav_yaw(), ui.get_nav_pitch()],
                    "cube_faces": (0..ui.get_cube_faces().row_count()).filter_map(|i| ui.get_cube_faces().row_data(i).map(|f| f.label.to_string())).collect::<Vec<_>>(),
                    "object_position": [ui.get_position_x_text().to_string(),ui.get_position_y_text().to_string(),ui.get_position_z_text().to_string()],
                    "can_undo": document.has_past(),
                    "plate_action": ui.get_plate_action_message().as_str(),
                    "preview_fps": ui.get_benchmark_fps(),
                    "supports": ui.get_supports_count(),
                    "supports_stale": ui.get_supports_stale(),
                    "object_rows": (0..ui.get_parts_list().row_count()).filter_map(|i| ui.get_parts_list().row_data(i).map(|row| row.text.to_string())).collect::<Vec<_>>(),
                    "slice_recovery": ui.get_slice_recovery_message().as_str(),
                    "supports_error": ui.get_supports_error().as_str(),
                    "benchmark_frames": ui.get_benchmark_frames(),
                    "slider": { "visible": ui.get_section() == WorkspaceSection::Slice, "minimum": if ui.get_layer_total() > 0 { 1 } else { 0 }, "maximum": ui.get_layer_total(), "value": if ui.get_layer_total() > 0 { ui.get_layer_shown() as u32 + 1 } else { 0 } },
                    "stats": { "print_time": ui.get_estimated_print_time_text().as_str(), "volume": ui.get_estimated_resin_volume_text().as_str(), "cost": ui.get_estimated_resin_cost_text().as_str(), "weight": ui.get_estimated_weight_text().as_str(), "exposure": ui.get_exposure_summary().as_str(), "z": ui.get_layer_z_text().as_str(), "layers": ui.get_layer_total() },
                    "window": { "width": capture.width(), "height": capture.height() }
                });
                state["machine_message"]=serde_json::json!(ui.get_machine_message().as_str());
                state["machine_tool"]=serde_json::json!(ui.get_machine_tool());
                state["orient_preset"]=serde_json::json!(ui.get_orient_preset());
                state["orient_options"]=serde_json::json!(ui.get_orient_options_open());
                state["calibration_message"]=serde_json::json!(ui.get_calibration_message().as_str());
                state["calibration_tiles"]=serde_json::json!(ui.get_calibration_tiles().iter().map(|s|s.to_string()).collect::<Vec<_>>());
                state["motion_timeline"]=serde_json::json!(motion_editor.as_ref().map(|d|d.get_motion_timeline().to_string()));
                state["recipe_summary"]=ui.get_recipe_summary().to_string().into();
                state["recipe_progress"]=serde_json::to_value(ui.get_recipe_progress().iter().map(|s|s.to_string()).collect::<Vec<_>>())?;
                state["model_tool_error"]=ui.get_model_tool_error().to_string().into();
                state["model_tool_mode"]=ui.get_model_tool_mode().into();
                state["cut_z_mm"]=ui.get_cut_z_mm().to_string().into();
                state["cut_plane_markers"]=ui.get_cut_plane_markers().row_count().into();
                state["sliced_plates"]=document.all_plate_slices().len().into();
                state["active_plate"]=document.active_plate().into();
                state["plate_count"]=ui.get_plate_ids().row_count().into();
                state["batch_summary"]=ui.get_batch_summary().to_string().into();
                state["print_risks"]=serde_json::json!((0..ui.get_print_risks().row_count()).filter_map(|i|ui.get_print_risks().row_data(i).map(|r|serde_json::json!({"group":r.group.to_string(),"title":r.title.to_string(),"error":r.error,"kind":r.kind,"layer":r.layer+1,"last":r.last+1,"action":r.action}))).collect::<Vec<_>>());
                state["selected_risk"]=ui.get_selected_risk().into();state["risk_errors"]=ui.get_risk_error_count().into();
                std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
            }
            Ok(())
        };
        if let Err(e) = write() { eprintln!("Verification output failed: {e}"); std::process::exit(1); }
        stop.stop();
        let _ = slint::quit_event_loop();
    });
    Ok(())
}

// Audit uses real exported dialog components and the ordinary printer / profile
// presenters; nothing is drawn into the captured PNG after take_snapshot.
enum AuditDialog {
    Printer(crate::PrinterEditor),
    Profile(crate::ProfileViewDialog),
    Fork(crate::ForkDialog),
    Extend(crate::ExtendDialog),
    Error(crate::ErrorDialog),
    Info(crate::InfoDialog),
    Overwrite(crate::SliceOverwriteDialog),
    About(crate::AboutWindow),
    Shortcuts(crate::ShortcutsDialog),
    Preferences(crate::preferences_window::PreferencesWindowBinding),
}
impl AuditDialog {
    fn window(&self) -> &slint::Window {
        match self {
            Self::Printer(d) => d.window(),
            Self::Profile(d) => d.window(),
            Self::Fork(d) => d.window(),
            Self::Extend(d) => d.window(),
            Self::Error(d) => d.window(),
            Self::Info(d) => d.window(),
            Self::Overwrite(d) => d.window(),
            Self::About(d) => d.window(),
            Self::Shortcuts(d) => d.window(),
            Self::Preferences(d) => d.window.window(),
        }
    }
    fn open(
        mode: &str,
        theme: crate::ThemePreference,
        size: Option<(u32, u32)>,
        preferences: &crate::preferences_bus::UserPreferences,
    ) -> Result<Self, Box<dyn Error>> {
        macro_rules! show {
            (About,$dialog:expr) => {{
                let d = $dialog;
                d.set_about_color_preference(theme);
                d.show()?;
                Self::About(d)
            }};
            (Profile,$dialog:expr) => {{
                let d = $dialog;
                d.set_viewer_theme(theme);
                d.show()?;
                Self::Profile(d)
            }};
            (Fork,$dialog:expr) => {{
                let d = $dialog;
                d.set_fork_dialog_theme(theme);
                d.show()?;
                Self::Fork(d)
            }};
            (Extend,$dialog:expr) => {{
                let d = $dialog;
                d.set_extension_dialog_theme(theme);
                d.show()?;
                Self::Extend(d)
            }};
            ($variant:ident,$dialog:expr) => {{
                let d = $dialog;
                d.set_theme(theme);
                d.show()?;
                Self::$variant(d)
            }};
        }
        let libraries = sc_print_profiles::ProfileLibraries::platform_default();
        let result = match mode {
            "preferences" | "preferences-printer" | "preferences-resin" | "preferences-slicing" => {
                let mut snapshot = preferences.clone();
                snapshot.theme = theme;
                let bus = crate::preferences_bus::PreferenceHub::new(snapshot);
                let catalog = Rc::new(std::cell::RefCell::new(Rc::new(
                    crate::profile_catalog::InstalledProfiles::scan_libraries(&libraries),
                )));
                let d = crate::preferences_window::build_preferences_window(&bus, catalog)?;
                d.window.set_theme(theme);
                d.window.set_section(match mode {
                    "preferences-printer" => 1,
                    "preferences-resin" => 2,
                    "preferences-slicing" => 3,
                    _ => 0,
                });
                d.window.show()?;
                Self::Preferences(d)
            }
            "printer" => show!(
                Printer,
                crate::printer_editor::open(
                    libraries.load_printer_by_id(&"sleptic-diy-msla".parse()?)?,
                    false,
                    Rc::new(|_| {})
                )?
            ),
            "profile" => {
                let d = crate::ProfileViewDialog::new()?;
                let (m, _) = libraries.effective_material_by_id(&"standard-405".parse()?)?;
                let catalog = crate::profile_catalog::InstalledProfiles::scan_libraries(&libraries);
                let sections = crate::profile_view::profile_view_rows(
                    &m,
                    catalog.printers(),
                    Some(&"sleptic-diy-msla".parse()?),
                    sc_print_profiles::ProfileSource::Bundled,
                );
                d.set_resin_heading(m.identity().name.as_str().into());
                d.set_profile_cards(slint::ModelRc::new(slint::VecModel::from(sections)));
                show!(Profile, d)
            }
            "fork" => {
                let d = crate::ForkDialog::new()?;
                d.set_fork_source_caption("Standard 405 nm".into());
                d.set_new_profile_name("Standard 405 - calibrated".into());
                d.set_new_profile_brand("Sleptic".into());
                d.set_source_block_captions(slint::ModelRc::new(slint::VecModel::from(vec![
                    "Generic printer".into(),
                    "Sleptic DIY MSLA".into(),
                ])));
                show!(Fork, d)
            }
            "extend" => {
                let d = crate::ExtendDialog::new()?;
                d.set_extension_source_title("Standard 405 nm".into());
                d.set_extension_target_choices(slint::ModelRc::new(slint::VecModel::from(vec![
                    "Sleptic DIY MSLA".into(),
                ])));
                d.set_extension_target_selection(0);
                d.set_extension_normal_exposure_input("2.5".into());
                d.set_extension_bottom_exposure_input("25".into());
                d.set_extension_bottom_layer_input("5".into());
                d.set_extension_lift_distance_input("6".into());
                d.set_extension_lift_speed_input("60".into());
                d.set_extension_retract_speed_input("150".into());
                show!(Extend, d)
            }
            "error" => {
                let d = crate::ErrorDialog::new()?;
                d.set_error_heading_text("Could not export calibration".into());
                d.set_error_context_text("Active plate".into());
                d.set_error_message_text("Choose an output folder and try again.".into());
                d.set_error_details_text("The selected folder is read-only.".into());
                d.set_details_inline(crate::error_dialog::inline_details(&d.get_error_details_text()));
                show!(Error, d)
            }
            "error-technical" => {
                let d = crate::ErrorDialog::new()?;
                d.set_error_heading_text("Failed to open 3D model".into());
                d.set_error_context_text("File: C:\\Models\\bracket.stl".into());
                d.set_error_message_text("The file could not be read.".into());
                d.set_error_details_text("Io(Os { code: 32, kind: Uncategorized, message: \"The process cannot access the file because it is being used by another process.\" }) at sc_mesh_formats::loader::load_path".into());
                d.set_details_inline(crate::error_dialog::inline_details(&d.get_error_details_text()));
                show!(Error, d)
            }
            "info" => {
                let d = crate::InfoDialog::new()?;
                d.set_body_text("Machine calibration test is ready.".into());
                show!(Info, d)
            }
            "overwrite" => show!(Overwrite, crate::SliceOverwriteDialog::new()?),
            "about" => {
                let d = crate::AboutWindow::new()?;
                d.set_about_release_label(env!("CARGO_PKG_VERSION").into());
                show!(About, d)
            }
            "shortcuts" => {
                let d = crate::ShortcutsDialog::new()?;
                d.set_groups(crate::shortcuts_window::shortcut_groups_model(cfg!(
                    target_os = "macos"
                )));
                show!(Shortcuts, d)
            }
            _ => return Err(format!("Unknown audit dialog: {mode}").into()),
        };
        if let Some((w, h)) = size {
            result
                .window()
                .set_size(slint::LogicalSize::new(w as f32, h as f32));
        }
        Ok(result)
    }
}

/// Where launch automation stands on the model it was asked to open.
#[derive(Debug, PartialEq)]
enum SceneWait {
    Ready,
    Waiting,
    /// Nothing is loading and no scene arrived: the open failed (its error
    /// dialog is up). Automation used to wait for the scene until its
    /// six-minute timeout (R92).
    Failed,
}

/// Polls (150 ms each) with nothing loading and no scene before an open
/// counts as failed: long enough for a loaded scene to be installed.
const OPEN_FAILED_AFTER_POLLS: u32 = 20;

fn scene_wait(loading: bool, ready: bool, idle: &mut u32) -> SceneWait {
    if ready {
        return SceneWait::Ready;
    }
    if loading {
        *idle = 0;
        return SceneWait::Waiting;
    }
    *idle += 1;
    if *idle > OPEN_FAILED_AFTER_POLLS {
        SceneWait::Failed
    } else {
        SceneWait::Waiting
    }
}

// A focused text field can blink its caret while the rest of a window is settled.
fn frames_settled(current: &[u8], previous: &[u8]) -> bool {
    current.len() == previous.len()
        && current
            .iter()
            .zip(previous)
            .filter(|(a, b)| a != b)
            .take(513)
            .count()
            <= 512
}
#[cfg(test)]
mod capture_tests {
    #[test]
    fn caret_blinks_settle_but_layout_changes_do_not() {
        let old = vec![0; 4096];
        let mut current = old.clone();
        assert!(super::frames_settled(&current, &old));
        current[..128].fill(255);
        assert!(super::frames_settled(&current, &old));
        current[..1024].fill(255);
        assert!(!super::frames_settled(&current, &old));
        assert!(!super::frames_settled(&current, &[]));
    }
}

#[cfg(test)]
mod scene_wait_tests {
    use super::{OPEN_FAILED_AFTER_POLLS, SceneWait, scene_wait};

    /// R92: a model that fails to open ends automation with an error instead
    /// of waiting for a scene that never comes; a slow load is not cut short.
    #[test]
    fn a_failed_open_stops_waiting_but_a_long_load_does_not() {
        let mut idle = 0;
        for _ in 0..1000 {
            assert_eq!(scene_wait(true, false, &mut idle), SceneWait::Waiting);
        }
        assert_eq!(scene_wait(false, true, &mut idle), SceneWait::Ready);
        let mut idle = 0;
        for _ in 0..OPEN_FAILED_AFTER_POLLS {
            assert_eq!(scene_wait(false, false, &mut idle), SceneWait::Waiting);
        }
        assert_eq!(scene_wait(false, false, &mut idle), SceneWait::Failed);
        // Loading again resets the count.
        let mut idle = OPEN_FAILED_AFTER_POLLS;
        assert_eq!(scene_wait(true, false, &mut idle), SceneWait::Waiting);
        assert_eq!(scene_wait(false, false, &mut idle), SceneWait::Waiting);
    }
}
