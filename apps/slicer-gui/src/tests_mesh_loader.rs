// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_core::{MeshMetadata, Vertex};
use sc_mesh_fixtures::{cuboid_with_y_tunnel, cuboid_with_z_tunnel, unit_cube_mesh};
use sc_mesh_formats::filetypes::WriteFormat;
use sc_mesh_formats::scene_writer::write_scene;
use sc_project_formats::{EmbedProfiles, infer_project_file_type_from_extension, write_project};
use sc_scene::node::{LeafPayload, NodeKind};

use super::*;
use crate::error_dialog::MessageWindows;
use crate::load_progress::{PROGRESS_PERMILLE_MAX, decode};

const VOLUME: BuildVolume = BuildVolume::new(100.0, 80.0, 200.0);

/// A cube of side `size` standing on z = 0 with its centre at `x`, as a
/// root part.
fn add_cube(scene: &mut Scene, size: f32, x: f32) {
    let mut mesh = unit_cube_mesh();
    for v in &mut mesh.vertices {
        *v = Vertex::new([v[0] * size + x, v[1] * size, (v[2] + 0.5) * size]);
    }
    let id = scene.add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(mesh))).unwrap();
    scene.add_root(id).unwrap();
}

fn cubes(count: usize, size: f32, spacing: f32) -> Scene {
    let mut scene = Scene::new();
    for i in 0..count {
        add_cube(&mut scene, size, i as f32 * spacing);
    }
    scene
}

#[test]
fn only_files_within_the_size_limit_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let small = dir.path().join("small.stl");
    std::fs::write(&small, b"hello").unwrap();
    assert!(refuse_oversized_file(&small).is_ok());
    // A missing file passes; opening it gives the real error.
    assert!(refuse_oversized_file(&dir.path().join("missing.stl")).is_ok());
    let huge = dir.path().join("huge.stl");
    std::fs::File::create(&huge).unwrap().set_len(MESH_FILE_BYTE_LIMIT + 1).unwrap();
    let err = refuse_oversized_file(&huge).unwrap_err().to_string();
    assert_eq!(err, format!("File is {MESH_FILE_MIB_LIMIT} MiB, which exceeds the {MESH_FILE_MIB_LIMIT} MiB limit."));
}

#[test]
fn known_extensions_open_in_any_case() {
    for ext in OPENABLE_EXTENSIONS {
        assert!(can_open_extension(Path::new(&format!("/tmp/model.{ext}"))), "{ext}");
    }
    assert!(can_open_extension(Path::new("/tmp/model.STL")));
    assert!(can_open_extension(Path::new("/tmp/model.Obj")));
    for refused in ["/tmp/model.png", "/tmp/model.txt", "/tmp/model.ply", "/tmp/model.amf", "/tmp/no-extension", "/tmp/"] {
        assert!(!can_open_extension(Path::new(refused)), "{refused}");
    }
}

#[test]
fn a_mesh_is_centred_on_the_plate_resting_on_it() {
    let mut empty = Scene::new();
    centre_on_plate(&mut empty, VOLUME).unwrap();
    assert!(empty.tight_aabb().unwrap().is_none());
    assert!(fits_printer(&empty, VOLUME));
    // A box off to the side and up in the air lands centred on z = 0.
    let mut away = Scene::new();
    let root = away.add_group(None, Transform3D::Identity);
    away.add_root(root).unwrap();
    let mut mesh = unit_cube_mesh();
    for v in &mut mesh.vertices {
        *v = Vertex::new([v[0] * 10.0 + 105.0, v[1] * 20.0 + 210.0, v[2] * 10.0 + 55.0]);
    }
    let id = away.add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(mesh))).unwrap();
    away.add_child(root, id).unwrap();
    assert!(!fits_printer(&away, VOLUME));
    centre_on_plate(&mut away, VOLUME).unwrap();
    let bounds = away.tight_aabb().unwrap().unwrap();
    assert_eq!((bounds.min().coords(), bounds.max().coords()), ([-5.0, -10.0, 0.0], [5.0, 10.0, 10.0]));
    assert!(fits_printer(&away, VOLUME));
}

#[test]
fn a_project_stays_where_saved_if_it_fits_else_is_centred_or_arranged() {
    let mut fitting = cubes(1, 10.0, 0.0);
    let before = fitting.aabb().unwrap();
    assert_eq!(settle_project(&mut fitting, VOLUME).unwrap(), "");
    assert_eq!(fitting.aabb().unwrap(), before);
    // Laid out for another bed: centring is enough.
    let mut corner = Scene::new();
    add_cube(&mut corner, 1.0, 128.0);
    assert_eq!(settle_project(&mut corner, VOLUME).unwrap(), "Project centred on the build plate.");
    assert!(fits_printer(&corner, VOLUME));
    let mut wide = cubes(3, 20.0, 100.0);
    assert_eq!(settle_project(&mut wide, VOLUME).unwrap(), "Project automatically arranged on one plate.");
    assert_eq!(wide.plates().order.len(), 1);
    assert!(fits_printer(&wide, VOLUME));
    let mut crowded = cubes(3, 60.0, 100.0);
    assert_eq!(settle_project(&mut crowded, VOLUME).unwrap(), "Project automatically arranged across 3 plates.");
    for plate in &crowded.plates().order {
        assert!(fits_printer(&crowded.plate_scene(plate.id).unwrap(), VOLUME));
    }
    let mut huge = cubes(1, 150.0, 0.0);
    assert_eq!(settle_project(&mut huge, VOLUME).unwrap(), "Project centred; one object cannot fit the build volume.");
}

/// Opens a loader whose next load is started on `path`; returns what was
/// delivered.
fn start(loader: &SceneLoadCoordinator, path: &Path) -> (PathBuf, String) {
    let ticket = loader.request_load_ticket().unwrap();
    begin_ticketed_load(ticket, path.to_owned(), loader, BuildVolume::new(100.0, 60.0, 80.0));
    let (delivered, result) = loader.take_delivery_for_test().expect("an early exit still delivers");
    (delivered, result.err().expect("an early exit is an error").to_string())
}

#[test]
fn a_load_that_cannot_start_delivers_why() {
    let dir = tempfile::tempdir().unwrap();
    let (huge, small) = (dir.path().join("huge.stl"), dir.path().join("small.stl"));
    std::fs::File::create(&huge).unwrap().set_len(MESH_FILE_BYTE_LIMIT + 1).unwrap();
    std::fs::File::create(&small).unwrap();
    let loader = SceneLoadCoordinator::loader_for_test(MessageWindows::new());
    let (path, err) = start(&loader, &huge);
    assert!(path == huge && err.contains("exceeds"), "{err}");
    // No device yet: say it is still coming.
    assert_eq!(start(&loader, &small), (small.clone(), NoGraphics::NotYet.explain()));
    // A renderer that cannot be used: say which.
    loader.record_unsupported_graphics_api("GraphicsAPI::NativeOpenGL".into());
    let (_, err) = start(&loader, &small);
    assert_eq!(err, NoGraphics::ForeignApi("GraphicsAPI::NativeOpenGL".into()).explain());
    assert!(err.contains("GraphicsAPI::NativeOpenGL"));
}

#[test]
fn files_wait_for_a_coming_device_and_report_one_that_will_not_come() {
    assert_eq!(open_verdict(None), OpenVerdict::LoadNow);
    assert_eq!(open_verdict(Some(NoGraphics::NotYet)), OpenVerdict::HoldUntilGpu);
    for reason in [
        NoGraphics::ForeignApi("GraphicsAPI::WebGL".into()),
        NoGraphics::Refused("mesh pipeline refused".into()),
        NoGraphics::Gone,
    ] {
        assert_eq!(open_verdict(Some(reason.clone())), OpenVerdict::CannotLoad(reason));
    }
    let OpenVerdict::CannotLoad(reason) = open_verdict(Some(NoGraphics::ForeignApi("GraphicsAPI::WebGL".into())))
    else {
        unreachable!()
    };
    assert!(reason.explain().contains("GraphicsAPI::WebGL"));
}

#[test]
fn a_worker_panic_becomes_an_error_with_its_message() {
    let boxed = |payload: Box<dyn Any + Send>| panic_text(&payload);
    assert_eq!(boxed(Box::new("boom")), "boom");
    assert_eq!(boxed(Box::new(String::from("dynamic boom"))), "dynamic boom");
    let sync: Box<dyn std::error::Error + Send + Sync> = "sync error".into();
    assert_eq!(boxed(Box::new(sync)), "sync error");
    let send: Box<dyn std::error::Error + Send> = Box::new(std::io::Error::other("send error"));
    assert_eq!(boxed(Box::new(send)), "send error");
    assert_eq!(boxed(Box::new(42i32)), "unknown panic payload");
    assert_eq!(catch_worker_panic::<()>(|| panic!("boom")).unwrap_err().to_string(), "The mesh loader panicked: boom");
    assert_eq!(catch_worker_panic(|| Ok(3)).unwrap(), 3);
    assert_eq!(catch_worker_panic::<()>(|| Err(anyhow::anyhow!("plain failure"))).unwrap_err().to_string(), "plain failure");
}

/// Three named parts under one group, as a project or OBJ opens.
fn three_parts() -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    for (name, mut mesh, x) in [("Cube", unit_cube_mesh(), 0.0), ("ZTunnel", cuboid_with_z_tunnel(), 5.0), ("YTunnel", cuboid_with_y_tunnel(), -5.0)] {
        mesh.meta = Some(MeshMetadata { name: Some(name.into()), header_comment: None });
        let at = Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 0.0]));
        let id = scene.add_leaf(None, at, LeafPayload::Mesh(Arc::new(mesh))).unwrap();
        scene.add_child(root, id).unwrap();
    }
    scene
}

/// Reads `path`, returning its parts under the root and the last progress.
fn read(path: &Path) -> (usize, (OpenStep, u32), Scene) {
    let cell = Arc::new(AtomicU32::new(0));
    let parsed = read_source(path, &LoadProgressSink::new(cell.clone())).unwrap();
    let parts = match &parsed.scene_read.node(parsed.scene_read.roots()[0]).unwrap().kind {
        NodeKind::Group { children } => children.len(),
        _ => 1,
    };
    (parts, decode(cell.load(Ordering::Relaxed)), parsed.scene_read)
}

#[test]
fn reading_ends_with_the_read_phase_complete_for_every_format() {
    let dir = tempfile::tempdir().unwrap();
    let done = (OpenStep::Parse, PROGRESS_PERMILLE_MAX);
    let obj = dir.path().join("trio.obj");
    write_scene(&three_parts(), &obj, WriteFormat::Obj).unwrap();
    let (parts, progress, _) = read(&obj);
    assert_eq!((parts, progress), (3, done));
    // STL is sniffed and rewound first, a seek on the progress reader.
    let stl = dir.path().join("trio.stl");
    write_scene(&three_parts(), &stl, WriteFormat::StlBin).unwrap();
    let (_, progress, scene) = read(&stl);
    assert_eq!(progress, done);
    assert!(scene.aabb().unwrap().is_some());
    let project = dir.path().join("trio.3mf");
    let kind = infer_project_file_type_from_extension(&project).unwrap();
    let payload = ProjectPayload { printer: None, resin: None, first_layer_height: None, process_overrides: None, aa_level: None, embedded: Vec::new() };
    let mut bytes = Vec::new();
    write_project(&three_parts(), &payload, EmbedProfiles::No, None, None, kind, &mut bytes).unwrap();
    std::fs::write(&project, bytes).unwrap();
    let (parts, progress, _) = read(&project);
    assert_eq!((parts, progress), (3, done));
}

#[test]
fn a_lone_unnamed_mesh_takes_the_files_name() {
    let one = |name: Option<&str>| {
        let mut scene = Scene::new();
        let meta = name.map(|n| sc_scene::SceneMetadata { name: Some(n.into()), comment: None });
        let id = scene.add_leaf(meta, Transform3D::Identity, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
        scene.add_root(id).unwrap();
        (scene, id)
    };
    let name_of = |scene: &Scene, id| scene.node(id).and_then(|n| n.meta.clone()).and_then(|m| m.name);
    let (mut plain, id) = one(None);
    name_part_after_file(&mut plain, Path::new("/models/Bracket v2.stl"));
    assert_eq!(name_of(&plain, id).as_deref(), Some("Bracket v2"));
    let (mut named, id) = one(Some("Hull"));
    name_part_after_file(&mut named, Path::new("/models/other.obj"));
    assert_eq!(name_of(&named, id).as_deref(), Some("Hull"));
    let (mut project, id) = one(None);
    name_part_after_file(&mut project, Path::new("/models/plate.3mf"));
    assert_eq!(name_of(&project, id), None);
}

/// A real project arranged for another printer; set SLEPTIC_WALLET_3MF to
/// its path to run it.
#[test]
#[ignore = "needs a local project file"]
fn a_local_project_is_arranged_onto_one_plate() {
    let path = std::env::var("SLEPTIC_WALLET_3MF").expect("SLEPTIC_WALLET_3MF");
    let mut scene = load_scene(Path::new(&path), None, |_| {}).unwrap().scene;
    let volume = BuildVolume::new(192.0, 120.0, 200.0);
    assert!(settle_project(&mut scene, volume).unwrap().contains("arranged"));
    assert_eq!(scene.plates().order.len(), 1);
    assert!(fits_printer(&scene, volume));
}

#[test]
fn a_project_naming_a_printer_that_is_not_installed_warns_about_it() {
    let folder = tempfile::tempdir().unwrap();
    let project = folder.path().join("elsewhere.3mf");
    let kind = infer_project_file_type_from_extension(&project).unwrap();
    let missing = sc_print_primitives::PrinterId::new("no-such-printer-anywhere").unwrap();
    let payload = ProjectPayload { printer: Some(missing), ..ProjectPayload::default() };
    let mut bytes = Vec::new();
    write_project(&three_parts(), &payload, EmbedProfiles::No, None, None, kind, &mut bytes).unwrap();
    std::fs::write(&project, bytes).unwrap();
    let opened = read_source(&project, &LoadProgressSink::new(Arc::new(AtomicU32::new(0)))).unwrap();
    assert_eq!(opened.missing_profiles.len(), 1, "{:?}", opened.missing_profiles);
}
