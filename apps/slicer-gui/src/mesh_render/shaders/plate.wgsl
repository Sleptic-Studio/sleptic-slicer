// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

// Everything drawn for the build plates: the plate slab with its grid, the
// build volume's edges, the origin axes, the tool tiles beside each plate
// with their icons, and the five warning walls that show red stripes where
// a part leaves the build volume. Positions are world mm; each vertex
// carries a kind (low 5 bits, the SHADE_* numbers in pipelines/plate.rs)
// and its plate's index (the bits above).

// The mesh pass's group-0 buffer, up to the fields used here.
struct Scene {
    clip_from_world: mat4x4<f32>,
    // w: Preview's cut height (huge outside Preview).
    light: vec4<f32>,
    volume_lo: vec4<f32>,
    volume_hi: vec4<f32>,
    // x: the wall bits of a single plate: 0 -X, 1 +X, 2 -Y, 3 +Y, 4 top,
    // 5 below the plate.
    walls: vec4<u32>,
    // z: 1 in the light theme.
    extra: vec4<f32>,
    // The same six bits for each of up to 40 plates: plate p in word p / 5,
    // starting at bit (p % 5) * 6.
    plate_walls: array<vec4<u32>, 2>,
};

@group(0) @binding(0) var<uniform> scene: Scene;

const FLOOR: u32 = 0u;
const RIM: u32 = 2u;
const SIDE: u32 = 3u;
const UNDERSIDE: u32 = 4u;
const WALL_FIRST: u32 = 5u;
const WALL_TOP: u32 = 9u;
const AXIS_X: u32 = 10u;
const AXIS_Y: u32 = 11u;
const AXIS_Z: u32 = 12u;
const ACTIVE_RIM: u32 = 13u;
const TILE: u32 = 14u;
const ICON: u32 = 15u;
const ICON_DANGER: u32 = 16u;
const ICON_DIM: u32 = 17u;
const ICON_ACCENT: u32 = 18u;
const TILE_DANGER: u32 = 19u;
const EDGE_LINE: u32 = 20u;
const LABEL: u32 = 21u;

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) role: u32,
    @location(2) @interpolate(flat) plate: u32,
};

@vertex
fn vertex_stage(@location(0) at: vec3<f32>, @location(1) tagged: u32) -> Varyings {
    var v: Varyings;
    v.role = tagged & 31u;
    v.plate = tagged >> 5u;
    v.world = at;
    v.position = scene.clip_from_world * vec4<f32>(at, 1.0);
    // Pull the origin axes a hair towards the camera so the build volume
    // edge they lie on does not hide them.
    if (v.role >= AXIS_X && v.role <= AXIS_Z) {
        v.position.z -= v.position.w * 0.000002;
    }
    return v;
}

fn wall_bits(plate: u32) -> u32 {
    if (plate >= 40u) {
        return scene.walls.x;
    }
    let word = plate / 5u;
    return (scene.plate_walls[word / 4u][word % 4u] >> ((plate % 5u) * 6u)) & 63u;
}

fn in_preview() -> bool {
    return scene.light.w < 1.0e30;
}

// Plate colours. R113: one charcoal plate in both themes, a thin light
// line round the printable area, slate tiles with white icons.
fn pick(dark: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    return select(dark, light, scene.extra.z > 0.5);
}
const BED_DARK: vec3<f32> = vec3<f32>(0.235, 0.242, 0.258);
const BED_LIGHT: vec3<f32> = vec3<f32>(0.255, 0.262, 0.278);
const LINE_DARK: vec3<f32> = vec3<f32>(0.31, 0.32, 0.34);
const LINE_LIGHT: vec3<f32> = vec3<f32>(0.33, 0.34, 0.36);
const BOLD_LINE_DARK: vec3<f32> = vec3<f32>(0.40, 0.41, 0.44);
const BOLD_LINE_LIGHT: vec3<f32> = vec3<f32>(0.42, 0.43, 0.46);
const SIDE_DARK: vec3<f32> = vec3<f32>(0.17, 0.176, 0.19);
const SIDE_LIGHT: vec3<f32> = vec3<f32>(0.19, 0.196, 0.21);
const VOLUME_EDGE_DARK: vec3<f32> = vec3<f32>(0.42, 0.44, 0.47);
const VOLUME_EDGE_LIGHT: vec3<f32> = vec3<f32>(0.52, 0.55, 0.60);
const AREA_LINE: vec3<f32> = vec3<f32>(0.66, 0.68, 0.72);
const LABEL_GREY: vec3<f32> = vec3<f32>(0.62, 0.64, 0.68);
const DELETE_RED: vec3<f32> = vec3<f32>(0.58, 0.09, 0.10);
const ICON_WHITE: vec3<f32> = vec3<f32>(0.95, 0.96, 0.97);
const ACCENT_BLUE: vec3<f32> = vec3<f32>(0.45, 0.70, 1.0);
const AXIS_BLUE: vec3<f32> = vec3<f32>(0.294, 0.573, 0.933);
// Seen from below, the underside lets the scene show through.
const UNDERSIDE_ALPHA: f32 = 0.45;

// Grid: a line every 10 mm and a bolder one every 50 mm (R113), with
// widths fixed in mm so zooming never changes them (R115).
const FINE_PITCH_MM: f32 = 10.0;
const BOLD_PITCH_MM: f32 = 50.0;
const FINE_WIDTH_MM: f32 = 0.3;
const BOLD_WIDTH_MM: f32 = 0.6;

// How much of this point a grid of lines `pitch` apart and `width` wide
// covers, anti-aliased over one pixel. A line thinner than a pixel is drawn
// a pixel wide but only as strong as the part of it that is there.
fn line_coverage(xy: vec2<f32>, pitch: f32, width: f32) -> f32 {
    let folded = abs(fract(xy / pitch) - 0.5) - 0.5;
    let distance_mm = min(abs(folded.x), abs(folded.y)) * pitch;
    let pixel = max(max(fwidth(xy.x), fwidth(xy.y)), 1e-4);
    let drawn = max(width, pixel);
    let edge = smoothstep(0.5 * (drawn - pixel), 0.5 * (drawn + pixel), distance_mm);
    return (1.0 - edge) * (width / drawn);
}

fn bed(xy: vec2<f32>) -> vec3<f32> {
    let fine = line_coverage(xy, FINE_PITCH_MM, FINE_WIDTH_MM);
    let bold = line_coverage(xy, BOLD_PITCH_MM, BOLD_WIDTH_MM);
    let with_fine = mix(pick(BED_DARK, BED_LIGHT), pick(LINE_DARK, LINE_LIGHT), fine);
    return mix(with_fine, pick(BOLD_LINE_DARK, BOLD_LINE_LIGHT), bold);
}

// Out-of-volume warnings: red diagonal stripes, 12 mm apart, half of each
// period red, translucent on the walls so the offending part shows through.
const WARNING_RED: vec3<f32> = vec3<f32>(1.0, 0.15, 0.15);
const STRIPE_PITCH_MM: f32 = 12.0;
const STRIPE_RED_SHARE: f32 = 0.50;
const WALL_ALPHA: f32 = 0.55;

fn on_stripe(along: f32) -> bool {
    return fract(along / STRIPE_PITCH_MM) < STRIPE_RED_SHARE;
}

fn opaque(c: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(c, 1.0);
}

fn floor_fragment(v: Varyings) -> vec4<f32> {
    let grid = bed(v.world.xy);
    // Bit 5: some part reaches below the plate.
    if ((wall_bits(v.plate) & 32u) == 0u) {
        return opaque(grid);
    }
    if (in_preview()) {
        // Preview keeps only a thin red border round the build area.
        let inset = min(v.world.xy - scene.volume_lo.xy, scene.volume_hi.xy - v.world.xy);
        return opaque(select(grid, WARNING_RED, min(inset.x, inset.y) < 0.35));
    }
    return opaque(select(grid, WARNING_RED, on_stripe(v.world.x + v.world.y)));
}

fn wall_fragment(v: Varyings) -> vec4<f32> {
    let wall = v.role - WALL_FIRST;
    if (((wall_bits(v.plate) >> wall) & 1u) == 0u) {
        discard;
    }
    // The two axes lying in this wall: Y and Z for the X walls, X and Z for
    // the Y walls, X and Y for the top.
    var q = v.world.yz;
    var lo = scene.volume_lo.yz;
    var hi = scene.volume_hi.yz;
    if (wall == 2u || wall == 3u) {
        q = v.world.xz;
        lo = scene.volume_lo.xz;
        hi = scene.volume_hi.xz;
    } else if (wall == 4u) {
        q = v.world.xy;
        lo = scene.volume_lo.xy;
        hi = scene.volume_hi.xy;
    }
    if (in_preview()) {
        // Only the breached wall's outline, kept thin on screen.
        let inset = min(q - lo, hi - q);
        if (all(inset > max(fwidth(q) * 1.5, vec2<f32>(0.08)))) {
            discard;
        }
        return opaque(WARNING_RED);
    }
    if (!on_stripe(q.x + q.y)) {
        discard;
    }
    return vec4<f32>(WARNING_RED, WALL_ALPHA);
}

@fragment
fn fragment_stage(v: Varyings) -> @location(0) vec4<f32> {
    let k = v.role;
    if (k == FLOOR) {
        return floor_fragment(v);
    }
    if (k >= WALL_FIRST && k <= WALL_TOP) {
        return wall_fragment(v);
    }
    // The tiles and their icons belong to Prepare's plate tools.
    if (k >= TILE && k <= TILE_DANGER && in_preview()) {
        discard;
    }
    switch k {
        case AXIS_X: { return vec4<f32>(0.894, 0.322, 0.322, 1.0); }
        case AXIS_Y: { return vec4<f32>(0.329, 0.722, 0.416, 1.0); }
        case AXIS_Z, ACTIVE_RIM: { return opaque(AXIS_BLUE); }
        case EDGE_LINE: { return opaque(AREA_LINE); }
        case LABEL: { return opaque(LABEL_GREY); }
        case TILE: { return opaque(pick(BED_DARK, BED_LIGHT)); }
        case TILE_DANGER: { return opaque(DELETE_RED); }
        case ICON: { return opaque(ICON_WHITE); }
        case ICON_DANGER: { return opaque(vec3<f32>(1.0)); }
        case ICON_ACCENT: { return opaque(ACCENT_BLUE); }
        case ICON_DIM: { return opaque(mix(pick(BED_DARK, BED_LIGHT), ICON_WHITE, 0.35)); }
        case RIM: { return opaque(pick(BED_DARK, BED_LIGHT)); }
        case SIDE: { return opaque(pick(SIDE_DARK, SIDE_LIGHT)); }
        case UNDERSIDE: { return vec4<f32>(pick(SIDE_DARK, SIDE_LIGHT), UNDERSIDE_ALPHA); }
        default: { return opaque(pick(VOLUME_EDGE_DARK, VOLUME_EDGE_LIGHT)); }
    }
}
