// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The move, turn and scale handles, drawn over the resolved image with a
//! depth buffer of their own. One shader and one layout make two
//! pipelines: solid shapes with back faces culled, and lines culling
//! nothing.
use slint::wgpu_29::wgpu;

use super::super::GIZMO_DEPTH_FORMAT;
use super::super::device_ops::Guarded;
use super::super::gizmo_buffers::GizmoBuffers;
use super::super::gizmo_entries::{HANDLE_ENTRY_BYTES, HandleUniforms};
use super::{Recipe, bind_one, depth_only, pipeline_layout, render_pipeline, shader, uniform_layout};
use crate::gizmo_geometry::HandleVertex;

pub(in crate::mesh_render) struct GizmoPipeline {
    pub(in crate::mesh_render) triangle_pipeline: wgpu::RenderPipeline,
    pub(in crate::mesh_render) line_pipeline: wgpu::RenderPipeline,
    pub(in crate::mesh_render) uniforms_bind_group: wgpu::BindGroup,
    pub(in crate::mesh_render) entries_bind_group: wgpu::BindGroup,
}

impl GizmoPipeline {
    pub(in crate::mesh_render) fn create(device: &wgpu::Device, buffers: &GizmoBuffers) -> Result<GizmoPipeline, wgpu::Error> {
        device.guarded(|| {
            let frame_bytes = size_of::<HandleUniforms>() as u64;
            let frame_layout = uniform_layout(device, "mesh-viewer gizmo uniforms bgl", wgpu::ShaderStages::VERTEX, frame_bytes, false);
            // The handle table, one entry bound per draw at its offset, the
            // way the parts' table is.
            let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
            let table_layout = uniform_layout(device, "mesh-viewer gizmo entries bgl", both, HANDLE_ENTRY_BYTES, true);
            let layout = pipeline_layout(device, "mesh-viewer gizmo pll", &[&frame_layout, &table_layout]);

            let positions = wgpu::vertex_attr_array![0 => Float32x3];
            let corners = [wgpu::VertexBufferLayout {
                array_stride: size_of::<HandleVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &positions,
            }];
            let shader = shader(device, "mesh-viewer gizmo shader", include_str!("../shaders/gizmo.wgsl"));
            let pipeline_for = |label: &str, topology, cull| {
                let recipe = Recipe {
                    label,
                    shader: &shader,
                    vertices: &corners,
                    topology,
                    cull,
                    depth: Some(depth_only(GIZMO_DEPTH_FORMAT, wgpu::CompareFunction::Less)),
                    // The resolved image has a single sample.
                    samples: 1,
                    blend: None,
                };
                render_pipeline(device, &layout, recipe)
            };
            GizmoPipeline {
                triangle_pipeline: pipeline_for("mesh-viewer gizmo triangle pipeline", wgpu::PrimitiveTopology::TriangleList, Some(wgpu::Face::Back)),
                line_pipeline: pipeline_for("mesh-viewer gizmo line pipeline", wgpu::PrimitiveTopology::LineList, None),
                uniforms_bind_group: bind_one(device, "mesh-viewer gizmo uniforms bg", &frame_layout, buffers.uniforms_buffer.as_entire_binding()),
                entries_bind_group: bind_one(device, "mesh-viewer gizmo entries bg", &table_layout, buffers.entries_binding_window()),
            }
        })
    }
}
