// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

// The cut face in Preview. Above the chosen layer the models are clipped
// away (mesh_render.wgsl); this draws one flat rectangle over the build
// area at the cut height, textured with that layer's coverage, so the cut
// looks like the printer's UV light shining through the LCD.
//
// It reads the mesh pass's group-0 buffer, so `Scene` matches the start of
// that struct: the cut height is `light.w`, the build area is
// `volume_lo..volume_hi`, `extra.xy` moves the face onto the active plate
// and `extra.w` is the cure shade.

struct Scene {
    clip_from_world: mat4x4<f32>,
    light: vec4<f32>,
    volume_lo: vec4<f32>,
    volume_hi: vec4<f32>,
    walls: vec4<u32>,
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> scene: Scene;
@group(1) @binding(0) var layer: texture_2d<f32>;
@group(1) @binding(1) var layer_sampler: sampler;

struct Varyings {
    @builtin(position) position: vec4<f32>,
    // 0..1 across the build area, starting at its minimum X and Y corner,
    // like the coverage image's rows.
    @location(0) across: vec2<f32>,
};

@vertex
fn vertex_stage(@builtin(vertex_index) i: u32) -> Varyings {
    // Six vertices, two triangles: (0,0) (1,0) (1,1) and (0,0) (1,1) (0,1).
    let across = vec2<f32>(f32(i == 1u || i == 2u || i == 4u), f32(i == 2u || i == 4u || i == 5u));
    let xy = mix(scene.volume_lo.xy, scene.volume_hi.xy, across) + scene.extra.xy;
    var v: Varyings;
    v.position = scene.clip_from_world * vec4<f32>(xy, scene.light.w, 1.0);
    v.across = across;
    return v;
}

// 405 nm UV violet: #a259ff for a normal exposure, darkening to #4b0082 the
// longer the layer cures (the shade in `extra.w`, as in the 2D view).
const UV_BRIGHT: vec3<f32> = vec3<f32>(0.635, 0.349, 1.0);
const UV_DARK: vec3<f32> = vec3<f32>(0.294, 0.0, 0.510);

fn coverage(at: vec2<f32>) -> f32 {
    return textureSampleLevel(layer, layer_sampler, at, 0.0).r;
}

// Average coverage on a circle of `radius` mm round `at`, from 16 points.
fn circle_average(at: vec2<f32>, radius: f32, per_mm: vec2<f32>) -> f32 {
    let step = 6.2831853 / 16.0;
    var total = 0.0;
    for (var k = 0; k < 16; k++) {
        let angle = f32(k) * step;
        total += coverage(at + vec2<f32>(cos(angle), sin(angle)) * radius * per_mm);
    }
    return total / 16.0;
}

@fragment
fn fragment_stage(v: Varyings) -> @location(0) vec4<f32> {
    // The coverage is a distance field that crosses 0.5 on the outline;
    // smoothing over one pixel's change keeps the outline unstepped.
    let c = coverage(v.across);
    let pixel = max(fwidth(c), 1e-4);
    let inside = smoothstep(0.5 - pixel, 0.5 + pixel, c);
    let colour = mix(UV_BRIGHT, UV_DARK, clamp(scene.extra.w, 0.0, 1.0));
    // Past the outline the light spreads onto the plate as a violet glow
    // fading over about 3 mm: five circles, nearer ones weighing more.
    var glow = 0.0;
    if (inside < 0.999) {
        let per_mm = 1.0 / max(scene.volume_hi.xy - scene.volume_lo.xy, vec2<f32>(1.0));
        let radii = array<f32, 5>(0.4, 0.9, 1.5, 2.2, 3.0);
        let weights = array<f32, 5>(0.30, 0.25, 0.20, 0.15, 0.10);
        for (var r = 0; r < 5; r++) {
            glow += weights[r] * circle_average(v.across, radii[r], per_mm);
        }
    }
    let alpha = max(inside, clamp(glow * 0.8, 0.0, 0.65));
    if (alpha < 0.005) {
        discard;
    }
    return vec4<f32>(colour, alpha);
}
