// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! "Export Sliced File" support: pure functions over `&SceneDocument`
//! plus the worker-side write step, mirroring `scene_export.rs`'s
//! shape for the (unrelated) raw-model export flow.
//!
//! This writes the *slice*, not the model: the same contours and
//! layer schedule the Slice panel already computed and is holding.
//! Debug SVG/PNG exports need geometry alone. Printer ZIP exports additionally
//! snapshot resolved process settings and render their job script on the worker.

use std::path::Path;

use anyhow::Result;
use sc_sl1cer::VERSION;
use sc_sl1cer::contour::ContourStack;
use sc_sla_formats::{Format, WriterOptions, write_slice};

use crate::scene_document::SceneDocument;

/// Snapshots the held slice's merged contours and builds the writer
/// options from the exact profile they were produced under. `None`
/// when no slice is held (the caller gates the menu item on this
/// same condition, but a stray callback could still race a scene
/// close).
pub(crate) fn prepare_slice_export(doc: &SceneDocument) -> Option<(ContourStack, WriterOptions)> {
    let (merged, profile) = doc.slice_export_data()?;
    let options = WriterOptions::from_profile(&profile, VERSION);
    Some((merged, options))
}

/// Writes a snapshotted slice to `target` in `format`. Pure with
/// respect to the document, so it runs on a worker thread; the caller
/// surfaces the error on the UI thread. `sc_sla_formats::write_slice`
/// has no incremental progress signal to report (unlike the mesh
/// exporter), so there is no progress callback here either -- showing
/// a fabricated percentage would be worse than showing none.
pub(crate) fn write_slice_export(
    stack: &ContourStack,
    target: &Path,
    format: Format,
    options: &WriterOptions,
) -> Result<()> {
    write_slice(stack, target, format, options).map_err(anyhow::Error::from)
}

/// Process settings snapshotted before dispatching export to a worker.
#[derive(Debug)]
pub(crate) struct JobExportSettings {
    config: sc_print_profiles::ResolvedPrintConfig,
    pub(crate) regions: Vec<sc_print_glue::calibration::Region>,
    density: Option<f64>,
    price: Option<f64>,
}

pub(crate) fn prepare_job_export(doc: &SceneDocument) -> Result<JobExportSettings> {
    let params = doc
        .slicing_inputs()
        .ok_or_else(|| anyhow::anyhow!("No slice parameters"))?;
    let mut settings = prepare_job_export_for(&params, &doc.process_overrides.borrow())?;
    settings.regions = doc
        .read_scene(|s| {
            let scene = s
                .graph()
                .plate_scene(doc.active_plate())
                .map_err(|e| e.to_string())?;
            sc_print_glue::calibration::regions(&scene).map_err(crate::calibration::error_message)
        })
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?
        .unwrap_or_default();
    Ok(settings)
}

pub(crate) fn prepare_job_export_for(
    params: &crate::slice_result::SliceParams,
    overrides: &sc_print_profiles::SceneOverrides,
) -> Result<JobExportSettings> {
    let (printer, material) =
        crate::slice_projection::load_chosen_profiles(&params.printer, &params.resin)?;
    let config = sc_print_profiles::resolve(&printer, &material, overrides)?;
    Ok(JobExportSettings {
        regions: Vec::new(),
        config,
        density: material.defaults().density_g_ml,
        price: material.defaults().price_per_liter.map(|p| p.value()),
    })
}

/// Raster integration and script rendering run on the export worker.
pub(crate) fn build_job_export(
    settings: &JobExportSettings,
    stack: &ContourStack,
    options: &WriterOptions,
) -> Result<sc_sla_formats::PreparedJob> {
    let profile = sc_sl1cer::profile::SliceProfile::new(
        *options.build_volume(),
        *options.pixel_resolution(),
        *options.layer_schedule(),
        options.mirror(),
        options.aa(),
        sc_sl1cer::profile::PackingMode::None,
    )
    .with_calibration(options.calibration());
    let mut schedule = sc_print_job::ExposureSchedule::new(
        &settings.config.material_resolved,
        profile.layer_schedule(),
    );
    let exposures: Vec<Vec<f64>> = settings
        .regions
        .iter()
        .map(|r| {
            let mut material = settings.config.material_resolved.clone();
            material.normal_exposure_sec =
                sc_print_primitives::PositiveFiniteSec::new(r.exposure_s)
                    .expect("validated calibration exposure");
            sc_print_job::ExposureSchedule::new(&material, profile.layer_schedule())
                .layers
                .iter()
                .map(|l| l.exposure_s)
                .collect()
        })
        .collect();
    if !exposures.is_empty() {
        for (i, layer) in schedule.layers.iter_mut().enumerate() {
            layer.exposure_s = exposures.iter().map(|v| v[i]).sum();
        }
    }
    let estimate = sc_print_job::PrintEstimate::new(
        &schedule,
        sc_print_job::raster_volume_ml(stack, &profile),
        settings.density,
        settings.price,
    );
    for diagnostic in &estimate.diagnostics {
        eprintln!("warning: {diagnostic}");
    }
    let script = if exposures.is_empty() {
        sc_print_job::render_job(&settings.config, &schedule, &estimate)?
    } else {
        sc_print_job::render_multi_job(&settings.config, &schedule, &estimate, &exposures)?
    };
    Ok(sc_sla_formats::PreparedJob {
        regions: settings.regions.iter().map(|r| (r.min, r.max)).collect(),
        layer_z_mm: schedule.layers.iter().map(|l| l.z_mm).collect(),
        exposure_s: schedule.layers.iter().map(|l| l.exposure_s).collect(),
        script,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sc_mesh_core::transform::Transform3D;
    use sc_scene::Scene;
    use sc_scene::node::LeafPayload;
    use sc_sl1cer::contour::{ClosedRing, ContourLayer, Polygon};
    use sc_sl1cer::geom2d::Point2D;
    use sc_sl1cer::profile::{
        AaLevel, BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
    };
    use tempfile::tempdir;

    use super::*;
    use crate::slice_result::{SliceParams, SliceResult};
    use sc_mesh_fixtures::unit_cube_mesh;
    use sc_print_primitives::{MaterialId, PrinterId};

    fn installed_document() -> std::rc::Rc<SceneDocument> {
        let doc = SceneDocument::new();
        let mut scene = Scene::new();
        let leaf = scene
            .add_leaf(
                None,
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(unit_cube_mesh())),
            )
            .expect("add leaf");
        scene.add_root(leaf).expect("leaf becomes root");
        doc.take_mesh_scene(scene, std::path::PathBuf::from("/tmp/cube.stl"));
        doc
    }

    fn sample_stack() -> ContourStack {
        let outer = ClosedRing::new(
            [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
                .into_iter()
                .map(|(x, y)| Point2D::new(x, y))
                .collect(),
        );
        let square = Polygon::new(outer, vec![]).expect("a CCW square is valid");
        ContourStack::new(vec![ContourLayer::new(0.05, vec![square])])
            .expect("a one-layer stack is valid")
    }

    fn sample_profile() -> SliceProfile {
        SliceProfile::new(
            BuildVolume::new(60.0, 60.0, 100.0).expect("valid build volume"),
            PixelResolution::new(1920, 1080).expect("valid resolution"),
            LayerSchedule::uniform(0.05, 0.05, 1).expect("a positive, non-empty schedule"),
            Mirror::default(),
            AaLevel::Off,
            PackingMode::None,
        )
    }

    fn hold_a_slice(doc: &SceneDocument) {
        doc.keep_slice(SliceResult {
            merged: sample_stack(),
            schedule: *sample_profile().layer_schedule(),
            token: "test-token".to_string(),
            params: SliceParams {
                printer: PrinterId::new("elegoo-saturn-4-ultra").expect("valid id"),
                resin: MaterialId::new("standard-405").expect("valid id"),
                first_layer_height: None,
                aa_level: AaLevel::Off,
                geometry: doc.revision(),
            },
            profile: sample_profile(),
        });
    }

    #[test]
    fn prepare_slice_export_is_none_without_a_held_slice() {
        let doc = installed_document();
        assert!(prepare_slice_export(&doc).is_none());
    }

    #[test]
    fn prepare_slice_export_returns_the_held_stack_and_options() {
        let doc = installed_document();
        hold_a_slice(&doc);
        let (stack, options) = prepare_slice_export(&doc).expect("a slice is held");
        assert_eq!(stack.layers().len(), 1);
        assert_eq!(options.layer_schedule(), sample_profile().layer_schedule());
        assert_eq!(options.slicer_version(), VERSION);
    }

    #[test]
    fn sliced_project_export_contains_only_the_requested_plate() {
        let doc = installed_document();
        let source = doc.read_scene(|s| s.graph().clone()).unwrap();
        doc.add_plate();
        doc.import_geometry(
            &[source],
            crate::build_volume::BuildVolume::new(60.0, 60.0, 100.0),
        )
        .unwrap();
        hold_a_slice(&doc);
        let (index, result) = doc.all_plate_slices().into_iter().next().unwrap();
        assert_eq!(index, 2);
        let temporary = tempdir().unwrap();
        let target = temporary.path().join("project_plate2.3mf");
        let bundle =
            crate::scene_save::ProjectSnapshot::of_plate(&doc, index, &result, target.clone()).unwrap();
        bundle.write(|_, _, _| {}).unwrap();
        let loaded = sc_project_formats::load_scene(&target, None, |_| {}).unwrap();
        assert_eq!(loaded.scene.plates().order.len(), 1);
        assert_eq!(loaded.scene.plates().order[0].id, 2);
        assert_eq!(
            loaded
                .scene
                .reachable_leaves(
                    sc_scene::CsgPolicy::Descend,
                    sc_scene::ReferencePolicy::Skip
                )
                .unwrap()
                .len(),
            1
        );
        assert!(loaded.slices.is_some());
        assert_eq!(doc.read_scene(|s| s.leaf_ids().len()), Some(2));
    }

    #[test]
    fn write_slice_export_writes_a_combined_svg() {
        let doc = installed_document();
        hold_a_slice(&doc);
        let (stack, options) = prepare_slice_export(&doc).expect("a slice is held");

        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("plate.svg");
        write_slice_export(&stack, &target, Format::SvgCombined, &options)
            .expect("the write succeeds");
        assert!(target.is_file());
        assert!(std::fs::metadata(&target).unwrap().len() > 0);
    }

    #[test]
    fn write_slice_export_writes_png_layers_into_a_directory() {
        let doc = installed_document();
        hold_a_slice(&doc);
        let (stack, options) = prepare_slice_export(&doc).expect("a slice is held");

        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("layers");
        write_slice_export(&stack, &target, Format::PngPerLayer, &options)
            .expect("the write succeeds");
        assert!(target.is_dir());
        let entries: Vec<_> = std::fs::read_dir(&target).unwrap().collect();
        assert_eq!(entries.len(), 1, "one layer should produce one PNG file");
    }

    #[test]
    fn write_slice_export_surfaces_the_underlying_writer_error() {
        // ThreeMf cannot be written from a merged stack; the merged-
        // stack entry point must refuse it with a real error, not
        // panic or silently no-op.
        let doc = installed_document();
        hold_a_slice(&doc);
        let (stack, options) = prepare_slice_export(&doc).expect("a slice is held");

        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("plate.slices.3mf");
        let err = write_slice_export(&stack, &target, Format::ThreeMf, &options).unwrap_err();
        assert!(
            err.to_string().contains("merged contour stack"),
            "err: {err}"
        );
    }
    #[test]
    fn gui_job_export_uses_held_geometry_grid_and_shared_estimates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles");
        let printer = sc_print_profiles::load_printer_profile_from_path(
            &root.join("printers/sleptic-diy-msla.toml"),
        )
        .unwrap();
        let material = sc_print_profiles::EffectiveMaterial::from(
            sc_print_profiles::load_material_profile_from_path(
                &root.join("materials/standard-405.toml"),
            )
            .unwrap(),
        );
        let config = sc_print_profiles::resolve(
            &printer,
            &material,
            &sc_print_profiles::SceneOverrides::default(),
        )
        .unwrap();
        let settings = JobExportSettings {
            regions: Vec::new(),
            config,
            density: Some(1.2),
            price: Some(40.0),
        };
        let options = WriterOptions::from_profile(&sample_profile(), VERSION);
        let job = build_job_export(&settings, &sample_stack(), &options).unwrap();
        assert_eq!(job.layer_z_mm, vec![0.05]);
        assert_eq!(job.exposure_s, vec![30.0]);
        assert!(job.script.contains(";LAYER_START:1"));
        let dir = tempdir().unwrap();
        let target = dir.path().join("gui.zip");
        sc_sla_formats::write_gcode_zip(&sample_stack(), &target, &options, &job).unwrap();
        assert!(target.is_file());
    }
}
