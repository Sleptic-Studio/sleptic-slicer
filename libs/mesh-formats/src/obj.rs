// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    error::{LoadObjError as E, WriteObjError},
    scene_writer::{WriteSceneProgress, tick},
};
use sc_mesh_core::{
    IndexedMesh, IndexedTriangle, MeshMetadata, Normal, Vertex, transform::Transform3D,
};
use sc_scene::{CsgPolicy, LeafPayload, Scene, SceneMetadata, bake_mesh_to_world};
use std::{
    collections::HashMap,
    io::{Read, Seek, Write},
    sync::Arc,
};
/// Something an OBJ file had (or lacked) that a resin print does without:
/// each skipped line is reported with its line number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadObjWarning {
    NoFaces,
    NoVertices,
    SkippedComment { line: usize },
    SkippedVertexNormal { line: usize },
    SkippedTextureCoordinates { line: usize },
    SkippedSmoothingGroup { line: usize },
    SkippedGroup { name: String, line: usize },
    SkippedMaterialLibrary { path: String, line: usize },
    SkippedMaterialReference { material: String, line: usize },
    SkippedUnknownDirective { directive: String, line: usize },
}

fn utf8(b: &[u8]) -> Result<&str, E> {
    std::str::from_utf8(b).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
        .into()
    })
}
fn number(s: &str, line: usize) -> Result<f32, E> {
    let v: f32 = s.parse().map_err(|source| E::InvalidFloat {
        line,
        token: s.into(),
        source,
    })?;
    if !v.is_finite() {
        return Err(E::NonFiniteFloat { line, value: v });
    }
    Ok(v)
}
struct Pending {
    name: Option<String>,
    vertices: Vec<Vertex>,
    faces: Vec<IndexedTriangle>,
    map: HashMap<usize, u32>,
}
impl Pending {
    fn new(name: Option<String>) -> Self {
        Self {
            name,
            vertices: Vec::new(),
            faces: Vec::new(),
            map: HashMap::new(),
        }
    }
    fn flush(
        &mut self,
        g: &mut Scene,
        root: sc_scene::NodeId,
        count: &mut usize,
        line: usize,
    ) -> Result<(), E> {
        if self.faces.is_empty() {
            return Ok(());
        }
        if *count >= 100000 {
            return Err(E::ObjectCountExceedsLimit {
                line,
                limit: 100000,
            });
        }
        *count += 1;
        let meta = self.name.take().map(|name| MeshMetadata {
            name: Some(name),
            header_comment: None,
        });
        let m = IndexedMesh::new(
            meta,
            std::mem::take(&mut self.vertices),
            std::mem::take(&mut self.faces),
        );
        self.map.clear();
        let id = g
            .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(m)))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        g.add_child(root, id)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }
}
pub fn read_obj(r: &mut (impl Read + Seek)) -> Result<(Scene, Vec<LoadObjWarning>), E> {
    let mut b = Vec::new();
    r.read_to_end(&mut b)?;
    while b.last() == Some(&0) {
        b.pop();
    }
    let mut g = Scene::new();
    let root = g.add_group(None, Transform3D::Identity);
    g.add_root(root)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut global = Vec::new();
    let mut pending = Pending::new(None);
    let mut count = 0;
    let mut warnings = Vec::new();
    let mut header = true;
    let mut comments = Vec::new();
    let mut last = 0;
    for (i, b) in b.split_inclusive(|c| *c == b'\n').enumerate() {
        let line = i + 1;
        last = line;
        let s = utf8(b)?.trim();
        let mut it = s.split_whitespace();
        let Some(d) = it.next() else { continue };
        let rest = s[d.len()..].trim();
        if d.starts_with('#') {
            if header {
                comments.push(s[1..].trim().to_string())
            } else {
                warnings.push(LoadObjWarning::SkippedComment { line })
            }
            continue;
        }
        match d {
            "v" => {
                header = false;
                if global.len() >= crate::MAX_MESH_ELEMENT_COUNT {
                    return Err(E::VertexCountExceedsLimit {
                        line,
                        limit: crate::MAX_MESH_ELEMENT_COUNT,
                    });
                }
                let t: Vec<_> = it.collect();
                if t.len() != 3 && t.len() != 4 {
                    return Err(E::VertexCoordCountInvalid { line, got: t.len() });
                }
                let mut a = [
                    number(t[0], line)?,
                    number(t[1], line)?,
                    number(t[2], line)?,
                ];
                if t.len() == 4 {
                    let w = number(t[3], line)?;
                    if w.abs().to_bits() <= 2 {
                        return Err(E::VertexWZero { line });
                    }
                    for v in &mut a {
                        *v /= w;
                        if !v.is_finite() {
                            return Err(E::NonFiniteFloat { line, value: *v });
                        }
                    }
                }
                global
                    .try_reserve(1)
                    .map_err(|_| E::OutOfMemoryVertices { line })?;
                global.push(Vertex::new(a));
            }
            "o" => {
                header = false;
                pending.flush(&mut g, root, &mut count, line)?;
                pending = Pending::new(if rest.is_empty() {
                    None
                } else {
                    Some(rest.into())
                });
            }
            "f" => {
                header = false;
                let t: Vec<_> = it.collect();
                if t.len() < 3 {
                    return Err(E::FaceTooFewVertices { line, got: t.len() });
                }
                if t.len() > 1024 {
                    return Err(E::FaceExceedsTokenLimit { line, limit: 1024 });
                }
                let mut indices = [0u32; 1024];
                for (j, token) in t.iter().enumerate() {
                    let head = token.split('/').next().unwrap_or("");
                    let index: i64 = head.parse().map_err(|source| E::InvalidFaceIndex {
                        line,
                        token: (*token).into(),
                        source,
                    })?;
                    if index == 0 {
                        return Err(E::FaceIndexZero { line });
                    }
                    let absolute = if index < 0 {
                        if index.checked_neg().is_none() {
                            return Err(E::NegativeFaceIndexOutOfRange { line, index });
                        }
                        global.len() as i64 + index
                    } else {
                        index - 1
                    };
                    if absolute < 0 || absolute >= global.len() as i64 {
                        return Err(E::FaceIndexOutOfRange {
                            line,
                            index,
                            vertex_count: global.len(),
                        });
                    }
                    let next = pending.vertices.len() as u32;
                    pending
                        .vertices
                        .try_reserve(1)
                        .map_err(|_| E::OutOfMemoryVertices { line })?;
                    pending
                        .map
                        .try_reserve(1)
                        .map_err(|_| E::OutOfMemoryVertices { line })?;
                    let local = *pending.map.entry(absolute as usize).or_insert_with(|| {
                        pending.vertices.push(global[absolute as usize]);
                        next
                    });
                    indices[j] = local;
                }
                pending
                    .faces
                    .try_reserve(t.len() - 2)
                    .map_err(|_| E::OutOfMemoryFaces { line })?;
                for j in 1..t.len() - 1 {
                    let ids = [indices[0], indices[j], indices[j + 1]];
                    let vs = ids.map(|id| pending.vertices[id as usize]);
                    let n = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(&vs)
                        .unwrap_or(Normal::new([0.; 3]));
                    pending
                        .faces
                        .push(IndexedTriangle::from_raw_indices(n, ids));
                }
            }
            "vt" => warnings.push(LoadObjWarning::SkippedTextureCoordinates { line }),
            "vn" => warnings.push(LoadObjWarning::SkippedVertexNormal { line }),
            "g" => warnings.push(LoadObjWarning::SkippedGroup {
                line,
                name: rest.into(),
            }),
            "s" => warnings.push(LoadObjWarning::SkippedSmoothingGroup { line }),
            "usemtl" => warnings.push(LoadObjWarning::SkippedMaterialReference {
                line,
                material: rest.into(),
            }),
            "mtllib" => warnings.push(LoadObjWarning::SkippedMaterialLibrary {
                line,
                path: rest.into(),
            }),
            "l" | "p" | "vp" | "cstype" | "deg" | "bmat" | "step" | "curv" | "curv2" | "surf"
            | "parm" | "trim" | "hole" | "scrv" | "sp" | "end" | "con" | "mg" | "bevel"
            | "c_interp" | "d_interp" | "lod" | "shadow_obj" | "trace_obj" | "ctech" | "stech"
            | "maplib" | "usemap" => warnings.push(LoadObjWarning::SkippedUnknownDirective {
                line,
                directive: d.into(),
            }),
            _ => {
                return Err(E::UnrecognizedDirective {
                    line,
                    directive: d.into(),
                });
            }
        }
    }
    pending.flush(&mut g, root, &mut count, last)?;
    if !comments.is_empty() {
        g.set_meta(Some(SceneMetadata {
            name: None,
            comment: Some(comments.join("\n")),
        }));
    }
    if count == 0 {
        if global.is_empty() {
            warnings.push(LoadObjWarning::NoVertices)
        }
        warnings.push(LoadObjWarning::NoFaces)
    }
    Ok((g, warnings))
}
pub fn write_obj(g: &Scene, w: &mut impl Write) -> Result<(), WriteObjError> {
    write_obj_with_progress(g, w, &mut |_| {})
}
pub fn write_obj_with_progress(
    g: &Scene,
    w: &mut impl Write,
    cb: &mut impl FnMut(WriteSceneProgress),
) -> Result<(), WriteObjError> {
    let count = g.expanded_leaf_count(CsgPolicy::Descend)?;
    if count > 100000 {
        return Err(WriteObjError::LeafBudgetExceeded { limit: 100000 });
    }
    let mut total = 0;
    g.for_each_expanded_leaf::<WriteObjError, _>(CsgPolicy::Descend, |_, _, m| {
        if !m.faces.is_empty() {
            total += m.vertices.len() + m.faces.len();
        }
        Ok(())
    })?;
    tick(cb, 0, total);
    if let Some(c) = g.meta().as_ref().and_then(|m| m.comment.as_deref()) {
        for l in c.split('\n') {
            writeln!(w, "# {l}")?
        }
    }
    let mut offset = 0u32;
    let mut done = 0;
    g.for_each_expanded_leaf::<WriteObjError, _>(CsgPolicy::Descend, |_, t, m| {
        if m.faces.is_empty() {
            return Ok(());
        }
        let mut baked = bake_mesh_to_world(m, t)?;
        baked.meta = m.meta.clone();
        let m = baked;
        if let Some(name) = m
            .meta
            .as_ref()
            .and_then(|m| m.name.as_deref())
            .and_then(|s| crate::clean_name(s, true))
        {
            writeln!(w, "o {name}")?
        } else if count > 1 {
            writeln!(w, "o")?
        }
        let added =
            u32::try_from(m.vertices.len()).map_err(|_| WriteObjError::VertexCountExceedsU32 {
                vertex_count: m.vertices.len(),
            })?;
        for v in &m.vertices {
            let [x, y, z] = v.coords();
            writeln!(w, "v {x} {y} {z}")?;
            done += 1;
            tick(cb, done, total)
        }
        for f in &m.faces {
            let mut ids = [0; 3];
            for (i, v) in f.vertices.iter().enumerate() {
                ids[i] = v
                    .get()
                    .checked_add(offset)
                    .and_then(|v| v.checked_add(1))
                    .ok_or(WriteObjError::FaceIndexOverflow {
                        local: v.get(),
                        offset,
                    })?
            }
            writeln!(w, "f {} {} {}", ids[0], ids[1], ids[2])?;
            done += 1;
            tick(cb, done, total)
        }
        offset = offset
            .checked_add(added)
            .ok_or(WriteObjError::VertexOffsetOverflow { offset, added })?;
        Ok(())
    })?;
    w.flush()?;
    Ok(())
}
