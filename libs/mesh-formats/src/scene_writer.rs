// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{error::WriteSceneError as E, filetypes::WriteFormat, };
use sc_mesh_core::{IndexedMesh, IndexedTriangle};
use sc_scene::{CsgPolicy, Scene, bake_mesh_to_world};
use std::{
    io::{BufWriter, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
/// How far writing a scene has got: each stage counts its own units of
/// work up to its total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteSceneProgress {
    Flatten { done: u64, total: u64 },
    Serialize { done: u64, total: u64 },
    Compress { done: u64, total: u64 },
    Write { done: u64, total: u64 },
}

/// Reports serializing progress at the start, the end, and about every
/// thousandth of the way, so a big mesh does not flood the callback.
pub(crate) fn tick(report: &mut impl FnMut(WriteSceneProgress), done: usize, total: usize) {
    let step = (total / 1000).max(1);
    if done == 0 || done == total || done % step == 0 {
        report(WriteSceneProgress::Serialize { done: done as u64, total: total as u64 });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlattenWarning {
    MultiObjectMerge { object_count: usize },
}
impl FlattenWarning {
    pub fn user_message(&self) -> String {
        match self {
            Self::MultiObjectMerge { object_count } => format!(
                "Merging {object_count} objects into a single mesh for output; object boundaries and names will be lost."
            ),
        }
    }
}
#[derive(Debug)]
pub struct FlattenedScene {
    pub mesh: IndexedMesh,
    pub warnings: Vec<FlattenWarning>,
}
pub fn flatten_graph_to_mesh(g: &Scene) -> Result<FlattenedScene, E> {
    let count = g.expanded_leaf_count(CsgPolicy::Refuse)?;
    if count == 0 {
        return Err(E::EmptyScene);
    }
    if count > 100000 {
        return Err(E::LeafBudgetExceeded { limit: 100000 });
    }
    if count == 1 {
        let mut mesh = None;
        g.for_each_expanded_leaf::<E, _>(CsgPolicy::Refuse, |_, t, m| {
            let mut baked = if matches!(t, sc_mesh_core::transform::Transform3D::Identity) {
                m.clone()
            } else {
                bake_mesh_to_world(m, t)?
            };
            baked.meta = m.meta.clone();
            mesh = Some(baked);
            Ok(())
        })?;
        return Ok(FlattenedScene {
            mesh: mesh.ok_or(E::EmptyScene)?,
            warnings: Vec::new(),
        });
    }
    let mut vs = Vec::new();
    let mut fs = Vec::new();
    let mut meta = None;
    g.for_each_expanded_leaf::<E, _>(CsgPolicy::Refuse, |_, t, m| {
        if count == 1 {
            meta = m.meta.clone();
        }
        let m = bake_mesh_to_world(m, t)?;
        let offset = u32::try_from(vs.len())
            .map_err(|_| std::io::Error::other("flattened vertex index overflow"))?;
        for f in m.faces {
            let mut ids = [0; 3];
            for (i, v) in f.vertices.into_iter().enumerate() {
                ids[i] = v
                    .get()
                    .checked_add(offset)
                    .ok_or_else(|| std::io::Error::other("flattened vertex index overflow"))?;
            }
            fs.push(IndexedTriangle::from_raw_indices(f.normal, ids));
        }
        vs.extend(m.vertices);
        Ok(())
    })?;
    Ok(FlattenedScene {
        mesh: IndexedMesh::new(meta, vs, fs),
        warnings: if count > 1 {
            vec![FlattenWarning::MultiObjectMerge {
                object_count: count,
            }]
        } else {
            Vec::new()
        },
    })
}
static TEMP: AtomicU64 = AtomicU64::new(0);
fn atomic<T>(
    path: &Path,
    serialize: impl FnOnce(&mut BufWriter<std::fs::File>) -> Result<T, E>,
    cb: &mut impl FnMut(WriteSceneProgress),
) -> Result<T, E> {
    let filename = path
        .file_name()
        .ok_or_else(|| E::PathHasNoFileName(path.into()))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut base = std::ffi::OsString::from(".");
    base.push(filename);
    base.push(format!(
        ".tmp.{}.{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = parent.join(base);
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|source| E::CreateTempFile {
            path: temp.clone(),
            source,
        })?;
    let result = (|| {
        let mut w = BufWriter::with_capacity(262144, f);
        let value = serialize(&mut w)?;
        cb(WriteSceneProgress::Write { done: 0, total: 1 });
        w.flush()?;
        w.get_ref().sync_data()?;
        drop(w);
        std::fs::rename(&temp, path).map_err(|source| E::Rename {
            from: temp.clone(),
            to: path.into(),
            source,
        })?;
        cb(WriteSceneProgress::Write { done: 1, total: 1 });
        Ok(value)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
pub fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<(), E> {
    atomic(
        path,
        |w| {
            w.write_all(bytes)?;
            Ok(())
        },
        &mut |_| {},
    )
}
pub fn write_scene(g: &Scene, path: &Path, format: WriteFormat) -> Result<Vec<FlattenWarning>, E> {
    write_scene_with_progress(g, path, format, &mut |_| {})
}
pub fn write_scene_with_progress(
    g: &Scene,
    path: &Path,
    format: WriteFormat,
    cb: impl FnMut(WriteSceneProgress),
) -> Result<Vec<FlattenWarning>, E> {
    // A shared callback is needed by serialization and the final replacement phase.
    let callback = std::cell::RefCell::new(cb);
    atomic(
        path,
        |w| {
            let mut report = |p| (callback.borrow_mut())(p);
            match format {
                WriteFormat::StlAscii | WriteFormat::StlBin => {
                    report(WriteSceneProgress::Flatten { done: 0, total: 1 });
                    if g.expanded_leaf_count(CsgPolicy::Refuse)? == 1 {
                        // A single occurrence already has its own vertex space.
                        // Borrow identity geometry directly instead of copying
                        // it solely to hand it to the streaming serializer.
                        g.for_each_expanded_leaf::<E, _>(CsgPolicy::Refuse, |_, t, m| {
                            let baked;
                            let mesh =
                                if matches!(t, sc_mesh_core::transform::Transform3D::Identity) {
                                    m
                                } else {
                                    baked = {
                                        let mut mesh = bake_mesh_to_world(m, t)?;
                                        mesh.meta = m.meta.clone();
                                        mesh
                                    };
                                    &baked
                                };
                            report(WriteSceneProgress::Flatten { done: 1, total: 1 });
                            if format == WriteFormat::StlAscii {
                                crate::stl::write_stla_with_progress(mesh, w, &mut report)?;
                            } else {
                                crate::stl::write_stlb_with_progress(mesh, w, &mut report)?;
                            }
                            Ok(())
                        })?;
                        return Ok(Vec::new());
                    }
                    let f = flatten_graph_to_mesh(g)?;
                    report(WriteSceneProgress::Flatten { done: 1, total: 1 });
                    if format == WriteFormat::StlAscii {
                        crate::stl::write_stla_with_progress(&f.mesh, w, &mut report)?
                    } else {
                        crate::stl::write_stlb_with_progress(&f.mesh, w, &mut report)?
                    }
                    Ok(f.warnings)
                }
                WriteFormat::Obj => {
                    crate::obj::write_obj_with_progress(g, w, &mut report)?;
                    Ok(Vec::new())
                }
                WriteFormat::ThreeMf => {
                    crate::threemf::write_threemf_with_progress(g, w, &mut report)?;
                    Ok(Vec::new())
                }
            }
        },
        &mut |p| (callback.borrow_mut())(p),
    )
}
