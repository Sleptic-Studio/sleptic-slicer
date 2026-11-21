// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The slice cap: one flat quad across the build area at the cut height,
//! textured with the chosen layer's coverage, so a cut part reads as solid
//! rather than hollow.
//!
//! It needs no uniforms of its own: group 0 is the parts' frame buffer
//! (view and projection, the cut height in `light_dir.w`, the build area in
//! `env_min`/`env_max`). Group 1 is the coverage texture and its sampler,
//! made again whenever the layer changes. The quad's corners come from the
//! vertex index.
use slint::wgpu_29::wgpu;

use super::super::device_ops::Guarded;
use super::super::frame::FrameUniforms;
use super::super::{DEPTH_FORMAT, MSAA_SAMPLES};
use super::mesh::MeshPipeline;
use super::{Recipe, bind_one, fragment_binding, fragment_texture, group_layout, pipeline_layout, render_pipeline, shader, uniform_layout};

pub(in crate::mesh_render) struct CapPipeline {
    pub(in crate::mesh_render) pipeline: wgpu::RenderPipeline,
    pub(in crate::mesh_render) uniforms_bind_group: wgpu::BindGroup,
    pub(in crate::mesh_render) texture_bind_group_layout: wgpu::BindGroupLayout,
    pub(in crate::mesh_render) sampler: wgpu::Sampler,
}

/// Where the cap meets a part's top face exactly, it is pulled towards the
/// camera so the two do not z-fight into streaks (reported 2026-10-30).
const TOWARDS_CAMERA: wgpu::DepthBiasState = wgpu::DepthBiasState { constant: -8, slope_scale: -2.0, clamp: 0.0 };

impl CapPipeline {
    pub(in crate::mesh_render) fn create(device: &wgpu::Device, mesh: &MeshPipeline) -> Result<CapPipeline, wgpu::Error> {
        device.guarded(|| {
            // wgpu matches layouts by identity, not shape, so the cap makes
            // its own layout for the parts' buffer (as the plate does).
            let both_stages = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
            let frame_bytes = size_of::<FrameUniforms>() as u64;
            let uniforms_layout = uniform_layout(device, "mesh-viewer cap uniforms bgl", both_stages, frame_bytes, false);
            let coverage = fragment_texture(0, wgpu::TextureSampleType::Float { filterable: true }, false);
            let smoothing = fragment_binding(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering));
            let texture_bind_group_layout = group_layout(device, "mesh-viewer cap texture bgl", &[coverage, smoothing]);
            let layout = pipeline_layout(device, "mesh-viewer cap pll", &[&uniforms_layout, &texture_bind_group_layout]);

            let shader = shader(device, "mesh-viewer cap shader", include_str!("../shaders/cap.wgsl"));
            let recipe = Recipe {
                label: "mesh-viewer cap pipeline",
                shader: &shader,
                vertices: &[],
                topology: wgpu::PrimitiveTopology::TriangleList,
                // A flat quad, seen from above or below alike.
                cull: None,
                depth: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    // The stencil is left alone: the outline still reads it.
                    stencil: wgpu::StencilState::default(),
                    bias: TOWARDS_CAMERA,
                }),
                samples: MSAA_SAMPLES,
                // The layer is opaque; the glow painted around it blends.
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            };
            let edge = wgpu::AddressMode::ClampToEdge;
            CapPipeline {
                pipeline: render_pipeline(device, &layout, recipe),
                uniforms_bind_group: bind_one(device, "mesh-viewer cap uniforms bg", &uniforms_layout, mesh.uniform_buffer.as_entire_binding()),
                texture_bind_group_layout,
                sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("mesh-viewer cap sampler"),
                    address_mode_u: edge,
                    address_mode_v: edge,
                    address_mode_w: edge,
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                }),
            }
        })
    }
}
