// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The GUI renderer on real GPUs, for what the in-crate GPU tests cannot
//! reach: handing a frame to Slint (`slint::Image::try_from`) and refusing
//! a scene built on another device. Each render test runs on every
//! adapter wgpu finds; without one the tests skip, so CI without a
//! software adapter is not blocked.
//! The tests take turns on the GPU, and nothing GPU-side outlives a
//! turn: one test at a time opens one device per adapter and closes them
//! before the next. Two crashes (a few percent of runs, SIGSEGV) forced
//! this. Creating an instance while another thread had work in flight
//! let the Vulkan loader read a torn pointer in
//! `vkSetDebugUtilsObjectNameEXT`; keeping devices on several drivers
//! open until exit moved the fault into teardown, through a dangling
//! dispatch pointer.
use std::sync::{Arc, Mutex};

use sc_mesh_core::{IndexedMesh, primitives::IndexedTriangle, primitives::Normal, primitives::Vertex};
use sc_mesh_core::transform::{AffineTransform3D as Affine, Transform3D};
use sc_scene::{Scene, node::LeafPayload, node::NodeId, node::NodeKind};
use sc_slicer_gui::build_volume::FALLBACK_VOLUME as PLATE;
use sc_slicer_gui::camera::ViewCamera;
use sc_slicer_gui::mesh_render::scene_upload::UploadedScene;
use sc_slicer_gui::mesh_render::{DeviceLoss, DeviceStamp, ViewRenderer, SceneRender};
use slint::wgpu_29::wgpu;

/// Every backend wgpu was built with but GL, which the GUI does not ship.
/// (`all()` includes the no-op backend, which stays off unless enabled
/// through `BackendOptions`, never done here.)
const BACKENDS_UNDER_TEST: wgpu::Backends = wgpu::Backends::all().difference(wgpu::Backends::GL);

/// Side of the square test frames, in pixels.
const FRAME_PX: u32 = 64;

/// Held for the length of one test's GPU work.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A device on one adapter; `title` names the adapter and backend in
/// failure messages.
struct SweepGpu {
    title: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// The devices for one test. Field order matters: the devices drop before
/// the instance they came from.
struct GpuSweep {
    gpus: Vec<SweepGpu>,
    wgpu_instance: wgpu::Instance,
}

impl GpuSweep {
    fn open_all() -> Self {
        let mut options = wgpu::InstanceDescriptor::new_without_display_handle();
        options.backends = BACKENDS_UNDER_TEST;
        let wgpu_instance = wgpu::Instance::new(options);
        let mut gpus = Vec::new();
        for adapter in list_adapters(&wgpu_instance) {
            let about = adapter.get_info();
            let title = format!("{} on {:?}", about.name, about.backend);
            if let Some((device, queue)) = default_device(&adapter, &title) {
                gpus.push(SweepGpu { title, device, queue });
            }
        }
        GpuSweep { gpus, wgpu_instance }
    }

    /// A second device, on a freshly listed adapter rather than one the
    /// sweep holds: WebGPU allows one device per adapter, even where wgpu
    /// does not check.
    fn second_device(&self) -> Option<(wgpu::Device, wgpu::Queue)> {
        let first = list_adapters(&self.wgpu_instance).into_iter().next()?;
        default_device(&first, "integration-render spare device")
    }

    /// Lets every queue drain, then drops the devices and the instance.
    fn finish(self) {
        for gpu in &self.gpus {
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        }
    }
}

/// Opens every adapter, runs `test` with them while holding the GPU turn,
/// and closes them again. Skips (with a note) when there is no adapter.
fn with_gpu_sweep(test: impl FnOnce(&GpuSweep)) {
    // A test that panicked poisons the turn; its own failure is already
    // reported, so the next test takes the turn anyway.
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let sweep = GpuSweep::open_all();
    if sweep.gpus.is_empty() {
        eprintln!("skipping: no wgpu adapter available in this environment");
        return;
    }
    let titles: Vec<&str> = sweep.gpus.iter().map(|g| g.title.as_str()).collect();
    eprintln!("render sweep covers {} adapter(s):\n  {}", titles.len(), titles.join("\n  "));
    test(&sweep);
    sweep.finish();
}

/// `check` on each adapter's device in turn.
fn on_each_gpu(check: impl Fn(&SweepGpu)) {
    with_gpu_sweep(|sweep| sweep.gpus.iter().for_each(&check));
}

fn list_adapters(wgpu_instance: &wgpu::Instance) -> Vec<wgpu::Adapter> {
    pollster::block_on(wgpu_instance.enumerate_adapters(BACKENDS_UNDER_TEST))
}

/// A device with every request field at its default, the limits Slint
/// itself asks for; an adapter that refuses them is left out.
fn default_device(adapter: &wgpu::Adapter, title: &str) -> Option<(wgpu::Device, wgpu::Queue)> {
    let request = wgpu::DeviceDescriptor { label: Some(title), ..Default::default() };
    pollster::block_on(adapter.request_device(&request)).ok()
}

/// A scene with one triangle part, shifted by `shift`, inside a group.
fn triangle_scene(shift: [f64; 3]) -> Scene {
    let triangle = IndexedMesh {
        meta: None,
        vertices: vec![Vertex::new([0.0; 3]), Vertex::new([1.0, 0.0, 0.0]), Vertex::new([0.0, 1.0, 0.0])],
        faces: vec![IndexedTriangle::from_raw_indices(Normal::new([0.0, 0.0, 1.0]), [0, 1, 2])],
    };
    let mut scene = Scene::new();
    let group = scene.add_group(None, Transform3D::Identity);
    scene.add_root(group).expect("the group becomes a root");
    let moved = Transform3D::Affine(Affine::from_translation(shift));
    let part = scene.add_leaf(None, moved, LeafPayload::Mesh(Arc::new(triangle))).expect("a part");
    scene.add_child(group, part).expect("the part goes in the group");
    scene
}

/// The scene's first part, depth first.
fn first_part(scene: &Scene) -> NodeId {
    fn find(scene: &Scene, id: NodeId) -> Option<NodeId> {
        match &scene.node(id)?.kind {
            NodeKind::Leaf(_) => Some(id),
            NodeKind::Group { children } | NodeKind::Csg { children, .. } => {
                children.iter().find_map(|&child| find(scene, child))
            }
            NodeKind::Reference { .. } => None,
        }
    }
    scene.roots().iter().find_map(|&root| find(scene, root)).expect("the fixture has a part")
}

/// A renderer on `gpu`'s device and queue.
fn renderer_on(device: &wgpu::Device, queue: wgpu::Queue) -> ViewRenderer {
    ViewRenderer::new(device.clone(), DeviceStamp::issue(), queue, DeviceLoss::default())
        .expect("a renderer on the test device")
}

/// The triangle scene drawn at FRAME_PX square on `gpu` with the camera
/// framing it; `outline` selects the part, `cut_at` sets a slice clip.
fn draw_triangle(gpu: &SweepGpu, outline: bool, cut_at: Option<f32>) -> wgpu::Texture {
    let title = &gpu.title;
    let mut renderer = renderer_on(&gpu.device, gpu.queue.clone());
    let scene = triangle_scene([5.0, 0.0, 0.0]);
    if let Err(problem) = renderer.prepare_scene(&scene) {
        panic!("uploading the scene under default limits failed on {title}: {problem:?}");
    }
    let (lo, hi) = renderer.shown_bounds().expect("the uploaded scene has bounds");
    let reach = ((hi - lo).length() / 2.0).max(1e-3);
    // The same radius for framing and zooming (the crate's own
    // `OrbitCamera::new` is test-only, so not visible here).
    let camera = ViewCamera::framing_with_detail((lo + hi) / 2.0, reach, reach);
    let request = SceneRender {
        world_bounds: None,
        selected_ids: &[],
        scene: &scene,
        outlined: outline.then(|| first_part(&scene)),
        handles: None,
        cut_height: cut_at,
    };
    match renderer.draw_frame(Some(request), PLATE, &camera, FRAME_PX, FRAME_PX) {
        Ok(Some(texture)) => texture,
        Ok(None) => panic!("no texture was drawn on {title}"),
        Err(refusal) => panic!("drawing was refused on {title}: {refusal}"),
    }
}

/// What the viewport does in `handle_before_rendering`: hand the frame's
/// texture to Slint as an image.
#[test]
fn a_frame_becomes_a_slint_image() {
    on_each_gpu(|gpu| {
        let texture = draw_triangle(gpu, true, None);
        let image = match slint::Image::try_from(texture) {
            Ok(image) => image,
            Err(refusal) => panic!("Slint refused the texture from {}: {refusal:?}", gpu.title),
        };
        let px = image.size();
        assert_eq!([px.width, px.height], [FRAME_PX; 2], "on {}", gpu.title);
    });
}

/// A slice clip switches on the mesh shader's discard; it must still
/// compile and draw.
#[test]
fn a_frame_with_a_slice_clip_draws() {
    on_each_gpu(|gpu| {
        let px = draw_triangle(gpu, false, Some(0.5)).size();
        assert_eq!([px.width, px.height], [FRAME_PX; 2], "on {}", gpu.title);
    });
}

/// A scene uploaded on one device must not be shown by a renderer on
/// another (the loader uploads again instead), and the renderer keeps
/// showing nothing.
#[test]
fn a_scene_from_another_device_is_refused() {
    with_gpu_sweep(|sweep| {
        let Some((device_b, queue_b)) = sweep.second_device() else {
            eprintln!("skipping: could not open a second wgpu device");
            return;
        };
        // Tokens tell devices apart, so each device gets its own.
        let mut renderer = renderer_on(&device_b, queue_b);
        let built_on_a = UploadedScene::create_gpu_scene(
            &sweep.gpus[0].device,
            DeviceStamp::issue(),
            &triangle_scene([0.0; 3]),
            &DeviceLoss::default(),
        )
        .expect("the scene uploads on the first device");
        assert!(!renderer.swap_in_scene(built_on_a), "a scene from another device is refused");
        assert!(renderer.shown_bounds().is_none(), "and nothing is attached");
        // Closed within the turn, like the sweep's own devices.
        let _ = device_b.poll(wgpu::PollType::wait_indefinitely());
    });
}
