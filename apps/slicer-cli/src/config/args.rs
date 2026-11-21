// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
// Clean-room implementation from specification, 2026-11-20.

use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;

use bpaf::{command, construct, long, positional, pure, short, OptionParser, Parser};
use sc_mesh_core::indexed_mesh::RepairPasses;
use sc_mesh_formats::filetypes::MeshFileType;

use sc_print_primitives::{AaLevel, PositiveFiniteMm};
use sc_print_profiles::support_settings::SupportSettings;
use sc_sla_formats::Format;
use crate::commands::slice::SliceArgs;
use crate::config::profile_ref::{MaterialProfileRef, PrinterProfileRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListKind {
    All,
    Printers,
    Materials,
}

impl FromStr for ListKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "printers" => Ok(ListKind::Printers),
            "materials" => Ok(ListKind::Materials),
            word => Err(format!("expected 'printers' or 'materials', got '{word}'")),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    Convert {
        source: OsString,
        source_format: Option<MeshFileType>,
        target: OsString,
        target_format: Option<MeshFileType>,
    },
    Diagnose {
        source: OsString,
        source_format: Option<MeshFileType>,
    },
    Inspect {
        source: OsString,
        source_format: Option<MeshFileType>,
    },
    Fix {
        source: OsString,
        source_format: Option<MeshFileType>,
        target: OsString,
        target_format: Option<MeshFileType>,
        passes: RepairPasses,
    },
    Help,
    Version,
    Slice(Box<SliceArgs>),
    ProfilesExport {
        output: OsString,
        printers: bool,
        materials: bool,
        ids: Vec<String>,
    },
    ProfilesImport {
        input: OsString,
        force: bool,
    },
    ProfilesList {
        kind: ListKind,
    },
}

fn file_in(short_c: char, long_s: &'static str, help_s: &'static str, meta: &'static str) -> impl Parser<OsString> {
    short(short_c).long(long_s).help(help_s).argument::<PathBuf>(meta).map(PathBuf::into_os_string)
}

fn convert_parser() -> impl Parser<Command> {
    let source = file_in('s', "source", "Input mesh filepath", "SOURCE_FILEPATH");
    let source_format = long("source-format").help("Override input mesh format").argument::<MeshFileType>("SOURCE_FORMAT").optional();
    let target = file_in('t', "target", "Output mesh filepath", "TARGET_FILEPATH");
    let target_format = long("target-format").help("Override output mesh format").argument::<MeshFileType>("TARGET_FORMAT").optional();
    construct!(Command::Convert { source, source_format, target, target_format })
}

fn diagnose_parser() -> impl Parser<Command> {
    let source = file_in('s', "source", "Mesh file to diagnose", "SOURCE_FILEPATH");
    let source_format = long("source-format").help("Override input mesh format").argument::<MeshFileType>("SOURCE_FORMAT").optional();
    construct!(Command::Diagnose { source, source_format })
}

fn inspect_parser() -> impl Parser<Command> {
    let source = file_in('s', "source", "Mesh file to inspect", "SOURCE_FILEPATH");
    let source_format = long("source-format").help("Override input mesh format").argument::<MeshFileType>("SOURCE_FORMAT").optional();
    construct!(Command::Inspect { source, source_format })
}

fn fix_parser() -> impl Parser<Command> {
    let source = file_in('s', "source", "Input mesh to repair", "SOURCE_FILEPATH");
    let source_format = long("source-format").help("Override input format").argument::<MeshFileType>("SOURCE_FORMAT").optional();
    let target = file_in('t', "target", "Output repaired mesh", "TARGET_FILEPATH");
    let target_format = long("target-format").help("Override output format").argument::<MeshFileType>("TARGET_FORMAT").optional();

    let fix_zero_length_edges = long("fix-zero-length-edges").help("Remove degenerate faces").argument::<bool>("BOOL").fallback(true);
    let fix_flipped_faces = long("fix-flipped-faces").help("Correct face orientation").argument::<bool>("BOOL").fallback(true);
    let fix_face_normals = long("fix-face-normals").help("Recalculate normals").argument::<bool>("BOOL").fallback(false);

    let passes = construct!(fix_zero_length_edges, fix_flipped_faces, fix_face_normals)
        .map(|(zero_length_edges, flipped_faces, face_normals)| RepairPasses {
            zero_length_edges,
            flipped_faces,
            face_normals,
            separate_shells: true,
        });

    construct!(Command::Fix { source, source_format, target, target_format, passes })
}

fn profiles_parser() -> impl Parser<Command> {
    let list_kind = positional::<ListKind>("KIND").help("Profile kind ('printers' or 'materials')").fallback(ListKind::All);
    let list_cmd = command("list", list_kind.map(|kind| Command::ProfilesList { kind }).to_options().descr("List profiles"));

    let export_output = positional::<PathBuf>("OUTPUT_ZIP").help("Destination ZIP archive").map(PathBuf::into_os_string);
    let export_printers = long("printers").help("Export printer profiles").switch();
    let export_materials = long("materials").help("Export material profiles").switch();
    let export_ids = long("id").help("Profile ID to export").argument::<String>("ID").many();
    let export_cmd = command(
        "export",
        construct!(export_printers, export_materials, export_ids, export_output).map(|(printers, materials, ids, output)| Command::ProfilesExport { output, printers, materials, ids })
            .to_options()
            .descr("Export profiles to ZIP"),
    );

    let import_input = positional::<PathBuf>("PACKAGE_ZIP").help("Source package ZIP archive").map(PathBuf::into_os_string);
    let import_force = long("force").help("Overwrite existing profiles").switch();
    let import_cmd = command(
        "import",
        construct!(import_force, import_input).map(|(force, input)| Command::ProfilesImport { input, force })
            .to_options()
            .descr("Import profiles from package"),
    );

    construct!([list_cmd, export_cmd, import_cmd])
}

fn opt_f64(name: &'static str, doc: &'static str) -> impl Parser<Option<f64>> {
    long(name).help(doc).argument::<f64>("VALUE").optional()
}

fn support_settings_parser() -> impl Parser<SupportSettings> {
    let c1 = opt_f64("support-min-column-diameter-mm", "Min column diameter in mm (default: 0.8)");
    let c2 = opt_f64("support-max-column-diameter-mm", "Max column diameter in mm (default: 2.0)");
    let c3 = opt_f64("support-branch-angle-deg", "Branch angle in degrees (default: 40)");
    let c4 = opt_f64("support-overhang-angle-deg", "Overhang angle in degrees (default: 45)");
    let c5 = opt_f64("support-spacing-mm", "Support spacing in mm (default: 4)");
    let g1 = construct!(c1, c2, c3, c4, c5);

    let t1 = opt_f64("support-tip-contact-diameter-mm", "Tip contact diameter in mm (default: 0.4)");
    let t2 = opt_f64("support-tip-length-mm", "Tip length in mm (default: 2)");
    let t3 = opt_f64("support-shaft-diameter-mm", "Shaft diameter in mm (default: 1.2)");
    let t4 = opt_f64("support-base-diameter-mm", "Base diameter in mm (default: 4)");
    let t5 = opt_f64("support-base-height-mm", "Base height in mm (default: 0.6)");
    let g2 = construct!(t1, t2, t3, t4, t5);

    let r1 = opt_f64("support-raft-thickness-mm", "Raft thickness in mm (default: 1)");
    let r2 = opt_f64("support-raft-margin-mm", "Raft margin in mm (default: 3)");
    let r3 = opt_f64("support-lift-mm", "Lift height in mm (default: 5)");
    let r4 = opt_f64("support-contact-depth-mm", "Contact depth in mm (default: 0.1)");
    let g3 = construct!(r1, r2, r3, r4);

    let b1 = long("symmetric-supports").help("Symmetric supports (default: false)").switch();
    let b2 = long("tree-supports").help("Tree supports").switch();
    let b3 = long("support-no-tree").help("Disable tree supports").switch();
    let b4 = long("support-lattice-raft").help("Lattice raft").switch();
    let b5 = long("support-solid-raft").help("Solid raft").switch();
    let b6 = long("support-skate-raft").help("Skate raft (default: false)").switch();
    let b7 = long("support-no-raft").help("Disable raft (default: false)").switch();
    let g4 = construct!(b1, b2, b3, b4, b5, b6, b7);

    construct!(g1, g2, g3, g4)
        .map(|(a, b, c, d)| {
            let mut s = SupportSettings::default();
            if let Some(v) = a.0 { s.min_column_diameter_mm = v; }
            if let Some(v) = a.1 { s.max_column_diameter_mm = v; }
            if let Some(v) = a.2 { s.branch_angle_deg = v; }
            if let Some(v) = a.3 { s.overhang_angle_deg = v; }
            if let Some(v) = a.4 { s.spacing_mm = v; }

            if let Some(v) = b.0 { s.tip_contact_diameter_mm = v; }
            if let Some(v) = b.1 { s.tip_length_mm = v; }
            if let Some(v) = b.2 { s.shaft_diameter_mm = v; }
            if let Some(v) = b.3 { s.base_diameter_mm = v; }
            if let Some(v) = b.4 { s.base_height_mm = v; }

            if let Some(v) = c.0 { s.raft_thickness_mm = v; }
            if let Some(v) = c.1 { s.raft_margin_mm = v; }
            if let Some(v) = c.2 { s.lift_mm = v; }
            if let Some(v) = c.3 { s.contact_depth_mm = v; }

            s.symmetric_supports = d.0;
            s.tree_supports = d.1 || !d.2;
            s.lattice_raft = d.3 && !d.4;
            s.skate_raft = d.5;
            s.raft = !d.6;
            s
        })
        .parse(|s| s.validate().map(|()| s).map_err(|e| e.to_string()))
}

fn slice_parser() -> impl Parser<Command> {
    let source = file_in('s', "source", "Source mesh filepath", "SOURCE_FILEPATH");
    let target = file_in('t', "target", "Target output path", "TARGET_PATH");
    let target_format = long("target-format").help("Output format").argument::<Format>("TARGET_FORMAT").optional();
    let core_files = construct!(source, target, target_format);

    let printer_profile = long("printer-profile").help("Printer profile reference").argument::<PrinterProfileRef>("PRINTER_PROFILE");
    let material_profile = long("material-profile").help("Material profile reference").argument::<MaterialProfileRef>("MATERIAL_PROFILE");
    let recipe = long("recipe").help("Saved slicing recipe").argument::<String>("NAME_OR_FILE").optional();
    let profiles = construct!(printer_profile, material_profile, recipe);

    let plate = long("plate").help("Plate index (1-based)").argument::<usize>("N").optional();
    let all_plates = long("all-plates").help("Slice all plates").switch();
    let no_auto_place = long("no-auto-place").help("Preserve original coordinates").switch();
    let plates = construct!(plate, all_plates, no_auto_place);

    let layer_height = long("layer-height").help("Layer height in mm").argument::<PositiveFiniteMm>("MM").optional();
    let first_layer_height = long("first-layer-height").help("First layer height in mm").argument::<PositiveFiniteMm>("MM").optional();
    let aa = long("aa").help("Antialiasing level (0, 2, 4, 8, off, x2, x4, x8)").argument::<String>("AA").parse(|s| {
        let name = match s.as_str() { "0" => "off", "2" => "x2", "4" => "x4", "8" => "x8", _ => &s };
        name.parse::<AaLevel>()
    }).fallback(AaLevel::Off);
    let layers = construct!(layer_height, first_layer_height, aa);

    let verbose = short('v').long("verbose").help("Detailed diagnostics").switch();
    let enable_repair = long("repair").help("Enable repair").req_flag(true);
    let disable_repair = long("no-repair").help("Disable repair").req_flag(false);
    let repair = construct!([enable_repair, disable_repair])
    .many()
    .map(|v| v.into_iter().last().unwrap_or(true));
    let islands = long("islands").help("Report unsupported islands").switch();
    let risks = long("risks").help("Report print risk diagnostics").switch();
    let default_mf = "0.3".parse::<PositiveFiniteMm>().expect("0.3 is valid mm");
    let minimum_feature_mm = long("minimum-feature-mm").help("Minimum feature threshold mm").argument::<PositiveFiniteMm>("MM").fallback(default_mf);
    let diag = construct!(verbose, repair, islands, risks, minimum_feature_mm);

    let supports = long("supports")
        .help("Support mode ('off' or 'auto')")
        .argument::<String>("MODE")
        .parse(|s| match s.as_str() {
            "off" => Ok(false),
            "auto" => Ok(true),
            w => Err(format!("expected 'off' or 'auto', got '{w}'")),
        })
        .fallback(false);
    let supp = construct!(supports, support_settings_parser());

    construct!(core_files, profiles, plates, layers, diag, supp).map(|(cf, pr, pl, ly, dg, sp)| {
        Command::Slice(Box::new(SliceArgs {
            recipe: pr.2,
            plate: pl.0,
            all_plates: pl.1,
            source: cf.0,
            target: cf.1,
            target_format: cf.2,
            printer_profile: pr.0,
            material_profile: pr.1,
            layer_height: ly.0,
            first_layer_height: ly.1,
            aa: ly.2,
            verbose: dg.0,
            no_auto_place: pl.2,
            repair: dg.1,
            islands: dg.2,
            risks: dg.3,
            supports: sp.0,
            minimum_feature_mm: dg.4,
            support_settings: sp.1,
        }))
    })
}

pub(crate) fn parser() -> OptionParser<Command> {
    let convert_cmd = command("convert", convert_parser().to_options().descr("Convert mesh formats"));
    let diagnose_cmd = command("diagnose", diagnose_parser().to_options().descr("Diagnose mesh defects"));
    let inspect_cmd = command("inspect", inspect_parser().to_options().descr("Inspect mesh properties"));
    let fix_cmd = command("fix", fix_parser().to_options().descr("Repair mesh faces and normals"));
    let slice_cmd = command("slice", slice_parser().to_options().descr("Slice 3D mesh for printing"));
    let help_cmd = command("help", pure(Command::Help).to_options().descr("Print help message"));
    let version_cmd = command("version", pure(Command::Version).to_options().descr("Print package version"));
    let profiles_cmd = command("profiles", profiles_parser().to_options().descr("Manage profiles"));

    construct!([
        convert_cmd,
        diagnose_cmd,
        inspect_cmd,
        fix_cmd,
        slice_cmd,
        help_cmd,
        version_cmd,
        profiles_cmd,
    ])
    .fallback(Command::Help)
    .to_options()
    .version(env!("CARGO_PKG_VERSION"))
    .descr("Mesh slicer command line interface")
}
