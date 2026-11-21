// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The renderer as the viewport sees it. The controller and frame host
//! hold a `dyn MeshRenderer`, so tests can drive them with a renderer that
//! has no GPU (`mock.rs`). Crate-private: only this crate implements it.
use anyhow::Result;
use glam::Vec3;
use sc_scene::Scene;
use slint::wgpu_29::wgpu;

use super::scene_upload::UploadedScene;
use super::{FrameRefusal, SceneRender, ViewRenderer};
use crate::build_volume::BuildVolume;
use crate::camera::ViewCamera;
use crate::slice_cap::CutImage;

/// Everything about how the scene looks that is not the scene itself.
/// The viewport fills it in before each frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SceneLook {
    /// The plate being worked on, and whether it is drawn alone (the
    /// Slice view shows only the sliced plate).
    pub(crate) active_plate: u32,
    pub(crate) only_active_plate: bool,
    /// Parts not drawn, by their index in the scene's leaves.
    pub(crate) hidden_objects: Vec<usize>,
    /// Thickness in mm of the previewed layer under the cut, painted in
    /// the cap's UV violet; 0 for none.
    pub(crate) layer_band: f32,
    /// How deep the slice cap's UV-pink is, 0..1.
    pub(crate) cure_shade: f32,
    /// The parts' colour (the resin's, R121), linear.
    pub(crate) model_color: [f32; 3],
    /// The colour of supports that need attention, sRGB 0..255.
    pub(crate) support_warning_rgb: [u32; 3],
    /// The light theme's background, plate, edges and tiles.
    pub(crate) light_scene: bool,
}

impl Default for SceneLook {
    fn default() -> SceneLook {
        SceneLook {
            active_plate: 1,
            only_active_plate: false,
            hidden_objects: Vec::new(),
            layer_band: 0.0,
            cure_shade: 0.0,
            model_color: [0.74, 0.75, 0.77],
            support_warning_rgb: [0; 3],
            light_scene: false,
        }
    }
}

pub(crate) trait MeshRenderer {
    /// The look to fill in for the next frame; none for a renderer that
    /// draws nothing.
    fn look(&mut self) -> Option<&mut SceneLook> {
        None
    }

    /// Builds the scene's GPU buffers here, on the UI thread.
    fn prepare_scene(&mut self, graph: &Scene) -> Result<()>;

    /// Installs a scene built on a worker. False when it was built for a
    /// device this renderer no longer has; the caller then builds it again
    /// with [`MeshRenderer::prepare_scene`].
    fn swap_in_scene(&mut self, gpu: UploadedScene) -> bool;

    /// The installed scene's bounds, if one is installed.
    fn shown_bounds(&self) -> Option<(Vec3, Vec3)>;

    /// Forgets the scene and everything sized to the view.
    fn drop_scene(&mut self);

    /// Forgets only what is sized to the view, so the next frame makes it
    /// at the exact size.
    fn forget_sizes(&mut self);

    /// Shows the slice cap for layer `key`, or none. `build` is called
    /// only when the key changes; a refused upload leaves that key capless.
    fn refresh_cut_face(&mut self, key: Option<u64>, build: &mut dyn FnMut() -> Option<CutImage>) -> Result<(), FrameRefusal>;

    /// Draws one frame, or nothing for an empty view. A view larger than
    /// the GPU allows is drawn smaller in the same shape.
    fn draw_frame(
        &mut self,
        scene_render: Option<SceneRender<'_>>,
        build_volume: BuildVolume,
        camera: &ViewCamera,
        width: u32,
        height: u32,
    ) -> Result<Option<wgpu::Texture>, FrameRefusal>;
}

/// Implements the trait's `&mut self` methods on the GPU renderer by
/// calling its own methods of the same names.
macro_rules! same_named {
    ($(fn $name:ident(&mut self $(, $arg:ident: $kind:ty)*) -> $out:ty;)+) => {
        $(fn $name(&mut self $(, $arg: $kind)*) -> $out {
            ViewRenderer::$name(self $(, $arg)*)
        })+
    };
}

impl MeshRenderer for ViewRenderer {
    fn look(&mut self) -> Option<&mut SceneLook> {
        Some(&mut self.look)
    }

    fn shown_bounds(&self) -> Option<(Vec3, Vec3)> {
        ViewRenderer::shown_bounds(self)
    }

    same_named! {
        fn prepare_scene(&mut self, graph: &Scene) -> Result<()>;
        fn swap_in_scene(&mut self, gpu: UploadedScene) -> bool;
        fn drop_scene(&mut self) -> ();
        fn forget_sizes(&mut self) -> ();
        fn refresh_cut_face(&mut self, key: Option<u64>, build: &mut dyn FnMut() -> Option<CutImage>) -> Result<(), FrameRefusal>;
        fn draw_frame(&mut self, scene_render: Option<SceneRender<'_>>, build_volume: BuildVolume, camera: &ViewCamera, width: u32, height: u32) -> Result<Option<wgpu::Texture>, FrameRefusal>;
    }
}
