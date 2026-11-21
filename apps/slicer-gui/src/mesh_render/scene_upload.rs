// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The scene on the GPU: a vertex and index buffer per part, in the part's
//! own coordinates, and a table of per-part entries (placement, selection,
//! warnings, painted support regions) the parts pass reads one at a time.
use std::fmt;

use anyhow::{Result, anyhow};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use sc_mesh_core::primitives::Vertex;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_core::{Aabb, IndexedMesh};
use sc_scene::{CsgPolicy, NodeId, Scene, SceneError};
use slint::wgpu_29::wgpu;

use super::device_loss::DeviceLoss;
use super::device_ops::Guarded;
use crate::build_volume::BuildVolume;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(super) struct SceneUploadVertex {
    vertex_coordinates: [f32; 3],
    /// The smoothed normal (R90, `smooth_normals`).
    normal: [f32; 3],
}

/// One part's entry, as `Part` in `mesh_render.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(super) struct PartTransformEntry {
    part_transform: [[f32; 4]; 4],
    /// x: selected; y: sticks out of the build volume; z: 1 a support, 2 a
    /// support left over from before an edit; w: bit 0 a preview seam, then
    /// the plate's grid column from bit 1 and row from bit 16.
    pub(super) flags: [u32; 4],
    /// Painted support regions: centre and radius, the radius negative for
    /// "no supports here".
    paint: [[f32; 4]; 256],
}

pub(super) const PART_TRANSFORM_ENTRY_BYTES: u64 = size_of::<PartTransformEntry>() as u64;

/// `size` rounded up to a multiple of `alignment`, a power of two (a
/// device's uniform offset alignment always is).
pub(super) const fn round_buffer_size_up(size: u64, alignment: u64) -> u64 {
    let below = alignment - 1;
    (size + below) & !below
}

/// One part's buffers. Its placement is not baked in: it comes from the
/// part's entry when drawn. `node_id` ties the draw to the scene, for the
/// selection outline.
pub(super) struct ScenePartDraw {
    upload_part_node: NodeId,
    draw_vertex_storage: wgpu::Buffer,
    draw_index_storage: wgpu::Buffer,
    draw_index_total: u32,
}

impl ScenePartDraw {
    pub(super) fn upload_part_node(&self) -> NodeId {
        self.upload_part_node
    }

    pub(super) fn draw_vertex_storage(&self) -> &wgpu::Buffer {
        &self.draw_vertex_storage
    }

    pub(super) fn draw_index_storage(&self) -> &wgpu::Buffer {
        &self.draw_index_storage
    }

    pub(super) fn draw_index_total(&self) -> u32 {
        self.draw_index_total
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartBufferCategory {
    VertexData,
    IndexData,
}

/// A part too big for one buffer on this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartBufferOverLimit {
    pub upload_part_node: NodeId,
    pub oversized_part_name: Option<String>,
    pub oversized_buffer_kind: PartBufferCategory,
    /// Vertices, or triangles for the index buffer.
    pub oversized_element_count: usize,
    pub upload_buffer_byte_count: u64,
    pub device_buffer_limit: u64,
}

impl fmt::Display for PartBufferOverLimit {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let counted = if self.oversized_buffer_kind == PartBufferCategory::IndexData {
            "triangles"
        } else {
            "vertices"
        };
        let subject = self
            .oversized_part_name
            .as_ref()
            .map_or_else(|| "An object".to_string(), |name| format!("Object '{name}'"));
        let count = self.oversized_element_count;
        write!(
            out,
            "{subject} has more {counted} ({count}) than this graphics device can hold in one buffer"
        )
    }
}

impl std::error::Error for PartBufferOverLimit {}

/// The device refused a buffer.
#[derive(Debug)]
pub struct SceneBufferRefused {
    /// What it was for, in the user's words ("index buffer of object
    /// 'hull'", "per-object transform table").
    pub refused_buffer_description: String,
    pub upload_buffer_byte_count: u64,
    pub upload_error_source: wgpu::Error,
}

impl fmt::Display for SceneBufferRefused {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { refused_buffer_description: what, upload_buffer_byte_count: num_bytes, .. } = self;
        write!(out, "The graphics device refused the {what} ({num_bytes} bytes)")
    }
}

impl std::error::Error for SceneBufferRefused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.upload_error_source)
    }
}

#[derive(Debug)]
pub struct SceneUploadDeviceLost;

impl fmt::Display for SceneUploadDeviceLost {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "The graphics device was lost while uploading the scene")
    }
}

impl std::error::Error for SceneUploadDeviceLost {}

/// "vertex buffer of object 'name'", or "of an object" when unnamed.
fn part_buffer(buffer: &str, name: Option<&str>) -> String {
    let Some(name) = name else {
        return format!("{buffer} of an object");
    };
    format!("{buffer} of object '{name}'")
}

/// A buffer filled with `contents`; a refusal or a lost device is an
/// error, never a panic. Empty contents give an empty buffer.
fn upload_scene_buffer(
    device: &wgpu::Device,
    device_loss: &DeviceLoss,
    label: &'static str,
    contents: &[u8],
    usage: wgpu::BufferUsages,
    what: impl FnOnce() -> String,
) -> Result<wgpu::Buffer> {
    // A buffer mapped when made needs a non-zero size that is a multiple of
    // the copy alignment.
    let size = match contents.len() as u64 {
        0 => 0,
        length => round_buffer_size_up(length, wgpu::COPY_BUFFER_ALIGNMENT).max(wgpu::COPY_BUFFER_ALIGNMENT),
    };
    let descriptor = wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: size > 0,
    };
    let buffer =
        device.try_buffer(&descriptor).map_err(|source| SceneBufferRefused {
            refused_buffer_description: what(),
            upload_buffer_byte_count: size,
            upload_error_source: source,
        })?;
    if contents.is_empty() {
        return Ok(buffer);
    }
    if device_loss.is_lost() {
        return Err(SceneUploadDeviceLost.into());
    }
    // wgpu panics instead of reporting when a buffer of a lost device is
    // mapped. Nothing here can be left half-written by that panic: wgpu has
    // already failed inside before its API layer panics.
    let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        buffer
            .get_mapped_range_mut(..)
            .slice(..contents.len())
            .copy_from_slice(contents);
        buffer.unmap();
    }));
    match written {
        Ok(()) => Ok(buffer),
        Err(_) => Err(SceneUploadDeviceLost.into()),
    }
}

/// A scene's GPU copy: a draw per part, in the order
/// `Scene::for_each_expanded_leaf` visits them, and the entry table, each
/// entry at a multiple of the device's uniform offset alignment.
///
/// Building it needs a device but not the UI, so it can run on the thread
/// that loaded the scene; the UI thread installs it (`Renderer::install_scene`).
/// The bounds are taken once, to frame the camera.
pub struct UploadedScene {
    upload_scene_parts: Vec<ScenePartDraw>,
    scene_transform_buffer: wgpu::Buffer,
    /// Bytes from one entry to the next.
    scene_transform_stride: u64,
    /// The device it was built on; it is only installed on that one.
    scene_device_key: super::DeviceStamp,
    upload_bounds_min: Vec3,
    upload_bounds_max: Vec3,
}

// Buffers print as nothing useful; the part count, stride and bounds do.
impl fmt::Debug for UploadedScene {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut shown = out.debug_struct("GpuScene");
        shown.field("objects", &self.scene_draw_count());
        shown.field("model_stride", &self.scene_transform_stride);
        shown.field("aabb_min", &self.upload_bounds_min).field("aabb_max", &self.upload_bounds_max);
        shown.finish_non_exhaustive()
    }
}

/// Progress of a build, in triangles. Each part counts twice, once when
/// its vertices are prepared and once when its buffers are made, so the
/// total is twice the scene's triangles and the last report reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneUploadProgress {
    pub uploaded_triangle_count: u64,
    pub upload_triangle_total: u64,
}

impl UploadedScene {
    pub fn create_gpu_scene(
        device: &wgpu::Device,
        device_token: super::DeviceStamp,
        graph: &Scene,
        device_loss: &DeviceLoss,
    ) -> Result<Self> {
        let ignore_progress = |_: SceneUploadProgress| {};
        Self::create_gpu_scene_reporting(device, device_token, graph, device_loss, ignore_progress)
    }

    /// [`GpuScene::build`], reporting progress after each step of a part.
    pub fn create_gpu_scene_reporting(
        device: &wgpu::Device,
        device_token: super::DeviceStamp,
        graph: &Scene,
        device_loss: &DeviceLoss,
        mut progress: impl FnMut(SceneUploadProgress),
    ) -> Result<Self> {
        let mut triangles = 0u64;
        each_part(graph, |_, _, mesh| triangles += mesh.faces().len() as u64)?;
        let total_triangles = triangles.saturating_mul(2);
        let mut processed_triangles = 0u64;
        let mut advance = |done: u64| {
            processed_triangles += done;
            progress(SceneUploadProgress {
                uploaded_triangle_count: processed_triangles,
                upload_triangle_total: total_triangles,
            });
        };
        let data = build_gpu_scene_data(graph, &mut advance)?;
        let largest_buffer = device.limits().max_buffer_size;
        check_part_buffer_limits(&data.upload_scene_parts, largest_buffer)?;
        let mut objects = Vec::with_capacity(data.upload_scene_parts.len());
        for part in &data.upload_scene_parts {
            let name = part.part_display_name.as_deref();
            // The vertices first, then the indices; a refusal names the part.
            let upload = |label: &'static str, bytes: &[u8], usage: wgpu::BufferUsages, buffer: &str| {
                upload_scene_buffer(device, device_loss, label, bytes, usage, || part_buffer(buffer, name))
            };
            let vertex_buffer = upload(
                "mesh-viewer object vertex buffer",
                bytemuck::cast_slice(&part.part_vertex_data),
                wgpu::BufferUsages::VERTEX,
                "vertex buffer",
            )?;
            let index_buffer = upload(
                "mesh-viewer object index buffer",
                bytemuck::cast_slice(&part.part_index_data),
                wgpu::BufferUsages::INDEX,
                "index buffer",
            )?;
            let index_count = part.part_index_data.len() as u32;
            let node_id = part.upload_part_node;
            objects.push(ScenePartDraw { upload_part_node: node_id, draw_vertex_storage: vertex_buffer, draw_index_storage: index_buffer, draw_index_total: index_count });
            advance((part.part_index_data.len() / 3) as u64);
        }
        let model_stride = round_buffer_size_up(
            PART_TRANSFORM_ENTRY_BYTES,
            u64::from(device.limits().min_uniform_buffer_offset_alignment),
        );
        // Never zero: a scene without parts was refused above.
        let model_size = objects.len() as u64 * model_stride;
        let descriptor = wgpu::BufferDescriptor {
            label: Some("mesh-viewer model uniform"),
            size: model_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        };
        let model_buffer =
            device.try_buffer(&descriptor).map_err(|source| SceneBufferRefused {
                refused_buffer_description: "per-object transform table".to_string(),
                upload_buffer_byte_count: model_size,
                upload_error_source: source,
            })?;
        let (aabb_min, aabb_max) = (data.upload_bounds_min, data.upload_bounds_max);
        Ok(Self { upload_scene_parts: objects, scene_transform_buffer: model_buffer, scene_transform_stride: model_stride, scene_device_key: device_token, upload_bounds_min: aabb_min, upload_bounds_max: aabb_max })
    }

    pub fn scene_device_key(&self) -> super::DeviceStamp {
        self.scene_device_key
    }

    pub(super) fn scene_draw_count(&self) -> usize {
        self.upload_scene_parts.len()
    }

    pub(super) fn upload_scene_parts(&self) -> &[ScenePartDraw] {
        &self.upload_scene_parts
    }

    pub(super) fn scene_transform_buffer(&self) -> &wgpu::Buffer {
        &self.scene_transform_buffer
    }

    pub(super) fn scene_transform_stride(&self) -> u64 {
        self.scene_transform_stride
    }

    pub(super) fn uploaded_scene_bounds(&self) -> (Vec3, Vec3) {
        (self.upload_bounds_min, self.upload_bounds_max)
    }
}

/// Calls `visit` for every part the parts pass draws, in drawing order.
fn each_part(
    graph: &Scene,
    mut visit: impl FnMut(NodeId, &Transform3D, &IndexedMesh),
) -> Result<()> {
    graph.for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Descend, |id, transform, mesh| {
        visit(id, transform, mesh);
        Ok(())
    })?;
    Ok(())
}

/// A placement as a column-major glam matrix, in f32 for the GPU.
fn transform_to_mat4(t: &Transform3D) -> Mat4 {
    let Transform3D::Affine(a) = t else {
        return Mat4::IDENTITY;
    };
    affine_to_gpu_matrix(a)
}

fn affine_to_gpu_matrix(a: &AffineTransform3D) -> Mat4 {
    let (l, t) = (a.linear(), a.translation());
    let column = |c: usize| Vec4::new(l[0][c] as f32, l[1][c] as f32, l[2][c] as f32, 0.0);
    let offset = Vec4::new(t[0] as f32, t[1] as f32, t[2] as f32, 1.0);
    Mat4::from_cols(column(0), column(1), column(2), offset)
}

/// Every part's entry, in drawing order. A `selection` that is not a drawn
/// part selects nothing.
pub(super) fn build_part_transform_entries(
    graph: &Scene,
    selection: Option<NodeId>,
    build_volume: BuildVolume,
) -> Result<Vec<PartTransformEntry>> {
    let plates = &graph.plates().order;
    // The plate grid's width, in plates (as `plate_grid_offset` lays it).
    let columns = (plates.len().max(1) as f64).sqrt().ceil() as usize;
    let mut entries = Vec::new();
    each_part(graph, |id, transform, mesh| {
        let comment = graph
            .node(id)
            .and_then(|node| node.meta.as_ref())
            .and_then(|meta| meta.comment.as_deref());
        let mut paint = [[0.0; 4]; 256];
        for (slot, region) in paint.iter_mut().zip(
            comment
                .map(sc_print_glue::supports::painted_regions)
                .into_iter()
                .flatten(),
        ) {
            let [x, y, z] = region.centre;
            *slot = [
                x,
                y,
                z,
                if region.keep {
                    region.radius
                } else {
                    -region.radius
                },
            ];
        }
        let plate = graph.plate_of(id);
        let [dx, dy] = graph.plate_offset(plate, build_volume.width_mm(), build_volume.depth_mm());
        let model = Mat4::from_translation(Vec3::new(dx, dy, 0.0)) * transform_to_mat4(transform);
        let support = if crate::scene_document::support_is_stale(graph, id) {
            2
        } else {
            u32::from(comment.is_some_and(|c| c.starts_with("sc-generated-support")))
        };
        let seam = mesh
            .meta
            .as_ref()
            .and_then(|meta| meta.header_comment.as_deref())
            == Some("sc-preview-seam");
        let index = plates.iter().position(|p| p.id == plate).unwrap_or(0);
        let cell = (((index % columns) as u32) << 1) | (((index / columns) as u32) << 16);
        entries.push(PartTransformEntry {
            part_transform: model.to_cols_array_2d(),
            flags: [
                u32::from(selection == Some(id)),
                u32::from(sticks_out(mesh, transform, &build_volume)),
                support,
                u32::from(seam) | cell,
            ],
            paint,
        });
    })?;
    Ok(entries)
}

/// Whether any of the placed part is outside the build volume. Its box
/// is outside exactly when a vertex is, the volume being a box itself.
fn sticks_out(mesh: &IndexedMesh, transform: &Transform3D, build_volume: &BuildVolume) -> bool {
    let matrix = transform_to_mat4(transform);
    let mut bounds = Aabb::empty();
    for v in mesh.vertices() {
        let p = matrix.transform_point3(Vec3::from(v.coords()));
        bounds.extend(Vertex::new([p.x, p.y, p.z]));
    }
    !build_volume.holds_box(bounds)
}

/// The plate shader's wall bits: four sides and the top light warning
/// walls; the floor (-Z) puts stripes on the plate itself.
pub(super) const OUTSIDE_FACE_LEFT: u32 = 1 << 0;
pub(super) const OUTSIDE_FACE_RIGHT: u32 = 1 << 1;
pub(super) const OUTSIDE_FACE_FRONT: u32 = 1 << 2;
pub(super) const OUTSIDE_FACE_BACK: u32 = 1 << 3;
pub(super) const OUTSIDE_FACE_TOP: u32 = 1 << 4;
pub(super) const OUTSIDE_FACE_BOTTOM: u32 = 1 << 5;

/// The volume's corners widened by the same micron of slack
/// `BuildVolume::holds_box` allows: a part settled on the plate can sit
/// a rounding error below z = 0 (R51: stripes on the plate mid-drag).
fn walls(build_volume: BuildVolume) -> (Vec3, Vec3) {
    let slack = Vec3::splat(crate::build_volume::CONTAINS_TOLERANCE_MM);
    (
        build_volume.envelope_min_mm() - slack,
        build_volume.envelope_max_mm() + slack,
    )
}

/// The walls a placed part's vertices pass. Checking vertices is exact: a
/// part crosses a wall of a box only where one of its vertices does.
fn walls_crossed(mesh: &IndexedMesh, transform: &Transform3D, (lo, hi): (Vec3, Vec3)) -> u32 {
    let matrix = transform_to_mat4(transform);
    let mut bits = 0;
    for v in mesh.vertices() {
        let p = matrix.transform_point3(Vec3::from(v.coords()));
        for (out, bit) in [
            (p.x < lo.x, OUTSIDE_FACE_LEFT),
            (p.x > hi.x, OUTSIDE_FACE_RIGHT),
            (p.y < lo.y, OUTSIDE_FACE_FRONT),
            (p.y > hi.y, OUTSIDE_FACE_BACK),
            (p.z < lo.z, OUTSIDE_FACE_BOTTOM),
            (p.z > hi.z, OUTSIDE_FACE_TOP),
        ] {
            if out {
                bits |= bit;
            }
        }
    }
    bits
}

/// The walls any part crosses.
pub(super) fn build_envelope_warning_mask(graph: &Scene, build_volume: BuildVolume) -> Result<u32> {
    let walls = walls(build_volume);
    let mut mask = 0;
    each_part(graph, |_, transform, mesh| {
        mask |= walls_crossed(mesh, transform, walls)
    })?;
    Ok(mask)
}

/// The walls each plate's own parts cross, in plate order (parts are kept
/// in their plate's coordinates, so a part only reaches its own plate's
/// walls).
pub(super) fn build_plate_warning_masks(
    graph: &Scene,
    build_volume: BuildVolume,
) -> Result<Vec<u32>> {
    let walls = walls(build_volume);
    let order: Vec<_> = graph.plates().order.iter().map(|plate| plate.id).collect();
    let mut masks = vec![0; order.len().max(1)];
    each_part(graph, |id, transform, mesh| {
        let plate = order
            .iter()
            .position(|&p| p == graph.plate_of(id))
            .unwrap_or(0);
        masks[plate] |= walls_crossed(mesh, transform, walls);
    })?;
    Ok(masks)
}

/// How many parts the scene draws; zero means there is nothing to upload.
/// A dangling id is an error, not zero.
pub(super) fn count_renderable_parts(graph: &Scene) -> Result<usize> {
    let mut count = 0;
    each_part(graph, |_, _, _| count += 1)?;
    Ok(count)
}

/// One part's vertices and triangles, before any buffer is made.
struct PreparedPartData {
    upload_part_node: NodeId,
    part_display_name: Option<String>,
    part_vertex_data: Vec<SceneUploadVertex>,
    part_index_data: Vec<u32>,
}

/// Refuses the first part, in drawing order, with a buffer over
/// `max_buffer_size`, its vertex buffer checked before its index buffer.
/// Vertex and index sizes are multiples of the copy alignment, so these are
/// the sizes that would be asked for.
fn check_part_buffer_limits(
    objects: &[PreparedPartData],
    max_buffer_size: u64,
) -> std::result::Result<(), PartBufferOverLimit> {
    for part in objects {
        let vertex_bytes = (part.part_vertex_data.len() * size_of::<SceneUploadVertex>()) as u64;
        let index_bytes = (part.part_index_data.len() * size_of::<u32>()) as u64;
        let over = if vertex_bytes > max_buffer_size {
            (PartBufferCategory::VertexData, part.part_vertex_data.len(), vertex_bytes)
        } else if index_bytes > max_buffer_size {
            (PartBufferCategory::IndexData, part.part_index_data.len() / 3, index_bytes)
        } else {
            continue;
        };
        let (buffer_kind, num_elements, num_bytes) = over;
        return Err(PartBufferOverLimit {
            upload_part_node: part.upload_part_node,
            oversized_part_name: part.part_display_name.clone(),
            oversized_buffer_kind: buffer_kind,
            oversized_element_count: num_elements,
            upload_buffer_byte_count: num_bytes,
            device_buffer_limit: max_buffer_size,
        });
    }
    Ok(())
}

struct PreparedSceneData {
    upload_scene_parts: Vec<PreparedPartData>,
    upload_bounds_min: Vec3,
    upload_bounds_max: Vec3,
}

/// Every part's vertices and triangles, and the scene's bounds. Apart from
/// buffers, so it can be tested without a GPU. `prepared` hears each part's
/// triangle count (0 for an empty part) in drawing order.
fn build_gpu_scene_data(graph: &Scene, mut prepared: impl FnMut(u64)) -> Result<PreparedSceneData> {
    let mut objects = Vec::new();
    each_part(graph, |node_id, _, mesh| {
        prepared(mesh.faces().len() as u64);
        let own_name = graph
            .node(node_id)
            .and_then(|node| node.meta.as_ref())
            .and_then(|meta| meta.name.clone());
        let name = own_name.or_else(|| mesh.meta.as_ref().and_then(|meta| meta.name.clone()));
        // An empty part still gets an entry, drawing nothing, so the draws
        // and `build_model_entries` stay one for one.
        let (vertices, indices) = if mesh.faces().is_empty() {
            (Vec::new(), Vec::new())
        } else {
            // Smooth normals, split at creases (R90): lit per screen-space
            // facet, thin round supports looked like stair steps.
            let positions: Vec<[f32; 3]> = mesh.vertices().iter().map(|v| v.coords()).collect();
            let triangles: Vec<[u32; 3]> = mesh
                .faces()
                .iter()
                .map(|face| face.vertices.map(u32::from))
                .collect();
            let (smooth, indices) = super::smooth_normals::smooth_vertices(&positions, &triangles);
            let vertices = smooth
                .into_iter()
                .map(|(position, normal)| SceneUploadVertex { vertex_coordinates: position, normal })
                .collect();
            (vertices, indices)
        };
        objects.push(PreparedPartData {
            upload_part_node: node_id,
            part_display_name: name,
            part_vertex_data: vertices,
            part_index_data: indices,
        });
    })?;
    if objects.is_empty() {
        return Err(anyhow!("Graph has no leaves to render"));
    }
    // The exact bounds, for framing; all parts empty gives a point at the
    // origin (the view keeps a minimum size, so that is fine).
    let (aabb_min, aabb_max) = match graph.tight_aabb()? {
        Some(aabb) => (
            Vec3::from(aabb.min().coords()),
            Vec3::from(aabb.max().coords()),
        ),
        None => (Vec3::ZERO, Vec3::ZERO),
    };
    Ok(PreparedSceneData {
        upload_scene_parts: objects,
        upload_bounds_min: aabb_min,
        upload_bounds_max: aabb_max,
    })
}

#[cfg(test)]
#[path = "tests_scene_upload.rs"]
mod tests;
