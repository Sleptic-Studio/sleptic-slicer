// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

// Move-tool arrows. One arrow mesh, modelled along +X in units of its own
// length, is drawn once per axis: `Arrow.turn` swings it onto that axis and
// `Arrow.colour` paints it. The arrows are interface, so they are not lit.

// Bind group 0, shared by all three draws.
struct Placement {
    clip_from_world: mat4x4<f32>,
    // xyz: the point the arrows start from, in world mm; w: how long an
    // arrow is in world mm, chosen on the CPU so it keeps its size on screen.
    origin_and_length: vec4<f32>,
};

// Bind group 1, one window per arrow (dynamic offset).
struct Arrow {
    turn: mat4x4<f32>,
    colour: vec4<f32>,
};

@group(0) @binding(0) var<uniform> place: Placement;
@group(1) @binding(0) var<uniform> arrow: Arrow;

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) colour: vec4<f32>,
};

@vertex
fn vertex_stage(@location(0) corner: vec3<f32>) -> Varyings {
    let sized = vec4<f32>(corner * place.origin_and_length.w, 1.0);
    let at = (arrow.turn * sized).xyz + place.origin_and_length.xyz;
    var v: Varyings;
    v.position = place.clip_from_world * vec4<f32>(at, 1.0);
    v.colour = arrow.colour;
    return v;
}

@fragment
fn fragment_stage(v: Varyings) -> @location(0) vec4<f32> {
    return v.colour;
}
