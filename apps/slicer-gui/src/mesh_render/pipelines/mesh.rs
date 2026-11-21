// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The parts: every object's triangles into the multisampled colour target.
//! The selected object also marks its visible pixels 1 in the stencil,
//! which the outline pass then traces.
use slint::wgpu_29::wgpu;
use wgpu::{CompareFunction as Compare, StencilOperation as Then};

use super::super::device_ops::Guarded;
use super::super::frame::FrameUniforms;
use super::super::scene_upload::{PART_TRANSFORM_ENTRY_BYTES, SceneUploadVertex};
use super::super::{DEPTH_FORMAT, MSAA_SAMPLES};
use super::{Recipe, bind_one, pipeline_layout, render_pipeline, shader, uniform_layout};

pub(in crate::mesh_render) struct MeshPipeline {
    pub(in crate::mesh_render) pipeline: wgpu::RenderPipeline,
    pub(in crate::mesh_render) uniform_buffer: wgpu::Buffer,
    pub(in crate::mesh_render) uniforms_bind_group: wgpu::BindGroup,
    /// The layout of a scene's part table; every uploaded scene binds its
    /// own buffer through it.
    pub(in crate::mesh_render) models_bind_group_layout: wgpu::BindGroupLayout,
}

/// Depth testing, and the stencil set to the draw's reference value
/// (1 for the selected part, 0 otherwise) wherever a fragment wins the
/// depth test, so it holds 1 exactly where the selection is seen.
fn depth_and_marks() -> wgpu::DepthStencilState {
    let mark = wgpu::StencilFaceState { compare: Compare::Always, fail_op: Then::Keep, depth_fail_op: Then::Keep, pass_op: Then::Replace };
    let stencil = wgpu::StencilState { front: mark, back: mark, read_mask: u32::MAX, write_mask: u32::MAX };
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(Compare::Less),
        stencil,
        bias: Default::default(),
    }
}

impl MeshPipeline {
    pub(in crate::mesh_render) fn create(device: &wgpu::Device) -> Result<MeshPipeline, wgpu::Error> {
        device.guarded(|| {
            let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
            let frame_bytes = size_of::<FrameUniforms>() as u64;
            let frame_layout = uniform_layout(device, "mesh-viewer uniforms bgl", both, frame_bytes, false);
            // The part table: one entry per draw, chosen by a dynamic
            // offset. Storage buffers would be simpler, but the downlevel
            // profile Slint runs on allows none in the vertex stage, while
            // dynamic uniform buffers work everywhere at default limits.
            let models_bind_group_layout = uniform_layout(device, "mesh-viewer models bgl", both, PART_TRANSFORM_ENTRY_BYTES, true);
            let layout = pipeline_layout(device, "mesh-viewer pll", &[&frame_layout, &models_bind_group_layout]);

            // Position and normal.
            let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
            let corners = wgpu::VertexBufferLayout {
                array_stride: size_of::<SceneUploadVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            };
            let shader = shader(device, "mesh-viewer shader", include_str!("../shaders/mesh_render.wgsl"));
            let pipeline = render_pipeline(
                device,
                &layout,
                Recipe {
                    label: "mesh-viewer pipeline",
                    shader: &shader,
                    vertices: &[corners],
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull: Some(wgpu::Face::Back),
                    depth: Some(depth_and_marks()),
                    samples: MSAA_SAMPLES,
                    blend: None,
                },
            );

            let usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
            let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh-viewer uniforms"),
                size: frame_bytes,
                usage,
                mapped_at_creation: false,
            });
            let uniforms_bind_group = bind_one(device, "mesh-viewer uniforms bg", &frame_layout, uniform_buffer.as_entire_binding());
            MeshPipeline { pipeline, uniform_buffer, uniforms_bind_group, models_bind_group_layout }
        })
    }
}
