// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Loading a file in the background: reading it, placing it on the plate,
//! checking its meshes, preparing its spatial caches and uploading it to the
//! GPU, all on a worker thread; the result is delivered through the load's
//! ticket. A worker panic becomes an ordinary load error, and the ticket
//! ends the load even if the worker never delivers. The menu, file dialog
//! and checks before a load are in `wiring.rs`.
use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use sc_mesh_formats::DEFAULT_MAX_MESH_FILE_SIZE_BYTES;
use sc_mesh_formats::messages::LoadMeshWarning;
use sc_mesh_spatial::SceneSpatial;
use sc_print_profiles::ProfileLibraries;
use sc_project_formats::{
    ProfileRefWarning, ProjectPayload, ReadProjectWarning, SUPPORTED_SCENE_FILE_EXTENSIONS,
    is_supported_scene_file_extension, load_scene,
};
use sc_scene::Scene;
use slint::wgpu_29::wgpu;

use crate::build_volume::BuildVolume;
use crate::graphics_status::NoGraphics;
use crate::i18n::{ctr, tr};
use crate::load_progress::{OpenStep, LoadProgressSink};
use crate::mesh_health::scene_mesh_health;
use crate::mesh_render::{DeviceLoss, DeviceStamp, UploadedScene};
use crate::project_profiles::{material_installed, printer_installed};
use crate::reloaded_slices::{SavedSlices, check_saved_slices};
use crate::scene_loader::{SceneLoadTicket, LoadedSceneBundle, SceneLoadCoordinator};

pub(crate) const MESH_FILE_BYTE_LIMIT: u64 = DEFAULT_MAX_MESH_FILE_SIZE_BYTES;
pub(crate) const MESH_FILE_MIB_LIMIT: u64 = MESH_FILE_BYTE_LIMIT >> 20;

pub(crate) const OPENABLE_EXTENSIONS: &[&str] = SUPPORTED_SCENE_FILE_EXTENSIONS;

/// Whether the file's extension is one we open, in any case.
#[inline]
pub(crate) fn can_open_extension(path: &Path) -> bool {
    is_supported_scene_file_extension(path)
}

/// Refuses a file over the size limit. A file whose size cannot be read
/// passes: opening it will fail with a clearer error.
pub(crate) fn refuse_oversized_file(path: &Path) -> anyhow::Result<()> {
    let size = match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MESH_FILE_BYTE_LIMIT => meta.len(),
        _ => return Ok(()),
    };
    let message = ctr!(
        "ErrorDialog",
        "File is {} MiB, which exceeds the {} MiB limit.",
        size / (1024 * 1024),
        MESH_FILE_MIB_LIMIT
    );
    Err(anyhow::Error::msg(message.to_string()))
}

/// What to do with a file asked for, given the window's graphics device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OpenVerdict {
    /// There is a device: load now.
    LoadNow,
    /// No device yet, but one may come: keep the file and open it when the
    /// view starts, rather than fail a request the user would have to make
    /// again.
    HoldUntilGpu,
    /// No device, and none will come: say why, rather than keep the file for
    /// the rest of the session.
    CannotLoad(NoGraphics),
}

/// Decides from [`SceneLoader::graphics_unavailable`]. Every way of opening
/// a file (menu, command line, drop) goes through here, so they all behave
/// alike before the window's first frame.
pub(crate) fn open_verdict(unavailable: Option<NoGraphics>) -> OpenVerdict {
    let Some(reason) = unavailable else {
        return OpenVerdict::LoadNow;
    };
    if reason.worth_waiting() {
        OpenVerdict::HoldUntilGpu
    } else {
        OpenVerdict::CannotLoad(reason)
    }
}

/// Starts loading the picked `path` with its ticket: the menu (which takes
/// a ticket before its dialog) and the paths opened directly both end here.
/// A file over the size limit, or no device, delivers an error on the ticket
/// so the UI still sees one finished load. `build_volume` is taken now: the
/// worker places the scene with it, and a later change does not move a load
/// already running.
pub(crate) fn begin_ticketed_load(
    ticket: SceneLoadTicket,
    path: PathBuf,
    loader: &SceneLoadCoordinator,
    build_volume: BuildVolume,
) {
    if let Err(err) = refuse_oversized_file(&path) {
        return ticket.deliver_loaded_scene(path, Err(err));
    }
    let Some((device, device_token, _queue)) = loader.gpu_snapshot() else {
        // Rendering has not started, or stopped since the ticket was taken:
        // say why there is no device, in words the user can act on.
        let reason = loader.unavailable_graphics_reason().unwrap_or(NoGraphics::NotYet);
        let why = anyhow::Error::msg(reason.explain());
        return ticket.deliver_loaded_scene(path, Err(why));
    };
    let (sink, device_loss) = (loader.worker_progress_sink(), loader.lost_device_state());
    // After the early exits, so an error never flashes the progress panel.
    loader.start_load_progress_display();
    load_on_worker(ticket, path, device, device_token, device_loss, build_volume, sink);
}

/// Puts the scene on the plate: centred on the build area, its bottom on
/// the floor, the parts keeping their places relative to each other
/// ([`Scene::drop_at`], which leaves an empty or already placed scene).
pub(crate) fn centre_on_plate(graph: &mut Scene, build_volume: BuildVolume) -> anyhow::Result<()> {
    let (lo, hi) = (build_volume.envelope_min_mm(), build_volume.envelope_max_mm());
    let centre = (lo + hi) * 0.5;
    graph.drop_at([centre.x, centre.y], lo.z)?;
    Ok(())
}

/// Whether the whole scene is inside the build volume; an empty one is.
pub(crate) fn fits_printer(graph: &Scene, build_volume: BuildVolume) -> bool {
    match graph.tight_aabb() {
        Ok(Some(bounds)) => build_volume.holds_box(bounds),
        Ok(None) => true,
        Err(_) => false,
    }
}

/// Keeps a project where it was saved if every plate fits; else centres it,
/// and if that is not enough arranges it over as many plates as needed. The
/// arranging works on a copy, so a part too big for any plate still leaves
/// the centred project. Returns what to tell the user, or nothing.
pub(crate) fn settle_project(
    graph: &mut Scene,
    volume: BuildVolume,
) -> anyhow::Result<String> {
    let fits = |scene: &Scene| {
        scene.plates().order.iter().all(|plate| {
            scene
                .plate_scene(plate.id)
                .is_ok_and(|s| fits_printer(&s, volume))
        })
    };
    if fits(graph) {
        return Ok(String::new());
    }
    let mut centred = graph.clone();
    centre_on_plate(&mut centred, volume)?;
    let message = if fits(&centred) {
        tr!("Project centred on the build plate.")
    } else {
        match crate::layout_tools::arrange_all(&centred, volume, 3.0) {
            Ok(edit) => {
                edit.apply_to_scene(&mut centred)?;
                let plates = centred.plates().order.len();
                tr!(
                    "Project automatically arranged on one plate.",
                    "Project automatically arranged across {n} plates.",
                    plates
                )
            }
            Err(too_big) => tr!(
                "Project centred; one object cannot fit the build volume.",
                "Project centred; {n} objects cannot fit the build volume.",
                too_big.len()
            ),
        }
    };
    *graph = centred;
    Ok(message.to_string())
}

/// What reading one file gives: the scene, its warnings, and any project
/// data (empty for plain meshes).
pub(crate) struct ReadSource {
    pub(crate) scene_read: Scene,
    pub(crate) mesh_notes: Vec<LoadMeshWarning>,
    pub(crate) source_project: Option<ProjectPayload>,
    pub(crate) project_notes: Vec<ReadProjectWarning>,
    pub(crate) missing_profiles: Vec<ProfileRefWarning>,
    /// The project's saved slices, if still valid for the scene.
    pub(crate) saved_slices: Option<SavedSlices>,
}

/// Reads the file at `path`, reporting progress to `sink`. For a project it
/// also, here on the worker (both read the disk): notes the printer and
/// resin profiles it names that are not installed, and drops saved slices
/// that no longer match the scene.
pub(crate) fn read_source(path: &Path, sink: &LoadProgressSink) -> anyhow::Result<ReadSource> {
    let report = |read: sc_project_formats::LoadProgress| {
        sink.set(OpenStep::Parse, read.done_bytes, read.total_bytes)
    };
    let opened = load_scene(path, Some(MESH_FILE_BYTE_LIMIT), report)?;
    // Only a project names profiles; each one not installed is a warning.
    let profile_warnings = opened.payload.as_ref().map_or_else(Vec::new, |payload| {
        let installed = ProfileLibraries::platform_default();
        let printer = |id: &sc_print_primitives::PrinterId| printer_installed(&installed, id);
        let resin = |id: &sc_print_primitives::MaterialId| material_installed(&installed, id);
        payload.profile_ref_warnings(printer, resin)
    });
    let payload = opened.payload;
    let reloaded_slices =
        check_saved_slices(payload.as_ref(), &opened.scene, opened.slices, opened.slice_fingerprint);
    let mut graph = opened.scene;
    if payload.is_none() {
        name_part_after_file(&mut graph, path);
    }
    Ok(ReadSource {
        scene_read: graph,
        mesh_notes: opened.mesh_warnings,
        project_notes: opened.project_warnings,
        source_project: payload,
        missing_profiles: profile_warnings,
        saved_slices: reloaded_slices,
    })
}

/// Names a plain mesh file's (STL, OBJ) single unnamed part after the file:
/// STL headers hold exporter noise ("solid Shape0", "Exported from ..."),
/// the file name is what the user chose. 3MF parts and files of several
/// parts keep their names.
pub(crate) fn name_part_after_file(graph: &mut Scene, path: &Path) {
    let plain = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("stl") || e.eq_ignore_ascii_case("obj"));
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let Ok(leaves) = graph.reachable_leaves(
        sc_scene::CsgPolicy::Descend,
        sc_scene::ReferencePolicy::Skip,
    ) else {
        return;
    };
    let [part] = leaves[..] else {
        return;
    };
    let meta = graph.node(part).and_then(|node| node.meta.clone());
    if !plain || meta.as_ref().is_some_and(|m| m.name.is_some()) {
        return;
    }
    let mut meta = meta.unwrap_or_default();
    meta.name = Some(stem.to_owned());
    let _ = graph.set_node_meta(part, Some(meta));
}

/// When a profiled load (`SLEPTIC_PROFILE_LOAD` set) began.
static PROFILE_LOAD_START: OnceLock<Instant> = OnceLock::new();

/// Logs, once, how long a profiled load took to its first drawn frame.
pub(crate) fn time_first_frame() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if let Some(start) = PROFILE_LOAD_START.get()
        && !REPORTED.swap(true, Ordering::Relaxed)
    {
        eprintln!(
            "R16 first rendered frame: {:.6} s",
            start.elapsed().as_secs_f64()
        );
    }
}

/// Loads `path` on a new thread and delivers the result, or the error (a
/// panic included), through `ticket`.
pub(crate) fn load_on_worker(
    ticket: SceneLoadTicket,
    path: PathBuf,
    device: wgpu::Device,
    device_token: DeviceStamp,
    device_loss: DeviceLoss,
    build_volume: BuildVolume,
    sink: LoadProgressSink,
) {
    if std::env::var_os("SLEPTIC_PROFILE_LOAD").is_some() {
        let _ = PROFILE_LOAD_START.set(Instant::now());
    }
    std::thread::spawn(move || {
        let result = catch_worker_panic(|| {
            let profiling = std::env::var_os("SLEPTIC_PROFILE_LOAD").is_some();
            let timed = |label: &str, started: Instant| {
                if profiling {
                    eprintln!("R16 load {label}: {:.6} s", started.elapsed().as_secs_f64());
                }
            };
            let started = Instant::now();
            let parsed = read_source(&path, &sink)?;
            timed("parse", started);
            let ReadSource {
                mut scene_read,
                mesh_notes: warnings,
                source_project: project_payload,
                project_notes: project_warnings,
                missing_profiles: profile_warnings,
                mut saved_slices,
            } = parsed;
            // Placed before the upload and the caches, which both use the
            // placed coordinates. A project fitting the plate reopens as
            // saved; one laid out for another printer's bed (Bambu, Orca,
            // Prusa use a corner origin) is moved as a whole.
            let is_project = project_payload.is_some()
                || path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("3mf"));
            let placement_message = if is_project {
                settle_project(&mut scene_read, build_volume)?
            } else {
                centre_on_plate(&mut scene_read, build_volume)?;
                String::new()
            };
            // Moved parts no longer match their saved slices.
            if !placement_message.is_empty() {
                saved_slices = None;
            }
            // Mesh health here, so the UI gets it ready; it picks the
            // section the window opens on. Placing moves parts, not their
            // meshes, so it does not change the result.
            let started = Instant::now();
            let mesh_health = scene_mesh_health(&scene_read);
            timed("health", started);
            // The spatial caches too, so the first click or gizmo does not
            // build them on the UI thread.
            let started = Instant::now();
            let primed = |step: sc_mesh_spatial::PrimingProgress| {
                sink.set(OpenStep::Priming, step.primed_triangles, step.total_triangles)
            };
            let spatial = SceneSpatial::primed_graph_with_progress(scene_read, primed);
            timed("spatial", started);
            let started = Instant::now();
            let uploaded = |step: crate::mesh_render::scene_upload::SceneUploadProgress| {
                sink.set(OpenStep::GpuUpload, step.uploaded_triangle_count, step.upload_triangle_total)
            };
            let graph = spatial.graph();
            let gpu_scene =
                UploadedScene::create_gpu_scene_reporting(&device, device_token, graph, &device_loss, uploaded)?;
            timed("GPU", started);
            let source_path = path.clone();
            Ok(LoadedSceneBundle {
                loaded_spatial_index: spatial,
                gpu_scene,
                loaded_mesh_health: mesh_health,
                placement_message,
                loaded_source_path: source_path,
                mesh_load_warnings: warnings,
                loaded_project_data: project_payload,
                loaded_project_warnings: project_warnings,
                missing_profile_warnings: profile_warnings,
                restored_slice_stack: saved_slices,
            })
        });
        ticket.deliver_loaded_scene(path, result);
    });
}

/// Runs `body`, turning a panic into an error with the panic's message.
pub(crate) fn catch_worker_panic<T>(body: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).unwrap_or_else(|payload| {
        let message = panic_text(&payload);
        Err(anyhow::anyhow!("The mesh loader panicked: {message}"))
    })
}

/// A panic payload's message: a string, a boxed error, or a placeholder.
pub(crate) fn panic_text(payload: &Box<dyn Any + Send>) -> String {
    type SyncError = Box<dyn std::error::Error + Send + Sync + 'static>;
    type SendError = Box<dyn std::error::Error + Send + 'static>;
    // `panic!` gives a `&str` or a `String`; `panic_any` can give anything.
    let text = payload
        .downcast_ref::<&'static str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned());
    let error = || {
        let sync = payload.downcast_ref::<SyncError>().map(ToString::to_string);
        sync.or_else(|| payload.downcast_ref::<SendError>().map(ToString::to_string))
    };
    text.or_else(error).unwrap_or_else(|| "unknown panic payload".to_string())
}

#[cfg(test)]
#[path = "tests_mesh_loader.rs"]
mod tests;
