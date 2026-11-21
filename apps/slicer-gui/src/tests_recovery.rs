// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
#[test]
fn crash_snapshot_survives_owner_drop_and_clean_exit_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery-test.3mf");
    let file = RecoveryFile::claim(path.clone()).unwrap();
    fs::write(&path, b"snapshot").unwrap();
    assert!(
        pending(dir.path()).is_empty(),
        "live owner must be excluded"
    );
    drop(file); // Simulate process death: OS releases the lock, data remains.
    let found = pending(dir.path());
    assert_eq!(found.len(), 1);
    assert_eq!(fs::read(&found[0].path).unwrap(), b"snapshot");
    found[0].remove().unwrap();
    found[0].remove().unwrap(); // Cleanup is idempotent.
    assert!(!path.exists());
}
#[test]
fn recovery_bundle_is_a_loadable_project_and_does_not_mark_document_saved() {
    let dir = tempfile::tempdir().unwrap();
    let doc = SceneDocument::new();
    let mut scene = sc_scene::Scene::new();
    let id = scene
        .add_leaf(
            None,
            sc_mesh_core::transform::Transform3D::default(),
            sc_scene::node::LeafPayload::Mesh(std::sync::Arc::new(
                sc_mesh_fixtures::unit_cube_mesh(),
            )),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    doc.take_mesh_scene(scene, PathBuf::from("cube.stl"));
    doc.choose_printer(Some("sleptic-diy-msla".parse().unwrap()));
    doc.add_plate();
    doc.add_plate();
    doc.choose(crate::selection::Selection::One(crate::selection::LeafId(
        id,
    )));
    doc.move_selection_to_plate(3);
    doc.edit_plates(|p| p.order[2].locked = true);
    assert!(doc.has_unsaved_changes());
    let path = dir.path().join("recovery-test.3mf");
    let payload = doc.project_payload(None, None);
    let bundle = scene_save::ProjectSnapshot::of_document(&doc, payload, path.clone()).unwrap();
    bundle.write(|_, _, _| {}).unwrap();
    assert!(path.is_file());
    if let Some(target) = std::env::var_os("SC_R9_RECOVERY_FIXTURE") {
        let target = PathBuf::from(target);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(&path, target).unwrap();
    }
    let loaded = sc_project_formats::loader::load_scene(&path, None, |_| {}).unwrap();
    assert!(loaded.payload.is_some());
    assert_eq!(loaded.scene.plates().order.len(), 3);
    assert!(loaded.scene.plates().order[2].locked);
    let ids = loaded
        .scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap();
    assert!(ids.iter().all(|id| loaded.scene.plate_of(*id) == 3));
    assert!(!doc.opened_from_project());
    assert!(
        doc.has_unsaved_changes(),
        "autosave must not consume the document save point"
    );
    assert_eq!(doc.file_path(), Some(PathBuf::from("cube.stl")));
}

#[test]
fn restored_project_is_dirty_and_save_requires_a_new_target() {
    let doc = SceneDocument::new();
    let Some(dir) = directory() else {
        return;
    };
    doc.take_project_scene(
        sc_scene::Scene::new(),
        dir.join("recovery-test.3mf"),
        sc_project_formats::ProjectPayload::default(),
    );
    assert!(doc.has_unsaved_changes());
    assert!(!doc.opened_from_project());
    assert_eq!(
        scene_save::SaveAction::for_document(&doc),
        scene_save::SaveAction::AskWhere
    );
}

#[test]
fn clean_exit_waits_for_in_flight_snapshot_before_deleting_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery-closing.3mf");
    let file = Rc::new(RecoveryFile::claim(path.clone()).unwrap());
    let worker = std::thread::spawn(move || {
        fs::write(path, b"last snapshot").unwrap();
    });
    let autosave = Autosave {
        timer: slint::Timer::default(),
        worker: Rc::new(RefCell::new(Some(worker))),
        file,
        _subscription: None,
    };
    autosave.clean_exit();
    assert!(!autosave.file.path.exists());
    assert!(!autosave.file.path.with_extension("lock").exists());
    assert!(autosave.worker.borrow().is_none());
    autosave.clean_exit();
}

#[test]
fn startup_sweeps_stale_empty_locks_but_preserves_live_windows() {
    let dir = tempfile::tempdir().unwrap();
    let stale = dir.path().join("recovery-stale.lock");
    fs::write(&stale, b"").unwrap();
    let live_path = dir.path().join("recovery-live.3mf");
    let live = RecoveryFile::claim(live_path.clone()).unwrap();
    assert!(pending(dir.path()).is_empty());
    assert!(!stale.exists());
    assert!(live_path.with_extension("lock").exists());
    assert!(RecoveryFile::claim(live_path.clone()).is_err());
    drop(live);
    assert!(!live_path.with_extension("lock").exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn consumed_crash_snapshot_is_offered_once_and_leaves_no_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery-crashed.3mf");
    let session = RecoveryFile::claim(path.clone()).unwrap();
    fs::write(&path, b"unsaved work").unwrap();
    drop(session);
    let recovered = pending(dir.path());
    assert_eq!(recovered.len(), 1);
    assert!(pending(dir.path()).is_empty());
    recovered[0].remove().unwrap();
    drop(recovered);
    assert!(pending(dir.path()).is_empty());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn saving_invalidates_snapshot_immediately_but_keeps_live_lock() {
    let dir = tempfile::tempdir().unwrap();
    let doc = Rc::new(SceneDocument::new());
    doc.add_plate();
    assert!(doc.has_unsaved_changes());
    let path = dir.path().join("recovery-saved.3mf");
    let file = Rc::new(RecoveryFile::claim(path.clone()).unwrap());
    let worker = Rc::new(RefCell::new(Some(std::thread::spawn(move || {
        fs::write(path, b"old unsaved state").unwrap();
    }))));
    let _subscription = watch_saved_document(&doc, &file, &worker);
    doc.note_saved();
    assert!(!file.path.exists());
    assert!(worker.borrow().is_none());
    assert!(file.path.with_extension("lock").exists());
    assert!(pending(dir.path()).is_empty());
    assert!(RecoveryFile::claim(file.path.clone()).is_err());
}

/// Runs in an isolated test process so SLEPTIC_RECOVERY_DIR never races other
/// tests and process death genuinely releases an OS lock without Rust Drop.
#[test]
#[ignore = "child-process fixture, invoked by recovery process tests"]
fn recovery_process_fixture() {
    let dir = directory().unwrap();
    let role = std::env::var("SLEPTIC_R23_RECOVERY_ROLE").unwrap();
    let file = RecoveryFile::claim(dir.join("recovery-process.3mf")).unwrap();
    match role.as_str() {
        "crash" => {
            fs::write(&file.path, b"unsaved process snapshot").unwrap();
            std::process::exit(0);
        }
        "live" => {
            fs::write(dir.join("ready"), b"").unwrap();
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
        }
        "clean" => {
            fs::write(&file.path, b"unsaved process snapshot").unwrap();
            let autosave = Autosave {
                timer: slint::Timer::default(),
                worker: Rc::default(),
                file: Rc::new(file),
                _subscription: None,
            };
            autosave.clean_exit();
        }
        _ => panic!("unknown child role"),
    }
}

fn recovery_child(dir: &Path, role: &str) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "recovery::tests::recovery_process_fixture",
            "--ignored",
        ])
        .env("SLEPTIC_RECOVERY_DIR", dir)
        .env("SLEPTIC_R23_RECOVERY_ROLE", role)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn process_crash_is_offered_once_and_clean_exit_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        recovery_child(dir.path(), "clean")
            .wait()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(
        recovery_child(dir.path(), "crash")
            .wait()
            .unwrap()
            .success()
    );
    let offered = pending(dir.path());
    assert_eq!(offered.len(), 1);
    assert_eq!(
        fs::read(&offered[0].path).unwrap(),
        b"unsaved process snapshot"
    );
    assert!(pending(dir.path()).is_empty());
    offered[0].remove().unwrap();
    drop(offered);
    assert!(pending(dir.path()).is_empty());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn second_process_preserves_live_lock_and_sweeps_it_after_death() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = recovery_child(dir.path(), "live");
    let ready = dir.path().join("ready");
    let started = std::time::Instant::now();
    while !ready.exists() {
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("recovery child did not claim lock");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let lock = dir.path().join("recovery-process.lock");
    assert!(pending(dir.path()).is_empty());
    assert!(lock.exists());
    assert!(RecoveryFile::claim(lock.with_extension("3mf")).is_err());
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(pending(dir.path()).is_empty());
    assert!(!lock.exists());
}
