// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `slice`: load a model or project, optionally repair, place, run a recipe
//! and add supports, then slice one plate (or every plate) and write the job.
//!
//! Results go to stdout, warnings and failures to stderr; `messages` holds
//! every line either stream gets.
mod messages;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use sc_mesh_core::indexed_mesh::RepairPasses;
use sc_mesh_formats::DEFAULT_MAX_MESH_FILE_SIZE_BYTES;
use sc_print_glue::{SlicePlateError, SliceRun, SlicingParams};
use sc_print_primitives::{AaLevel, BuildVolume, PositiveFiniteMm};
use sc_print_profiles::scene_overrides::PlateSettings;
use sc_print_profiles::support_settings::SupportSettings;
use sc_print_profiles::{
    EffectiveMaterial, MaterialExtensionIssue, PrinterProfile, ProfileLibraries,
    load_material_profile_from_path, load_printer_profile_from_path,
};
use sc_project_formats::load_scene;
use sc_scene::Scene;
use sc_sl1cer::error::SliceSceneError;
use sc_sl1cer::{CancelToken, VERSION};
use sc_sla_formats::format::format_from_path;
use sc_sla_formats::{Format, WriterOptions, write_scene_slices, write_slice};

use crate::config::profile_ref::{MaterialProfileRef, PrinterProfileRef};
use crate::filesystem::{
    display_path, ensure_distinct_files, validate_sourcefile_path, validate_targetfile_path,
};

/// Everything `slice` was asked to do. Profile references were sorted into
/// paths and ids by the parser; lengths are positive by type.
#[derive(Clone, Debug)]
pub(crate) struct SliceArgs {
    pub recipe: Option<String>,
    pub plate: Option<usize>,
    pub all_plates: bool,
    pub source: OsString,
    pub target: OsString,
    pub target_format: Option<Format>,
    pub printer_profile: PrinterProfileRef,
    pub material_profile: MaterialProfileRef,
    pub layer_height: Option<PositiveFiniteMm>,
    pub first_layer_height: Option<PositiveFiniteMm>,
    pub aa: AaLevel,
    pub verbose: bool,
    pub no_auto_place: bool,
    pub repair: bool,
    pub islands: bool,
    pub risks: bool,
    pub minimum_feature_mm: PositiveFiniteMm,
    pub supports: bool,
    pub support_settings: SupportSettings,
}

pub(crate) fn slice(args: SliceArgs, libraries: &ProfileLibraries) -> Result<()> {
    if args.all_plates {
        every_plate(args, libraries)
    } else {
        one_plate(args, libraries)
    }
}

/// `--all-plates`: each plate of a project becomes its own job in the target
/// folder, named `<project>_plate<N>.<ext>`.
fn every_plate(args: SliceArgs, libraries: &ProfileLibraries) -> Result<()> {
    if args.plate.is_some() {
        bail!("--plate and --all-plates cannot be combined");
    }
    let source = PathBuf::from(&args.source);
    let project = load_scene(&source, Some(DEFAULT_MAX_MESH_FILE_SIZE_BYTES), |_| {})?;
    if project.payload.is_none() {
        bail!("--all-plates requires a project file");
    }
    let folder = PathBuf::from(&args.target);
    std::fs::create_dir_all(&folder)?;
    let Some(format) = args.target_format else {
        bail!("--all-plates requires --target-format");
    };
    let extension = match format {
        Format::ThreeMf => "3mf",
        Format::SvgCombined => "svg",
        Format::GcodeZip => "zip",
        Format::PngPerLayer | Format::SvgPerLayer => "layers",
    };
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let plates = project.scene.plates().order.len();
    let started = Instant::now();
    for number in 1..=plates {
        let plate_started = Instant::now();
        one_plate(
            SliceArgs {
                all_plates: false,
                plate: Some(number),
                target: folder
                    .join(format!("{stem}_plate{number}.{extension}"))
                    .into_os_string(),
                ..args.clone()
            },
            libraries,
        )?;
        println!(
            "Plate {number}/{plates}: {:.3} s",
            plate_started.elapsed().as_secs_f64()
        );
    }
    println!("All plates: {:.3} s", started.elapsed().as_secs_f64());
    Ok(())
}

/// The format to write and the checked place to write it.
fn output(args: &SliceArgs, source: &Path) -> Result<(Format, PathBuf)> {
    let asked = PathBuf::from(&args.target);
    let format = match args.target_format.or_else(|| format_from_path(&asked)) {
        Some(format) => format,
        None => bail!(
            "could not infer target format from path \"{}\"; pass --target-format with one of: svg-per-layer, svg-combined, png-per-layer, 3mf, gcode-zip",
            display_path(&asked),
        ),
    };
    // A folder format's writer insists on an empty folder, which already
    // keeps it off the source. A file format would truncate the target, and
    // the source is fully read by then, so it must not be the same file.
    if format.is_directory_format() {
        return Ok((format, asked));
    }
    let target = validate_targetfile_path(asked)?;
    ensure_distinct_files(source, &target)?;
    Ok((format, target))
}

fn one_plate(args: SliceArgs, libraries: &ProfileLibraries) -> Result<()> {
    let source = validate_sourcefile_path(PathBuf::from(&args.source))?;
    let (format, target) = output(&args, &source)?;
    let printer = printer(&args.printer_profile, libraries)?;
    let (material, issues) = material(&args.material_profile, libraries)?;
    for issue in &issues {
        eprintln!("warning: {issue}");
    }

    let opened = load_scene(&source, Some(DEFAULT_MAX_MESH_FILE_SIZE_BYTES), |_| {})?;
    let stderr = &mut std::io::stderr();
    crate::commands::report::display_warnings(&opened.mesh_warnings, stderr);
    messages::project_warnings(&opened.project_warnings, stderr);
    let is_project = opened.payload.is_some();
    let plates = &opened.scene.plates().order;
    let number = args.plate.unwrap_or(1);
    let Some(plate) = number.checked_sub(1).and_then(|at| plates.get(at)) else {
        bail!("plate index {number} is outside 1..={}", plates.len());
    };
    let plate_id = plate.id;
    // Settings saved with the plate win over the flags they overlap.
    let saved = plate
        .settings
        .as_deref()
        .and_then(|text| PlateSettings::parse(text).ok())
        .unwrap_or_default();
    let mut scene = opened.scene.plate_scene(plate_id)?;

    if args.repair {
        // Refusals only warn: slicing's own validation decides what is sliceable.
        let outcomes = scene.repair_meshes(RepairPasses::default_repair());
        messages::repair_summary(&outcomes, args.verbose, stderr);
    }
    // Where a project's parts sit is part of the project; a bare mesh is
    // centred on the plate unless told otherwise.
    if !is_project && !args.no_auto_place {
        place_on_plate(&mut scene, &printer.hardware.build_volume())?;
    }
    let params = SlicingParams {
        process_overrides: saved.process,
        layer_height_request: args.layer_height,
        first_layer_height: args.first_layer_height.or(saved.first_layer_height),
        aa_level: saved.aa.unwrap_or(args.aa),
    };
    if let Some(recipe) = &args.recipe {
        run_recipe(&mut scene, recipe, &source, &printer, &material, params)?;
    }
    let mut supported = None;
    let mut support_count = None;
    if args.supports {
        let (count, analysed) = add_supports(
            &mut scene,
            &args.support_settings,
            &printer,
            &material,
            params,
            args.repair,
        )?;
        support_count = Some(count);
        supported = Some(analysed);
    }

    let started = Instant::now();
    let sliced = match sc_print_glue::PlateJob::new(&scene, &printer, &material, &params).slice(&CancelToken::new(), |_| {}, args.repair) {
        Ok(SliceRun::Completed(sliced)) => sliced,
        // The token above is never cancelled.
        Ok(SliceRun::Cancelled) => bail!("slicing was cancelled"),
        Err(SlicePlateError::Prepare(why)) => return Err(messages::unsliceable(why)),
        Err(SlicePlateError::Engine(error @ SliceSceneError::LeavesValidationFailed(_))) => {
            messages::validation_failure(&error, args.verbose, stderr);
            bail!("mesh validation failed; slicing aborted");
        }
        Err(SlicePlateError::Engine(other)) => return Err(other.into()),
    };
    println!("Slice geometry: {:.6} s", started.elapsed().as_secs_f64());
    for leaf in sliced.leaves().iter().filter(|leaf| leaf.tolerant()) {
        let name = match leaf.leaf_name() {
            Some(name) => name.to_owned(),
            None => format!("leaf #{}", leaf.leaf().get()),
        };
        eprintln!(
            "warning: tolerant slicing used for {name}; dropped fragments: {}",
            leaf.dropped_fragments()
        );
    }
    // All stacks share one layer grid and merging only joins each layer's
    // polygons, so these counts hold for every writer.
    let models = sliced.leaves().len();
    let layers = sliced
        .leaves()
        .first()
        .map_or(0, |leaf| leaf.stack().layers().len());
    let polygons: usize = sliced
        .leaves()
        .iter()
        .flat_map(|leaf| leaf.stack().layers())
        .map(|layer| layer.polygons().len())
        .sum();
    let profile = sliced.profile().clone();
    // The support pass already analysed this scene; its result stands only
    // if the final slice uses exactly the same layers, pixels and AA.
    let analysis = supported
        .and_then(|(analysed, analysis)| (analysed == profile).then_some(analysis));
    let placement_risks = if args.risks {
        sc_print_glue::scene_placement_risks(&scene, sliced.leaves(), &profile)?
    } else {
        Vec::new()
    };

    let mut options = WriterOptions::from_profile(&profile, VERSION);
    if let Some(mesh) = scene.merge_meshes()? {
        options = options.with_preview_mesh(&mesh);
    }
    // Resolved now, used after the raster work: geometry-only formats still
    // export when the resin has no full calibration.
    let resolved = sc_print_profiles::resolve(&printer, &material, &saved.process);
    let calibration = sc_print_glue::calibration::regions(&scene).map_err(|e| anyhow!(e))?;
    if !calibration.is_empty() && format != Format::GcodeZip {
        bail!(
            "Exposure calibration requires PNG + run.gcode (G-code ZIP); this format cannot express sequential exposures"
        );
    }
    if format == Format::ThreeMf {
        write_scene_slices(&scene, sliced.leaves(), &target, &options)?;
    }
    let stack = sliced
        .merge()
        .map_err(|err| anyhow!("layer grid mismatch: {err:?}"))?;
    let analysis = if args.risks {
        // The support pass measures islands and volume only.
        let settings = sc_print_job::RiskSettings {
            minimum_feature_mm: args.minimum_feature_mm.value(),
            ..Default::default()
        };
        let analysis = sc_print_job::analyze_layers_with_settings(&stack, &profile, settings);
        let mut risks = analysis.risks.clone();
        risks.extend(placement_risks);
        messages::risks(risks);
        Some(analysis)
    } else {
        analysis
    };

    let process = match resolved {
        Ok(config) => {
            let (schedule, _) = sc_print_glue::calibration::schedule(
                &config.material_resolved,
                profile.layer_schedule(),
                &calibration,
            );
            let volume_ml = match &analysis {
                Some(analysis) => analysis.volume_ml,
                None => sc_print_job::raster_volume_ml(&stack, &profile),
            };
            let defaults = material.defaults();
            let estimate = sc_print_job::PrintEstimate::new(
                &schedule,
                volume_ml,
                defaults.density_g_ml,
                defaults.price_per_liter.map(|price| price.value()),
            );
            for diagnostic in &estimate.diagnostics {
                eprintln!("warning: {diagnostic}");
            }
            Some((config, schedule, estimate))
        }
        // Only the G-code job needs exposures and lifts.
        Err(err) if format == Format::GcodeZip => return Err(err.into()),
        Err(err) => {
            eprintln!("warning: print estimate unavailable: {err}");
            None
        }
    };
    match (format, &process) {
        (Format::GcodeZip, Some((config, schedule, estimate))) => {
            let (_, exposures) = sc_print_glue::calibration::schedule(
                &config.material_resolved,
                profile.layer_schedule(),
                &calibration,
            );
            let script = if exposures.is_empty() {
                sc_print_job::render_job(config, schedule, estimate)?
            } else {
                sc_print_job::render_multi_job(config, schedule, estimate, &exposures)?
            };
            let job = sc_sla_formats::PreparedJob {
                regions: calibration.iter().map(|region| (region.min, region.max)).collect(),
                layer_z_mm: schedule.layers.iter().map(|layer| layer.z_mm).collect(),
                exposure_s: schedule.layers.iter().map(|layer| layer.exposure_s).collect(),
                script,
            };
            sc_sla_formats::write_gcode_zip(&stack, &target, &options, &job)?;
        }
        (Format::GcodeZip, None) => unreachable!("a G-code job returned early without a process"),
        (Format::ThreeMf, _) => {}
        (other, _) => write_slice(&stack, &target, other, &options)?,
    }
    if let Some((config, schedule, estimate)) = &process {
        println!("{}", estimate.summary(layers));
        println!("Exposure: {}", schedule.summary(&config.material_resolved));
    }
    if let Some(count) = support_count {
        println!("Supports: {count}");
    }
    if args.islands {
        let islands = match analysis {
            Some(analysis) => analysis.islands,
            None => sc_print_job::find_islands(&stack, &profile),
        };
        messages::islands(&islands);
    }
    println!(
        "Sliced {models} {}: {layers} layers ({polygons} polygons total) to {}",
        if models == 1 { "model" } else { "models" },
        display_path(&target),
    );
    Ok(())
}

fn printer(reference: &PrinterProfileRef, libraries: &ProfileLibraries) -> Result<PrinterProfile> {
    match reference {
        PrinterProfileRef::Path(path) => load_printer_profile_from_path(path).map_err(|e| e.to_string()),
        PrinterProfileRef::Id(id) => libraries.load_printer_by_id(id).map_err(|e| e.to_string()),
    }
    .map_err(|why| anyhow!("could not load the printer profile: {why}"))
}

/// The reference names the base resin; a same-id extension in the user
/// library is applied on top either way.
fn material(
    reference: &MaterialProfileRef,
    libraries: &ProfileLibraries,
) -> Result<(EffectiveMaterial, Vec<MaterialExtensionIssue>)> {
    match reference {
        MaterialProfileRef::Path(path) => load_material_profile_from_path(path)
            .map(|base| libraries.compose_with_extension(base))
            .map_err(|e| e.to_string()),
        MaterialProfileRef::Id(id) => libraries
            .effective_material_by_id(id)
            .map_err(|e| e.to_string()),
    }
    .map_err(|why| anyhow!("could not load the material profile: {why}"))
}

/// Centres the scene on the printer's plate with its lowest point on the
/// floor, keeping the parts' layout relative to each other.
fn place_on_plate(scene: &mut Scene, volume: &BuildVolume) -> Result<()> {
    let [x0, y0, floor] = volume.envelope_min_mm();
    let [x1, y1, _] = volume.envelope_max_mm();
    scene.drop_at(
        [((x0 + x1) * 0.5) as f32, ((y0 + y1) * 0.5) as f32],
        floor as f32,
    )?;
    Ok(())
}

fn run_recipe(
    scene: &mut Scene,
    recipe: &str,
    source: &Path,
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
    params: SlicingParams,
) -> Result<()> {
    let recipe = sc_print_profiles::recipes::load(recipe).map_err(|e| anyhow!(e))?;
    let targets =
        scene.reachable_leaves(sc_scene::CsgPolicy::Refuse, sc_scene::ReferencePolicy::Skip)?;
    let job_name = source.file_stem().unwrap_or_default().to_string_lossy();
    let context = crate::recipe_service::Context {
        printer,
        material,
        params,
        job_name: &job_name,
    };
    let prepared = crate::recipe_service::run(
        scene,
        &targets,
        &recipe,
        &context,
        &CancelToken::new(),
        |_, message| eprintln!("{message}"),
    )
    .map_err(|e| anyhow!(e))?;
    println!("{}", prepared.summary);
    // A recipe's own slice step is not reused: the export slices the final
    // scene with the command line's settings.
    drop(prepared.slices);
    // The recipe's edit refers to node slots the prepared scene allocated.
    while scene.nodes_len() < prepared.slots {
        scene.allocate_unbound_node_id();
    }
    prepared.edit.apply_to_scene(scene)?;
    debug_assert_eq!(scene.nodes_len(), prepared.scene.nodes_len());
    Ok(())
}

/// Adds automatic supports and reports each model's count. Returns the
/// number of contacts and the profile and analysis of the final check, which
/// the export reuses when it slices on the same grid.
fn add_supports(
    scene: &mut Scene,
    settings: &SupportSettings,
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
    params: SlicingParams,
    repair: bool,
) -> Result<(usize, (sc_sl1cer::profile::SliceProfile, sc_print_job::LayerAnalysis))> {
    // The grid comes from the lifted, supported geometry, so the check below
    // sees the whole model on the same layers as the export.
    let (lifted, _) = sc_print_glue::supports::geometric_supported_scene(scene, settings)?;
    let profile = sc_print_glue::slice_profile_for_selection(
        &lifted,
        printer,
        material,
        params.layer_height_request,
        params.first_layer_height,
        params.aa_level,
        sc_sl1cer::profile::PackingMode::None,
    )
    .map_err(messages::unsliceable)?;
    let (supported, structure, analysis) =
        sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
            scene, settings, &profile, repair,
        )?;
    if !analysis.islands.is_empty() {
        eprintln!(
            "warning: {} islands remain after three support passes",
            analysis.islands.len()
        );
    }
    for model in sc_print_glue::supports::model_results(&supported, &structure, settings) {
        if model.direct {
            println!("{}: prints directly on the plate, no supports needed", model.name);
        } else {
            println!("{}: {} supports", model.name, model.contacts);
        }
    }
    *scene = supported;
    Ok((structure.points.len(), (profile, analysis)))
}

#[cfg(test)]
#[path = "tests_slice.rs"]
mod tests;
