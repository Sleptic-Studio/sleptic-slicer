// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::fs;
use std::io::Cursor;
use std::sync::Arc;

use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_print_primitives::{AaLevel, PositiveFiniteMm};
use sc_project_formats::{EmbeddedProfile, read_project};
use sc_scene::LeafPayload;
use tempfile::TempDir;

use super::*;
use crate::scene_document::SceneEdit;
use crate::slice_result::{SliceParams, SliceResult};

fn printer(id: &str) -> PrinterId {
    PrinterId::new(id).unwrap()
}

fn resin(id: &str) -> MaterialId {
    MaterialId::new(id).unwrap()
}

/// One part under a group, as files open.
fn one_part() -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let part = scene
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())))
        .unwrap();
    scene.add_child(root, part).unwrap();
    scene
}

/// Opened from a mesh file.
fn from_mesh(path: impl Into<PathBuf>) -> std::rc::Rc<SceneDocument> {
    let doc = SceneDocument::new();
    doc.take_mesh_scene(one_part(), path.into());
    doc
}

/// Opened from a project carrying this data.
fn from_project(path: impl Into<PathBuf>, payload: ProjectPayload) -> std::rc::Rc<SceneDocument> {
    let doc = SceneDocument::new();
    doc.take_project_scene(one_part(), path.into(), payload);
    doc
}

/// A project naming a printer it carries a copy of, and a resin.
fn carried() -> ProjectPayload {
    ProjectPayload {
        printer: Some(printer("held-printer")),
        resin: Some(resin("held-resin")),
        embedded: vec![EmbeddedProfile { profile: sc_project_formats::NamedProfile::Printer(printer("held-printer")), extension: false, bytes: b"kind = \"printer\"".to_vec() }],
        ..ProjectPayload::default()
    }
}

/// Moves the part, so the document has a change.
fn edit(doc: &SceneDocument) {
    let node_id = doc.read_scene(|s| s.leaf_id_at_index(0)).flatten().unwrap();
    let after = Transform3D::Affine(AffineTransform3D::from_translation([1.0, 0.0, 0.0]));
    let change = SceneEdit::SetLocalTransform { node_id, before: Transform3D::Identity, after };
    assert!(doc.commit_edit(change.clone()).is_ok());
    doc.record_edit(change);
}

fn read_back(path: &Path) -> sc_project_formats::LoadedProject {
    read_project(&mut Cursor::new(fs::read(path).unwrap()), ProjectFileType::ThreeMf).unwrap()
}

#[test]
fn save_overwrites_only_a_changed_project_and_asks_for_anything_else() {
    assert_eq!(SaveAction::for_document(&SceneDocument::new()), SaveAction::NothingOpen);
    let project = from_project("/tmp/plate.3mf", ProjectPayload::default());
    assert_eq!(SaveAction::for_document(&project), SaveAction::UpToDate);
    edit(&project);
    assert_eq!(SaveAction::for_document(&project), SaveAction::WriteOver(PathBuf::from("/tmp/plate.3mf")));
    // A mesh file is never written over, unchanged, edited or with a
    // printer, resin or first layer picked (picking makes it changed).
    let mesh = from_mesh("/tmp/widget.stl");
    assert!(!mesh.has_unsaved_changes());
    assert_eq!(SaveAction::for_document(&mesh), SaveAction::AskWhere);
    let picks: [&dyn Fn(&SceneDocument) -> bool; 3] = [
        &|d| d.choose_printer(Some(printer("picked-printer"))),
        &|d| d.choose_resin(Some(resin("picked-resin"))),
        &|d| d.choose_first_layer(Some(PositiveFiniteMm::new(0.1).unwrap())),
    ];
    for pick in picks {
        let mesh = from_mesh("/tmp/widget.stl");
        assert!(pick(&mesh) && mesh.has_unsaved_changes());
        assert_eq!(SaveAction::for_document(&mesh), SaveAction::AskWhere);
    }
}

#[test]
fn save_as_takes_a_3mf_name_in_any_case_or_adds_the_extension() {
    let ready = |name: &str| SaveAsAnswer::WriteTo(PathBuf::from(name));
    assert_eq!(SaveAsAnswer::from_dialog(None), SaveAsAnswer::Dismissed);
    assert_eq!(SaveAsAnswer::from_dialog(Some("/tmp/plate.3mf".into())), ready("/tmp/plate.3mf"));
    assert_eq!(SaveAsAnswer::from_dialog(Some("/tmp/plate.3MF".into())), ready("/tmp/plate.3MF"));
    assert_eq!(SaveAsAnswer::from_dialog(Some("/tmp/plate".into())), ready("/tmp/plate.3mf"));
    assert_eq!(SaveAsAnswer::from_dialog(Some("/tmp/plate.stl".into())), SaveAsAnswer::NotAProjectName("/tmp/plate.stl".into()));
}

#[test]
fn saved_choices_are_the_documents_own_with_the_defaults_for_the_rest() {
    let none = ProfileLibraries::new(None, vec![]);
    let defaults = |doc: &SceneDocument| project_data_for_save(doc, Some(printer("global-printer")), Some(resin("global-resin")), &none);
    // A project keeps its own.
    let saved = defaults(&from_project("/tmp/plate.3mf", carried()));
    assert_eq!((saved.printer, saved.resin), (Some(printer("held-printer")), Some(resin("held-resin"))));
    // A mesh takes the defaults, and nothing to copy is found.
    let saved = defaults(&from_mesh("/tmp/widget.stl"));
    assert_eq!((saved.printer, saved.resin), (Some(printer("global-printer")), Some(resin("global-resin"))));
    assert!(saved.embedded.is_empty());
    // The same printer and resin the slice panel uses: a pick, else the
    // default.
    let picked = from_mesh("/tmp/widget.stl");
    assert!(picked.choose_printer(Some(printer("picked-printer"))));
    let saved = defaults(&picked);
    assert_eq!((saved.printer, saved.resin, saved.first_layer_height), (Some(printer("picked-printer")), Some(resin("global-resin")), None));
    let layered = from_mesh("/tmp/widget.stl");
    let height = PositiveFiniteMm::new(0.1).unwrap();
    assert!(layered.choose_first_layer(Some(height)));
    let saved = defaults(&layered);
    assert_eq!((saved.printer, saved.first_layer_height), (Some(printer("global-printer")), Some(height)));
}

/// A profile folder with these files.
fn folder(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

#[test]
fn named_profiles_are_copied_in_but_a_carried_copy_is_kept() {
    let installed = folder(&[
        ("printers/global-printer.toml", "kind = \"printer\""),
        ("materials/global-resin.toml", "kind = \"material\""),
        ("materials/extensions/global-resin.toml", "kind = \"extension\""),
        ("printers/held-printer.toml", "kind = \"library-copy\""),
    ]);
    let libraries = ProfileLibraries::new(Some(installed.path().to_owned()), vec![]);
    let saved = project_data_for_save(&from_mesh("/tmp/widget.stl"), Some(printer("global-printer")), Some(resin("global-resin")), &libraries);
    assert_eq!(
        saved.embedded,
        [
            EmbeddedProfile { profile: sc_project_formats::NamedProfile::Printer(printer("global-printer")), extension: false, bytes: b"kind = \"printer\"".to_vec() },
            EmbeddedProfile { profile: sc_project_formats::NamedProfile::Resin(resin("global-resin")), extension: false, bytes: b"kind = \"material\"".to_vec() },
            EmbeddedProfile { profile: sc_project_formats::NamedProfile::Resin(resin("global-resin")), extension: true, bytes: b"kind = \"extension\"".to_vec() },
        ]
    );
    let saved = project_data_for_save(&from_project("/tmp/plate.3mf", carried()), None, None, &libraries);
    let printers: Vec<&[u8]> = saved
        .embedded
        .iter()
        .filter_map(|p| match p {
            EmbeddedProfile { profile: sc_project_formats::NamedProfile::Printer(_), extension: false, bytes, .. } => Some(bytes.as_slice()),
            _ => None,
        })
        .collect();
    assert_eq!(printers, [b"kind = \"printer\"".as_slice()]);
}

#[test]
fn a_save_writes_the_project_and_the_document_then_saves_there() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("widget.3mf");
    assert!(save_now(&SceneDocument::new(), &target, None, None).is_err());
    assert!(!target.exists());
    let doc = from_project(dir.path().join("widget.stl"), carried());
    save_now(&doc, &target, None, None).unwrap();
    assert_eq!(doc.file_path(), Some(target.clone()));
    assert!(doc.opened_from_project() && !doc.has_unsaved_changes());
    assert_eq!(SaveAction::for_document(&doc), SaveAction::UpToDate);
    let opened = read_back(&target);
    assert_eq!((opened.payload, opened.scene.roots().len()), (Some(carried()), 1));
    // Saving again writes the same file again.
    let first = fs::read(&target).unwrap().len();
    save_now(&doc, &target, None, None).unwrap();
    assert_eq!(fs::read(&target).unwrap().len(), first);
}

#[test]
fn saving_a_mesh_as_a_project_leaves_the_mesh_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (mesh, target) = (dir.path().join("widget.stl"), dir.path().join("widget.3mf"));
    let stl = b"solid widget\nendsolid widget\n";
    fs::write(&mesh, stl).unwrap();
    let doc = from_mesh(&mesh);
    assert!(doc.choose_printer(Some(printer("picked-printer"))));
    save_now(&doc, &target, None, None).unwrap();
    assert_eq!(fs::read(&mesh).unwrap(), stl);
    assert!(doc.opened_from_project() && !doc.has_unsaved_changes());
    edit(&doc);
    assert_eq!(SaveAction::for_document(&doc), SaveAction::WriteOver(target));
}

#[test]
fn the_defaults_saved_become_the_documents_choices() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("widget.3mf");
    let doc = from_mesh(dir.path().join("widget.stl"));
    save_now(&doc, &target, Some(printer("nonexistent-printer")), Some(resin("nonexistent-resin"))).unwrap();
    let saved = read_back(&target).payload.unwrap();
    assert_eq!((saved.printer, saved.resin), (Some(printer("nonexistent-printer")), Some(resin("nonexistent-resin"))));
    assert!(saved.embedded.is_empty());
    assert_eq!((doc.chosen_printer(), doc.chosen_resin()), (Some(printer("nonexistent-printer")), Some(resin("nonexistent-resin"))));
}

/// Holds a slice made with this printer and resin, as slicing would.
fn hold_slice(doc: &SceneDocument, printer: &PrinterId, resin: &MaterialId) {
    use crate::slice_projection::resolve_current_profile;
    use sc_sl1cer::contour::{ContourLayer, ContourStack};
    let scene = doc.read_scene(|s| s.graph().clone()).unwrap();
    let profile = resolve_current_profile(printer, resin, None, AaLevel::Off, &scene).unwrap();
    let schedule = *profile.layer_schedule();
    let token = sc_print_glue::slice_input_fingerprint(&scene, &schedule).unwrap().to_token();
    let layers = (0..schedule.total_layers()).map(|_| ContourLayer::new(0.05, vec![])).collect();
    doc.keep_slice(SliceResult {
        merged: ContourStack::new_unchecked(layers),
        schedule,
        token,
        params: SliceParams {
            printer: printer.clone(),
            resin: resin.clone(),
            first_layer_height: None,
            aa_level: AaLevel::Off,
            geometry: doc.revision(),
        },
        profile,
    });
}

#[test]
fn slices_made_on_the_default_or_a_partly_picked_profile_are_saved() {
    // A fresh mesh has no printer or resin of its own; slicing used the
    // defaults the panel shows, and the save must keep its slices.
    let (mars, clear) = (printer("elegoo-mars-5-goo"), resin("sirayatech-craft-ultra-clear-saturn4"));
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("widget.3mf");
    let default = from_mesh(dir.path().join("widget.stl"));
    hold_slice(&default, &mars, &clear);
    save_now(&default, &target, Some(mars.clone()), Some(clear.clone())).unwrap();
    let opened = read_back(&target);
    assert!(opened.slices.is_some() && opened.slice_fingerprint.is_some());
    // A picked printer with the default resin: both named, slices kept.
    let picked = from_mesh(dir.path().join("widget.stl"));
    assert!(picked.choose_printer(Some(mars.clone())));
    hold_slice(&picked, &mars, &clear);
    save_now(&picked, &target, Some(printer("global-printer")), Some(clear.clone())).unwrap();
    let opened = read_back(&target);
    let saved = opened.payload.unwrap();
    assert_eq!((saved.printer, saved.resin), (Some(mars), Some(clear)));
    assert!(opened.slices.is_some() && opened.slice_fingerprint.is_some());
}

#[test]
fn a_prepared_save_writes_on_its_own_reporting_each_phase_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("plate.3mf");
    let doc = from_mesh(dir.path().join("widget.stl"));
    assert!(ProjectSnapshot::of_document(&SceneDocument::new(), carried(), target.clone()).is_none());
    let bundle = ProjectSnapshot::of_document(&doc, carried(), target.clone()).unwrap();
    let mut phases = Vec::new();
    bundle.write(|phase, _, _| phases.push(phase)).unwrap();
    assert_eq!(read_back(&target).payload, Some(carried()));
    // Serialize, then compress, then writing: one run of each.
    phases.dedup();
    assert_eq!(phases, [WriteStep::Serialize, WriteStep::Compress, WriteStep::Writing]);
}
