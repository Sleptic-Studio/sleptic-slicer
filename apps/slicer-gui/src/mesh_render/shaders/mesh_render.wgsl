// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

// Models and supports. Field for field this is `Uniforms` in frame.rs; the
// plate and cap pipelines read the same buffer.
struct Scene {
    clip_from_world: mat4x4<f32>,
    // xyz: towards the light. w: Preview's cut height (huge when not cut).
    light: vec4<f32>,
    // The build volume in mm. volume_lo.w: one layer's thickness.
    volume_lo: vec4<f32>,
    volume_hi: vec4<f32>,
    // x: wall warning bits (plate.wgsl). yzw: the selected support colour,
    // 0..255.
    walls: vec4<u32>,
    // xy: cap shift, z: 1 in the light theme, w: cure shade.
    extra: vec4<f32>,
    plate_walls: array<vec4<u32>, 2>,
    // rgb: the resin's colour, which models are drawn in (R121).
    resin: vec4<f32>,
};

// One part, bound with a dynamic offset before its draw.
struct Part {
    world_from_model: mat4x4<f32>,
    // x: selected. y: some vertex is outside the build volume.
    // z: 0 model, 1 support, 2 the selected support.
    // w: bit 0 seam, bits 1..15 plate column, bits 16.. plate row.
    state: vec4<u32>,
    // Support paint: xyz centre (model space), w radius; w > 0 asks for
    // supports, w < 0 forbids them, w = 0 ends the list.
    brushes: array<vec4<f32>, 256>,
};

@group(0) @binding(0) var<uniform> scene: Scene;
@group(1) @binding(0) var<uniform> part: Part;

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    // Flat: one value for the whole part, never blended between parts.
    @location(1) @interpolate(flat) state: vec4<u32>,
    // Centroid: an MSAA edge sample outside the triangle must not
    // extrapolate a part resting on the plate to below it, which painted red
    // specks along its bottom edge (R58).
    @location(2) @interpolate(perspective, centroid) on_plate: vec3<f32>,
    @location(3) model: vec3<f32>,
    @location(4) normal: vec3<f32>,
};

// sc_scene::PLATE_GAP_MM: the space between plates laid out in a grid.
const PLATE_GAP_MM: f32 = 40.0;

@vertex
fn vertex_stage(@location(0) corner: vec3<f32>, @location(1) normal: vec3<f32>) -> Varyings {
    let m = part.world_from_model;
    let world = m * vec4<f32>(corner, 1.0);
    var v: Varyings;
    v.position = scene.clip_from_world * world;
    v.world = world.xyz;
    v.model = corner;
    // The cofactor matrix keeps normals square to the surface under uneven
    // scaling; a mirror flips their sign, which the lighting ignores.
    let a = m[0].xyz;
    let b = m[1].xyz;
    let c = m[2].xyz;
    v.normal = mat3x3<f32>(cross(b, c), cross(c, a), cross(a, b)) * normal;
    v.state = part.state;
    // Position relative to the part's own plate, whose grid cell is packed
    // in state.w (rows go towards -Y).
    let cell = vec2<f32>(f32((part.state.w >> 1u) & 0x7fffu), -f32(part.state.w >> 16u));
    let pitch = scene.volume_hi.xy - scene.volume_lo.xy + vec2<f32>(PLATE_GAP_MM);
    v.on_plate = world.xyz - vec3<f32>(cell * pitch, 0.0);
    return v;
}

// Out of the build volume: a pink wash over the whole part, and solid red
// where the surface is actually outside.
const OUTSIDE_WASH: vec3<f32> = vec3<f32>(0.95, 0.55, 0.55);
const OUTSIDE_RED: vec3<f32> = vec3<f32>(1.00, 0.15, 0.15);
const WASH_AMOUNT: f32 = 0.55;
// BuildVolume's CONTAINS_TOLERANCE_MM, so rounding never reddens a part
// standing on the plate.
const TOLERANCE_MM: f32 = 0.001;

// The cured layer's violet, as in cap.wgsl.
const UV_BRIGHT: vec3<f32> = vec3<f32>(0.635, 0.349, 1.0);
const UV_DARK: vec3<f32> = vec3<f32>(0.294, 0.0, 0.510);

const SUPPORT_WHITE: vec3<f32> = vec3<f32>(0.93, 0.94, 0.95);
const SELECTION_TINT: vec3<f32> = vec3<f32>(0.26, 0.53, 0.95);
const SEAM_GREY: vec3<f32> = vec3<f32>(0.22, 0.24, 0.28);
const PAINT_KEEP: vec3<f32> = vec3<f32>(0.33, 0.72, 0.42);

// The colour before lighting, warnings and Preview.
fn surface_colour(v: Varyings) -> vec3<f32> {
    let support = v.state.z;
    var own = scene.resin.rgb;
    if (support == 2u) {
        own = vec3<f32>(scene.walls.yzw) / 255.0;
    } else if (support != 0u) {
        own = SUPPORT_WHITE;
    }
    // R113: selection tints towards blue (whitening light parts confused).
    var colour = mix(own, mix(own, SELECTION_TINT, 0.42), f32(v.state.x));
    if ((v.state.w & 1u) != 0u) {
        colour = SEAM_GREY;
    }
    var asked = false;
    var forbidden = false;
    for (var k = 0u; k < 256u; k++) {
        let brush = part.brushes[k];
        if (brush.w == 0.0) {
            break;
        }
        if (distance(v.model, brush.xyz) <= abs(brush.w)) {
            asked = asked || brush.w > 0.0;
            forbidden = forbidden || brush.w < 0.0;
        }
    }
    if (asked) {
        colour = PAINT_KEEP;
    }
    if (forbidden) {
        colour = mix(own, vec3<f32>(0.0), 0.55);
    }
    return colour;
}

@fragment
fn fragment_stage(v: Varyings) -> @location(0) vec4<f32> {
    // Preview cuts everything above the chosen layer.
    let cut = scene.light.w;
    if (v.world.z > cut) {
        discard;
    }
    // Smooth vertex normals (R90); the screen-space facet normal is only a
    // fallback for a degenerate one.
    var n = normalize(cross(dpdx(v.world), dpdy(v.world)));
    if (dot(v.normal, v.normal) > 1e-12) {
        n = normalize(v.normal);
    }
    // Two-sided light, so winding never matters. The light theme lifts the
    // ambient so models are not heavy dark shapes on the bed.
    var ambient = 0.25;
    if (scene.extra.z > 0.5) {
        ambient = 0.45;
    }
    let shade = ambient + (1.0 - ambient) * abs(dot(n, normalize(scene.light.xyz)));

    let p = v.on_plate;
    let outside = any(p < scene.volume_lo.xyz - vec3<f32>(TOLERANCE_MM))
        || any(p > scene.volume_hi.xyz + vec3<f32>(TOLERANCE_MM));
    var colour = mix(surface_colour(v), OUTSIDE_WASH, f32(v.state.y) * WASH_AMOUNT);
    if (outside) {
        colour = OUTSIDE_RED;
    }

    // Preview: the slab one layer thick under the cut is the layer curing,
    // lit violet on its walls as well as its face; just below it the walls
    // pick up a faint violet bleed, like the cap's glow.
    let layer = scene.volume_lo.w;
    if (cut < 1.0e30 && layer > 0.0 && !outside) {
        let uv = mix(UV_BRIGHT, UV_DARK, clamp(scene.extra.w, 0.0, 1.0));
        let below = cut - v.world.z;
        // A micron of margin: a flat face lying on the layer's bottom
        // flickered in and out on interpolation noise.
        if (below < layer - 0.001) {
            return vec4<f32>(uv, 1.0);
        }
        let steepness = 1.0 - abs(n.z);
        let bleed = 0.35 * steepness * exp(-max(below - layer, 0.0) / max(layer, 0.02));
        return vec4<f32>(mix(colour * shade, uv, bleed), 1.0);
    }
    return vec4<f32>(colour * shade, 1.0);
}
