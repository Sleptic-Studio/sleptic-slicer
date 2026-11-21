// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The renderer's GPU pipelines: the parts, the selection outline, the
//! gizmo, the build plate and the slice cap. Each module owns its pipelines,
//! layouts and the enum that numbers its `@group(N)` bindings as its shader
//! does. The builders below hold what they all share.
use std::borrow::Cow;
use std::num::NonZeroU64;

use slint::wgpu_29::wgpu;

pub(super) mod cap;
pub(super) mod gizmo;
pub(super) mod mesh;
pub(super) mod outline;
pub(super) mod plate;

/// Names for a shader's `@group(N)` numbers, so a bind call says which
/// group it fills rather than a bare number.
macro_rules! shader_groups {
    ($($(#[$doc:meta])* $name:ident in $shader:literal { $($group:ident = $number:literal),+ })+) => {
        $($(#[$doc])*
        #[derive(Clone, Copy, Debug)]
        pub(in crate::mesh_render) enum $name {
            $($group,)+
        }

        impl $name {
            pub(in crate::mesh_render) fn slot(self) -> u32 {
                match self {
                    $($name::$group => $number,)+
                }
            }
        })+

        /// Each shader's groups as (shader, group, number), for the test
        /// that holds them to the shader sources.
        #[cfg(test)]
        const GROUP_NUMBERS: &[(&str, &str, u32)] = &[$($(($shader, stringify!($group), $number),)+)+];
    };
}

shader_groups! {
    /// The parts shader: the frame's uniforms, then the part's entry.
    PartsGroup in "mesh_render.wgsl" { Frame = 0, PartTable = 1 }
    /// The outline shader: the stencil marks.
    OutlineGroup in "outline.wgsl" { Marks = 0 }
    /// The gizmo shader: its uniforms, then the handle's entry.
    HandlesGroup in "gizmo.wgsl" { Frame = 0, HandleTable = 1 }
    /// The build plate shader: the parts' frame uniforms.
    PlateGroup in "plate.wgsl" { Frame = 0 }
}


fn shader(device: &wgpu::Device, label: &str, source: &'static str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
    })
}

/// A layout of one uniform buffer at binding 0, at least `size` bytes; a
/// dynamic one is bound at a different offset for each draw.
fn uniform_layout(
    device: &wgpu::Device,
    label: &str,
    visibility: wgpu::ShaderStages,
    size: u64,
    dynamic: bool,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: NonZeroU64::new(size),
            },
            count: None,
        }],
    })
}

/// A bind group of one buffer binding at 0.
fn bind_one(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    resource: wgpu::BindingResource<'_>,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource,
        }],
    })
}

/// What sets one render pipeline apart; the rest is the same for all:
/// entry points `vertex_stage` and `fragment_stage`, counter-clockwise front faces,
/// filled polygons, one colour target in the renderer's format.
struct Recipe<'a> {
    label: &'a str,
    shader: &'a wgpu::ShaderModule,
    vertices: &'a [wgpu::VertexBufferLayout<'a>],
    topology: wgpu::PrimitiveTopology,
    cull: Option<wgpu::Face>,
    depth: Option<wgpu::DepthStencilState>,
    samples: u32,
    blend: Option<wgpu::BlendState>,
}

fn pipeline_layout(
    device: &wgpu::Device,
    label: &str,
    groups: &[&wgpu::BindGroupLayout],
) -> wgpu::PipelineLayout {
    let groups: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().copied().map(Some).collect();
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &groups,
        immediate_size: 0,
    })
}

fn render_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    recipe: Recipe<'_>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(recipe.label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: recipe.shader,
            entry_point: Some("vertex_stage"),
            compilation_options: Default::default(),
            buffers: recipe.vertices,
        },
        primitive: wgpu::PrimitiveState {
            topology: recipe.topology,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: recipe.cull,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: recipe.depth,
        multisample: wgpu::MultisampleState {
            count: recipe.samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: recipe.shader,
            entry_point: Some("fragment_stage"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: super::OUTPUT_FORMAT,
                blend: recipe.blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Depth testing with writes on, no stencil and no bias.
fn depth_only(
    format: wgpu::TextureFormat,
    compare: wgpu::CompareFunction,
) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(true),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// A fragment-stage binding of `ty` at `binding`.
fn fragment_binding(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None }
}

/// A 2D texture read by the fragment stage.
fn fragment_texture(binding: u32, sample_type: wgpu::TextureSampleType, multisampled: bool) -> wgpu::BindGroupLayoutEntry {
    let view_dimension = wgpu::TextureViewDimension::D2;
    fragment_binding(binding, wgpu::BindingType::Texture { sample_type, view_dimension, multisampled })
}

fn group_layout(device: &wgpu::Device, label: &str, entries: &[wgpu::BindGroupLayoutEntry]) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
}

#[cfg(test)]
mod tests {
    use super::GROUP_NUMBERS;

    /// Every named group is declared in its shader with that number.
    #[test]
    fn group_names_match_their_shaders() {
        let source = |shader: &str| match shader {
            "mesh_render.wgsl" => include_str!("../shaders/mesh_render.wgsl"),
            "outline.wgsl" => include_str!("../shaders/outline.wgsl"),
            "gizmo.wgsl" => include_str!("../shaders/gizmo.wgsl"),
            "plate.wgsl" => include_str!("../shaders/plate.wgsl"),
            other => panic!("no shader {other}"),
        };
        for &(shader, group, number) in GROUP_NUMBERS {
            let declared = format!("@group({number})");
            assert!(source(shader).contains(&declared), "{shader} has no {declared} for {group}");
        }
    }
}
