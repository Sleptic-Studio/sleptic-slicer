// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Per-window crash snapshots. OS file locks exclude running app instances.
use crate::{preferences_bus::PreferenceHub, scene_document::SceneDocument, scene_save};
use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    rc::Rc,
    thread::JoinHandle,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) fn directory() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SLEPTIC_RECOVERY_DIR") {
        return Some(dir.into());
    }
    sc_app_paths::project_dirs().map(|d| d.data_local_dir().join("recovery"))
}

pub(crate) struct RecoveryFile {
    pub path: PathBuf,
    _lock: File,
}
impl RecoveryFile {
    pub(crate) fn claim(path: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(path.parent().expect("recovery parent"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))?;
        lock.try_lock().map_err(std::io::Error::other)?;
        Ok(Self { path, _lock: lock })
    }
    pub(crate) fn remove(&self) -> std::io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn remove_session(&self) -> std::io::Result<()> {
        self.remove()?;
        match fs::remove_file(self.path.with_extension("lock")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

impl Drop for RecoveryFile {
    fn drop(&mut self) {
        // Keep crash snapshots offerable; remove only an empty session's lock.
        // The OS lock remains held throughout the existence check and unlink.
        if !self.path.exists() {
            let _ = fs::remove_file(self.path.with_extension("lock"));
        }
    }
}

/// Sweep abandoned empty sessions while holding their OS lock. PID reuse and
/// multiple windows in one process cannot make a live session look abandoned.
fn sweep_empty_locks(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().is_some_and(|ext| ext == "lock")
            && path
                .file_stem()
                .is_some_and(|name| name.to_string_lossy().starts_with("recovery-"))
        {
            let snapshot = path.with_extension("3mf");
            if snapshot.exists() {
                continue;
            }
            // Drop removes the empty lock only after successfully claiming it.
            // Recheck snapshot existence under the lock, inside Drop.
            if let Ok(session) = RecoveryFile::claim(snapshot) {
                drop(session);
            }
        }
    }
}

pub(crate) fn pending(dir: &Path) -> Vec<RecoveryFile> {
    sweep_empty_locks(dir);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "3mf"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| RecoveryFile::claim(p).ok())
        .collect()
}

pub(crate) struct Autosave {
    timer: slint::Timer,
    worker: Rc<RefCell<Option<JoinHandle<()>>>>,
    file: Rc<RecoveryFile>,
    _subscription: Option<crate::scene_document::Watch>,
}
impl Autosave {
    pub(crate) fn start(
        doc: Rc<SceneDocument>,
        prefs: Rc<PreferenceHub>,
        id: u64,
    ) -> std::io::Result<Self> {
        let dir =
            directory().ok_or_else(|| std::io::Error::other("App data directory unavailable"))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let file = Rc::new(RecoveryFile::claim(
            dir.join(format!("recovery-{}-{stamp}-{id}.3mf", std::process::id())),
        )?);
        let worker: Rc<RefCell<Option<JoinHandle<()>>>> = Rc::default();
        let subscription = watch_saved_document(&doc, &file, &worker);
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, Duration::from_secs(120), {
            let file = file.clone();
            let worker = worker.clone();
            move || {
                if worker.borrow().as_ref().is_some_and(|h| !h.is_finished()) {
                    return;
                }
                if let Some(h) = worker.borrow_mut().take() {
                    let _ = h.join();
                }
                if !doc.has_unsaved_changes() || !doc.has_scene() {
                    if let Err(e) = file.remove() {
                        eprintln!("Recovery cleanup: {e}");
                    }
                    return;
                }
                let snap = prefs.current_values();
                let payload = scene_save::project_data_for_save(
                    &doc,
                    snap.default_printer.clone(),
                    snap.default_resin.clone(),
                    &sc_print_profiles::ProfileLibraries::platform_default(),
                );
                if let Some(bundle) = scene_save::ProjectSnapshot::of_document(&doc, payload, file.path.clone()) {
                    *worker.borrow_mut() = Some(std::thread::spawn(move || {
                        if let Err(e) = bundle.write(|_, _, _| {}) {
                            eprintln!("Recovery snapshot: {e}");
                        }
                    }));
                }
            }
        });
        Ok(Self {
            timer,
            worker,
            file,
            _subscription: Some(subscription),
        })
    }
    pub(crate) fn clean_exit(&self) {
        self.timer.stop();
        if let Some(h) = self.worker.borrow_mut().take() {
            let _ = h.join();
        }
        if let Err(e) = self.file.remove_session() {
            eprintln!("Recovery cleanup: {e}");
        }
    }
}
/// A save/undo-to-save-point must invalidate the previous snapshot immediately,
/// rather than leaving already-saved work in Restore until the next timer tick.
fn watch_saved_document(
    doc: &Rc<SceneDocument>,
    file: &Rc<RecoveryFile>,
    worker: &Rc<RefCell<Option<JoinHandle<()>>>>,
) -> crate::scene_document::Watch {
    let weak_doc = Rc::downgrade(doc);
    let file = file.clone();
    let worker = worker.clone();
    doc.watch(move |event| {
        if matches!(event, crate::scene_document::DocumentEvent::UnsavedChanged)
            && weak_doc.upgrade().is_some_and(|doc| !doc.has_unsaved_changes())
        {
            // Finish any old write before removing it; otherwise that worker
            // could recreate a stale snapshot after the user's explicit save.
            if let Some(handle) = worker.borrow_mut().take() {
                let _ = handle.join();
            }
            if let Err(error) = file.remove() {
                eprintln!("Recovery cleanup: {error}");
            }
        }
    })
}

#[cfg(test)]
#[path = "tests_recovery.rs"]
mod tests;
