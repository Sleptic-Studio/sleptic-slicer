// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{LoadSceneError, ProjectPayload, ReadProjectWarning};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub const SUPPORTED_SCENE_FILE_EXTENSIONS: &[&str] = &["3mf", "obj", "stl", "stla", "stlb"];
pub fn is_supported_scene_file_extension(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        SUPPORTED_SCENE_FILE_EXTENSIONS
            .iter()
            .any(|s| s.eq_ignore_ascii_case(e))
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadProgress {
    pub done_bytes: u64,
    pub total_bytes: u64,
}
#[derive(Debug)]
pub struct LoadedScene {
    pub scene: sc_scene::Scene,
    pub mesh_warnings: Vec<sc_mesh_formats::messages::LoadMeshWarning>,
    pub project_warnings: Vec<ReadProjectWarning>,
    pub payload: Option<ProjectPayload>,
    pub slices: Option<sc_sl1cer::contour::ContourStack>,
    pub slice_fingerprint: Option<String>,
}
struct Counting<'a, R, F> {
    reader: R,
    progress: &'a mut F,
    total: u64,
}
impl<R: Read + Seek, F: FnMut(LoadProgress)> Read for Counting<'_, R, F> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(b)?;
        let done = self.reader.stream_position()?;
        (self.progress)(LoadProgress {
            done_bytes: done,
            total_bytes: self.total,
        });
        Ok(n)
    }
}
impl<R: Seek, F> Seek for Counting<'_, R, F> {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        self.reader.seek(p)
    }
}
pub fn load_scene(
    p: &Path,
    limit: Option<u64>,
    mut progress: impl FnMut(LoadProgress),
) -> Result<LoadedScene, LoadSceneError> {
    if !is_supported_scene_file_extension(p) {
        return Err(LoadSceneError::UnsupportedExtension { path: p.into() });
    }
    let mut file = std::fs::File::open(p)?;
    let size = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(0))?;
    let excess = limit.filter(|&maximum| size > maximum);
    excess.map_or(Ok(()), |limit_bytes| {
        Err(LoadSceneError::FileTooLarge {
            actual_bytes: size,
            limit_bytes,
        })
    })?;
    let project = p
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("3mf"));
    let mut total = size;
    let result = if project {
        let loaded = crate::threemf::read_threemf_project_with_progress(&mut file, |v| {
            total = v.total_bytes;
            progress(LoadProgress {
                done_bytes: v.decompressed_bytes,
                total_bytes: v.total_bytes,
            })
        })
        .map_err(crate::ReadProjectError::ThreeMf)?;
        let mut warnings = vec![];
        if let Some(w) = sc_mesh_formats::loader::scene_shared_vertices_warning(&loaded.scene) {
            warnings.push(sc_mesh_formats::messages::LoadMeshWarning::Scene(w));
        }
        LoadedScene {
            scene: loaded.scene,
            mesh_warnings: warnings,
            project_warnings: loaded.warnings,
            payload: loaded.payload,
            slices: loaded.slices,
            slice_fingerprint: loaded.slice_fingerprint,
        }
    } else {
        let kind = sc_mesh_formats::filetypes::infer_mesh_file_format_from_extension(p)
            .map_err(|_| LoadSceneError::UnsupportedExtension { path: p.into() })?;
        let mut reader = Counting {
            reader: std::io::BufReader::new(file),
            progress: &mut progress,
            total: size,
        };
        let loaded = sc_mesh_formats::loader::load_mesh_from_reader(kind, &mut reader)?;
        LoadedScene {
            scene: loaded.graph,
            mesh_warnings: loaded.warnings,
            project_warnings: vec![],
            payload: None,
            slices: None,
            slice_fingerprint: None,
        }
    };
    progress(LoadProgress {
        done_bytes: total,
        total_bytes: total,
    });
    Ok(result)
}
