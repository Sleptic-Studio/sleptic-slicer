// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{contour::ContourStack, error::*, profile::SliceProfile, section::section};
use rayon::prelude::*;
use sc_mesh_core::{
    IndexedMesh,
    indexed_mesh::{MeshTopology, ValidatedEdgeManifoldMesh},
};
use sc_scene::{NodeId, NodeKind, Scene, bake_leaf_to_world};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceProgress {
    pub completed: u32,
    pub total: u32,
}
impl SliceProgress {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.
        } else {
            self.completed as f32 / self.total as f32
        }
    }
}
#[derive(Debug)]
pub enum SliceOutcome<T> {
    Completed(T),
    Cancelled,
}
#[derive(Debug, Clone)]
pub struct LeafSlices {
    leaf: NodeId,
    leaf_name: Option<String>,
    stack: ContourStack,
    tolerant: bool,
    discarded_fragments: usize,
}
impl LeafSlices {
    pub fn leaf(&self) -> NodeId {
        self.leaf
    }
    pub fn leaf_name(&self) -> Option<&str> {
        self.leaf_name.as_deref()
    }
    pub fn stack(&self) -> &ContourStack {
        &self.stack
    }
    pub fn into_stack(self) -> ContourStack {
        self.stack
    }
    pub fn tolerant(&self) -> bool {
        self.tolerant
    }
    pub fn dropped_fragments(&self) -> usize {
        self.discarded_fragments
    }
    pub fn discarded_fragments(&self) -> usize {
        self.discarded_fragments
    }
}
pub fn sliceable_leaves(scene: &Scene) -> Result<Vec<NodeId>, SliceSceneError> {
    fn visit(s: &Scene, id: NodeId, out: &mut Vec<NodeId>) -> Result<(), SliceSceneError> {
        if let Some(n) = s.node(id) {
            match &n.kind {
                NodeKind::Leaf(_) => out.push(id),
                NodeKind::Group { children } => {
                    for &c in children {
                        visit(s, c, out)?
                    }
                }
                NodeKind::Csg { .. } => {
                    return Err(SliceSceneError::CsgNotYetSupported { node_id: id });
                }
                NodeKind::Reference { .. } => {
                    return Err(SliceSceneError::SubtreeReferenceNotYetSupported { node_id: id });
                }
            }
        }
        Ok(())
    }
    let mut out = vec![];
    for &id in scene.roots() {
        visit(scene, id, &mut out)?
    }
    if out.is_empty() {
        Err(SliceSceneError::EmptyScene)
    } else {
        Ok(out)
    }
}
fn validate(
    mesh: &IndexedMesh,
    allow: bool,
) -> Result<Option<ValidatedEdgeManifoldMesh<'_>>, SliceError> {
    mesh.validate_vertices()
        .map_err(|errors| SliceError::VerticesValidationFailed { errors })?;
    let faces = mesh
        .validated_faces()
        .map_err(|errors| SliceError::FacesValidationFailed { errors })?;
    match faces.validated_edge_manifold() {
        Ok(v) => Ok(Some(v)),
        Err(_) if allow => Ok(None),
        Err(errors) => Err(SliceError::ManifoldCheckFailed { errors }),
    }
}
fn run(
    mesh: &IndexedMesh,
    profile: &SliceProfile,
    validated: Option<&ValidatedEdgeManifoldMesh<'_>>,
    cancel: &CancelToken,
    completed: &AtomicU32,
    total: u32,
    progress: &(impl Fn(SliceProgress) + Sync),
) -> Result<Option<(ContourStack, usize)>, SliceError> {
    let tolerant = validated.is_none();
    if tolerant {
        for v in &mesh.vertices {
            for axis in 0..2 {
                let value = v[axis] as f64 * 100000.;
                if value.abs() >= (1u64 << 61) as f64 {
                    return Err(SliceError::TolerantCoordinateOutOfRange);
                }
            }
        }
    }
    let zs: Vec<_> = profile.schedule().iter_z().collect();
    let mut counts = vec![0usize; zs.len() + 1];
    let ranges: Vec<_> = mesh
        .faces
        .iter()
        .map(|f| {
            let z = f.vertices.map(|i| mesh.vertices[usize::from(i)][2] as f64);
            let lo = z.into_iter().fold(f64::INFINITY, f64::min) - 1e-6;
            let hi = z.into_iter().fold(f64::NEG_INFINITY, f64::max) + 1e-6;
            let a = zs.partition_point(|&z| z < lo);
            let b = zs.partition_point(|&z| z <= hi);
            for c in &mut counts[a + 1..b + 1] {
                *c += 1
            }
            (a, b)
        })
        .collect();
    for i in 1..counts.len() {
        counts[i] += counts[i - 1]
    }
    let mut cursor = counts[..zs.len()].to_vec();
    let mut indices = vec![0usize; *counts.last().unwrap()];
    for (fi, &(a, b)) in ranges.iter().enumerate() {
        for c in &mut cursor[a..b] {
            indices[*c] = fi;
            *c += 1
        }
    }
    let topology = validated.map(MeshTopology::from_edge_manifold);
    let results: Vec<_> = zs
        .par_iter()
        .enumerate()
        .map(|(i, &z)| {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            let result = section(
                mesh,
                &indices[counts[i]..counts[i + 1]],
                z,
                topology.as_ref(),
                tolerant,
            )?;
            let done = completed
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    Some(n.saturating_add(1))
                })
                .unwrap()
                .saturating_add(1)
                .min(total);
            progress(SliceProgress {
                completed: done,
                total,
            });
            Ok(Some(result))
        })
        .collect();
    let mut layers = Vec::with_capacity(zs.len());
    let mut discarded = 0;
    for result in results {
        let Some((layer, count)) = result? else {
            return Ok(None);
        };
        layers.push(layer);
        discarded += count;
    }
    let stack = (ContourStack::new_unchecked(layers), discarded);
    Ok((!cancel.is_cancelled()).then_some(stack))
}
fn selected(scene: &Scene, id: NodeId) -> Result<(), SliceError> {
    match &scene
        .node(id)
        .ok_or(SliceError::NodeNotFound { node_id: id })?
        .kind
    {
        NodeKind::Leaf(_) => Ok(()),
        NodeKind::Group { .. } => Err(SliceError::NotALeaf { node_id: id }),
        NodeKind::Csg { .. } => Err(SliceError::CsgNotYetSupported { node_id: id }),
        NodeKind::Reference { .. } => {
            Err(SliceError::SubtreeReferenceNotYetSupported { node_id: id })
        }
    }
}
pub fn slice_leaf(
    scene: &Scene,
    leaf: NodeId,
    profile: &SliceProfile,
) -> Result<ContourStack, SliceError> {
    selected(scene, leaf)?;
    let mesh =
        bake_leaf_to_world(scene, leaf).map_err(|source| SliceError::BakeFailed { source })?;
    let validated = validate(&mesh, false)?;
    Ok(run(
        &mesh,
        profile,
        validated.as_ref(),
        &CancelToken::new(),
        &AtomicU32::new(0),
        profile.schedule().total_layers(),
        &|_| {},
    )?
    .unwrap()
    .0)
}
pub fn slice_scene(
    scene: &Scene,
    profile: &SliceProfile,
) -> Result<Vec<LeafSlices>, SliceSceneError> {
    match slice_scene_with_fallback(scene, profile, &CancelToken::new(), |_| {}, false)? {
        SliceOutcome::Completed(v) => Ok(v),
        SliceOutcome::Cancelled => unreachable!(),
    }
}
pub fn slice_scene_with_fallback(
    scene: &Scene,
    profile: &SliceProfile,
    cancel: &CancelToken,
    progress: impl Fn(SliceProgress) + Sync,
    allow: bool,
) -> Result<SliceOutcome<Vec<LeafSlices>>, SliceSceneError> {
    let ids = sliceable_leaves(scene)?;
    let total = (ids.len() as u32).saturating_mul(profile.schedule().total_layers());
    let completed = AtomicU32::new(0);
    let mut output = Vec::with_capacity(ids.len());
    let mut failures = vec![];
    for leaf in ids {
        if cancel.is_cancelled() {
            return Ok(SliceOutcome::Cancelled);
        }
        let leaf_name = scene
            .node(leaf)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.name.clone())
            .or_else(|| {
                scene
                    .mesh_at_leaf(leaf)
                    .ok()
                    .and_then(|m| m.meta.as_ref())
                    .and_then(|m| m.name.clone())
            });
        let fail = |error| SliceSceneError::LeafSlicingFailed {
            leaf,
            leaf_name: leaf_name.clone(),
            error,
        };
        let mesh = bake_leaf_to_world(scene, leaf)
            .map_err(|source| fail(SliceError::BakeFailed { source }))?;
        let validated = match validate(&mesh, allow) {
            Ok(t) => t,
            Err(error) => {
                let geometry = match error {
                    SliceError::VerticesValidationFailed { errors } => {
                        InvalidLeafGeometry::Vertices(errors)
                    }
                    SliceError::FacesValidationFailed { errors } => {
                        InvalidLeafGeometry::Faces(errors)
                    }
                    SliceError::ManifoldCheckFailed { errors } => {
                        InvalidLeafGeometry::Manifold(errors)
                    }
                    _ => return Err(fail(error)),
                };
                failures.push(LeafValidationFailure {
                    leaf,
                    leaf_name,
                    geometry,
                });
                continue;
            }
        };
        if !failures.is_empty() {
            continue;
        }
        let mut tolerant = validated.is_none();
        let attempt = run(
            &mesh,
            profile,
            validated.as_ref(),
            cancel,
            &completed,
            total,
            &progress,
        );
        let result = match attempt {
            Err(SliceError::NonManifoldDuringStitching { .. }) if allow => {
                tolerant = true;
                run(&mesh, profile, None, cancel, &completed, total, &progress)
            }
            r => r,
        }
        .map_err(fail)?;
        let Some((stack, discarded_fragments)) = result else {
            return Ok(SliceOutcome::Cancelled);
        };
        output.push(LeafSlices {
            leaf,
            leaf_name,
            stack,
            tolerant,
            discarded_fragments,
        })
    }
    if failures.is_empty() {
        Ok(SliceOutcome::Completed(output))
    } else {
        Err(SliceSceneError::LeavesValidationFailed(failures))
    }
}
