// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;
use crate::{build_volume::FALLBACK_VOLUME, gizmo::axis::Axis};
use sc_mesh_core::{
    IndexedMesh,
    primitives::{IndexedTriangle, Normal, Vertex},
    transform::{AffineTransform3D, Transform3D},
};
use sc_scene::node::LeafPayload;
use std::sync::{Arc, OnceLock};

const BED: BuildVolume = FALLBACK_VOLUME;
const BUFFER_CAP: u64 = 1024;
const GEOMETRY_CAP: u64 = 64;
const TEXTURE_CAP: u32 = 256;
type Gpu = (wgpu::Device, wgpu::Queue);

#[derive(Clone, Copy)]
enum Profile {
    Normal,
    Buffer,
    Geometry,
    Texture,
    Binding,
    Doomed,
    Logged,
}

fn request(adapter: &wgpu::Adapter, limits: wgpu::Limits) -> Option<Gpu> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("renderer contract device"),
        required_limits: limits,
        required_features: wgpu::Features::empty(),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

fn gpu(profile: Profile) -> Option<Gpu> {
    // Adapter discovery can race in the driver; serialize it and all device requests.
    // Doomed and Logged belong exclusively to their respective tests.
    static FLEET: OnceLock<Option<Vec<Gpu>>> = OnceLock::new();
    let fleet = FLEET.get_or_init(|| {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let mut result = Vec::new();
        for profile in [
            Profile::Normal,
            Profile::Buffer,
            Profile::Geometry,
            Profile::Texture,
            Profile::Binding,
            Profile::Doomed,
            Profile::Logged,
        ] {
            let mut limits = wgpu::Limits::default();
            match profile {
                Profile::Buffer | Profile::Logged => limits.max_buffer_size = BUFFER_CAP,
                Profile::Geometry => limits.max_buffer_size = GEOMETRY_CAP,
                Profile::Texture => limits.max_texture_dimension_2d = TEXTURE_CAP,
                Profile::Binding => limits.max_bind_groups = 1,
                _ => {}
            }
            result.push(request(&adapter, limits)?);
        }
        Some(result)
    });
    if fleet.is_none() {
        eprintln!("GPU contract skipped: adapter or test device unavailable");
    }
    fleet
        .as_ref()
        .map(|devices| devices[profile as usize].clone())
}

fn renderer(pair: &Gpu) -> ViewRenderer {
    ViewRenderer::new(
        pair.0.clone(),
        DeviceStamp::issue(),
        pair.1.clone(),
        DeviceLoss::default(),
    )
    .expect("default pipeline contract")
}

fn empty() -> (Scene, NodeId) {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    (scene, root)
}

fn mesh(points: [[f32; 3]; 3], faces: usize) -> IndexedMesh {
    IndexedMesh {
        meta: None,
        vertices: points.into_iter().map(Vertex::new).collect(),
        faces: (0..faces)
            .map(|_| IndexedTriangle::from_raw_indices(Normal::new([0., 0., 1.]), [0, 1, 2]))
            .collect(),
    }
}

fn append(scene: &mut Scene, root: NodeId, offset: [f64; 3], faces: usize) -> NodeId {
    let shape = mesh([[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], faces);
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation(offset)),
            LeafPayload::Mesh(Arc::new(shape)),
        )
        .unwrap();
    scene.add_child(root, id).unwrap();
    id
}

fn triangle(offset: [f64; 3], faces: usize) -> (Scene, NodeId) {
    let (mut scene, root) = empty();
    let id = append(&mut scene, root, offset, faces);
    (scene, id)
}

fn view(
    scene: &Scene,
    selected: Option<NodeId>,
    gizmo: Option<HandleFrame>,
    clip_z: Option<f32>,
) -> SceneRender<'_> {
    SceneRender {
        scene: scene,
        world_bounds: None,
        selected_ids: &[],
        outlined: selected,
        handles: gizmo,
        cut_height: clip_z,
    }
}

fn camera(renderer: &ViewRenderer) -> ViewCamera {
    let (lo, hi) = renderer.shown_bounds().expect("installed bounds");
    ViewCamera::new((lo + hi) * 0.5, ((hi - lo).length() * 0.5).max(1e-3))
}

fn plate_camera() -> ViewCamera {
    let (centre, radius) = BED.framing_sphere(None);
    ViewCamera::new(centre, radius)
}

fn frame(
    renderer: &mut ViewRenderer,
    scene: Option<SceneRender<'_>>,
    camera: &ViewCamera,
) -> wgpu::Texture {
    let texture = renderer
        .draw_frame(scene, BED, camera, 64, 64)
        .expect("frame accepted")
        .expect("frame present");
    assert_eq!((texture.width(), texture.height()), (64, 64));
    texture
}

fn raster(side: u32) -> Option<CutImage> {
    Some(CutImage {
        texels: vec![255; (side * side) as usize],
        width: side,
        height: side,
    })
}

fn mapped_bytes(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |status| {
            send.send(status).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

fn staging(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("contract readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn pixels(pair: &Gpu, texture: &wgpu::Texture) -> Vec<u8> {
    let stride = texture.width() * 4;
    assert!(stride.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
    let buffer = staging(&pair.0, u64::from(stride * texture.height()));
    let mut commands = pair.0.create_command_encoder(&Default::default());
    commands.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    pair.1.submit([commands.finish()]);
    mapped_bytes(&pair.0, &buffer)
}

#[test]
fn pipeline_limit_contract() {
    let Some(normal) = gpu(Profile::Normal) else {
        return;
    };
    let _ = renderer(&normal);
    let Some((device, queue)) = gpu(Profile::Binding) else {
        return;
    };
    let Err(error) = ViewRenderer::new(
        device.clone(),
        DeviceStamp::issue(),
        queue.clone(),
        DeviceLoss::default(),
    ) else {
        panic!("one-group device accepted two-group pipeline")
    };
    assert_eq!(error.step, StartStep::PartsShader);
    let message = error.to_string();
    assert!(message.contains("mesh pipeline"), "{message}");
    assert!(message.contains("bind"), "{message}");
    assert!(MeshPipeline::create(&device).is_err());
    assert!(OutlinePipeline::create(&device).is_ok());
    let geometry = GizmoBuffers::create(&device, &queue).unwrap();
    assert!(GizmoPipeline::create(&device, &geometry).is_err());
}

fn assert_upload_loss(error: &anyhow::Error) {
    assert!(
        error
            .downcast_ref::<scene_upload::SceneUploadDeviceLost>()
            .is_some(),
        "{error}"
    );
}

#[test]
fn loss_blocks_all_renderer_entry_points() {
    let Some((device, queue)) = gpu(Profile::Normal) else {
        return;
    };
    let lost = DeviceLoss::default();
    let token = DeviceStamp::issue();
    let mut renderer = ViewRenderer::new(device.clone(), token, queue, lost.clone()).unwrap();
    let (scene, _) = triangle([5., 0., 0.], 1);
    let prepared = UploadedScene::create_gpu_scene(&device, token, &scene, &DeviceLoss::default()).unwrap();
    lost.mark("Unknown: contract loss".into());
    assert!(matches!(
        renderer.draw_frame(None, BED, &plate_camera(), 64, 64),
        Err(FrameRefusal::Lost)
    ));
    let calls = std::cell::Cell::new(0);
    assert!(matches!(
        renderer.refresh_cut_face(Some(1), &mut || {
            calls.set(calls.get() + 1);
            raster(4)
        }),
        Err(FrameRefusal::Lost)
    ));
    assert_eq!(calls.get(), 0);
    assert_upload_loss(&renderer.prepare_scene(&scene).unwrap_err());
    assert!(!renderer.swap_in_scene(prepared));
    let (four_faces, _) = triangle([0.; 3], 4);
    assert_upload_loss(
        &UploadedScene::create_gpu_scene(&device, DeviceStamp::issue(), &four_faces, &lost).unwrap_err(),
    );
}

#[test]
fn destroyed_device_upload_is_typed_with_either_flag() {
    let Some((device, _)) = gpu(Profile::Doomed) else {
        return;
    };
    let flag = DeviceLoss::default();
    let callback = flag.clone();
    device.set_device_lost_callback(move |reason, detail| {
        callback.mark(format!("{reason:?}: {detail}"))
    });
    device.destroy();
    let _ = device.poll(wgpu::PollType::Poll);
    assert!(flag.is_lost());
    assert!(flag.reason().is_some_and(|text| text.contains("Destroyed")));
    let (scene, _) = triangle([0.; 3], 4);
    // A late notification must turn a mapping panic into the same typed refusal.
    for loss in [flag, DeviceLoss::default()] {
        assert_upload_loss(
            &UploadedScene::create_gpu_scene(&device, DeviceStamp::issue(), &scene, &loss).unwrap_err(),
        );
    }
}

#[test]
fn upload_layout_and_progress() {
    let Some((device, _)) = gpu(Profile::Normal) else {
        return;
    };
    let (scene, _) = triangle([10., 0., 0.], 1);
    let uploaded =
        UploadedScene::create_gpu_scene(&device, DeviceStamp::issue(), &scene, &DeviceLoss::default()).unwrap();
    assert!(uploaded.scene_transform_stride() >= PART_TRANSFORM_ENTRY_BYTES);
    assert_eq!(
        uploaded.scene_transform_stride() % u64::from(device.limits().min_uniform_buffer_offset_alignment),
        0
    );
    let mut observed = Vec::new();
    UploadedScene::create_gpu_scene_reporting(
        &device,
        DeviceStamp::issue(),
        &scene,
        &DeviceLoss::default(),
        |tick| observed.push(tick),
    )
    .unwrap();
    let counts: Vec<_> = observed
        .iter()
        .map(|tick| (tick.uploaded_triangle_count, tick.upload_triangle_total))
        .collect();
    assert_eq!(counts, [(1, 2), (2, 2)]);
}

#[test]
fn upload_buffer_budget_errors() {
    let Some((device, _)) = gpu(Profile::Buffer) else {
        return;
    };
    for faces in [100, 85] {
        let (scene, _) = triangle([0.; 3], faces);
        let error = UploadedScene::create_gpu_scene(&device, DeviceStamp::issue(), &scene, &DeviceLoss::default())
            .unwrap_err();
        if faces == 100 {
            let detail = error
                .downcast_ref::<scene_upload::PartBufferOverLimit>()
                .expect("index refusal type");
            assert_eq!(detail.oversized_buffer_kind, scene_upload::PartBufferCategory::IndexData);
            assert_eq!(
                (
                    detail.oversized_element_count,
                    detail.upload_buffer_byte_count,
                    detail.device_buffer_limit
                ),
                (100, 1200, BUFFER_CAP)
            );
        } else {
            let detail = error
                .downcast_ref::<scene_upload::SceneBufferRefused>()
                .expect("uniform refusal type");
            assert_eq!(detail.refused_buffer_description, "per-object transform table");
            assert!(detail.upload_buffer_byte_count > BUFFER_CAP);
        }
    }
}

fn buffer_descriptor(size: u64, mapped: bool) -> wgpu::BufferDescriptor<'static> {
    wgpu::BufferDescriptor {
        label: Some("buffer boundary probe"),
        size,
        usage: wgpu::BufferUsages::INDEX,
        mapped_at_creation: mapped,
    }
}

fn validation(error: wgpu::Error) {
    assert!(matches!(error, wgpu::Error::Validation { .. }));
    assert!(error.to_string().contains("maximum buffer size"), "{error}");
}

#[test]
fn buffer_boundary_and_static_bytes() {
    let Some((device, _)) = gpu(Profile::Buffer) else {
        return;
    };
    validation(
        device.try_buffer(&buffer_descriptor(BUFFER_CAP + 8, false))
            .unwrap_err(),
    );
    let accepted =
        device.try_buffer(&buffer_descriptor(BUFFER_CAP, true)).unwrap();
    accepted
        .get_mapped_range_mut(..)
        .copy_from_slice(&[7; BUFFER_CAP as usize]);
    accepted.unmap();
    assert_eq!(accepted.size(), BUFFER_CAP);
    let Some((device, queue)) = gpu(Profile::Normal) else {
        return;
    };
    let bytes: Vec<u8> = (0..48).collect();
    let uploaded = device.try_upload(&queue,
        "byte probe",
        &bytes,
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_SRC)
    .unwrap();
    assert_eq!(uploaded.size(), 48);
    let destination = staging(&device, uploaded.size());
    let mut commands = device.create_command_encoder(&Default::default());
    commands.copy_buffer_to_buffer(&uploaded, 0, &destination, 0, uploaded.size());
    queue.submit([commands.finish()]);
    assert_eq!(mapped_bytes(&device, &destination), bytes);
}

#[test]
fn static_geometry_refusals() {
    let Some((device, queue)) = gpu(Profile::Geometry) else {
        return;
    };
    validation(GizmoBuffers::create(&device, &queue).err().unwrap());
    let error = PlateGpuBuffers::one_plate_buffers(&device, &queue, BED).err().unwrap();
    assert!(matches!(
        error,
        FrameRefusal::Refused {
            resource: FrameResource::PlateBuffers,
            ..
        }
    ));
    assert!(error.to_string().contains("maximum buffer size"), "{error}");
}

#[test]
fn logger_survives_unscoped_validation() {
    let Some((device, _)) = gpu(Profile::Logged) else {
        return;
    };
    log_uncaught_gpu_errors(&device);
    let worker = std::thread::spawn(move || {
        device
            .create_buffer(&buffer_descriptor(BUFFER_CAP + 8, false))
            .size()
    });
    assert_eq!(
        worker.join().expect("logger protects caller"),
        BUFFER_CAP + 8
    );
}

#[test]
fn parallel_error_scopes() {
    let Some((device, _)) = gpu(Profile::Buffer) else {
        return;
    };
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let device = device.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..500 {
                    assert!(
                        device.try_buffer(&buffer_descriptor(BUFFER_CAP + 8, false)
                        )
                        .is_err()
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("independent scope stacks");
    }
}

#[test]
fn draw_paths_and_resource_reuse() {
    let Some(pair) = gpu(Profile::Normal) else {
        return;
    };
    let mut renderer = renderer(&pair);
    let (scene, selected) = triangle([5., 0., 0.], 1);
    renderer.prepare_scene(&scene).unwrap();
    let camera = camera(&renderer);
    let original = renderer.shown_bounds().unwrap();
    frame(&mut renderer, Some(view(&scene, None, None, None)), &camera);
    let (replacement, _) = triangle([0., 7., 0.], 1);
    renderer.prepare_scene(&replacement).unwrap();
    assert_ne!(renderer.shown_bounds().unwrap(), original);
    frame(
        &mut renderer,
        Some(view(&replacement, None, None, None)),
        &camera,
    );

    // Separate renderers retain first-frame validation as well as cache reuse.
    renderer = self::renderer(&pair);
    renderer.prepare_scene(&scene).unwrap();
    for _ in 0..2 {
        frame(
            &mut renderer,
            Some(view(&scene, Some(selected), None, None)),
            &camera,
        );
    }
    renderer = self::renderer(&pair);
    renderer.prepare_scene(&scene).unwrap();
    renderer.refresh_cut_face(Some(1), &mut || raster(4)).unwrap();
    frame(
        &mut renderer,
        Some(view(&scene, None, None, Some(1.))),
        &camera,
    );

    let (scene, selected) = triangle([0.; 3], 1);
    renderer.prepare_scene(&scene).unwrap();
    let camera = self::camera(&renderer);
    let (lo, hi) = renderer.shown_bounds().unwrap();
    for visibility in [
        HandleShowing::Moving { axis: Axis::X },
        HandleShowing::Resting,
        HandleShowing::Turning { axis: Axis::X },
        HandleShowing::Stretching { axis: Axis::Y },
    ] {
        renderer = self::renderer(&pair);
        renderer.prepare_scene(&scene).unwrap();
        let translating = matches!(visibility, HandleShowing::Moving { .. });
        let gizmo = HandleFrame {
            anchor: (lo + hi) * 0.5,
            arm_length: 0.5,
            showing: visibility,
            fixed_axes: [false, false, translating],
            fixed_rings: [false; 3],
            under_pointer: None,
            live_drag: None,
        };
        for _ in 0..if translating { 2 } else { 1 } {
            frame(
                &mut renderer,
                Some(view(&scene, Some(selected), Some(gizmo), None)),
                &camera,
            );
        }
    }
    let (mut multiple, root) = empty();
    for offset in [[0.; 3], [10., 0., 0.], [0., 10., 0.]] {
        append(&mut multiple, root, offset, 1);
    }
    renderer = self::renderer(&pair);
    renderer.prepare_scene(&multiple).unwrap();
    let camera = self::camera(&renderer);
    frame(
        &mut renderer,
        Some(view(&multiple, None, None, None)),
        &camera,
    );
}

#[cfg(windows)]
#[test]
fn dx12_draw() {
    // The generic adapter may be Vulkan; explicitly cover the Windows shipping backend.
    static DX12: OnceLock<Option<Gpu>> = OnceLock::new();
    let Some(pair) = DX12.get_or_init(|| {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        request(&adapter, wgpu::Limits::default())
    }) else {
        eprintln!("GPU contract skipped: DX12 unavailable");
        return;
    };
    let mut renderer = renderer(pair);
    let (scene, _) = triangle([5., 0., 0.], 1);
    renderer.prepare_scene(&scene).unwrap();
    let camera = camera(&renderer);
    frame(&mut renderer, Some(view(&scene, None, None, None)), &camera);
}

#[test]
fn plate_cache_and_envelope() {
    let Some(pair) = gpu(Profile::Normal) else {
        return;
    };
    let mut renderer = renderer(&pair);
    for _ in 0..2 {
        frame(&mut renderer, None, &plate_camera());
    }
    let camera = ViewCamera::new(Vec3::ZERO, 100.);
    for bed in [BED, BuildVolume::new(50., 50., 50.)] {
        renderer
            .draw_frame(None, bed, &camera, 64, 64)
            .unwrap()
            .unwrap();
        assert_eq!(
            renderer
                .passes
                .bed_buffers
                .as_ref()
                .map(|buffer| buffer.for_volume),
            Some(bed)
        );
    }
}

#[test]
fn render_scales_to_texture_limit() {
    let Some(pair) = gpu(Profile::Texture) else {
        return;
    };
    let texture = renderer(&pair)
        .draw_frame(None, BED, &plate_camera(), 300, 150)
        .unwrap()
        .unwrap();
    assert_eq!(
        (texture.width(), texture.height()),
        (TEXTURE_CAP, TEXTURE_CAP / 2)
    );
}

#[test]
fn attachment_refusals() {
    let Some(pair) = gpu(Profile::Texture) else {
        return;
    };
    let mut renderer = renderer(&pair);
    let over = TEXTURE_CAP + 44;
    let msaa = DrawTargets::create(
        &pair.0,
        &renderer.passes.edge.bind_group_layout,
        over,
        over,
    )
    .err()
    .unwrap();
    assert!(matches!(
        msaa,
        FrameRefusal::Refused {
            resource: FrameResource::Multisampled,
            ..
        }
    ));
    let colour = OutputPair::create(&pair.0, over, over).err().unwrap();
    assert!(matches!(
        colour,
        FrameRefusal::Refused {
            resource: FrameResource::Outputs,
            ..
        }
    ));
    let calls = std::cell::Cell::new(0);
    let mut oversized = || {
        calls.set(calls.get() + 1);
        raster(TEXTURE_CAP * 2)
    };
    let error = renderer.refresh_cut_face(Some(1), &mut oversized).unwrap_err();
    assert!(matches!(
        error,
        FrameRefusal::Refused {
            resource: FrameResource::CutTexture,
            ..
        }
    ));
    assert_eq!(calls.get(), 1);
    // A refused key is remembered so every paint does not repeat the allocation.
    renderer.refresh_cut_face(Some(1), &mut oversized).unwrap();
    assert_eq!(calls.get(), 1);
    let _ = renderer.refresh_cut_face(Some(2), &mut oversized);
    assert_eq!(calls.get(), 2);
    frame(&mut renderer, None, &plate_camera());
}

fn breaching_scene() -> Scene {
    let (mut scene, root) = empty();
    let shape = mesh([[0., -1., 50.], [0., 1., 50.], [120., 0., 50.]], 1);
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(shape)),
        )
        .unwrap();
    scene.add_child(root, id).unwrap();
    scene
}

fn red_pixels(bytes: &[u8]) -> usize {
    // Loose colour bounds tolerate driver blending without accepting neutral surfaces.
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| {
            let [r, g, b] = [
                i32::from(pixel[0]),
                i32::from(pixel[1]),
                i32::from(pixel[2]),
            ];
            r > 128 && r - g > 40 && r - b > 40
        })
        .count()
}

#[test]
fn warning_pixels_and_preview() {
    let Some(pair) = gpu(Profile::Normal) else {
        return;
    };
    let mut renderer = renderer(&pair);
    let camera = ViewCamera::new(Vec3::new(0., 0., 100.), 250.);
    let (safe, _) = triangle([0., 0., 50.], 1);
    let breach = breaching_scene();
    let mut counts = Vec::new();
    for scene in [&safe, &breach] {
        renderer.prepare_scene(scene).unwrap();
        let texture = frame(&mut renderer, Some(view(scene, None, None, None)), &camera);
        counts.push(red_pixels(&pixels(&pair, &texture)));
    }
    let [safe, breach_count] = counts[..] else {
        unreachable!()
    };
    assert!(safe < 5, "safe warning pixels: {safe}");
    assert!(breach_count >= 20, "breach warning pixels: {breach_count}");
    assert!(breach_count > safe * 4);
    let mut preview = Vec::new();
    for clip in [None, Some(100.)] {
        let texture = renderer
            .draw_frame(
                Some(view(&breach, None, None, clip)),
                BED,
                &camera,
                256,
                256,
            )
            .unwrap()
            .unwrap();
        preview.push(red_pixels(&pixels(&pair, &texture)));
    }
    assert!(preview[1] > 0, "outline retained: {preview:?}");
    assert!(preview[1] * 2 < preview[0], "wall removed: {preview:?}");
}

#[test]
fn scene_install_clear_resize_and_refill() {
    let Some(pair) = gpu(Profile::Normal) else {
        return;
    };
    let token = DeviceStamp::issue();
    let mut renderer =
        ViewRenderer::new(pair.0.clone(), token, pair.1.clone(), DeviceLoss::default()).unwrap();
    let (mut scene, leaf) = triangle([5., 0., 0.], 1);
    let uploaded = UploadedScene::create_gpu_scene(&pair.0, token, &scene, &DeviceLoss::default()).unwrap();
    assert!(renderer.swap_in_scene(uploaded));
    assert!(renderer.shown_bounds().is_some());
    renderer.prepare_scene(&scene).unwrap();
    assert!(renderer.shown_bounds().is_some());
    renderer.drop_scene();
    assert!(renderer.shown_bounds().is_none());
    renderer.prepare_scene(&scene).unwrap();
    let bounds = renderer.shown_bounds().unwrap();
    renderer.forget_sizes();
    assert_eq!(renderer.shown_bounds().unwrap(), bounds);
    let camera = camera(&renderer);
    scene.remove_leaf(leaf).unwrap();
    renderer.prepare_scene(&scene).unwrap();
    assert!(renderer.shown_bounds().is_none());
    frame(&mut renderer, Some(view(&scene, None, None, None)), &camera);
    let (mut fresh, root) = empty();
    renderer.prepare_scene(&fresh).unwrap();
    assert!(renderer.shown_bounds().is_none());
    append(&mut fresh, root, [0.; 3], 1);
    renderer.prepare_scene(&fresh).unwrap();
    assert!(renderer.shown_bounds().is_some());
}

#[test]
fn live_graph_growth_is_safe() {
    let Some(pair) = gpu(Profile::Normal) else {
        return;
    };
    let mut renderer = renderer(&pair);
    let (mut scene, _) = triangle([0.; 3], 1);
    renderer.prepare_scene(&scene).unwrap();
    let root = scene.roots()[0];
    append(&mut scene, root, [10., 0., 0.], 1);
    let camera = camera(&renderer);
    frame(&mut renderer, Some(view(&scene, None, None, None)), &camera);
}

#[test]
fn a_scene_built_for_another_device_is_not_installed() {
    let Some(pair) = gpu(Profile::Normal) else {
        return;
    };
    let mut renderer = renderer(&pair);
    let (scene, _) = triangle([0.; 3], 1);
    let elsewhere = UploadedScene::create_gpu_scene(&pair.0, DeviceStamp::issue(), &scene, &DeviceLoss::default());
    assert!(!renderer.swap_in_scene(elsewhere.unwrap()), "another device's token");
    assert!(renderer.shown_bounds().is_none(), "nothing was installed");
    let here = UploadedScene::create_gpu_scene(&pair.0, renderer.gpu_token(), &scene, &DeviceLoss::default());
    assert!(renderer.swap_in_scene(here.unwrap()), "this renderer's own token");
    assert!(renderer.shown_bounds().is_some());
}

/// Pixels in the outline's #2f81f7, which neither the parts' tint nor the
/// plate comes near.
fn outline_pixels(bytes: &[u8]) -> usize {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] < 70 && (110..150).contains(&p[1]) && p[2] > 225)
        .count()
}

/// The outline appears round a selected part and nowhere without one. Two
/// bugs hid each other here until R212: the outline read the stencil from
/// `.x`, which is 0 on DX12 (the mark is in `.y` there), and this test framed
/// its 1 mm triangle from about 2 mm, inside the renderer's near plane (a
/// share of the plate's size), so nothing was drawn to outline at all.
#[test]
fn the_selected_part_is_outlined() {
    let Some(pair) = gpu(Profile::Normal) else {
        return;
    };
    let mut renderer = renderer(&pair);
    let (scene, id) = triangle([0., 0., 10.], 1);
    renderer.prepare_scene(&scene).unwrap();
    // Far enough back that the near plane, sized from the plate, does not
    // cut the triangle away.
    let (lo, hi) = renderer.shown_bounds().expect("installed bounds");
    let camera = ViewCamera::new((lo + hi) * 0.5, 5.0);
    let mut counts = Vec::new();
    for selected in [None, Some(id)] {
        let texture = renderer
            .draw_frame(Some(view(&scene, selected, None, None)), BED, &camera, 256, 256)
            .unwrap()
            .unwrap();
        counts.push(outline_pixels(&pixels(&pair, &texture)));
    }
    assert_eq!(counts[0], 0, "no outline without a selection: {counts:?}");
    assert!(counts[1] >= 40, "an outline round the selection: {counts:?}");
}
