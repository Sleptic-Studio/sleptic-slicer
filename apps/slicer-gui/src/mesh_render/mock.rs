// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! A renderer for tests that never touches the GPU, so the viewport
//! controller's drawing path (`handle_before_rendering`) runs on the CPU.
//!
//! Once boxed into the controller the renderer is out of reach, so it logs
//! every call into a script the test keeps a handle to, and answers from
//! what the test queued there.
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use anyhow::Result;
use glam::Vec3;
use sc_scene::Scene;
use sc_scene::node::NodeId;
use slint::wgpu_29::wgpu;

use super::scene_upload::UploadedScene;
use super::{FrameRefusal, MeshRenderer, SceneRender};
use crate::build_volume::BuildVolume;
use crate::camera::ViewCamera;
use crate::slice_cap::CutImage;

/// One call the renderer received, in order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Call {
    /// A scene to build buffers for (a copy of it).
    Upload(Scene),
    /// A scene built elsewhere, offered for installing.
    Install,
    DropScene,
    ForgetSizes,
    Draw(RecordedDraw),
}

/// What a frame was asked to show, as far as tests look at it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordedDraw {
    pub(crate) had_scene: bool,
    /// The selected part, when there was a scene.
    pub(crate) outlined_part: Option<NodeId>,
    /// Whether the transform handles were to be drawn.
    pub(crate) had_handles: bool,
    pub(crate) drawn_volume: BuildVolume,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The log of calls, and the answers queued for the next ones.
pub(crate) struct RendererScript {
    pub(crate) calls: Vec<Call>,
    /// The bounds to report, as if a scene that size were installed.
    pub(crate) bounds: Option<(Vec3, Vec3)>,
    /// Whether offered scenes install; false acts as if the device changed
    /// while the scene was built.
    pub(crate) installs_succeed: bool,
    /// Results for the next uploads, in turn; `Ok` once used up.
    pub(crate) upload_results: VecDeque<Result<()>>,
    /// Refusals for the next draws, in turn; then `Ok(None)`, as no
    /// texture exists without a GPU.
    pub(crate) draw_refusals: VecDeque<FrameRefusal>,
}

impl RendererScript {
    /// A script with nothing logged that answers yes and `Ok` to everything,
    /// shared between the test and the renderer.
    pub(crate) fn shared_script() -> Rc<RefCell<RendererScript>> {
        Rc::new(RefCell::new(RendererScript {
            calls: Vec::new(),
            bounds: None,
            installs_succeed: true,
            upload_results: VecDeque::new(),
            draw_refusals: VecDeque::new(),
        }))
    }

    /// The scenes uploaded, oldest first.
    pub(crate) fn uploads(&self) -> Vec<&Scene> {
        self.calls.iter().filter_map(|call| if let Call::Upload(scene) = call { Some(scene) } else { None }).collect()
    }

    /// The frames drawn, oldest first.
    pub(crate) fn draws(&self) -> Vec<&RecordedDraw> {
        self.calls.iter().filter_map(|call| if let Call::Draw(draw) = call { Some(draw) } else { None }).collect()
    }

    pub(crate) fn scene_drops(&self) -> usize {
        self.calls.iter().filter(|call| **call == Call::DropScene).count()
    }

    pub(crate) fn size_resets(&self) -> usize {
        self.calls.iter().filter(|call| **call == Call::ForgetSizes).count()
    }
}

/// The renderer that plays a [`RendererScript`].
pub(crate) struct ScriptedRenderer {
    script: Rc<RefCell<RendererScript>>,
}

impl ScriptedRenderer {
    pub(crate) fn new(script: Rc<RefCell<RendererScript>>) -> ScriptedRenderer {
        ScriptedRenderer { script }
    }

    fn log(&self, call: Call) {
        self.script.borrow_mut().calls.push(call);
    }
}

impl MeshRenderer for ScriptedRenderer {
    fn prepare_scene(&mut self, graph: &Scene) -> Result<()> {
        self.log(Call::Upload(graph.clone()));
        self.script.borrow_mut().upload_results.pop_front().unwrap_or(Ok(()))
    }

    fn swap_in_scene(&mut self, _built: UploadedScene) -> bool {
        self.log(Call::Install);
        self.script.borrow().installs_succeed
    }

    fn shown_bounds(&self) -> Option<(Vec3, Vec3)> {
        self.script.borrow().bounds
    }

    fn drop_scene(&mut self) {
        self.log(Call::DropScene);
    }

    fn forget_sizes(&mut self) {
        self.log(Call::ForgetSizes);
    }

    /// The cap is a texture; without a GPU there is nothing to keep.
    fn refresh_cut_face(&mut self, _key: Option<u64>, _build: &mut dyn FnMut() -> Option<CutImage>) -> Result<(), FrameRefusal> {
        Ok(())
    }

    fn draw_frame(
        &mut self,
        scene_render: Option<SceneRender<'_>>,
        build_volume: BuildVolume,
        _camera: &ViewCamera,
        width: u32,
        height: u32,
    ) -> Result<Option<wgpu::Texture>, FrameRefusal> {
        let scene = scene_render.as_ref();
        self.log(Call::Draw(RecordedDraw {
            had_scene: scene.is_some(),
            outlined_part: scene.and_then(|shown| shown.outlined),
            had_handles: scene.is_some_and(|shown| shown.handles.is_some()),
            drawn_volume: build_volume,
            width,
            height,
        }));
        match self.script.borrow_mut().draw_refusals.pop_front() {
            Some(refusal) => Err(refusal),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(renderer: &mut ScriptedRenderer) -> Result<Option<wgpu::Texture>, FrameRefusal> {
        let camera = ViewCamera::new(Vec3::ZERO, 1.0);
        renderer.draw_frame(None, BuildVolume::new(100.0, 60.0, 80.0), &camera, 800, 600)
    }

    #[test]
    fn an_untouched_script_has_an_empty_log_and_agrees_to_everything() {
        let script = RendererScript::shared_script();
        let script = script.borrow();
        assert!(script.calls.is_empty());
        assert!(script.installs_succeed && script.bounds.is_none());
        assert!(script.upload_results.is_empty() && script.draw_refusals.is_empty());
    }

    #[test]
    fn every_call_is_logged_in_order_and_queued_answers_come_out_in_turn() {
        let script = RendererScript::shared_script();
        let mut renderer = ScriptedRenderer::new(Rc::clone(&script));
        script.borrow_mut().upload_results.push_back(Err(anyhow::anyhow!("refused once")));
        script.borrow_mut().draw_refusals.push_back(FrameRefusal::Lost);
        let empty = Scene::new();
        assert!(renderer.prepare_scene(&empty).is_err(), "the queued refusal");
        assert!(renderer.prepare_scene(&empty).is_ok(), "then the default");
        assert!(matches!(frame(&mut renderer), Err(FrameRefusal::Lost)));
        assert!(matches!(frame(&mut renderer), Ok(None)));
        renderer.forget_sizes();
        renderer.drop_scene();
        script.borrow_mut().bounds = Some((Vec3::NEG_ONE, Vec3::ONE));
        assert_eq!(renderer.shown_bounds(), Some((Vec3::NEG_ONE, Vec3::ONE)));

        let script = script.borrow();
        let drawn = Call::Draw(RecordedDraw {
            had_scene: false,
            outlined_part: None,
            had_handles: false,
            drawn_volume: BuildVolume::new(100.0, 60.0, 80.0),
            width: 800,
            height: 600,
        });
        let expected = [Call::Upload(empty.clone()), Call::Upload(empty), drawn.clone(), drawn, Call::ForgetSizes, Call::DropScene];
        assert_eq!(script.calls, expected);
        assert_eq!((script.uploads().len(), script.draws().len(), script.size_resets(), script.scene_drops()), (2, 2, 1, 1));
    }
}
