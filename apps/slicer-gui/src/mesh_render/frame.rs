// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What each frame sends the shaders: the uniforms, the view-projection
//! matrix, the light and the background colours.
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use slint::wgpu_29::wgpu;

use crate::camera::ViewCamera;

/// The uniforms of `shaders/mesh_render.wgsl`, field for field. Every
/// field is 16 bytes or a multiple, as WGSL aligns them; three-component
/// values are padded to four.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(super) struct FrameUniforms {
    pub(super) clip_from_world: [[f32; 4]; 4],
    /// The light's direction, and in `.w` the slice cut height.
    pub(super) light_dir: [f32; 4],
    /// The build volume's corners in mm, so the shader can mark what
    /// sticks out.
    pub(super) env_min: [f32; 4],
    pub(super) env_max: [f32; 4],
    /// `.x`: which of the volume's walls (four sides and the top, one bit
    /// each) show warning stripes.
    pub(super) face_breach_mask: [u32; 4],
    /// `.xy`: where the active plate is drawn, which is where the slice
    /// cap goes; `.z`: light theme; `.w`: the cap's UV-pink cure shade.
    pub(super) cap_offset: [f32; 4],
    /// Each plate's own wall bits, so a part over one plate's edge lights
    /// only that plate's walls: plate `i`'s six bits are in word `i / 5`
    /// from bit `(i % 5) * 6`, for up to 40 plates.
    pub(super) plate_breach: [[u32; 4]; 2],
    /// `.rgb`: the resin's colour the parts are drawn in (R121), linear.
    pub(super) model_color: [f32; 4],
}

/// Packs each plate's wall bits, in plate order, into
/// [`FrameUniforms::plate_breach`]; plates past 40 are left out.
pub(super) fn pack_plate_breach(masks: &[u32]) -> [[u32; 4]; 2] {
    let mut words = [[0u32; 4]; 2];
    for (plate, mask) in masks.iter().take(40).enumerate() {
        let word = plate / 5;
        words[word / 4][word % 4] |= (mask & 63) << (plate % 5 * 6);
    }
    words
}

/// The light, in world space.
pub(super) const LIGHT_FROM: [f32; 4] = [0.5, 0.7, 1.0, 0.0];

/// The light with the slice cut height in `.w`; the shader discards what
/// is above it. Without a cut, `f32::MAX`, which nothing is above.
pub(super) fn light_and_cut(clip_z: Option<f32>) -> [f32; 4] {
    let [x, y, z, _] = LIGHT_FROM;
    [x, y, z, clip_z.unwrap_or(f32::MAX)]
}

/// The dark theme's background, #1a1b1e: neutral, so parts and handles
/// stand out.
pub(super) const BACKDROP: wgpu::Color = wgpu::Color {
    r: 0.102,
    g: 0.106,
    b: 0.118,
    a: 1.0,
};

/// The light theme's background, as `ShellTheme.viewport`.
pub(super) const CLEAR_COLOR_LIGHT: wgpu::Color = wgpu::Color {
    r: 0.902,
    g: 0.910,
    b: 0.922,
    a: 1.0,
};

/// Near and far planes as fractions of the scene's diagonal, so depth
/// precision holds for tiny and huge scenes alike, close up or far out.
const NEAR_SHARE: f32 = 0.01;
const FAR_SHARE: f32 = 100.0;

/// The frame's view-projection matrix for a scene of these bounds.
pub(super) fn clip_from_world_for(
    camera: &ViewCamera,
    aabb_min: Vec3,
    aabb_max: Vec3,
    width: u32,
    height: u32,
    fov_y: f32,
) -> Mat4 {
    let diagonal = (aabb_max - aabb_min).length().max(1e-3);
    let aspect = width as f32 / height as f32;
    camera.view_to_clip(fov_y, aspect, NEAR_SHARE * diagonal, FAR_SHARE * diagonal)
        * camera.world_to_view()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cut_height_rides_in_the_lights_fourth_lane() {
        assert_eq!(light_and_cut(None), [0.5, 0.7, 1.0, f32::MAX]);
        assert_eq!(light_and_cut(Some(3.5)), [0.5, 0.7, 1.0, 3.5]);
    }

    #[test]
    fn each_plate_gets_six_bits_five_plates_to_a_word() {
        assert_eq!(pack_plate_breach(&[]), [[0; 4]; 2]);
        // Plate 0 bits 0..6, plate 4 bits 24..30, plate 5 the next word;
        // only the low six bits of a mask count.
        let packed = pack_plate_breach(&[0b11, 0, 0, 0, 0xFF, 1]);
        assert_eq!(packed[0], [0b11 | 63 << 24, 1, 0, 0]);
        // Plate 39 is the last: word 7, its fifth slot.
        let mut masks = vec![0; 41];
        masks[39] = 1;
        masks[40] = 1;
        assert_eq!(pack_plate_breach(&masks), [[0; 4], [0, 0, 0, 1 << 24]]);
    }

    #[test]
    fn the_uniforms_keep_the_shaders_layout() {
        // A 4x4 matrix and eight 16-byte rows: 192 bytes.
        assert_eq!(size_of::<FrameUniforms>(), 64 + 8 * 16);
    }
}
