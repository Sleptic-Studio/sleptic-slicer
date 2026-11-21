// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The textures that follow the view's size: the multisampled targets a
//! frame is drawn into, and the pair of output images handed to Slint.
//! Each set is made again only when the size changes (`matches`).
use slint::wgpu_29::wgpu;
use wgpu::TextureUsages as Use;

use super::device_ops::Guarded;
use super::error::{FrameRefusal, FrameResource};
use super::{DEPTH_FORMAT, GIZMO_DEPTH_FORMAT, MSAA_SAMPLES, OUTPUT_FORMAT};

/// One kind of view-sized texture: its sample count, format and uses.
struct Sheet {
    label: &'static str,
    samples: u32,
    format: wgpu::TextureFormat,
    usage: Use,
}

impl Sheet {
    fn make(&self, device: &wgpu::Device, (width, height): (u32, u32)) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(self.label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: self.samples,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: self.usage,
            view_formats: &[],
        })
    }
}

const MSAA_COLOUR: Sheet = Sheet { label: "mesh-viewer msaa color", samples: MSAA_SAMPLES, format: OUTPUT_FORMAT, usage: Use::RENDER_ATTACHMENT };
/// Bindable as well, so the outline pass can read the stencil the parts
/// pass writes into it.
const MSAA_DEPTH: Sheet = Sheet {
    label: "mesh-viewer msaa depth",
    samples: MSAA_SAMPLES,
    format: DEPTH_FORMAT,
    usage: Use::RENDER_ATTACHMENT.union(Use::TEXTURE_BINDING),
};
/// The gizmo draws on the resolved one-sample image after its own clear,
/// so it stays on top of the parts; its axes still need depth where they
/// cross near the pivot, and the four-sample depth cannot serve that.
const GIZMO_DEPTH: Sheet = Sheet { label: "mesh-viewer gizmo depth", samples: 1, format: GIZMO_DEPTH_FORMAT, usage: Use::RENDER_ATTACHMENT };
/// What Slint shows. Copyable out, so tests and debug captures can read a
/// frame back (free when nobody does).
const OUTPUT: Sheet = Sheet {
    label: "mesh-viewer color",
    samples: 1,
    format: OUTPUT_FORMAT,
    usage: Use::RENDER_ATTACHMENT.union(Use::TEXTURE_BINDING).union(Use::COPY_SRC),
};

fn full_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&Default::default())
}

pub(super) struct DrawTargets {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) color_view: wgpu::TextureView,
    pub(super) depth_view: wgpu::TextureView,
    /// The depth texture's stencil, bound for the outline pass to read as
    /// a multisampled u32 texture; made with the texture it views.
    pub(super) outline_bind_group: wgpu::BindGroup,
    pub(super) gizmo_depth_view: wgpu::TextureView,
}

impl DrawTargets {
    pub(super) fn matches(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }

    pub(super) fn create(
        device: &wgpu::Device,
        outline_bind_group_layout: &wgpu::BindGroupLayout,
        width: u32,
        height: u32,
    ) -> Result<DrawTargets, FrameRefusal> {
        let size = (width, height);
        let made = device.guarded(|| {
            let depth = MSAA_DEPTH.make(device, size);
            let marks = depth.create_view(&wgpu::TextureViewDescriptor {
                label: Some("mesh-viewer msaa stencil view"),
                aspect: wgpu::TextureAspect::StencilOnly,
                ..Default::default()
            });
            let outline_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mesh-viewer outline bg"),
                layout: outline_bind_group_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&marks) }],
            });
            DrawTargets {
                width,
                height,
                color_view: full_view(&MSAA_COLOUR.make(device, size)),
                depth_view: full_view(&depth),
                outline_bind_group,
                gizmo_depth_view: full_view(&GIZMO_DEPTH.make(device, size)),
            }
        });
        made.map_err(|source| FrameRefusal::Refused { resource: FrameResource::Multisampled, source })
    }
}

/// Two output images drawn into in turn: while Slint still shows the last
/// frame's image, the next frame goes into the other. Keeping the pair
/// saves making a bindable render target at every step of an orbit.
pub(super) struct OutputPair {
    pub(super) width: u32,
    pub(super) height: u32,
    images: [wgpu::Texture; 2],
    /// Which of `images` the next frame uses.
    turn: usize,
}

impl OutputPair {
    pub(super) fn matches(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }

    pub(super) fn create(device: &wgpu::Device, width: u32, height: u32) -> Result<OutputPair, FrameRefusal> {
        let made = device.guarded(|| {
            let images = [(); 2].map(|()| OUTPUT.make(device, (width, height)));
            OutputPair { width, height, images, turn: 0 }
        });
        made.map_err(|source| FrameRefusal::Refused { resource: FrameResource::Outputs, source })
    }

    /// The image for this frame; the next call returns the other one.
    pub(super) fn next_output(&mut self) -> wgpu::Texture {
        let image = self.images[self.turn].clone();
        self.turn ^= 1;
        image
    }
}
