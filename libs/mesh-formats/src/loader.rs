// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    error::LoadMeshError,
    filetypes::{MeshFileType, infer_mesh_file_format_from_extension},
    obj::LoadObjWarning,
    stl::LoadStlWarning,
    threemf::LoadThreeMfWarning,
};
use sc_scene::{CsgPolicy, Scene};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{Read, Seek},
    path::Path,
};
/// A load warning from whichever reader produced it, or about the scene
/// as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadMeshWarning {
    Stl(LoadStlWarning),
    Obj(LoadObjWarning),
    ThreeMf(LoadThreeMfWarning),
    Scene(SceneLoadWarning),
}

/// About the loaded scene rather than one file's syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneLoadWarning {
    /// Separate objects touch at a vertex: they print fused there.
    SharedVertices { leaves: Vec<SharedVertexLeaf> },
}

/// One of the objects that share a vertex: its node, and its name if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedVertexLeaf {
    pub object_name: Option<String>,
    pub node_id: sc_scene::NodeId,
}

#[derive(Debug)]
pub struct LoadedMesh {
    pub graph: Scene,
    pub warnings: Vec<LoadMeshWarning>,
}
pub fn load_mesh_from_path(
    path: &Path,
    format: Option<MeshFileType>,
    max: Option<u64>,
) -> Result<LoadedMesh, LoadMeshError> {
    if let Some((actual_bytes, limit_bytes)) = max.and_then(|limit| {
        std::fs::metadata(path)
            .ok()
            .map(|meta| (meta.len(), limit))
            .filter(|(actual, limit)| actual > limit)
    }) {
        return Err(LoadMeshError::FileTooLarge {
            actual_bytes,
            limit_bytes,
        });
    }
    let t = format.map_or_else(|| infer_mesh_file_format_from_extension(path), Ok)?;
    load_mesh_from_reader(t, &mut std::fs::File::open(path)?)
}
pub fn load_mesh_from_reader(
    t: MeshFileType,
    r: &mut (impl Read + Seek),
) -> Result<LoadedMesh, LoadMeshError> {
    use MeshFileType::*;
    let (graph, mut warnings) = match t {
        Stl | StlBin => {
            let (g, w) = if t == Stl {
                crate::stl::read_stl(r)?
            } else {
                crate::stl::read_stlb(r)?
            };
            (
                g,
                w.into_iter().map(LoadMeshWarning::Stl).collect::<Vec<_>>(),
            )
        }
        StlAscii => (crate::stl::read_stla(r)?, Vec::new()),
        Obj => {
            let (g, w) = crate::obj::read_obj(r)?;
            (g, w.into_iter().map(LoadMeshWarning::Obj).collect())
        }
        _3mf => {
            let (g, w) = crate::threemf::read_threemf(r)?;
            (g, w.into_iter().map(LoadMeshWarning::ThreeMf).collect())
        }
        _ => return Err(LoadMeshError::UnsupportedFormat(t)),
    };
    if let Some(w) = scene_shared_vertices_warning(&graph) {
        warnings.push(LoadMeshWarning::Scene(w))
    }
    Ok(LoadedMesh { graph, warnings })
}
pub fn scene_shared_vertices_warning(g: &Scene) -> Option<SceneLoadWarning> {
    // One placement cannot share coordinates with another leaf. Avoid building
    // a coordinate table for the common single-model import.
    (g.expanded_leaf_count(CsgPolicy::Descend).ok()? >= 2).then_some(())?;
    let mut seen: HashMap<[u32; 3], sc_scene::NodeId> = HashMap::new();
    let mut affected = BTreeSet::new();
    let mut names = BTreeMap::new();
    let _: Result<(), sc_scene::SceneError> =
        g.for_each_expanded_leaf(CsgPolicy::Descend, |id, _, m| {
            names
                .entry(id)
                .or_insert_with(|| m.meta.as_ref().and_then(|m| m.name.clone()));
            for v in &m.vertices {
                let key = v.coords().map(|x| if x == 0. { 0 } else { x.to_bits() });
                if let Some(other) = seen.get(&key) {
                    if *other != id {
                        affected.insert(*other);
                        affected.insert(id);
                    }
                } else {
                    seen.insert(key, id);
                }
            }
            Ok(())
        });
    if affected.is_empty() {
        None
    } else {
        Some(SceneLoadWarning::SharedVertices {
            leaves: affected
                .into_iter()
                .map(|node_id| SharedVertexLeaf {
                    node_id,
                    object_name: names.remove(&node_id).flatten(),
                })
                .collect(),
        })
    }
}
