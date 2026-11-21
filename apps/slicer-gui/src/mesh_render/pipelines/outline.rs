// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The selection outline: one full-screen pass over the stencil the parts
//! pass wrote (1 where the selection shows, 0 elsewhere). Pixels where the
//! marks change from one neighbour to the next are painted; the rest are
//! discarded, so the image beneath is untouched.
use slint::wgpu_29::wgpu;

use super::super::device_ops::Guarded;
use super::{Recipe, fragment_texture, group_layout, pipeline_layout, render_pipeline, shader};

pub(in crate::mesh_render) struct OutlinePipeline {
    pub(in crate::mesh_render) pipeline: wgpu::RenderPipeline,
    /// The stencil's bind group is made again for each new view size, so
    /// its layout is kept.
    pub(in crate::mesh_render) bind_group_layout: wgpu::BindGroupLayout,
}

impl OutlinePipeline {
    pub(in crate::mesh_render) fn create(device: &wgpu::Device) -> Result<OutlinePipeline, wgpu::Error> {
        device.guarded(|| {
            // The depth texture seen through a stencil-only view: the
            // shader reads its multisampled u32 marks.
            let marks = fragment_texture(0, wgpu::TextureSampleType::Uint, true);
            let bind_group_layout = group_layout(device, "mesh-viewer outline bgl", &[marks]);
            let shader = shader(device, "mesh-viewer outline shader", include_str!("../shaders/outline.wgsl"));
            let recipe = Recipe {
                label: "mesh-viewer outline pipeline",
                shader: &shader,
                // The corners come from the vertex index.
                vertices: &[],
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull: None,
                // Drawn on the resolved one-sample image, with no depth;
                // painted pixels are opaque, so nothing blends.
                depth: None,
                samples: 1,
                blend: None,
            };
            let layout = pipeline_layout(device, "mesh-viewer outline pll", &[&bind_group_layout]);
            OutlinePipeline { pipeline: render_pipeline(device, &layout, recipe), bind_group_layout }
        })
    }
}
