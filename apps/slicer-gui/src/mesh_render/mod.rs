// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The 3D view's renderer: draws the plate, the parts, the selection
//! outline, the slice cap and the gizmo into a texture Slint shows.
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use glam::{Mat4, Vec3};
use sc_scene::Scene;
use sc_scene::node::NodeId;
use slint::wgpu_29::wgpu;

use crate::build_volume::BuildVolume;
use crate::camera::{HOME_RANGE_FACTOR, ViewCamera};
use crate::slice_cap::CutImage;

mod device_loss;
mod device_ops;
mod error;
mod frame;
mod gizmo_buffers;
mod gizmo_entries;
#[cfg(test)]
pub(crate) mod mock;
mod pipelines;
mod plate_glyphs;
mod render_size;
mod renderer_trait;
pub mod scene_upload;
mod smooth_normals;
mod targets;
#[cfg(test)]
mod tests_gpu;

pub use device_loss::DeviceLoss;
use device_ops::Guarded;
pub use error::{FrameResource, FrameRefusal, StartRefusal, StartStep};
use frame::FrameUniforms;
use gizmo_buffers::GizmoBuffers;
use gizmo_entries::{
    HandleGroup, HandleUniforms, ring_entries, stretch_cube_entries,
    arrow_entries, even_cube_entry,
};
pub use gizmo_entries::{LiveDrag, HandleFrame, HandleHover, HandleShowing};
use pipelines::cap::CapPipeline;
use pipelines::gizmo::GizmoPipeline;
use pipelines::{HandlesGroup, OutlineGroup, PartsGroup, PlateGroup};
use pipelines::mesh::MeshPipeline;
use pipelines::outline::OutlinePipeline;
use pipelines::plate::{PlateGpuBuffers, PlatePasses};
use render_size::fit_texture_limit;
pub(crate) use renderer_trait::{MeshRenderer, SceneLook};
pub(crate) use scene_upload::UploadedScene;
use scene_upload::{
    PART_TRANSFORM_ENTRY_BYTES, build_envelope_warning_mask, build_part_transform_entries, build_plate_warning_masks,
    count_renderable_parts,
};
use targets::{DrawTargets, OutputPair};

thread_local! {
    /// What the plate's front tab says (R118): the brand, then the model on
    /// a line of its own (empty for none), kept as one string so it can key
    /// the plate's cached mesh.
    static PLATE_LABEL: std::cell::RefCell<String> = std::cell::RefCell::new(String::from("SLEPTIC\n"));
}

/// The colour the parts are drawn in for a resin's colour (R121), 0..1 like
/// the shader's other colours. A resin darker than a quarter grey is lifted
/// to that, so its shape still shows under the light.
pub(crate) fn model_shade(rgb: [u8; 3]) -> [f32; 3] {
    const FLOOR: f32 = 0.24;
    let c = rgb.map(|v| f32::from(v) / 255.0);
    let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    if luma >= FLOOR {
        return c;
    }
    let lift = (FLOOR - luma) / (1.0 - luma).max(1e-3);
    c.map(|v| v + (1.0 - v) * lift)
}

/// Letters the plate with the printer in use (R118: its brand, not
/// "FRONT"), and under the brand its model; whether anything changed.
pub(crate) fn set_plate_printer(manufacturer: &str, model: &str) -> bool {
    let brand = plate_label_for(manufacturer);
    let model = if brand == "SLEPTIC" && manufacturer.trim().eq_ignore_ascii_case("generic") {
        String::new()
    } else {
        plate_model_for(manufacturer, model)
    };
    let label = format!("{brand}\n{model}");
    PLATE_LABEL.with(|current| current.replace(label.clone()) != label)
}

/// The model as printed under the brand: the name without the maker it may
/// start with ("Elegoo Mars 3" under ELEGOO reads "Mars 3"), trimmed.
pub(crate) fn plate_model_for(manufacturer: &str, model: &str) -> String {
    let (maker, model) = (manufacturer.trim(), model.trim());
    let rest = model
        .get(..maker.len())
        .filter(|head| !maker.is_empty() && head.eq_ignore_ascii_case(maker))
        .map_or(model, |_| model[maker.len()..].trim_start());
    rest.to_owned()
}

/// The maker in capitals, or "SLEPTIC" for none or a generic printer.
pub(crate) fn plate_label_for(manufacturer: &str) -> String {
    match manufacturer.trim() {
        "" => "SLEPTIC".into(),
        name if name.eq_ignore_ascii_case("generic") => "SLEPTIC".into(),
        name => name.to_uppercase(),
    }
}

pub(in crate::mesh_render) fn plate_label() -> String {
    PLATE_LABEL.with(|label| label.borrow().clone())
}

/// The plate's two lines: the brand, and the model (empty for none).
pub(in crate::mesh_render) fn plate_lines() -> (String, String) {
    let label = plate_label();
    let (brand, model) = label.split_once('\n').unwrap_or((label.as_str(), ""));
    (brand.to_owned(), model.to_owned())
}

/// What a frame draws besides the plate. Selection and gizmo only mean
/// something with a scene, so they come with it.
#[derive(Clone, Copy, Debug)]
pub struct SceneRender<'a> {
    pub scene: &'a Scene,
    /// The scene's bounds as shown, plate offsets included; from the
    /// viewport's spatial cache, so depth clipping follows every edit.
    pub world_bounds: Option<(Vec3, Vec3)>,
    /// The part the outline goes round; one not drawn counts as none.
    pub outlined: Option<NodeId>,
    /// Every selected part, the main one included; may be empty.
    pub selected_ids: &'a [NodeId],
    /// The gizmo, drawn only when given.
    pub handles: Option<HandleFrame>,
    /// The slice cut height in mm: the parts are not drawn above it, so the
    /// slice slider reveals them layer by layer.
    pub cut_height: Option<f32>,
}

const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The gizmo's own depth; it uses no stencil.
const GIZMO_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Depth with an 8-bit stencil, where the parts pass marks the selection
/// for the outline pass. Every backend supports it at default limits.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
/// 4x multisampling smooths edges and grid lines; 8x adds little here.
const MSAA_SAMPLES: u32 = 4;
/// Share of the view's height a scene fills at its first framing.
const FILL_SHARE: f32 = 0.75;

/// A scene on the GPU with the bind group over its entry table, which must
/// be made again whenever that table is.
struct LiveScene {
    buffers: UploadedScene,
    part_bindings: wgpu::BindGroup,
}

/// Tells devices apart, so a scene built for one is never installed on
/// another. `wgpu::Device`'s equality cannot: two separate devices can
/// compare equal. Every new device gets a new token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceStamp(u64);

impl DeviceStamp {
    /// A token no other call in this process returns.
    pub fn issue() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Logs the device's uncaught errors instead of wgpu's default, panicking.
pub fn log_uncaught_gpu_errors(device: &wgpu::Device) {
    let log = |error: wgpu::Error| eprintln!("wgpu error (uncaptured): {error}");
    device.on_uncaptured_error(std::sync::Arc::new(log));
}

/// The device, its queue and token: made together and dropped together.
struct GpuHandles {
    gpu: wgpu::Device,
    commands: wgpu::Queue,
    token: DeviceStamp,
    loss: DeviceLoss,
    /// The device's largest 2D texture side.
    texture_limit: u32,
}

/// The pipelines and their fixed buffers, the slice cap's texture, and the
/// plate's buffers for the current build volume and plates (made on first
/// use and again when either changes).
struct PassSet {
    parts: MeshPipeline,
    edge: OutlinePipeline,
    arrows: GizmoPipeline,
    arrow_buffers: GizmoBuffers,
    bed: PlatePasses,
    cut: CapPipeline,
    cut_state: CutFaceState,
    bed_buffers: Option<PlateGpuBuffers>,
}

/// The slice cap's texture, by the layer it was made for. The bind group
/// keeps the texture alive.
enum CutFaceState {
    Off,
    Shown {
        layer_key: u64,
        bind_group: wgpu::BindGroup,
    },
    Refused {
        layer_key: u64,
    },
}

impl CutFaceState {
    /// The layer key this state was made for; none when the cap is off.
    fn face_key(&self) -> Option<u64> {
        let (Self::Shown { layer_key, .. } | Self::Refused { layer_key }) = self else {
            return None;
        };
        Some(*layer_key)
    }
}

/// What is sized to the view: made on the first frame, and again when the
/// size changes or after `invalidate_sizing` / `clear_scene`.
#[derive(Default)]
struct FrameTargets {
    /// The multisampled colour and depth the passes draw into.
    multisampled: Option<DrawTargets>,
    /// The resolved textures handed to Slint, reused round robin.
    outputs: Option<OutputPair>,
}

/// The plates the plate buffers were made for: how many, which is active,
/// whether only that one is drawn, each one's lock (R110: its tile shows a
/// padlock), and the brand on the front tab.
type PlateLayout = (usize, usize, bool, Vec<bool>, String);

pub struct ViewRenderer {
    plate_layout: PlateLayout,
    /// What the next frame looks like, set by the viewport each frame.
    pub(crate) look: SceneLook,
    gpu: GpuHandles,
    passes: PassSet,
    live: Option<LiveScene>,
    frames: FrameTargets,
}

// GPU handles say nothing useful when printed; whether a scene is held does.
impl std::fmt::Debug for ViewRenderer {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let has_scene = self.live.is_some();
        out.debug_struct("ViewRenderer").field("has_scene", &has_scene).finish_non_exhaustive()
    }
}

/// The vertical field of view, in radians: wide enough that a scene's
/// bounding sphere fills `FILL_SHARE` of the height from the first
/// framing distance. It depends on nothing else, so resizing the window
/// scales the picture without changing its perspective.
pub fn starting_fov_y() -> f32 {
    2.0 * (1.0 / (FILL_SHARE * HOME_RANGE_FACTOR)).atan()
}

/// The plate layout before any scene is known.
fn single_plate() -> PlateLayout {
    (1, 0, false, vec![false], plate_label())
}

impl ViewRenderer {
    /// Makes everything that does not depend on the scene or the view's size.
    pub fn new(
        device: wgpu::Device,
        token: DeviceStamp,
        queue: wgpu::Queue,
        loss_flag: DeviceLoss,
    ) -> Result<Self, StartRefusal> {
        let failed = |stage| move |source| StartRefusal { step: stage, source };
        let mesh = MeshPipeline::create(&device).map_err(failed(StartStep::PartsShader))?;
        let outline = OutlinePipeline::create(&device).map_err(failed(StartStep::OutlineShader))?;
        let gizmo_buffers =
            GizmoBuffers::create(&device, &queue).map_err(failed(StartStep::HandleShapes))?;
        let gizmo =
            GizmoPipeline::create(&device, &gizmo_buffers).map_err(failed(StartStep::HandlesShader))?;
        let plate = PlatePasses::create(&device, &mesh).map_err(failed(StartStep::PlateShader))?;
        let cap = CapPipeline::create(&device, &mesh).map_err(failed(StartStep::CutShader))?;
        let max_texture_side = device.limits().max_texture_dimension_2d;
        Ok(Self {
            plate_layout: single_plate(),
            look: SceneLook::default(),
            gpu: GpuHandles {
                gpu: device,
                commands: queue,
                token,
                loss: loss_flag,
                texture_limit: max_texture_side,
            },
            passes: PassSet {
                parts: mesh,
                edge: outline,
                arrows: gizmo,
                arrow_buffers: gizmo_buffers,
                bed: plate,
                cut: cap,
                cut_state: CutFaceState::Off,
                bed_buffers: None,
            },
            live: None,
            frames: FrameTargets::default(),
        })
    }

    /// The token a worker passes to `GpuScene::build`, so the scene it
    /// builds can be checked against this renderer's device on install.
    pub fn gpu_token(&self) -> DeviceStamp {
        self.gpu.token
    }

    pub fn drop_scene(&mut self) {
        self.live = None;
        self.forget_sizes();
    }

    /// Drops what is sized to the view, so the next frame makes it at the
    /// exact size (the viewport calls this once a drag-resize settles,
    /// having drawn at rounded sizes during it).
    pub fn forget_sizes(&mut self) {
        self.frames = FrameTargets::default();
    }

    /// The installed scene's bounds, taken when it was installed.
    pub fn shown_bounds(&self) -> Option<(Vec3, Vec3)> {
        self.live.as_ref().map(|scene| scene.buffers.uploaded_scene_bounds())
    }

    /// Builds `graph` on the GPU here and now and installs it. On error no
    /// scene is held; a scene with no parts holds none either (the plate is
    /// drawn alone).
    pub fn prepare_scene(&mut self, graph: &Scene) -> Result<()> {
        let core = &self.gpu;
        anyhow::ensure!(!core.loss.is_lost(), scene_upload::SceneUploadDeviceLost);
        // Dropped first, so the old and new scene never share the GPU.
        self.live = None;
        // An empty entry table cannot be bound.
        let parts = count_renderable_parts(graph)?;
        if parts > 0 {
            let core = &self.gpu;
            let gpu = UploadedScene::create_gpu_scene(&core.gpu, core.token, graph, &core.loss)?;
            self.adopt_scene(gpu);
        } else {
            self.drop_scene();
        }
        Ok(())
    }

    /// Installs a scene a worker built, if it was built for this device and
    /// the device is not lost; otherwise false, and the caller uploads it
    /// again.
    pub fn swap_in_scene(&mut self, gpu: UploadedScene) -> bool {
        let usable = gpu.scene_device_key() == self.gpu.token && !self.gpu.loss.is_lost();
        if usable {
            self.adopt_scene(gpu);
        }
        usable
    }

    fn adopt_scene(&mut self, gpu: UploadedScene) {
        // One entry's window of the table, as large as the layout expects;
        // each draw moves it to its entry with a dynamic offset.
        let window = wgpu::BufferBinding {
            buffer: gpu.scene_transform_buffer(),
            offset: 0,
            size: NonZeroU64::new(PART_TRANSFORM_ENTRY_BYTES),
        };
        let entry = wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(window),
        };
        let descriptor = wgpu::BindGroupDescriptor {
            label: Some("mesh-viewer models bg"),
            layout: &self.passes.parts.models_bind_group_layout,
            entries: std::slice::from_ref(&entry),
        };
        let models_bind_group = self.gpu.gpu.create_bind_group(&descriptor);
        self.live = Some(LiveScene { buffers: gpu, part_bindings: models_bind_group });
    }

    /// Makes what is sized to the view for `width` x `height`, keeping
    /// whatever already has that size. An old set is freed before its
    /// replacement is made, so it cannot be why the new one is refused; the
    /// multisampled targets come first, and their failure stops the rest.
    fn ensure_view_targets(&mut self, width: u32, height: u32) -> Result<(), FrameRefusal> {
        let device = &self.gpu.gpu;
        let sized = &mut self.frames;
        if !sized.multisampled.as_ref().is_some_and(|msaa| msaa.matches(width, height)) {
            sized.multisampled = None;
            let layout = &self.passes.edge.bind_group_layout;
            sized.multisampled = Some(DrawTargets::create(device, layout, width, height)?);
        }
        if !sized.outputs.as_ref().is_some_and(|pool| pool.matches(width, height)) {
            sized.outputs = None;
            sized.outputs = Some(OutputPair::create(device, width, height)?);
        }
        Ok(())
    }

    /// The plate's buffers for `build_volume`, made again when it differs
    /// from the kept one. After an error none are kept, so the next frame
    /// tries again. The result shares the GPU buffers.
    fn plate_buffers_ready(
        &mut self,
        build_volume: BuildVolume,
    ) -> Result<PlateGpuBuffers, FrameRefusal> {
        let buffers = match self.passes.bed_buffers.take() {
            Some(kept) if kept.for_volume == build_volume => kept,
            _ => {
                let (count, active, only_active, locks, _) = &self.plate_layout;
                PlateGpuBuffers::buffers_for_layout(
                    &self.gpu.gpu,
                    &self.gpu.commands,
                    build_volume,
                    *count,
                    *active,
                    *only_active,
                    locks,
                )?
            }
        };
        Ok(self.passes.bed_buffers.insert(buffers).clone())
    }

    /// Sets the slice cap to the layer `key`, `None` for no cap. The same
    /// key (shown or refused) does nothing, so orbiting does not redraw the
    /// layer; a new one calls `build` and uploads its coverage. `build`
    /// returning nothing turns the cap off.
    pub(crate) fn refresh_cut_face(
        &mut self,
        layer_key: Option<u64>,
        build: &mut dyn FnMut() -> Option<CutImage>,
    ) -> Result<(), FrameRefusal> {
        // Unchanged layer: nothing to do, whether it was shown or refused.
        if layer_key.is_some() && layer_key == self.passes.cut_state.face_key() {
            return Ok(());
        }
        let Some(layer_key) = layer_key else {
            self.passes.cut_state = CutFaceState::Off;
            return Ok(());
        };
        if self.gpu.loss.is_lost() {
            return Err(FrameRefusal::Lost);
        }
        let Some(raster) = build() else {
            self.passes.cut_state = CutFaceState::Off;
            return Ok(());
        };
        let uploaded =
            self.gpu.gpu.guarded(|| self.cap_bind_group(&raster)).map_err(|source| {
                FrameRefusal::Refused {
                    resource: FrameResource::CutTexture,
                    source,
                }
            });
        let (state, result) = match uploaded {
            Ok(bind_group) => (CutFaceState::Shown { layer_key, bind_group }, Ok(())),
            Err(err) => (CutFaceState::Refused { layer_key }, Err(err)),
        };
        self.passes.cut_state = state;
        result
    }

    /// The coverage as a one-channel texture, bound for the cap pipeline.
    /// `create_texture_with_data` takes the rows tightly packed and does any
    /// padding the copy needs itself.
    fn cap_bind_group(&self, raster: &CutImage) -> wgpu::BindGroup {
        use wgpu::util::DeviceExt as _;
        let device = &self.gpu.gpu;
        let coverage = device.create_texture_with_data(
            &self.gpu.commands,
            &wgpu::TextureDescriptor {
                label: Some("mesh-viewer cap texture"),
                size: wgpu::Extent3d { width: raster.width, height: raster.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &raster.texels,
        );
        let coverage_view = coverage.create_view(&wgpu::TextureViewDescriptor::default());
        let cap = &self.passes.cut;
        let texture_entry = wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&coverage_view),
        };
        let sampler_entry = wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&cap.sampler),
        };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh-viewer cap texture bg"),
            layout: &cap.texture_bind_group_layout,
            entries: &[texture_entry, sampler_entry],
        })
    }

    /// The plates as the scene has them, or one plate without a scene.
    fn plate_layout_for(&self, graph: Option<&Scene>) -> PlateLayout {
        let Some(graph) = graph else {
            return single_plate();
        };
        let plates = &graph.plates().order;
        let active = plates
            .iter()
            .position(|p| p.id == self.look.active_plate)
            .unwrap_or(0);
        let locks = plates.iter().map(|p| p.locked).collect();
        (
            plates.len(),
            active,
            self.look.only_active_plate,
            locks,
            plate_label(),
        )
    }

    /// Writes every part's entry for this frame; whether the parts can be
    /// drawn. They cannot when the scene's parts no longer match the
    /// installed ones one for one, or cannot be walked: writing then would
    /// run past the table, or leave stale entries for the draws to use.
    /// The next install puts that right.
    fn write_entries(
        &self,
        graph: &Scene,
        selection: Option<NodeId>,
        selected: &[NodeId],
        volume: BuildVolume,
    ) -> bool {
        let Some(installed) = &self.live else {
            return false;
        };
        let gpu = &installed.buffers;
        let Ok(mut entries) = build_part_transform_entries(graph, selection, volume) else {
            return false;
        };
        if entries.len() != gpu.scene_draw_count() {
            return false;
        }
        // Each entry at its own stride (aligned, usually 256 bytes): the
        // writes cannot be merged.
        for (i, (entry, part)) in entries.iter_mut().zip(gpu.upload_scene_parts()).enumerate() {
            if selected.contains(&part.upload_part_node()) {
                entry.flags[0] = 1;
            }
            self.gpu.commands.write_buffer(
                gpu.scene_transform_buffer(),
                i as u64 * gpu.scene_transform_stride(),
                bytemuck::bytes_of(entry),
            );
        }
        !entries.is_empty()
    }

    /// Draws a frame into a texture Slint can show; `Ok(None)` for an empty
    /// view. A view over the device's texture limit is drawn at the limit,
    /// in the same shape, so the texture can be smaller than asked.
    pub fn draw_frame(
        &mut self,
        scene_view: Option<SceneRender<'_>>,
        volume: BuildVolume,
        camera: &ViewCamera,
        width: u32,
        height: u32,
    ) -> Result<Option<wgpu::Texture>, FrameRefusal> {
        match (self.gpu.loss.is_lost(), width.min(height)) {
            (true, _) => return Err(FrameRefusal::Lost),
            // An empty view draws nothing.
            (false, 0) => return Ok(None),
            (false, _) => {}
        }
        let (width, height) = fit_texture_limit(width, height, self.gpu.texture_limit);
        let graph = scene_view.map(|s| s.scene);
        let selection = scene_view.and_then(|s| s.outlined);
        let selected: &[NodeId] = scene_view.map_or(&[], |s| s.selected_ids);
        self.ensure_view_targets(width, height)?;
        let layout = self.plate_layout_for(graph);
        if self.plate_layout != layout {
            self.passes.bed_buffers = None;
            self.plate_layout = layout;
        }
        let plate = self.plate_buffers_ready(volume)?;
        let draw_parts =
            graph.is_some_and(|g| self.write_entries(g, selection, selected, volume));

        // Near and far follow the scene and the build volume together, so
        // the plate stays within the depth range.
        let (env_min, env_max) = (
            volume.envelope_min_mm(),
            volume.envelope_max_mm(),
        );
        let scene_bounds = scene_view
            .and_then(|s| s.world_bounds)
            .or_else(|| self.shown_bounds());
        let (lo, hi) = scene_bounds.map_or((env_min, env_max), |(lo, hi)| {
            (lo.min(env_min), hi.max(env_max))
        });
        let view_proj = frame::clip_from_world_for(camera, lo, hi, width, height, starting_fov_y());

        // The walls parts cross, worked out afresh each frame (one pass
        // over the vertices), in step with the parts' own overhang flag. A
        // scene that cannot be walked has no parts drawn and no walls lit.
        let walls = graph
            .and_then(|g| build_envelope_warning_mask(g, volume).ok())
            .unwrap_or(0);
        let plate_walls = graph
            .and_then(|g| build_plate_warning_masks(g, volume).ok())
            .unwrap_or_default();
        let (count, active) = (self.plate_layout.0, self.plate_layout.1);
        let [cap_x, cap_y] = sc_scene::plate_grid_offset(
            active,
            count,
            volume.width_mm(),
            volume.depth_mm(),
        );
        let [r, g, b] = self.look.support_warning_rgb;
        let uniforms = FrameUniforms {
            clip_from_world: view_proj.to_cols_array_2d(),
            light_dir: frame::light_and_cut(scene_view.and_then(|s| s.cut_height)),
            env_min: [env_min.x, env_min.y, env_min.z, self.look.layer_band],
            env_max: [env_max.x, env_max.y, env_max.z, 0.0],
            face_breach_mask: [walls, r, g, b],
            cap_offset: [
                cap_x,
                cap_y,
                if self.look.light_scene { 1.0 } else { 0.0 },
                self.look.cure_shade,
            ],
            plate_breach: frame::pack_plate_breach(&plate_walls),
            model_color: [
                self.look.model_color[0],
                self.look.model_color[1],
                self.look.model_color[2],
                1.0,
            ],
        };
        let uniform_bytes = bytemuck::bytes_of(&uniforms);
        let uniform_buffer = &self.passes.parts.uniform_buffer;
        self.gpu.commands.write_buffer(uniform_buffer, 0, uniform_bytes);

        let target = self
            .frames
            .outputs
            .as_mut()
            .expect("ensure_view_targets made the pool")
            .next_output();
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let msaa = self
            .frames
            .multisampled
            .as_ref()
            .expect("ensure_view_targets made the multisampled targets");
        let encoder_label = wgpu::CommandEncoderDescriptor {
            label: Some("mesh-viewer encoder"),
        };
        let mut encoder = self.gpu.gpu.create_command_encoder(&encoder_label);
        self.record_main_pass(
            &mut encoder,
            &target_view,
            msaa,
            &plate,
            draw_parts,
            selection,
            selected,
        );
        if selection.is_some() && draw_parts {
            self.record_outline(&mut encoder, &target_view, msaa);
        }
        if let Some(gizmo) = scene_view.and_then(|s| s.handles) {
            self.record_gizmo(&mut encoder, &target_view, msaa, &gizmo, view_proj);
        }
        self.gpu.commands.submit(std::iter::once(encoder.finish()));
        Ok(Some(target))
    }

    /// The main pass, multisampled and resolved into `target_view`, in this
    /// order: the plate's sides (writing depth, so a low view sees the slab
    /// hide what is behind it), its top (not writing depth, so a part
    /// standing on it always shows), the origin axes, the parts, the slice
    /// cap, the warning walls (after the parts, whose depth hides the walls
    /// where a part is in front) and last the translucent underside. Only
    /// the parts set a stencil reference other than 0.
    #[allow(clippy::too_many_arguments)]
    fn record_main_pass(
        &self,
        commands: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        sampled: &DrawTargets,
        plate: &PlateGpuBuffers,
        draw_parts: bool,
        outlined: Option<NodeId>,
        picked_parts: &[NodeId],
    ) {
        let background = if self.look.light_scene {
            frame::CLEAR_COLOR_LIGHT
        } else {
            frame::BACKDROP
        };
        let drawn = wgpu::RenderPassColorAttachment {
            view: &sampled.color_view,
            resolve_target: Some(out_view),
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(background), store: wgpu::StoreOp::Store },
            depth_slice: None,
        };
        // Depth and the outlined stencil both start clear and are kept: the
        // outline pass reads the stencil afterwards.
        let depth_and_marks = wgpu::RenderPassDepthStencilAttachment {
            view: &sampled.depth_view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
            stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
        };
        let mut pass = begin_pass(commands, "mesh-viewer pass", drawn, Some(depth_and_marks));
        let pipes = &self.passes.bed;
        let indexed = |pass: &mut wgpu::RenderPass<'_>,
                       buffer: &wgpu::Buffer,
                       indices: std::ops::Range<u32>| {
            pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(indices, 0, 0..1);
        };
        let bind_plate = |pass: &mut wgpu::RenderPass<'_>| {
            pass.set_bind_group(
                PlateGroup::Frame.slot(),
                &pipes.view_bindings,
                &[],
            );
            pass.set_vertex_buffer(0, plate.corner_buffer.slice(..));
        };
        pass.set_pipeline(&pipes.side_faces);
        bind_plate(&mut pass);
        pass.set_stencil_reference(0);
        indexed(
            &mut pass,
            &plate.side_list_buffer,
            0..plate.side_list_len,
        );
        pass.set_pipeline(&pipes.top_faces);
        indexed(&mut pass, &plate.top_list_buffer, 0..plate.top_list_len);
        // Only the origin axes: since 2026-10-30 the build volume's 12 edges
        // (indices 0..24) are not drawn, as Orca and Bambu show the plate
        // alone. They stay in the buffer for the geometry tests.
        pass.set_pipeline(&pipes.edge_lines);
        indexed(
            &mut pass,
            &plate.edge_list_buffer,
            24..plate.edge_list_len,
        );
        if draw_parts && let Some(installed) = &self.live {
            self.record_parts(&mut pass, installed, outlined, picked_parts);
        }
        // The cap: one quad at the cut, its corners and height from the
        // shared uniforms, so nothing else is bound.
        if let CutFaceState::Shown { bind_group, .. } = &self.passes.cut_state {
            pass.set_pipeline(&self.passes.cut.pipeline);
            pass.set_bind_group(0, &self.passes.cut.uniforms_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.set_stencil_reference(0);
            pass.draw(0..6, 0..1);
        }
        // The walls use the top pipeline: triangles, depth tested, not
        // written, nothing culled, blended. The parts unbound the plate's
        // vertices, so they are bound again.
        for (pipeline, buffer, count) in [
            (
                &pipes.top_faces,
                &plate.wall_list_buffer,
                plate.wall_list_len,
            ),
            (
                &pipes.under_face,
                &plate.under_list_buffer,
                plate.under_list_len,
            ),
        ] {
            pass.set_pipeline(pipeline);
            bind_plate(&mut pass);
            pass.set_stencil_reference(0);
            indexed(&mut pass, buffer, 0..count);
        }
    }

    /// A draw per part at its entry's offset. The stencil reference is 1
    /// for selected parts, so the stencil ends up 1 exactly over them.
    fn record_parts(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        live_scene: &LiveScene,
        outlined: Option<NodeId>,
        picked_parts: &[NodeId],
    ) {
        pass.set_pipeline(&self.passes.parts.pipeline);
        pass.set_bind_group(
            PartsGroup::Frame.slot(),
            &self.passes.parts.uniforms_bind_group,
            &[],
        );
        let gpu = &live_scene.buffers;
        for (i, part) in gpu.upload_scene_parts().iter().enumerate() {
            if self.look.hidden_objects.contains(&i) {
                continue;
            }
            let offset = (i as u64 * gpu.scene_transform_stride()) as u32;
            pass.set_bind_group(
                PartsGroup::PartTable.slot(),
                &live_scene.part_bindings,
                &[offset],
            );
            pass.set_vertex_buffer(0, part.draw_vertex_storage().slice(..));
            pass.set_index_buffer(part.draw_index_storage().slice(..), wgpu::IndexFormat::Uint32);
            let id = part.upload_part_node();
            pass.set_stencil_reference(u32::from(Some(id) == outlined || picked_parts.contains(&id)));
            pass.draw_indexed(0..part.draw_index_total(), 0, 0..1);
        }
    }

    /// The selection outline, a full-screen pass over the parts pass's
    /// stencil; it keeps the image and paints only the edge. Only worth
    /// running with a selection and drawn parts: otherwise the stencil is 0.
    fn record_outline(
        &self,
        commands: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        sampled: &DrawTargets,
    ) {
        let mut pass = begin_pass(commands, "mesh-viewer outline pass", keep_image(out_view), None);
        pass.set_pipeline(&self.passes.edge.pipeline);
        pass.set_bind_group(
            OutlineGroup::Marks.slot(),
            &sampled.outline_bind_group,
            &[],
        );
        pass.draw(0..3, 0..1);
    }

    /// The gizmo, last, over everything. Its depth starts clear, so the
    /// parts never hide it, but is tested within the pass, so its axes
    /// overlap properly near the pivot. Only the entries of the handles
    /// shown are written, and only those slots are drawn from.
    fn record_gizmo(
        &self,
        commands: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        sampled: &DrawTargets,
        handles: &HandleFrame,
        clip_from_world: Mat4,
    ) {
        use HandleShowing as V;
        let buffers = &self.passes.arrow_buffers;
        let queue = &self.gpu.commands;
        let uniforms = HandleUniforms {
            clip_from_world: clip_from_world.to_cols_array_2d(),
            anchor_and_length: [handles.anchor.x, handles.anchor.y, handles.anchor.z, handles.arm_length],
        };
        queue.write_buffer(&buffers.uniforms_buffer, 0, bytemuck::bytes_of(&uniforms));
        let moving = matches!(
            handles.showing,
            V::Resting | V::Move | V::Moving { .. }
        );
        let turning = matches!(
            handles.showing,
            V::Resting | V::Turn | V::Turning { .. }
        );
        let scaling = matches!(
            handles.showing,
            V::Resting | V::Resize | V::Stretching { .. }
        );
        // Scaling draws the move handles' axis lines, so it needs their
        // entries too.
        if moving || scaling {
            buffers.write_entries(
                queue,
                HandleGroup::Arrows.first(),
                &arrow_entries(handles),
            );
        }
        if turning {
            buffers.write_entries(
                queue,
                HandleGroup::Rings.first(),
                &ring_entries(handles),
            );
        }
        if scaling {
            buffers.write_entries(
                queue,
                HandleGroup::Centre.first(),
                &[even_cube_entry(handles)],
            );
            buffers.write_entries(
                queue,
                HandleGroup::Cubes.first(),
                &stretch_cube_entries(handles),
            );
        }

        // Cleared for the gizmo alone and not kept: nothing reads it after.
        let gizmo_depth = wgpu::RenderPassDepthStencilAttachment {
            view: &sampled.gizmo_depth_view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
            stencil_ops: None,
        };
        let mut pass = begin_pass(commands, "mesh-viewer gizmo pass", keep_image(out_view), Some(gizmo_depth));
        let pipes = &self.passes.arrows;
        pass.set_bind_group(
            HandlesGroup::Frame.slot(),
            &pipes.uniforms_bind_group,
            &[],
        );
        let at_slot = |pass: &mut wgpu::RenderPass<'_>, slot: u64| {
            let offset = (slot * buffers.entry_stride) as u32;
            pass.set_bind_group(
                HandlesGroup::HandleTable.slot(),
                &pipes.entries_bind_group,
                &[offset],
            );
        };
        // `shape` drawn once per axis, at that axis's entry in `slots`, with
        // the indices `range(axis)` (none skips the axis).
        let per_axis = |pass: &mut wgpu::RenderPass<'_>,
                        pipeline: &wgpu::RenderPipeline,
                        shape: &gizmo_buffers::HandleShape,
                        slots: HandleGroup,
                        range: &dyn Fn(u64) -> Option<std::ops::Range<u32>>| {
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, shape.vertices.slice(..));
            pass.set_index_buffer(shape.indices.slice(..), wgpu::IndexFormat::Uint32);
            for axis in 0..3 {
                let Some(indices) = range(axis) else { continue };
                at_slot(pass, slots.at(axis));
                pass.draw_indexed(indices, 0, 0..1);
            }
        };
        let (lines, triangles) = (&pipes.line_pipeline, &pipes.triangle_pipeline);
        let whole = |shape: &gizmo_buffers::HandleShape| 0..shape.count;
        if moving || scaling {
            // The axis lines. Parts stay on the plate, so moving has no Z
            // handle; scaling keeps all three.
            per_axis(&mut pass, lines, &buffers.shaft, HandleGroup::Arrows, &|axis| {
                (axis != 2 || scaling).then(|| whole(&buffers.shaft))
            });
        }
        if moving {
            // X and Y: the arrow cone. Z: only its square, flat in the XY
            // plane (no vertical move), which is drawn from the last indices.
            let total = buffers.cone.count;
            let square_from = total - crate::gizmo_geometry::MOVE_PLANE_INDEX_COUNT;
            per_axis(&mut pass, triangles, &buffers.cone, HandleGroup::Arrows, &|axis| {
                Some(if axis == 2 { square_from..total } else { 0..square_from })
            });
        }
        if turning {
            // Each ring's crisp edge, then its tube over it.
            per_axis(&mut pass, lines, &buffers.ring_edge, HandleGroup::Rings, &|_| Some(whole(&buffers.ring_edge)));
            per_axis(&mut pass, triangles, &buffers.ring_tube, HandleGroup::Rings, &|_| Some(whole(&buffers.ring_tube)));
        }
        if scaling {
            per_axis(&mut pass, triangles, &buffers.cube, HandleGroup::Cubes, &|_| Some(whole(&buffers.cube)));
            // The centre cube: the same shape at its own entry.
            at_slot(&mut pass, HandleGroup::Centre.first());
            pass.draw_indexed(whole(&buffers.cube), 0, 0..1);
        }
    }
}

/// Draws over `target_view` and keeps what is already there.
fn keep_image(target_view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view: target_view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        },
        depth_slice: None,
    }
}

/// Begins a pass drawing into `color` (and `depth`, when given), without
/// queries or timestamps.
fn begin_pass<'e>(
    commands: &'e mut wgpu::CommandEncoder,
    label: &str,
    color: wgpu::RenderPassColorAttachment<'_>,
    depth: Option<wgpu::RenderPassDepthStencilAttachment<'_>>,
) -> wgpu::RenderPass<'e> {
    commands.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(color)],
        depth_stencil_attachment: depth,
        ..Default::default()
    })
}

/// The scene's bounds as the view shows them, plate offsets added, from the
/// spatial cache.
pub(crate) fn display_bounds(
    spatial: &sc_mesh_spatial::SceneSpatial,
    volume: BuildVolume,
) -> Option<(Vec3, Vec3)> {
    let graph = spatial.graph();
    let mut bounds: Option<(Vec3, Vec3)> = None;
    graph
        .for_each_reachable_leaf::<sc_scene::SceneError, _>(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
            |id| {
                if let Some(leaf) = spatial.leaf(id)
                    && let Some(b) = leaf.transformed_aabb(leaf.world_transform())
                {
                    let [x, y] = graph.plate_offset(
                        graph.plate_of(id),
                        volume.width_mm(),
                        volume.depth_mm(),
                    );
                    let offset = Vec3::new(x, y, 0.0);
                    let (lo, hi) = (
                        Vec3::from(b.min().coords()) + offset,
                        Vec3::from(b.max().coords()) + offset,
                    );
                    bounds = Some(bounds.map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))));
                }
                Ok(())
            },
        )
        .ok()?;
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R121: a resin's colour is used as it is; near-black is lifted just
    /// enough to show shading.
    #[test]
    fn parts_take_the_resins_colour_lifted_if_too_dark() {
        assert_eq!(model_shade([204, 102, 51]), [0.8, 0.4, 0.2]);
        let black = model_shade([0x3a, 0x3a, 0x3e]);
        let luma = 0.2126 * black[0] + 0.7152 * black[1] + 0.0722 * black[2];
        assert!((luma - 0.24).abs() < 0.01, "{black:?}");
    }

    #[test]
    fn the_plate_carries_the_printers_brand_and_model() {
        assert_eq!(plate_model_for("Elegoo", "Elegoo Mars 3"), "Mars 3");
        assert_eq!(plate_model_for("Creality", "HALOT-MAGE PRO"), "HALOT-MAGE PRO");
        assert_eq!(plate_model_for("UniFormation", " GK3 Ultra "), "GK3 Ultra");
        assert_eq!(plate_label_for("  Elegoo "), "ELEGOO");
        assert_eq!(plate_label_for("Generic"), "SLEPTIC");
        assert_eq!(plate_label_for(""), "SLEPTIC");
        assert!(set_plate_printer("Anycubic", "Photon Mono X"));
        assert!(!set_plate_printer("anycubic ", " Photon Mono X"));
        assert_eq!(plate_lines(), ("ANYCUBIC".to_owned(), "Photon Mono X".to_owned()));
        assert!(set_plate_printer("Anycubic", "Photon Mono X2"), "a new model letters the plate again");
        assert!(set_plate_printer("", ""));
        assert_eq!(plate_lines(), ("SLEPTIC".to_owned(), String::new()));
        assert!(set_plate_printer("Elegoo", "Mars 3"));
        assert!(set_plate_printer("Generic", "Generic Machine"));
        assert_eq!(plate_lines(), ("SLEPTIC".to_owned(), String::new()), "a generic printer has no model line");
    }

    #[test]
    fn device_tokens_are_never_repeated() {
        let (a, b) = (DeviceStamp::issue(), DeviceStamp::issue());
        assert_ne!(a, b);
    }
}
