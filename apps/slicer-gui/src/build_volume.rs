// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The printer's build volume in millimetres. The plate lies at Z = 0,
//! centred on the origin (X from -W/2 to W/2, Y from -D/2 to D/2), and the
//! volume rises to Z = H.
use glam::Vec3;
use sc_mesh_core::Aabb;

/// Slack for rounding when checking a part fits: one micron, far below a
/// 50 µm pixel, so a part set exactly on the plate or a wall is never
/// flagged because its transform rounded a hair outside.
pub(crate) const CONTAINS_TOLERANCE_MM: f32 = 0.001;

/// Width (X), depth (Y) and height (Z), kept as one vector so the corner
/// maths below works on all three axes at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildVolume {
    size: Vec3,
}

impl BuildVolume {
    /// All sides must be positive (checked in debug builds).
    pub const fn new(width_mm: f32, depth_mm: f32, height_mm: f32) -> Self {
        debug_assert!(width_mm > 0.0 && depth_mm > 0.0 && height_mm > 0.0);
        BuildVolume { size: Vec3::new(width_mm, depth_mm, height_mm) }
    }

    pub fn width_mm(&self) -> f32 {
        self.size.x
    }

    pub fn depth_mm(&self) -> f32 {
        self.size.y
    }

    pub fn height_mm(&self) -> f32 {
        self.size.z
    }

    /// Plate-integrated tool tiles, Orca style: seven squares lying on the
    /// bed (Z = 0) in a column just outside the plate's right edge, from
    /// the back corner towards the front. Slot 0 shows the plate number,
    /// slots 1..=5 the plate actions and slot 6 adds a plate. Sizes follow
    /// Orca's PartPlate (16 mm on a 200 mm deep bed), capped so a tile fits
    /// the gap between plates.
    /// Returns `[min_x, min_y, max_x, max_y]` in plate-local mm.
    pub fn plate_tile_rects(&self) -> [[f32; 4]; 7] {
        const OVERHANG_MM: f32 = 8.0;
        // Orca's 16 mm per 200 mm of depth, but never under 14 mm (small
        // resin beds made them specks), capped by the free gap beside the
        // plate and by the plate's own depth for the seven-tile column.
        let factor = self.size.y / 200.0;
        let gap_left = (3.0 * factor).max(1.5);
        let gap_y = (3.0 * factor).max(1.5);
        let size = (16.0 * factor)
            .max(14.0)
            .min(sc_scene::PLATE_GAP_MM - 2.0 * OVERHANG_MM - 2.0 * gap_left)
            .min((self.size.y + 2.0 * OVERHANG_MM) / 7.0 - gap_y);
        let x0 = 0.5 * self.size.x + OVERHANG_MM + gap_left;
        let top = 0.5 * self.size.y + OVERHANG_MM;
        std::array::from_fn(|i| {
            let max_y = top - i as f32 * (size + gap_y);
            [x0, max_y - size, x0 + size, max_y]
        })
    }

    /// The corner of the volume nearest the origin's front left, on the
    /// plate: (-W/2, -D/2, 0).
    pub fn envelope_min_mm(&self) -> Vec3 {
        self.size * Vec3::new(-0.5, -0.5, 0.0)
    }

    /// The opposite corner, at the top: (W/2, D/2, H).
    pub fn envelope_max_mm(&self) -> Vec3 {
        self.size * Vec3::new(0.5, 0.5, 1.0)
    }

    /// Whether a part's bounding box fits, within the rounding slack; the
    /// volume being a box, the part fits exactly when its box does. An
    /// empty box always fits.
    pub fn holds_box(&self, aabb: Aabb) -> bool {
        let slack = Vec3::splat(CONTAINS_TOLERANCE_MM);
        aabb.is_empty() || {
            let (low, high) = (Vec3::from(aabb.min().coords()), Vec3::from(aabb.max().coords()));
            (self.envelope_min_mm() - slack).cmple(low).all() && high.cmple(self.envelope_max_mm() + slack).all()
        }
    }

    /// The part of a move `delta` a part with box `aabb_pre` may make,
    /// axis by axis: inside, up to the walls; already through a wall, no
    /// further out, but back in up to the far wall; wider than the volume,
    /// not at all on that axis.
    pub fn allowed_move(&self, aabb_pre: Aabb, delta: Vec3) -> Vec3 {
        let (min, max) = (aabb_pre.min().coords(), aabb_pre.max().coords());
        let (low, high) = (self.envelope_min_mm(), self.envelope_max_mm());
        Vec3::from(std::array::from_fn(|i| {
            // Room towards each wall; none on a side already through it.
            let back = if min[i] < low[i] { 0.0 } else { low[i] - min[i] };
            let forward = if max[i] > high[i] { 0.0 } else { high[i] - max[i] };
            delta[i].clamp(back, forward)
        }))
    }

    /// Centre and radius (at least a micron) of the sphere around the
    /// volume and, if given, the scene's box, for framing the camera. An
    /// inverted (empty) scene box leaves the volume alone.
    pub fn framing_sphere(&self, scene_aabb: Option<(Vec3, Vec3)>) -> (Vec3, f32) {
        let (mut min, mut max) = (self.envelope_min_mm(), self.envelope_max_mm());
        if let Some((scene_min, scene_max)) = scene_aabb {
            (min, max) = (scene_min.min(min), scene_max.max(max));
        }
        ((min + max) * 0.5, ((max - min).length() * 0.5).max(1e-3))
    }
}

/// The volume before a printer is chosen: a 192 x 120 mm plate, 200 mm high.
pub const FALLBACK_VOLUME: BuildVolume = BuildVolume::new(192.0, 120.0, 200.0);

#[cfg(test)]
#[path = "tests_build_volume.rs"]
mod tests;

#[cfg(test)]
mod plate_tile_tests {
    use super::BuildVolume;

    #[test]
    fn tiles_sit_beside_the_plate_inside_the_gap_without_overlapping() {
        for volume in [
            BuildVolume::new(143.43, 89.6, 175.0),
            BuildVolume::new(192.0, 120.0, 200.0),
            BuildVolume::new(300.0, 300.0, 300.0),
        ] {
            let tiles = volume.plate_tile_rects();
            let plate_edge = 0.5 * volume.width_mm() + 8.0;
            let next_plate = plate_edge + sc_scene::PLATE_GAP_MM - 16.0;
            for [x0, y0, x1, y1] in tiles {
                assert!(x0 > plate_edge && x1 < next_plate, "{volume:?}: {x0}..{x1}");
                assert!((x1 - x0 - (y1 - y0)).abs() < 1e-4, "square tiles");
                assert!(
                    y0 > -0.5 * volume.depth_mm() - 8.0,
                    "{volume:?}: below the plate"
                );
            }
            for pair in tiles.windows(2) {
                assert!(pair[1][3] < pair[0][1], "tiles overlap: {pair:?}");
            }
        }
    }
}
