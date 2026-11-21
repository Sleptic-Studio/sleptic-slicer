// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The slicer GUI as a library: `main.rs` only calls [`run`]. Modules the
//! integration tests under `tests/` use are `pub`; the rest stay private to
//! the crate.

mod aa_level;
mod about_window;
mod app_settings;
mod app_state;
mod auto_support_run;
mod batch_prepare;
pub mod build_volume;
pub mod camera;
mod clipboard;
mod confirm;
mod drop_import;
mod error_dialog;
mod estimate_calibration;
mod export_progress;
mod export_progress_panel;
mod failure_gate;
mod gizmo;
mod gizmo_geometry;
pub mod graphics_adapter;
mod graphics_status;
mod hollow_tools;
mod hub;
mod i18n;
mod language;
mod launch_options;
mod license_acceptance;
mod layout_tools;
mod load_progress;
mod load_warnings;
mod material_form;
mod measure;
mod mesh_health;
mod mesh_loader;
pub mod mesh_render;
mod model_tools;
mod onboarding;
mod pending_load;
mod picking;
mod place_on_face;
mod plate_preview;
mod panel_place;
mod preferences_bus;
mod preferences_window;
mod print_estimate;
mod print_history;
mod print_risks;
mod printer_editor;
mod process_settings;
mod product_pictures;
pub mod profile_catalog;
mod profile_extending;
mod profile_forking;
mod profile_transfer;
mod profile_view;
mod project_profiles;
#[path = "../../slicer-cli/src/recipe/service.rs"]
mod recipe_service;
mod recipes;
mod recovery;
mod reloaded_slices;
mod safe_decode;
mod repair_run;
mod resin_colors;
mod save_progress;
mod save_progress_panel;
mod scene_document;
mod scene_export;
mod scene_loader;
mod scene_save;
mod selection;
mod send_to_printer;
mod shortcuts_window;
mod single_instance;
mod slice_cap;
mod slice_export;
mod slice_panel;
mod slice_projection;
mod slice_result;
mod slice_run;
mod slice_selection;
mod slice_state;
mod surface_text;
#[cfg(test)]
mod tests_scene_dirty;
#[cfg(test)]
mod tests_slice_result;
mod text_metrics;
mod theme_sync;
mod thumbnails;
mod verification;
mod viewport;
mod window_icon;
mod window_prefs;
mod wiring;
mod workspace_section;

use std::error::Error;

// Slint's generated code allows the clippy groups by itself, but not
// the rustc lints the workspace denies. Lint attributes are ignored on
// macro invocations, hence the wrapper module.
#[allow(missing_debug_implementations, unreachable_pub, unused_qualifications)]
mod slint_ui {
    slint::include_modules!();
}
pub use slint_ui::*;

use slint::wgpu_29::wgpu;

use crate::app_state::AppState;
use crate::i18n::ctr;

/// Starts the app (`main.rs` only calls this): hands the launch to an
/// instance already running, or sets up the graphics device every window
/// shares, opens the first window with the launch's files, and runs the
/// event loop until the last window closes.
pub fn run() -> Result<(), Box<dyn Error>> {
    // Data from before the app had its own name is copied over once,
    // before anything reads it.
    sc_app_paths::migrate_legacy_dirs();
    let options = launch_options::LaunchOptions::parse(std::env::args_os().skip(1))?;
    let server = match single_instance::begin(&options) {
        single_instance::Launch::Forwarded => return Ok(()),
        single_instance::Launch::Local(server) => server,
    };
    // Explorer can start several launches at once; listening for them
    // before the slow graphics setup lets their files wait for the window.
    let (to_window, handed_over) = std::sync::mpsc::channel();
    let _listener = server.map(|server| server.listen(move |paths| to_window.send(paths).is_ok()));

    let gpu = SharedGpu::open()?;
    let loss = gpu.loss.clone();
    let device_for_exit = gpu.device.clone();
    gpu.give_to_slint()?;

    if !license_acceptance::confirm(&options)? { return Ok(()); }
    let app = AppState::new(loss.clone())?;
    let first = app.open_main_window()?;
    app.install_verification(first, options.clone())?;
    app.open_launch_paths(first, options.open.clone());
    // Automated runs stay on the workspace they drive; a plain launch with
    // nothing to open starts on Home, as Orca does, and the very first one
    // asks for printer, resin and look.
    let automated = options.screenshot.is_some() || options.verify_input.is_some();
    if automated {
        app.hide_recovery_prompt(first);
    } else {
        if options.open.is_empty() {
            app.show_home(first);
        }
        if app.needs_setup() {
            app.show_setup(first);
        }
    }
    let _handover_timer = deliver_handed_over_launches(handed_over);

    match slint::run_event_loop() {
        Ok(()) => {
            app.clean_recovery();
            Ok(())
        }
        Err(loop_error) => Err(explain_loop_exit(loop_error, &loss, &device_for_exit)),
    }
}

/// The adapter, device and queue every window draws with, and the flag a
/// device loss sets.
struct SharedGpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    loss: mesh_render::DeviceLoss,
}

impl SharedGpu {
    /// Picks the adapter (`graphics_adapter`: wgpu's own `request_adapter`
    /// takes whatever the platform lists first, the weaker GPU on some
    /// laptops), opens the device and watches it for loss.
    fn open() -> Result<Self, Box<dyn Error>> {
        let backends = skia_backend();
        let mut description = wgpu::InstanceDescriptor::new_without_display_handle();
        description.backends = backends;
        let instance = wgpu::Instance::new(description);
        let request = wgpu::DeviceDescriptor { label: Some("slicer shared device"), ..Default::default() };
        let chosen = graphics_adapter::open_best_adapter(&instance, backends, &request)?;
        log_adapter_detours(&chosen);
        let graphics_adapter::ChosenAdapter { adapter, device, queue, .. } = chosen;
        mesh_render::log_uncaught_gpu_errors(&device);
        let loss = mesh_render::DeviceLoss::default();
        let flag = loss.clone();
        device.set_device_lost_callback(move |kind, message| {
            let reason = format!("{kind:?}: {message}");
            flag.mark(reason.clone());
            // Fails only once the event loop is gone; `run` looks after that.
            let _ = slint::invoke_from_event_loop(move || AppState::report_device_lost(reason));
        });
        // wgpu-core's SNATCH_LOCK_TRACE thread-local must exist before
        // Slint's context (which will own the queue): thread-locals drop
        // newest first, and a trace first made at the first render would be
        // gone when the queue's drop takes a snatch lock, which panics
        // (https://github.com/gfx-rs/wgpu/issues/8259; debug builds only).
        // A poll now makes it.
        let _ = device.poll(wgpu::PollType::Poll);
        Ok(Self { instance, adapter, device, queue, loss })
    }

    /// Makes Slint draw every window with this device (`Manual`). Letting
    /// Slint open its own (`Automatic`) listed the Vulkan drivers again for
    /// each window, four or five ~225 ms stalls on NVIDIA, and New Window
    /// hung.
    fn give_to_slint(self) -> Result<(), Box<dyn Error>> {
        if let Some(asked) = std::env::var_os("SLINT_BACKEND") {
            eprintln!("Ignoring SLINT_BACKEND={asked:?}: this app always runs Slint's winit backend with Skia.");
        }
        let Self { instance, adapter, device, queue, .. } = self;
        let shared = slint::wgpu_29::WGPUConfiguration::Manual { instance, adapter, device, queue };
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("skia".into())
            .require_wgpu_29(shared)
            .select()?;
        Ok(())
    }
}

/// Opens files from later launches that another instance handed over, as
/// they arrive; they arrive until the returned timer is dropped.
fn deliver_handed_over_launches(handed_over: std::sync::mpsc::Receiver<Vec<std::path::PathBuf>>) -> slint::Timer {
    let timer = slint::Timer::default();
    let every = std::time::Duration::from_millis(25);
    timer.start(slint::TimerMode::Repeated, every, move || {
        for paths in handed_over.try_iter() {
            AppState::receive_launch(paths);
        }
    });
    timer
}

/// The error the app exits with when the event loop failed. A lost device
/// is the likely cause; wgpu only reports one from a poll, so the device is
/// polled once more unless a loss is already known (polling hardware that
/// has gone is a fatal path in wgpu).
fn explain_loop_exit(
    loop_error: slint::PlatformError,
    loss: &mesh_render::DeviceLoss,
    device: &wgpu::Device,
) -> Box<dyn Error> {
    if !loss.is_lost() {
        let _ = device.poll(wgpu::PollType::Poll);
    }
    match loss.reason() {
        Some(reason) => say_device_lost(&reason).into(),
        None => loop_error.into(),
    }
}

/// The single backend Skia accepts on each platform: DX12 on Windows,
/// Vulkan on Linux, Metal on macOS. With every backend enabled the chosen
/// adapter could be one Skia rejects (on Windows: "Unsupported WGPU backend
/// for use with Skia: vulkan"). Same cfgs as i-slint-renderer-skia.
fn skia_backend() -> wgpu::Backends {
    if cfg!(target_vendor = "apple") {
        wgpu::Backends::METAL
    } else if cfg!(target_family = "windows") {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::VULKAN
    }
}

/// Logs better adapters that refused, and a chosen one that only opened on
/// a retry. Either way the session runs on lesser hardware or after a
/// driver hiccup, which shows as nothing but a slower viewport, so a bug
/// report needs it from the log.
fn log_adapter_detours(chosen: &graphics_adapter::ChosenAdapter) {
    for refused in &chosen.passed_over {
        let tries: Vec<String> = refused.try_errors.iter().map(ToString::to_string).collect();
        let tries = tries.join("; on retry: ");
        let why = match &refused.predicted {
            Some(unmet) => format!("does not meet the device profile ({unmet}); it refused: {tries}"),
            None => format!("refused to open a device: {tries}"),
        };
        eprintln!("Skipped graphics adapter {}: {why}", refused.adapter_name);
    }
    if let Some(first) = &chosen.first_try {
        let name = chosen.adapter.get_info().name;
        eprintln!("Graphics adapter {name} opened on the second try; the first failed with: {}", first.refused_with);
    }
}

/// Tells the user the graphics device is gone when no window is left to
/// do it: on stderr and in a native message box (on Linux through
/// `zenity`, when it is installed). Returns the app's exit error.
fn say_device_lost(reason: &str) -> String {
    let title = ctr!("ErrorDialog", "The graphics device was lost").to_string();
    let advice = ctr!(
        "ErrorDialog",
        "The application has to close; restart it once the graphics driver is back."
    );
    let body = format!("{advice} {reason}");
    eprintln!("{title}: {body}");
    let native = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title(title.clone());
    let _ = native.set_description(body).set_buttons(rfd::MessageButtons::Ok).show();
    format!("{title}: {reason}")
}

mod object_transform;

#[cfg(test)]
mod tests_r10;

#[cfg(test)]
mod tests_recipes;

#[cfg(test)]
mod tests_r16_performance;

mod motion_form;

mod calibration;

#[cfg(test)]
mod tests_r21;

#[cfg(test)]
mod tests_r23_theme;

#[cfg(test)]
mod tests_ui_audit;
#[cfg(test)]
mod tests_viewer_input;
#[cfg(test)]
mod tests_fork_extend_dialogs;
#[cfg(test)]
mod tests_message_dialogs;
#[cfg(test)]
mod tests_menu_commands;
#[cfg(test)]
mod tests_window_overlays;
#[cfg(test)]
mod tests_window_prefs;
#[cfg(test)]
mod tests_cap_raster;
