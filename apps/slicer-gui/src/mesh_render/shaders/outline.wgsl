// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

// Selection outline, drawn after the scene. The main pass marks the
// selected parts with stencil value 1. Here every screen pixel compares its
// mark with the marks up to two pixels away; where they disagree the pixel
// lies on the selection's silhouette and is painted Sleptic blue. Elsewhere
// the pixel is discarded and the scene shows through.
//
// Only the first MSAA sample is read: along an edge the samples may differ,
// which moves the outline by at most a pixel.

@group(0) @binding(0) var marks: texture_multisampled_2d<u32>;

const REACH: i32 = 2;
// #2f81f7, the selection colour.
const SELECTION_BLUE: vec4<f32> = vec4<f32>(0.184, 0.506, 0.969, 1.0);

@vertex
fn vertex_stage(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle large enough to cover the whole viewport: corners at
    // (-1, -1), (3, -1) and (-1, 3).
    let x = f32(i == 1u) * 4.0 - 1.0;
    let y = f32(i == 2u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn mark_at(p: vec2<i32>, last: vec2<i32>) -> u32 {
    // Clamp to the texture: a load past the edge reads 0, which would draw
    // an outline along the window border under a selected part.
    let texel = textureLoad(marks, clamp(p, vec2<i32>(0), last), 0);
    // Vulkan and Metal return the stencil in .x. DX12 views the stencil
    // plane of Depth24PlusStencil8 as X24_TYPELESS_G8_UINT, so it arrives
    // in .y and .x reads 0 (measured on an RTX 4070 SUPER, R212); the
    // channel a backend does not use reads 0, so the larger of the two is
    // the mark everywhere.
    return max(texel.x, texel.y);
}

@fragment
fn fragment_stage(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let here = vec2<i32>(pixel.xy);
    let last = vec2<i32>(textureDimensions(marks)) - vec2<i32>(1);
    let own = mark_at(here, last);
    var edge = false;
    for (var y = -REACH; y <= REACH; y++) {
        for (var x = -REACH; x <= REACH; x++) {
            edge = edge || mark_at(here + vec2<i32>(x, y), last) != own;
        }
    }
    if (!edge) {
        discard;
    }
    return SELECTION_BLUE;
}
