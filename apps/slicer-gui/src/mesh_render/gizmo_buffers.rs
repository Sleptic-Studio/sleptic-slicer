// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The gizmo's buffers. Its five shapes are uploaded once, when the
//! renderer starts: the axis line, the move cone, the scale cube, the turn
//! ring's line and the turn ring's tube. Each frame writes only the
//! per-handle entries (`write_entries`).
use std::num::NonZeroU64;

use bytemuck::Pod;
use slint::wgpu_29::wgpu;

use super::device_ops::Guarded;
use super::gizmo_entries::{
    HANDLE_ENTRY_BYTES, HANDLE_ENTRY_COUNT, HandleEntry, HandleUniforms,
};
use super::scene_upload::round_buffer_size_up;
use crate::gizmo_geometry;

/// One of the gizmo's shapes on the GPU: its vertices, its triangle or
/// line indices, and how many indices there are.
pub(super) struct HandleShape {
    pub(super) vertices: wgpu::Buffer,
    pub(super) indices: wgpu::Buffer,
    pub(super) count: u32,
}

impl HandleShape {
    /// Uploads `geometry` once, its two buffers labelled `[vertex, index]`.
    fn upload<V: Pod>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        [vertex_label, index_label]: [&'static str; 2],
        (points, order): (Vec<V>, Vec<u32>),
    ) -> Result<Self, wgpu::Error> {
        let vertices = device.try_upload(queue, vertex_label, bytemuck::cast_slice(&points), wgpu::BufferUsages::VERTEX)?;
        let indices = device.try_upload(queue, index_label, bytemuck::cast_slice(&order), wgpu::BufferUsages::INDEX)?;
        Ok(Self { vertices, indices, count: order.len() as u32 })
    }
}

pub(super) struct GizmoBuffers {
    /// The axis: a one-pixel line from the pivot to the cone, in the
    /// handle's own space.
    pub(super) shaft: HandleShape,
    /// The move cone at the axis' tip (what a move drag grabs), with the
    /// flat square drawn for Z last.
    pub(super) cone: HandleShape,
    /// The scale cube at the end of each axis, also the centre cube.
    pub(super) cube: HandleShape,
    /// The turn ring's edge, a circle about the handle's +X that each
    /// axis's rotation turns onto X, Y or Z.
    pub(super) ring_edge: HandleShape,
    /// The turn ring's tube, which a turn drag grabs.
    pub(super) ring_tube: HandleShape,
    pub(super) uniforms_buffer: wgpu::Buffer,
    /// `HANDLE_ENTRY_COUNT` entries `entry_stride` apart, each bound alone
    /// at its offset (`HandleGroup` says which is which).
    pub(super) entries_buffer: wgpu::Buffer,
    pub(super) entry_stride: u64,
}

impl GizmoBuffers {
    pub(super) fn create(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self, wgpu::Error> {
        let shape = |labels, geometry| HandleShape::upload(device, queue, labels, geometry);
        let shaft = shape(
            ["mesh-viewer gizmo shaft line vbo", "mesh-viewer gizmo shaft line ibo"],
            gizmo_geometry::build_axis_shaft_line_geometry(),
        )?;
        let cone = shape(
            ["mesh-viewer gizmo cone tip vbo", "mesh-viewer gizmo cone tip ibo"],
            gizmo_geometry::build_move_handle_geometry(),
        )?;
        let cube = shape(
            ["mesh-viewer gizmo scale cube vbo", "mesh-viewer gizmo scale cube ibo"],
            gizmo_geometry::build_axis_scale_cube_geometry(),
        )?;
        let ring_edge = shape(
            ["mesh-viewer gizmo ring line vbo", "mesh-viewer gizmo ring line ibo"],
            gizmo_geometry::build_rotate_ring_line_geometry(),
        )?;
        let ring_tube = shape(
            ["mesh-viewer gizmo rotate ring vbo", "mesh-viewer gizmo rotate ring ibo"],
            gizmo_geometry::build_rotate_ring_geometry(),
        )?;
        let written_each_frame = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let uniform = |label, size| {
            let description = wgpu::BufferDescriptor { label: Some(label), size, usage: written_each_frame, mapped_at_creation: false };
            device.try_buffer(&description)
        };
        let uniforms_buffer = uniform("mesh-viewer gizmo uniforms", size_of::<HandleUniforms>() as u64)?;
        // A dynamic offset must be a multiple of the device's alignment, so
        // each entry takes a whole number of those.
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let entry_stride = round_buffer_size_up(HANDLE_ENTRY_BYTES, alignment);
        let entries_buffer = uniform("mesh-viewer gizmo entries", entry_stride * HANDLE_ENTRY_COUNT)?;
        Ok(Self { shaft, cone, cube, ring_edge, ring_tube, uniforms_buffer, entries_buffer, entry_stride })
    }

    /// One entry's window of the entries buffer, the size the entries
    /// layout expects, for the bind group used with dynamic offsets.
    pub(super) fn entries_binding_window(&self) -> wgpu::BindingResource<'_> {
        let window = wgpu::BufferBinding { buffer: &self.entries_buffer, offset: 0, size: NonZeroU64::new(HANDLE_ENTRY_BYTES) };
        wgpu::BindingResource::Buffer(window)
    }

    /// Writes `entries` into consecutive slots from `first_slot`.
    pub(super) fn write_entries(&self, queue: &wgpu::Queue, first_slot: u64, entries: &[HandleEntry]) {
        for (slot, entry) in (first_slot..).zip(entries) {
            queue.write_buffer(&self.entries_buffer, slot * self.entry_stride, bytemuck::bytes_of(entry));
        }
    }
}
