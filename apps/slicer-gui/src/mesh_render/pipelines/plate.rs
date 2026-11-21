// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The build plate: a slab under the build area with rounded corners, the
//! build volume's edges, its warning walls, the front tab with the printer's
//! brand, and the plate's tool tiles with their icons.
//!
//! Four pipelines share one vertex layout and the parts' uniform buffer
//! (only its view and projection are read):
//! - top: the top face. It writes no depth, so a part standing at z = 0
//!   always shows over the plate.
//! - solid: the sides. They write depth, so from low down the slab hides
//!   what is behind it.
//! - bottom: the underside, translucent and culled from above, so it is
//!   seen only from below, drawn last so what is above shows through.
//! - line: the volume's 12 edges and the origin axes, writing depth.
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use slint::wgpu_29::wgpu;

use super::super::device_ops::Guarded;
use super::super::error::{FrameResource, FrameRefusal};
use super::super::frame::FrameUniforms;
use super::super::{DEPTH_FORMAT, MSAA_SAMPLES};
use super::mesh::MeshPipeline;
use super::{Recipe, bind_one, pipeline_layout, render_pipeline, shader, uniform_layout};
use crate::build_volume::BuildVolume;
use crate::surface_text;

/// One plate vertex; `kind` picks the shader's colouring.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(in crate::mesh_render) struct PlateCorner {
    pub(in crate::mesh_render) position: [f32; 3],
    pub(in crate::mesh_render) shade_kind: u32,
}

// The kinds, as plate.wgsl numbers them.
const SHADE_FLOOR: u32 = 0;
const SHADE_CAGE: u32 = 1;
const SHADE_PLATE_TOP_BORDER: u32 = 2;
const SHADE_PLATE_SIDE: u32 = 3;
const SHADE_PLATE_BOTTOM: u32 = 4;
/// The build volume's walls: -X, +X, -Y, +Y and the top, numbered in a row
/// because the shader takes `kind - SHADE_ENVELOPE_WALL_X_NEG` as the wall's
/// bit in `face_breach_mask`. A wall is discarded unless its bit is set,
/// and then drawn as red stripes.
const SHADE_ENVELOPE_WALL_X_NEG: u32 = 5;
const SHADE_ENVELOPE_WALL_X_POS: u32 = 6;
const SHADE_ENVELOPE_WALL_Y_NEG: u32 = 7;
const SHADE_ENVELOPE_WALL_Y_POS: u32 = 8;
const SHADE_ENVELOPE_WALL_Z_POS: u32 = 9;
/// The origin's X axis; Y and Z follow it.
const SHADE_AXIS_X: u32 = 10;
/// The top border of the active plate when there are several.
const SHADE_ACTIVE_BORDER: u32 = 13;
/// A tool tile beside the plate.
const SHADE_PLATE_TILE: u32 = 14;
/// Icons and the plate number on the tiles (R110): plain, red (delete),
/// dimmed (an action a locked plate refuses) and accent (the active plate's
/// number).
const SHADE_GLYPH: u32 = 15;
const SHADE_GLYPH_DANGER: u32 = 16;
const SHADE_GLYPH_DIM: u32 = 17;
const SHADE_GLYPH_ACCENT: u32 = 18;
/// The delete tile: dark red under a white bin, so it reads as destructive
/// at a glance (R113; a red outline on a grey tile did not).
const SHADE_PLATE_TILE_DANGER: u32 = 19;
/// The thin light line round the build area (R113).
const SHADE_EDGE_LINE: u32 = 20;
/// The brand on the front tab.
const SHADE_LABEL: u32 = 21;

/// Bits of `kind` from here up hold the plate's number in plate order, so
/// the shader lights each plate's walls from that plate's breach bits.
pub(in crate::mesh_render) const PLATE_INDEX_SHIFT: u32 = 5;

/// Width of the line round the build area, mm.
const EDGE_LINE_MM: f32 = 0.5;
/// How far icons sit above their tile, mm.
const GLYPH_LIFT_MM: f32 = 0.03;
/// The slab runs from z = 0 down to minus this, mm: thin and crisp (R110;
/// a thick, softly rounded one looked unprofessional).
const PLATE_THICKNESS_MM: f32 = 2.5;
/// How far the slab reaches past the build area on each side, mm.
const PLATE_OVERHANG_MM: f32 = 5.0;
/// Corner radius of the slab, mm.
const PLATE_CORNER_RADIUS_MM: f32 = 10.0;
/// The corners are superellipses |x|^4 + |y|^4 = 1 (R120): they meet the
/// straight sides with no curvature and no change of curvature (G3), so no
/// kink shows where a circular arc's would.
const PLATE_CORNER_EXPONENT: f32 = 4.0;
/// Steps along each corner.
const PLATE_CORNER_SEGMENTS: u32 = 16;
/// Points across the front tab.
const TAB_SAMPLES: usize = 48;
/// Steps along each corner of a tool tile.
const TILE_CORNER_STEPS: u32 = 3;

pub(in crate::mesh_render) struct PlatePasses {
    pub(in crate::mesh_render) top_faces: wgpu::RenderPipeline,
    pub(in crate::mesh_render) side_faces: wgpu::RenderPipeline,
    pub(in crate::mesh_render) under_face: wgpu::RenderPipeline,
    pub(in crate::mesh_render) edge_lines: wgpu::RenderPipeline,
    /// Over the parts' uniform buffer, which the parts pass already fills
    /// every frame.
    pub(in crate::mesh_render) view_bindings: wgpu::BindGroup,
}

impl PlatePasses {
    pub(in crate::mesh_render) fn create(
        device: &wgpu::Device,
        mesh: &MeshPipeline,
    ) -> Result<Self, wgpu::Error> {
        device.guarded(|| {
            let shader = shader(
                device,
                "mesh-viewer plate shader",
                include_str!("../shaders/plate.wgsl"),
            );
            // The parts' uniforms layout has this shape, but wgpu matches
            // layouts by identity, so the plate keeps its own.
            let uniforms_layout = uniform_layout(
                device,
                "mesh-viewer plate uniforms bgl",
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                size_of::<FrameUniforms>() as u64,
                false,
            );
            let layout = pipeline_layout(device, "mesh-viewer plate pll", &[&uniforms_layout]);
            let vertex = wgpu::VertexBufferLayout {
                array_stride: size_of::<PlateCorner>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32],
            };
            // Blending is on throughout; only the underside is drawn with
            // alpha below 1. The stencil is left alone, so the outline pass
            // never sees the plate.
            let make = |label, topology, cull, writes_depth: bool| {
                let depth = wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(writes_depth),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                };
                let recipe = Recipe {
                    label,
                    shader: &shader,
                    vertices: std::slice::from_ref(&vertex),
                    topology,
                    cull,
                    depth: Some(depth),
                    samples: MSAA_SAMPLES,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                };
                render_pipeline(device, &layout, recipe)
            };
            use wgpu::PrimitiveTopology::{LineList, TriangleList};
            let top_pipeline = make("mesh-viewer plate top pipeline", TriangleList, None, false);
            let solid_pipeline = make("mesh-viewer plate solid pipeline", TriangleList, None, true);
            let line_pipeline = make("mesh-viewer plate cage pipeline", LineList, None, true);
            // The underside winds counter-clockwise seen from below, so
            // culling back faces hides it from above.
            let bottom_pipeline = make(
                "mesh-viewer plate bottom pipeline",
                TriangleList,
                Some(wgpu::Face::Back),
                false,
            );
            let uniforms_bind_group = bind_one(
                device,
                "mesh-viewer plate uniforms bg",
                &uniforms_layout,
                mesh.uniform_buffer.as_entire_binding(),
            );
            Self {
                top_faces: top_pipeline,
                side_faces: solid_pipeline,
                under_face: bottom_pipeline,
                edge_lines: line_pipeline,
                view_bindings: uniforms_bind_group,
            }
        })
    }
}

/// The plate's buffers for one build volume: all vertices in one buffer,
/// and an index buffer for each way they are drawn. Cloning shares the GPU
/// buffers.
#[derive(Clone)]
pub(in crate::mesh_render) struct PlateGpuBuffers {
    pub(in crate::mesh_render) for_volume: BuildVolume,
    pub(in crate::mesh_render) corner_buffer: wgpu::Buffer,
    pub(in crate::mesh_render) top_list_buffer: wgpu::Buffer,
    pub(in crate::mesh_render) side_list_buffer: wgpu::Buffer,
    pub(in crate::mesh_render) under_list_buffer: wgpu::Buffer,
    pub(in crate::mesh_render) edge_list_buffer: wgpu::Buffer,
    /// The warning walls, drawn with the top pipeline after the parts.
    pub(in crate::mesh_render) wall_list_buffer: wgpu::Buffer,
    pub(in crate::mesh_render) top_list_len: u32,
    pub(in crate::mesh_render) side_list_len: u32,
    pub(in crate::mesh_render) under_list_len: u32,
    pub(in crate::mesh_render) edge_list_len: u32,
    pub(in crate::mesh_render) wall_list_len: u32,
}

impl PlateGpuBuffers {
    /// One plate, number 1, unlocked.
    #[cfg(test)]
    pub(in crate::mesh_render) fn one_plate_buffers(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        build_volume: BuildVolume,
    ) -> Result<Self, FrameRefusal> {
        Self::buffers_for_layout(device, queue, build_volume, 1, 0, false, &[false])
    }

    /// `count` plates with `active` highlighted, or only `active` where it
    /// stands; `locks` says which are locked.
    pub(in crate::mesh_render) fn buffers_for_layout(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        build_volume: BuildVolume,
        count: usize,
        active: usize,
        only_active: bool,
        locks: &[bool],
    ) -> Result<Self, FrameRefusal> {
        let geometry = match only_active {
            true => active_plate_mesh(build_volume, count, active, locks),
            false => all_plates_mesh(build_volume, count, active, locks),
        };
        let upload = |label, contents: &[u8], usage| {
            device.try_upload(queue, label, contents, usage).map_err(|source| {
                FrameRefusal::Refused {
                    resource: FrameResource::PlateBuffers,
                    source,
                }
            })
        };
        let index = |label, indices: &[u32]| {
            upload(
                label,
                bytemuck::cast_slice(indices),
                wgpu::BufferUsages::INDEX,
            )
        };
        let count = |indices: &[u32]| indices.len() as u32;
        let g = &geometry;
        Ok(Self {
            for_volume: build_volume,
            corner_buffer: upload(
                "mesh-viewer plate vbo",
                bytemuck::cast_slice(&g.plate_corners),
                wgpu::BufferUsages::VERTEX,
            )?,
            top_list_buffer: index("mesh-viewer plate top ibo", &g.top_list)?,
            side_list_buffer: index("mesh-viewer plate solid ibo", &g.side_list)?,
            under_list_buffer: index("mesh-viewer plate bottom ibo", &g.under_list)?,
            edge_list_buffer: index("mesh-viewer plate cage ibo", &g.edge_list)?,
            wall_list_buffer: index("mesh-viewer plate wall ibo", &g.wall_list)?,
            top_list_len: count(&g.top_list),
            side_list_len: count(&g.side_list),
            under_list_len: count(&g.under_list),
            edge_list_len: count(&g.edge_list),
            wall_list_len: count(&g.wall_list),
        })
    }
}

/// A plate's vertices and the index lists of each pipeline.
#[derive(Default)]
pub(in crate::mesh_render) struct PlateMesh {
    pub(in crate::mesh_render) plate_corners: Vec<PlateCorner>,
    pub(in crate::mesh_render) top_list: Vec<u32>,
    pub(in crate::mesh_render) side_list: Vec<u32>,
    pub(in crate::mesh_render) under_list: Vec<u32>,
    pub(in crate::mesh_render) edge_list: Vec<u32>,
    pub(in crate::mesh_render) wall_list: Vec<u32>,
}

impl PlateMesh {
    /// Adds a vertex and returns its index.
    fn vertex(&mut self, position: [f32; 3], kind: u32) -> u32 {
        self.plate_corners.push(PlateCorner { position, shade_kind: kind });
        self.plate_corners.len() as u32 - 1
    }

    /// The next vertex's index.
    fn next(&self) -> u32 {
        self.plate_corners.len() as u32
    }

    /// Adds four corners as two triangles (0 1 2, 0 2 3) to a list chosen
    /// by `list`.
    fn quad(&mut self, corners: [[f32; 3]; 4], kind: u32, list: fn(&mut Self) -> &mut Vec<u32>) {
        let base = self.next();
        for corner in corners {
            self.vertex(corner, kind);
        }
        list(self).extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Adds flat triangles at height `z` to the top list.
    fn triangles(&mut self, triangles: impl IntoIterator<Item = [Vec2; 3]>, z: f32, kind: u32) {
        for triangle in triangles {
            let base = self.next();
            for p in triangle {
                self.vertex([p.x, p.y, z], kind);
            }
            self.top_list
                .extend_from_slice(&[base, base + 1, base + 2]);
        }
    }

    fn top(&mut self) -> &mut Vec<u32> {
        &mut self.top_list
    }

    fn solid(&mut self) -> &mut Vec<u32> {
        &mut self.side_list
    }

    /// Moves the plate by `(x, y)` and stamps every vertex with its plate
    /// number.
    fn place(&mut self, plate: usize, [x, y]: [f32; 2]) {
        for v in &mut self.plate_corners {
            v.position[0] += x;
            v.position[1] += y;
            v.shade_kind |= (plate as u32) << PLATE_INDEX_SHIFT;
        }
    }
}

/// Plate 1, unlocked and active.
#[cfg(test)]
pub(in crate::mesh_render) fn single_plate_mesh(build_volume: BuildVolume) -> PlateMesh {
    plate_mesh_for(build_volume, 1, false, true)
}

/// The slab's outline seen from above, counter-clockwise: the four corners
/// from +X+Y round to +X-Y, each `PLATE_CORNER_SEGMENTS + 1` points; the
/// straight sides run between one corner's last point and the next's first.
fn slab_outline(half_width: f32, half_depth: f32) -> Vec<[f32; 2]> {
    use std::f32::consts::{FRAC_PI_2, PI};
    // A small plate gets a smaller radius, so the outline never crosses.
    let radius = PLATE_CORNER_RADIUS_MM.min(half_width.min(half_depth));
    let (x, y) = (half_width - radius, half_depth - radius);
    let corners = [
        (x, y, 0.0_f32),
        (-x, y, FRAC_PI_2),
        (-x, -y, PI),
        (x, -y, 3.0 * FRAC_PI_2),
    ];
    // A superellipse point: sign(c)|c|^(2/n), sign(s)|s|^(2/n).
    let power = 2.0 / PLATE_CORNER_EXPONENT;
    let bend = |v: f32| v.signum() * v.abs().powf(power);
    let steps = PLATE_CORNER_SEGMENTS;
    let mut outline = Vec::with_capacity(4 * (steps as usize + 1));
    for (cx, cy, start) in corners {
        for i in 0..=steps {
            let angle = start + i as f32 / steps as f32 * FRAC_PI_2;
            outline.push([
                cx + radius * bend(angle.cos()),
                cy + radius * bend(angle.sin()),
            ]);
        }
    }
    outline
}

/// One plate: plate `number` on its tile (in the accent colour when
/// `active`), with an open or closed padlock, its actions dimmed when
/// `locked`.
///
/// The vertices start in a fixed order the shader and tests rely on, with
/// `n` points in the outline:
/// - 0..4: the build area's corners at z = 0, floor (the grid);
/// - 4..8: the same corners as top border. The ring's triangles use these,
///   so every ring triangle is all border: colour is taken flat from a
///   triangle's first vertex, and a floor corner would show the grid there;
/// - then the outline three times: top border at z = 0, side at z = 0, side
///   at the bottom;
/// - the underside's centre, as underside: the first vertex of each of its
///   triangles, so the whole underside is drawn translucent;
/// - the 8 corners of the build volume, bottom four then top four;
/// - the 5 walls, 4 corners each: -X, +X, -Y, +Y, top.
///
/// The origin axes, the line round the build area, the front tab and its
/// lettering, the tool tiles and their icons follow, in that order.
pub(in crate::mesh_render) fn plate_mesh_for(
    build_volume: BuildVolume,
    number: usize,
    locked: bool,
    active: bool,
) -> PlateMesh {
    let (lo, hi) = (
        build_volume.envelope_min_mm(),
        build_volume.envelope_max_mm(),
    );
    let half_width = 0.5 * build_volume.width_mm() + PLATE_OVERHANG_MM;
    let half_depth = 0.5 * build_volume.depth_mm() + PLATE_OVERHANG_MM;
    let bottom = -PLATE_THICKNESS_MM;
    let outline = slab_outline(half_width, half_depth);
    let n = outline.len() as u32;
    let mut g = PlateMesh::default();

    // Counter-clockwise from +X+Y, as the outline's corners.
    let area = [[hi.x, hi.y], [lo.x, hi.y], [lo.x, lo.y], [hi.x, lo.y]];
    for kind in [SHADE_FLOOR, SHADE_PLATE_TOP_BORDER] {
        for [x, y] in area {
            g.vertex([x, y, 0.0], kind);
        }
    }
    let rim = g.next();
    for (kind, z) in [
        (SHADE_PLATE_TOP_BORDER, 0.0),
        (SHADE_PLATE_SIDE, 0.0),
        (SHADE_PLATE_SIDE, bottom),
    ] {
        for &[x, y] in &outline {
            g.vertex([x, y, z], kind);
        }
    }
    let (side_top, side_bottom) = (rim + n, rim + 2 * n);
    let centre = g.vertex([0.0, 0.0, bottom], SHADE_PLATE_BOTTOM);
    // A rectangle's corners, walked round from its low corner; the cage,
    // the walls and their index lists all rely on this order.
    const ROUND: [[bool; 2]; 4] = [[false, false], [true, false], [true, true], [false, true]];
    let (min, max) = (lo.to_array(), hi.to_array());
    let pick = |axis: usize, high: bool| if high { max[axis] } else { min[axis] };
    let cage = g.next();
    for z in [lo.z, hi.z] {
        for [high_x, high_y] in ROUND {
            g.vertex([pick(0, high_x), pick(1, high_y), z], SHADE_CAGE);
        }
    }
    // Each wall lies in the volume's face, as do the edges; the edges are
    // drawn first and win the depth test, so they stay visible over the
    // stripes. A wall is fixed on one axis, at the volume's low or high
    // side, and spans the other two; there is none on the floor.
    let walls = g.next();
    let faces = [
        (SHADE_ENVELOPE_WALL_X_NEG, 0, false),
        (SHADE_ENVELOPE_WALL_X_POS, 0, true),
        (SHADE_ENVELOPE_WALL_Y_NEG, 1, false),
        (SHADE_ENVELOPE_WALL_Y_POS, 1, true),
        (SHADE_ENVELOPE_WALL_Z_POS, 2, true),
    ];
    for (shade, fixed, high_side) in faces {
        let [u, v] = [[1, 2], [0, 2], [0, 1]][fixed];
        for [high_u, high_v] in ROUND {
            let mut corner = [0.0; 3];
            corner[fixed] = pick(fixed, high_side);
            corner[u] = pick(u, high_u);
            corner[v] = pick(v, high_v);
            g.vertex(corner, shade);
        }
    }

    // Top: the build area as two triangles, then the ring between it and
    // the outline: for each corner of the area, a fan to its rounded
    // corner, then a quad along the straight side to the next.
    g.top_list.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    let steps = PLATE_CORNER_SEGMENTS;
    let per_corner = steps + 1;
    for c in 0..4 {
        let (inner, next_inner) = (4 + c, 4 + (c + 1) % 4);
        for s in 0..steps {
            let arc = rim + c * per_corner + s;
            g.top_list.extend_from_slice(&[inner, arc, arc + 1]);
        }
        let (last, next_first) = (rim + c * per_corner + steps, rim + (c + 1) % 4 * per_corner);
        g.top_list
            .extend_from_slice(&[inner, next_inner, next_first, inner, next_first, last]);
    }
    // Sides: a quad per outline step, wound counter-clockwise from outside.
    // Underside: a fan from its centre, counter-clockwise from below.
    for i in 0..n {
        let j = (i + 1) % n;
        let (top_i, top_j, low_i, low_j) =
            (side_top + i, side_top + j, side_bottom + i, side_bottom + j);
        g.side_list
            .extend_from_slice(&[top_i, low_i, low_j, top_i, low_j, top_j]);
        g.under_list.extend_from_slice(&[centre, low_j, low_i]);
    }
    // The volume's edges: each bottom edge, its top edge and a post.
    for i in 0..4 {
        let j = (i + 1) % 4;
        let (low, high) = (cage, cage + 4);
        g.edge_list.extend_from_slice(&[
            low + i,
            low + j,
            high + i,
            high + j,
            low + i,
            high + i,
        ]);
    }
    // The origin axes, short positive lines just over the floor at the
    // volume's corner.
    let origin = [lo.x, lo.y, 0.15];
    let length = build_volume
        .width_mm()
        .min(build_volume.depth_mm())
        .min(120.0)
        * 0.1;
    for axis in 0..3 {
        let mut end = origin;
        end[axis] += length;
        let kind = SHADE_AXIS_X + axis as u32;
        let start = g.vertex(origin, kind);
        g.vertex(end, kind);
        g.edge_list.extend_from_slice(&[start, start + 1]);
    }
    // Walls: their pipeline culls nothing, so one winding serves all five.
    for wall in 0..5 {
        let base = walls + 4 * wall;
        g.wall_list
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let tab = add_edge_and_front_tab(&mut g, build_volume, -half_depth, bottom);
    add_brand(&mut g, tab);
    add_tiles(&mut g, build_volume, locked);
    add_tile_icons(&mut g, build_volume, number, locked, active);
    g
}

/// Where the front tab's lettering goes: its centre, height and room.
struct TabLabel {
    centre: Vec2,
    depth: f32,
    room: f32,
}

/// The thin line round the build area and the tab off the front edge
/// (R113, after Lychee's plate with its front tab and Orca's rounded one).
fn add_edge_and_front_tab(
    g: &mut PlateMesh,
    build_volume: BuildVolume,
    front: f32,
    bottom: f32,
) -> TabLabel {
    let (w, d) = (0.5 * build_volume.width_mm(), 0.5 * build_volume.depth_mm());
    let (e, z) = (EDGE_LINE_MM, 0.02);
    let (top, solid) = (PlateMesh::top, PlateMesh::solid);
    for [x0, y0, x1, y1] in [
        [-w, -d, w, -d + e],
        [-w, d - e, w, d],
        [-w, -d, -w + e, d],
        [w - e, -d, w, d],
    ] {
        g.quad(
            [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]],
            SHADE_EDGE_LINE,
            top,
        );
    }
    // The tab is as thick as the plate. Its sides leave the edge along a
    // septic smoothstep, flat in slope, curvature and its change at both
    // ends (G3 into the edge and the tab's front, R120).
    let depth = (0.06 * build_volume.depth_mm() + 3.0).clamp(6.0, 12.0);
    let half = (0.24 * build_volume.width_mm()).clamp(3.0 * depth + 10.0, 60.0);
    let inner = half - 2.0 * depth;
    let reach = |x: f32| {
        let t = ((half - x.abs()) / (half - inner)).clamp(0.0, 1.0);
        depth * t.powi(4) * (35.0 - 84.0 * t + 70.0 * t * t - 20.0 * t * t * t)
    };
    let xs: Vec<f32> = (0..TAB_SAMPLES)
        .map(|i| -half + 2.0 * half * i as f32 / (TAB_SAMPLES - 1) as f32)
        .collect();
    for pair in xs.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (ya, yb) = (front - reach(a), front - reach(b));
        // Over the rim by half a millimetre, so no seam shows.
        g.quad(
            [
                [a, front + 0.5, 0.0],
                [a, ya, 0.0],
                [b, yb, 0.0],
                [b, front + 0.5, 0.0],
            ],
            SHADE_PLATE_TOP_BORDER,
            top,
        );
        // Both windings, so the side shows however the pipeline culls.
        g.quad(
            [[a, ya, 0.0], [a, ya, bottom], [b, yb, bottom], [b, yb, 0.0]],
            SHADE_PLATE_SIDE,
            solid,
        );
        g.quad(
            [[b, yb, 0.0], [b, yb, bottom], [a, ya, bottom], [a, ya, 0.0]],
            SHADE_PLATE_SIDE,
            solid,
        );
    }
    TabLabel {
        centre: Vec2::new(0.0, front - 0.5 * depth),
        depth,
        room: inner,
    }
}

/// Model lettering's size against the brand's.
const MODEL_SIZE: f32 = 0.55;
/// Space between the brand and model lines, in brand cap heights.
const LINE_GAP: f32 = 0.3;
/// How much of the lip's height and width the lettering may take.
const LIP_FILL_HEIGHT: f32 = 0.9;
const LIP_FILL_WIDTH: f32 = 0.95;

/// The lowest and highest y of an outline's points.
fn vertical_extent(text: &surface_text::TextOutline) -> (f32, f32) {
    use surface_text::Segment;
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for segment in &text.segments {
        let points: &[Vec2] = match segment {
            Segment::Move(p) | Segment::Line(p) => std::slice::from_ref(p),
            Segment::Quad(a, p) => &[*a, *p],
            Segment::Cubic(a, b, p) => &[*a, *b, *p],
            Segment::Close => &[],
        };
        for p in points {
            low = low.min(p.y);
            high = high.max(p.y);
        }
    }
    if low > high { (-0.5, 0.5) } else { (low, high) }
}

/// The printer on the front lip (R118, R120), as large as the lip allows:
/// the brand's symbol and name, and below them, smaller, the model.
fn add_brand(g: &mut PlateMesh, tab: TabLabel) {
    let (brand, model) = super::super::plate_lines();
    let brand = surface_text::brand_lockup(&brand);
    let (brand_low, brand_high) = vertical_extent(&brand);
    let width_room = 2.0 * LIP_FILL_WIDTH * tab.room;
    let height_room = LIP_FILL_HEIGHT * tab.depth;
    let glyphs = |text: &surface_text::TextOutline, origin: Vec2, scale: f32| {
        super::super::plate_glyphs::glyph_triangles(text, None, |p| origin + p * scale)
    };
    if model.is_empty() {
        let scale = (height_room / (brand_high - brand_low)).min(width_room / brand.width.max(0.01));
        let origin = tab.centre - Vec2::new(0.0, 0.5 * (brand_low + brand_high) * scale);
        g.triangles(glyphs(&brand, origin, scale), 0.02, SHADE_LABEL);
        return;
    }
    let model = surface_text::outline(&model);
    let (model_low, model_high) = vertical_extent(&model);
    // Both lines stacked, in units of the brand's lettering.
    let block = (brand_high - brand_low) + LINE_GAP + MODEL_SIZE * (model_high - model_low);
    let widest = brand.width.max(MODEL_SIZE * model.width).max(0.01);
    let scale = (height_room / block).min(width_room / widest);
    let top = tab.centre.y + 0.5 * block * scale;
    let brand_origin = Vec2::new(tab.centre.x, top - brand_high * scale);
    let model_top = top - (brand_high - brand_low + LINE_GAP) * scale;
    let model_origin = Vec2::new(tab.centre.x, model_top - MODEL_SIZE * model_high * scale);
    g.triangles(glyphs(&brand, brand_origin, scale), 0.02, SHADE_LABEL);
    g.triangles(glyphs(&model, model_origin, MODEL_SIZE * scale), 0.02, SHADE_LABEL);
}

/// The tool tiles beside the plate (see `BuildVolume::plate_tile_rects`):
/// flat rounded squares in the top face (R113), each a fan from its centre.
/// Delete (slot 1) is red unless the plate is locked.
fn add_tiles(g: &mut PlateMesh, build_volume: BuildVolume, locked: bool) {
    use std::f32::consts::PI;
    for (slot, [x0, y0, x1, y1]) in build_volume.plate_tile_rects().into_iter().enumerate() {
        let kind = if slot == 1 && !locked {
            SHADE_PLATE_TILE_DANGER
        } else {
            SHADE_PLATE_TILE
        };
        let r = 0.2 * (x1 - x0).min(y1 - y0);
        let centre = g.vertex([0.5 * (x0 + x1), 0.5 * (y0 + y1), 0.0], kind);
        // Corner centres and starting angles, in half turns.
        for (cx, cy, start) in [
            (x1 - r, y1 - r, 0.0f32),
            (x0 + r, y1 - r, 0.5),
            (x0 + r, y0 + r, 1.0),
            (x1 - r, y0 + r, 1.5),
        ] {
            for k in 0..=TILE_CORNER_STEPS {
                let a = (start + 0.5 * k as f32 / TILE_CORNER_STEPS as f32) * PI;
                g.vertex([cx + r * a.cos(), cy + r * a.sin(), 0.0], kind);
            }
        }
        let ring = 4 * (TILE_CORNER_STEPS + 1);
        for k in 0..ring {
            g.top_list
                .extend_from_slice(&[centre, centre + 1 + k, centre + 1 + (k + 1) % ring]);
        }
    }
}

/// The plate number and the action icons as flat triangles just over their
/// tiles (R110: drawn as 2D paths over the image, they vanished when a tile
/// was small on screen or a part stood in front).
fn add_tile_icons(
    g: &mut PlateMesh,
    build_volume: BuildVolume,
    number: usize,
    locked: bool,
    active: bool,
) {
    use super::super::plate_glyphs::glyph_triangles as glyphs;
    for (slot, [x0, y0, x1, y1]) in build_volume.plate_tile_rects().into_iter().enumerate() {
        let size = (x1 - x0).min(y1 - y0);
        let centre = Vec2::new(0.5 * (x0 + x1), 0.5 * (y0 + y1));
        let (triangles, kind) = if slot == 0 {
            let text = surface_text::outline(&number.to_string());
            let scale = (size * 0.55).min(size * 0.8 / text.width.max(0.01));
            let kind = if active {
                SHADE_GLYPH_ACCENT
            } else {
                SHADE_GLYPH
            };
            (glyphs(&text, None, |p| centre + p * scale), kind)
        } else {
            let icon = match slot {
                4 => surface_text::plate_lock_icon(locked),
                _ => surface_text::plate_icon(slot),
            };
            let kind = match slot {
                1..=3 if locked => SHADE_GLYPH_DIM,
                1 => SHADE_GLYPH_DANGER,
                _ => SHADE_GLYPH,
            };
            // Icons are drawn on a 24-unit grid, y down.
            let side = size * 0.72;
            (
                glyphs(icon, Some(1.6), |p| {
                    centre + Vec2::new(p.x / 24.0 - 0.5, 0.5 - p.y / 24.0) * side
                }),
                kind,
            )
        };
        g.triangles(triangles, GLYPH_LIFT_MM, kind);
    }
}

/// Only plate `active` of `count`, where it stands on the plate grid: the
/// Slice view shows only the plate that was sliced (as asked), so its border
/// is not highlighted; there is nothing to tell it from.
pub(in crate::mesh_render) fn active_plate_mesh(
    build_volume: BuildVolume,
    count: usize,
    active: usize,
    locks: &[bool],
) -> PlateMesh {
    let locked = locks.get(active).copied().unwrap_or(false);
    let mut g = plate_mesh_for(build_volume, active + 1, locked, true);
    let offset = sc_scene::plate_grid_offset(
        active,
        count.max(1),
        build_volume.width_mm(),
        build_volume.depth_mm(),
    );
    g.place(active, offset);
    g
}

/// All `count` plates on the plate grid in one geometry, `active` with a
/// highlighted border when there is more than one.
pub(in crate::mesh_render) fn all_plates_mesh(
    build_volume: BuildVolume,
    count: usize,
    active: usize,
    locks: &[bool],
) -> PlateMesh {
    let locked = |i: usize| locks.get(i).copied().unwrap_or(false);
    let highlight = |g: &mut PlateMesh| {
        for v in &mut g.plate_corners {
            if v.shade_kind == SHADE_PLATE_TOP_BORDER {
                v.shade_kind = SHADE_ACTIVE_BORDER;
            }
        }
    };
    let mut merged = plate_mesh_for(build_volume, 1, locked(0), active == 0);
    if count <= 1 {
        return merged;
    }
    if active == 0 {
        highlight(&mut merged);
    }
    for i in 1..count {
        let mut plate = plate_mesh_for(build_volume, i + 1, locked(i), i == active);
        if i == active {
            highlight(&mut plate);
        }
        plate.place(
            i,
            sc_scene::plate_grid_offset(i, count, build_volume.width_mm(), build_volume.depth_mm()),
        );
        let base = merged.next();
        let shifted = |indices: Vec<u32>| indices.into_iter().map(move |v| v + base);
        merged.plate_corners.extend(plate.plate_corners);
        merged.top_list.extend(shifted(plate.top_list));
        merged.side_list.extend(shifted(plate.side_list));
        merged.under_list.extend(shifted(plate.under_list));
        // Only the origin axes: the renderer skips plate 1's 12 volume edges
        // (the first 24 indices), and later plates must not add theirs
        // (R55: the box lines came back once a second plate existed).
        merged
            .edge_list
            .extend(shifted(plate.edge_list).skip(24));
        merged.wall_list.extend(shifted(plate.wall_list));
    }
    merged
}

#[cfg(test)]
#[path = "tests_plate.rs"]
mod tests;
