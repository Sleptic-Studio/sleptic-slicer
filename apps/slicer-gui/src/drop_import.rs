// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! File drops always target the current document; 3MF requires an explicit choice.
use crate::viewport::ViewController;
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DropRoute {
    Geometry,
    ProjectPrompt,
}
pub(crate) fn route(path: &Path) -> DropRoute {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("3mf"))
    {
        DropRoute::ProjectPrompt
    } else {
        DropRoute::Geometry
    }
}

pub(crate) fn start(ctrl: Rc<ViewController>, paths: Vec<PathBuf>) {
    let revision = ctrl.scene_doc().revision();
    let slot = Arc::new(Mutex::new(None));
    let worker = slot.clone();
    std::thread::spawn(move || {
        let result = crate::mesh_loader::catch_worker_panic(|| {
            paths
                .iter()
                .map(|p| {
                    sc_project_formats::load_scene(
                        p,
                        Some(crate::mesh_loader::MESH_FILE_BYTE_LIMIT),
                        |_| {},
                    )
                    .map(|s| {
                        let mut scene = s.scene;
                        if s.payload.is_none() {
                            crate::mesh_loader::name_part_after_file(&mut scene, p);
                        }
                        scene
                    })
                    .map_err(anyhow::Error::from)
                })
                .collect::<anyhow::Result<Vec<_>>>()
        })
        .map_err(|e| e.to_string());
        *worker.lock().expect("import result") = Some(result);
    });
    let timer = Rc::new(slint::Timer::default());
    let stop = timer.clone();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(25),
        move || {
            let Some(result) = slot.lock().expect("import result").take() else {
                return;
            };
            stop.stop();
            let result = if !ctrl.scene_doc().revision().same_document(revision) {
                Err(
                    crate::i18n::tr!("The scene changed while importing. Drop the files again.")
                        .to_string(),
                )
            } else {
                result.and_then(|sources| {
                    ctrl.scene_doc()
                        .import_geometry(&sources, ctrl.printer_volume())
                })
            };
            match result {
                Ok(()) => ctrl.switch_section(crate::WorkspaceSection::Transform),
                Err(error) => ctrl.dialog_manager().show_failure(
                    &crate::i18n::tr!("Import failed"),
                    None,
                    error,
                    String::new(),
                ),
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drops_route_geometry_to_current_scene_and_3mf_to_choice() {
        for path in ["part.stl", "part.OBJ", "part.stla", "part.stlb"] {
            assert_eq!(route(Path::new(path)), DropRoute::Geometry);
        }
        for path in ["plate.3mf", "PLATE.3MF"] {
            assert_eq!(route(Path::new(path)), DropRoute::ProjectPrompt);
        }
    }
}
