// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Images of one sliced layer for the views: the cut face of the 3D view
//! (coverage, then a distance texture), the 2D layer outline, and the
//! island, suction and risk overlays.
//!
//! The cut face only has to look right on screen, so it is drawn at most
//! [`MAX_CAP_SIDE_PX`] texels along the plate's longer side, not at the
//! printer's resolution. Layers are in the printer frame, centred on the
//! plate, which is what [`PixelMap`] maps from.

use sc_print_primitives::{Mirror, PixelResolution, PositiveFiniteMm};
use sc_sl1cer::{
    contour::ContourLayer,
    profile::AaLevel,
    raster::{
        accuracy::{Accuracy, rasterize_accurate},
        pixel_map::PixelMap,
    },
};
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::build_volume::BuildVolume;

/// Texels along the plate's longer side of the cut face: plenty for a view a
/// few hundred pixels across, and cheap enough to redo on every layer change.
const MAX_CAP_SIDE_PX: u32 = 2048;

/// One coverage byte per texel, rows from the plate's minimum Y upwards,
/// `width` texels to a row, as the cut-face texture is uploaded.
pub(crate) struct CutImage {
    pub texels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The cut face as plain coverage, without the print corrections.
#[cfg(test)]
pub(crate) fn build_cap_raster(layer: &ContourLayer, plate: &BuildVolume) -> Option<CutImage> {
    build_cap_accurate(layer, plate, Accuracy::default(), AaLevel::Off, 0)
}

/// `layer` drawn over the whole plate with the print corrections in
/// `accuracy` (for layer number `index`) and anti-aliasing `aa`. Texels are
/// as close to square as whole numbers allow. `None` for a plate without
/// area.
pub(crate) fn build_cap_accurate(
    layer: &ContourLayer,
    plate: &BuildVolume,
    accuracy: Accuracy,
    aa: AaLevel,
    index: usize,
) -> Option<CutImage> {
    let (lo, hi) = (plate.envelope_min_mm(), plate.envelope_max_mm());
    let size = [f64::from(hi.x - lo.x), f64::from(hi.y - lo.y)];
    let texel_mm = size[0].max(size[1]) / f64::from(MAX_CAP_SIDE_PX);
    if !(texel_mm > 0.0) {
        return None;
    }
    let [width, height] = size.map(|mm| ((mm / texel_mm).round() as u32).max(1));
    // No mirroring: that flips what the printer shows, while this face has
    // to line up with the models as they stand on screen.
    let map = PixelMap::new(
        PositiveFiniteMm::new(size[0]).ok()?,
        PositiveFiniteMm::new(size[1]).ok()?,
        PixelResolution::new(width, height).ok()?,
        Mirror::default(),
    );
    let runs = rasterize_accurate(layer, &map, aa, 0.0, accuracy, index);
    let coverage: Vec<u8> = runs
        .runs()
        .iter()
        .flat_map(|run| std::iter::repeat_n(run.value, run.length as usize))
        .collect();
    Some(CutImage {
        texels: coverage,
        width,
        height,
    })
}

/// Half-width of the distance band stored around every edge, in texels.
const EDGE_BAND_TEXELS: f64 = 2.0;

/// The 3D view's cut cap as a signed-distance texture. A plain coverage
/// raster (texels fully on or off) reads as a staircase once the camera
/// zooms in: a 3 mm hole is only ~16 texels wide on a 192 mm plate. Here
/// each texel near an edge stores its distance to the layer's real contour
/// (0.5 = on the edge, larger = inside), so the cap shader's linear filter
/// plus its 0.5 threshold rebuilds the edge between texels: round holes stay
/// round and straight edges straight at any zoom. Texels away from the edges
/// are plain 0 / 255. XY compensation and bottom-layer erosion are applied as
/// a shift of the threshold distance (erosion becomes a round rather than a
/// square kernel, invisible at display scale).
pub(crate) fn build_cap_distance(
    layer: &ContourLayer,
    build_volume: &BuildVolume,
    accuracy: Accuracy,
    index: usize,
) -> Option<CutImage> {
    // Inside/outside per texel centre from the uncompensated outline.
    let mut cap = build_cap_accurate(layer, build_volume, Accuracy::default(), AaLevel::Off, 0)?;
    let env_min = build_volume.envelope_min_mm();
    let env_max = build_volume.envelope_max_mm();
    let (w, h) = (cap.width as usize, cap.height as usize);
    let px_x = f64::from(env_max.x - env_min.x) / w as f64;
    let px_y = f64::from(env_max.y - env_min.y) / h as f64;
    let texel_mm = px_x.max(px_y);
    // Positive grows the solid; erosion shrinks the bottom layers.
    let shift = accuracy.xy_compensation_mm - accuracy.erosion_mm(index);
    let band_mm = EDGE_BAND_TEXELS * texel_mm + shift.abs();
    let mut distance = vec![f64::INFINITY; w * h];
    // Printer frame: XY centred on the plate; row 0 is the minimum-Y edge.
    let (x0, y0) = (f64::from(env_min.x), f64::from(env_min.y));
    let mut edge = |ax: f64, ay: f64, bx: f64, by: f64| {
        let (lo_x, hi_x) = (ax.min(bx) - band_mm, ax.max(bx) + band_mm);
        let (lo_y, hi_y) = (ay.min(by) - band_mm, ay.max(by) + band_mm);
        let c0 = (((lo_x - x0) / px_x - 0.5).floor().max(0.0)) as usize;
        let c1 = (((hi_x - x0) / px_x - 0.5).ceil().max(0.0) as usize).min(w.saturating_sub(1));
        let r0 = (((lo_y - y0) / px_y - 0.5).floor().max(0.0)) as usize;
        let r1 = (((hi_y - y0) / px_y - 0.5).ceil().max(0.0) as usize).min(h.saturating_sub(1));
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        for row in r0..=r1 {
            let py = y0 + (row as f64 + 0.5) * px_y;
            for col in c0..=c1 {
                let px = x0 + (col as f64 + 0.5) * px_x;
                let t = if len2 > 0.0 {
                    (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let d = (px - (ax + t * dx)).hypot(py - (ay + t * dy));
                let slot = &mut distance[row * w + col];
                if d < *slot {
                    *slot = d;
                }
            }
        }
    };
    // Long edges are split so each one's search box stays near the edge.
    let step = 8.0 * texel_mm;
    for polygon in layer.polygons() {
        for ring in std::iter::once(polygon.outer()).chain(polygon.holes()) {
            let vertices = ring.vertices();
            for (i, a) in vertices.iter().enumerate() {
                let b = &vertices[(i + 1) % vertices.len()];
                let pieces = ((b.x - a.x).hypot(b.y - a.y) / step).ceil().max(1.0) as usize;
                for k in 0..pieces {
                    let (s, e) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
                    edge(
                        a.x + (b.x - a.x) * s,
                        a.y + (b.y - a.y) * s,
                        a.x + (b.x - a.x) * e,
                        a.y + (b.y - a.y) * e,
                    );
                }
            }
        }
    }
    for (value, d) in cap.texels.iter_mut().zip(&distance) {
        let signed = if *value >= 128 { *d } else { -*d } + shift;
        *value = if d.is_finite() {
            ((0.5 + signed / (2.0 * EDGE_BAND_TEXELS * texel_mm)).clamp(0.0, 1.0) * 255.0).round()
                as u8
        } else if signed >= 0.0 {
            255
        } else {
            0
        };
    }
    Some(cap)
}

/// Fill for texels of unsupported islands in the slice-view panel: the
/// theme's error red (`ShellTheme.error`), so islands read as a problem.
const SLICE_VIEW_ISLAND_RGB: [u8; 3] = [0xd3, 0x2f, 0x2f];

/// Rescale `(row, x0, x1)` spans from a `from` = `(width, height)` raster
/// to a `to` raster covering the same plate (both bottom-to-top rows). A
/// span keeps at least one texel, so a thin full-resolution island stays
/// visible on the coarser display cap.
pub(crate) fn scale_spans(
    spans: &[(u32, u32, u32)],
    from: (u32, u32),
    to: (u32, u32),
) -> Vec<(u32, u32, u32)> {
    let (fw, fh) = (u64::from(from.0.max(1)), u64::from(from.1.max(1)));
    let (tw, th) = (u64::from(to.0), u64::from(to.1));
    spans
        .iter()
        .map(|&(row, x0, x1)| {
            let r = (u64::from(row) * th / fh).min(th.saturating_sub(1));
            let a = u64::from(x0) * tw / fw;
            let b = (u64::from(x1) * tw).div_ceil(fw).max(a + 1).min(tw);
            (r as u32, a as u32, b as u32)
        })
        .collect()
}

/// SVG path data for `layer`, for the 2D layer view's vector `Path`: every
/// ring (outer contours and holes) as `M ... L ... Z`, in millimetres within
/// a `plate_w` x `plate_h` viewbox whose origin is the plate's top-left corner
/// (the printer frame is centred on the plate, Y up; the viewbox is Y down).
/// Drawing vectors instead of a scaled raster keeps the edges crisp at every
/// zoom: the raster version showed soft, then pixelated, edges.
pub(crate) fn layer_path_commands(layer: &ContourLayer, plate_w: f64, plate_h: f64) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for polygon in layer.polygons() {
        for ring in std::iter::once(polygon.outer()).chain(polygon.holes()) {
            for (i, p) in ring.vertices().iter().enumerate() {
                let x = p.x + plate_w / 2.0;
                let y = plate_h / 2.0 - p.y;
                let _ = write!(out, "{}{x:.3} {y:.3} ", if i == 0 { "M" } else { "L" });
            }
            out.push_str("Z ");
        }
    }
    out
}

/// A transparent image the size of `raster` with only the island texels
/// painted (in [`SLICE_VIEW_ISLAND_RGB`]), top-down; drawn over the vector
/// layer path in the 2D layer view.
pub(crate) fn island_overlay_rgba(
    raster: &CutImage,
    islands: &[(u32, u32, u32)],
) -> SharedPixelBuffer<Rgba8Pixel> {
    let width = raster.width as usize;
    let height = raster.height as usize;
    let [r, g, b] = SLICE_VIEW_ISLAND_RGB;
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(raster.width, raster.height);
    let pixels = buffer.make_mut_slice();
    for &(row, x0, x1) in islands {
        let row = row as usize;
        if row >= height {
            continue;
        }
        // Cap rows are bottom-to-top; the image is top-down.
        let out_row = height - 1 - row;
        for col in (x0 as usize)..(x1 as usize).min(width) {
            pixels[out_row * width + col] = Rgba8Pixel { r, g, b, a: 255 };
        }
    }
    buffer
}

#[cfg(test)]
mod cut_tests {
    use super::*;
    use sc_sl1cer::contour::{ClosedRing, Polygon};
    use sc_sl1cer::geom2d::Point2D;

    /// A square `2 * half` mm across, centred on the plate.
    fn square_layer(half: f64) -> ContourLayer {
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let ring = ClosedRing::new(corners.iter().map(|&(x, y)| Point2D::new(x * half, y * half)).collect());
        ContourLayer::new(0.05, vec![Polygon::new(ring, vec![]).expect("square")])
    }

    #[test]
    fn a_centred_square_covers_the_middle_and_not_the_corner() {
        let cap = build_cap_raster(&square_layer(20.0), &BuildVolume::new(80.0, 80.0, 80.0)).unwrap();
        assert_eq!((cap.width, cap.height), (MAX_CAP_SIDE_PX, MAX_CAP_SIDE_PX));
        assert_eq!(cap.texels.len(), (cap.width * cap.height) as usize);
        let middle = (cap.height / 2 * cap.width + cap.width / 2) as usize;
        assert_eq!((cap.texels[middle], cap.texels[0]), (255, 0));
    }

    #[test]
    fn nothing_on_the_layer_covers_nothing() {
        let empty = ContourLayer::new(0.05, vec![]);
        let cap = build_cap_raster(&empty, &BuildVolume::new(80.0, 80.0, 80.0)).unwrap();
        assert!(cap.texels.iter().all(|&c| c == 0));
    }

    #[test]
    fn scaled_spans_keep_at_least_one_texel() {
        // A 1-pixel sliver on a 10x finer grid still covers one texel.
        assert_eq!(
            scale_spans(&[(55, 33, 34)], (100, 100), (10, 10)),
            vec![(5, 3, 4)]
        );
    }

    #[test]
    fn island_overlay_paints_only_islands_top_down_even_where_the_cap_is_empty() {
        // An island on cap row 0 (bottom), texel 1, of an empty 2x2 cap: it
        // must show (a thin sliver the coarse cap missed is still lit).
        let raster = CutImage {
            texels: vec![0; 4],
            width: 2,
            height: 2,
        };
        let rgba = island_overlay_rgba(&raster, &[(0, 1, 2)]);
        assert_eq!((rgba.width(), rgba.height()), (2, 2));
        let p = rgba.as_slice();
        // Output row 1 (bottom) holds cap row 0.
        assert_eq!((p[3].r, p[3].g, p[3].b, p[3].a), (0xd3, 0x2f, 0x2f, 255));
        assert!(
            p[..3].iter().all(|px| px.a == 0),
            "everything else transparent"
        );
    }

    #[test]
    fn island_overlay_scales_full_resolution_spans_onto_the_cap() {
        let bv = BuildVolume::new(80.0, 80.0, 80.0);
        let square = build_cap_raster(&square_layer(10.0), &bv).expect("cap");
        let (w, h) = (square.width, square.height);
        let islands = scale_spans(&[(h, w / 2, w + 2)], (2 * w, 2 * h), (w, h));
        assert_eq!(islands, vec![(h / 2, w / 4, w / 2 + 1)]);
        let rgba = island_overlay_rgba(&square, &islands);
        let width = w as usize;
        let red = rgba.as_slice()[(h as usize - 1 - h as usize / 2) * width + width / 2];
        assert_eq!((red.r, red.g, red.b, red.a), (0xd3, 0x2f, 0x2f, 255));
    }

    /// A 40 x 30 mm block with a round hole of `hole_r` mm at the centre.
    fn plate_with_hole(hole_r: f64) -> ContourLayer {
        let outer = ClosedRing::new(vec![
            Point2D::new(-20.0, -15.0),
            Point2D::new(20.0, -15.0),
            Point2D::new(20.0, 15.0),
            Point2D::new(-20.0, 15.0),
        ]);
        let hole = ClosedRing::new(
            (0..128)
                .rev()
                .map(|i| {
                    let a = i as f64 * std::f64::consts::TAU / 128.0;
                    Point2D::new(hole_r * a.cos(), hole_r * a.sin())
                })
                .collect(),
        );
        ContourLayer::new(
            0.05,
            vec![Polygon::new(outer, vec![hole]).expect("holed block")],
        )
    }

    /// What the GPU shows: bilinear filtering of the cap texture at plate
    /// point (x, y) mm, as the cap shader samples it (0..1, edge at 0.5).
    fn sample(cap: &CutImage, bv: &BuildVolume, x: f64, y: f64) -> f64 {
        let lo = bv.envelope_min_mm();
        let hi = bv.envelope_max_mm();
        let u = (x - f64::from(lo.x)) / f64::from(hi.x - lo.x) * cap.width as f64 - 0.5;
        let v = (y - f64::from(lo.y)) / f64::from(hi.y - lo.y) * cap.height as f64 - 0.5;
        let (c, r) = (u.floor(), v.floor());
        let (fx, fy) = (u - c, v - r);
        let at = |col: f64, row: f64| {
            let col = col.clamp(0.0, cap.width as f64 - 1.0) as usize;
            let row = row.clamp(0.0, cap.height as f64 - 1.0) as usize;
            f64::from(cap.texels[row * cap.width as usize + col]) / 255.0
        };
        let bottom = at(c, r) * (1.0 - fx) + at(c + 1.0, r) * fx;
        let top = at(c, r + 1.0) * (1.0 - fx) + at(c + 1.0, r + 1.0) * fx;
        bottom * (1.0 - fy) + top * fy
    }

    /// Worst radial error of the displayed hole edge around 360 directions.
    fn hole_edge_error(cap: &CutImage, bv: &BuildVolume, r: f64) -> f64 {
        (0..360)
            .map(|deg| {
                let a = f64::from(deg).to_radians();
                // Bisect along the ray for where the shader's 0.5 cut falls.
                let (mut lo, mut hi) = (r - 1.0, r + 1.0);
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if sample(cap, bv, mid * a.cos(), mid * a.sin()) >= 0.5 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                (0.5 * (lo + hi) - r).abs()
            })
            .fold(0.0, f64::max)
    }

    /// Sleptic's screenshot: a hole in the 3D view's blue cut face drawn as a
    /// visible staircase. On the 192 x 120 mm plate the coverage cap's texels
    /// are 0.19 mm, so a 3 mm hole's edge wandered by ~0.1 mm; the distance
    /// cap keeps it round to a few hundredths of a millimetre.
    #[test]
    fn distance_cap_keeps_a_small_round_hole_round() {
        let bv = BuildVolume::new(192.0, 120.0, 200.0);
        let layer = plate_with_hole(1.5);
        let coverage = build_cap_raster(&layer, &bv).unwrap();
        let distance = build_cap_distance(&layer, &bv, Accuracy::default(), 0).unwrap();
        let stair = hole_edge_error(&coverage, &bv, 1.5);
        let smooth = hole_edge_error(&distance, &bv, 1.5);
        // The coverage staircase scales with the texel (0.19 mm at 1024 px,
        // 0.094 mm at 2048 px); the distance cap stays well under it.
        let texel = 192.0 / f64::from(MAX_CAP_SIDE_PX);
        assert!(stair > 0.3 * texel, "coverage cap is visibly stepped: {stair}");
        assert!(smooth < 0.02 && smooth < 0.5 * stair, "distance cap edge error {smooth} mm");
    }

    /// The distance cap must agree with the printer raster about what is solid
    /// (same orientation and row order) and still apply XY compensation.
    #[test]
    fn distance_cap_matches_coverage_and_applies_xy_compensation() {
        let bv = BuildVolume::new(192.0, 120.0, 200.0);
        let layer = plate_with_hole(1.5);
        let coverage = build_cap_raster(&layer, &bv).unwrap();
        let distance = build_cap_distance(&layer, &bv, Accuracy::default(), 0).unwrap();
        let differ = coverage
            .texels
            .iter()
            .zip(&distance.texels)
            .filter(|(c, d)| (**c >= 128) != (**d >= 128))
            .count();
        assert!(differ <= 8, "{differ} texels disagree");
        // Growing the solid by 0.1 mm shrinks the hole to r = 1.4 mm.
        let grown = Accuracy {
            xy_compensation_mm: 0.1,
            ..Default::default()
        };
        let cap = build_cap_distance(&layer, &bv, grown, 0).unwrap();
        assert!(hole_edge_error(&cap, &bv, 1.4) < 0.02);
    }

    #[test]
    fn layer_path_maps_the_centred_frame_to_a_top_left_viewbox() {
        // A 20 mm square centred on an 80 x 60 mm plate.
        let path = layer_path_commands(&square_layer(10.0), 80.0, 60.0);
        assert!(
            path.starts_with("M30.000 40.000 L50.000 40.000 L50.000 20.000 L30.000 20.000 Z"),
            "{path}"
        );
        assert_eq!(
            layer_path_commands(&ContourLayer::new(0.05, vec![]), 80.0, 60.0),
            ""
        );
    }
}

#[cfg(test)]
mod tests_accuracy_preview {
    use super::*;
    use sc_sl1cer::{
        contour::{ClosedRing, Polygon},
        geom2d::Point2D,
    };
    #[test]
    fn preview_applies_xy_and_bottom_corrections_at_display_resolution() {
        let layer = ContourLayer::new(
            0.05,
            vec![
                Polygon::new(
                    ClosedRing::new(vec![
                        Point2D::new(-5.0, -5.0),
                        Point2D::new(5.0, -5.0),
                        Point2D::new(5.0, 5.0),
                        Point2D::new(-5.0, 5.0),
                    ]),
                    vec![],
                )
                .unwrap(),
            ],
        );
        let volume = BuildVolume::new(20.0, 20.0, 20.0);
        let full = build_cap_raster(&layer, &volume).unwrap();
        let corrected = build_cap_accurate(
            &layer,
            &volume,
            Accuracy {
                bottom_compensation_mm: 0.5,
                xy_compensation_mm: -0.2,
                bottom_layers: 1,
                ..Default::default()
            },
            AaLevel::Off,
            0,
        )
        .unwrap();
        assert!(
            corrected
                .texels
                .iter()
                .map(|&v| u64::from(v))
                .sum::<u64>()
                < full.texels.iter().map(|&v| u64::from(v)).sum::<u64>()
        );
    }
}

/// Selected diagnostic overlay, using the theme's reserved semantic colours.
pub(crate) fn risk_overlay_rgba(
    raster: &CutImage,
    risk: &crate::RiskRow,
    islands: &[(u32, u32, u32)],
) -> SharedPixelBuffer<Rgba8Pixel> {
    if risk.kind == sc_print_job::RiskKind::UnsupportedIsland as i32 {
        return island_overlay_rgba(raster, islands);
    }
    if risk.kind == sc_print_job::RiskKind::ThinWall as i32 {
        let mut out = island_overlay_rgba(raster, islands);
        for p in out.make_mut_slice() {
            if p.a > 0 {
                p.r = 0xe5;
                p.g = 0xa5;
                p.b = 0;
            }
        }
        return out;
    }
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(raster.width, raster.height);
    let pixels = out.make_mut_slice();
    let w = raster.width as usize;
    let h = raster.height as usize;
    let x0 = (risk.x0 * w as f32).floor().max(0.0) as usize;
    let x1 = (risk.x1 * w as f32).ceil().min(w as f32) as usize;
    let y0 = (risk.y0 * h as f32).floor().max(0.0) as usize;
    let y1 = (risk.y1 * h as f32).ceil().min(h as f32) as usize;
    let [r, g, b] = if risk.error {
        SLICE_VIEW_ISLAND_RGB
    } else {
        [0xe5, 0xa5, 0x00]
    };
    let void = risk.kind == sc_print_job::RiskKind::SuctionCup as i32
        || risk.kind == sc_print_job::RiskKind::TrappedCavity as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let lit = raster.texels[(h - 1 - y) * w + x] > 0;
            let border = x == x0 || x + 1 == x1 || y == y0 || y + 1 == y1;
            if (if void { !lit } else { lit }) || (risk.error && border) {
                pixels[y * w + x] = Rgba8Pixel { r, g, b, a: 180 };
            }
        }
    }
    out
}

#[cfg(test)]
mod risk_overlay_tests {
    use super::*;
    #[test]
    fn suction_overlay_is_yellow_only_in_void_and_errors_are_red() {
        let raster = CutImage {
            width: 2,
            height: 2,
            texels: vec![255, 0, 255, 255],
        };
        let mut risk = crate::RiskRow {
            kind: sc_print_job::RiskKind::SuctionCup as i32,
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
            ..Default::default()
        };
        let image = risk_overlay_rgba(&raster, &risk, &[]);
        let pixels = image.as_slice();
        assert_eq!(
            pixels[3],
            Rgba8Pixel {
                r: 0xe5,
                g: 0xa5,
                b: 0,
                a: 180
            }
        );
        assert!(pixels[..3].iter().all(|p| p.a == 0));
        risk.error = true;
        risk.kind = sc_print_job::RiskKind::OutOfBounds as i32;
        let image = risk_overlay_rgba(&raster, &risk, &[]);
        assert_eq!(
            (
                image.as_slice()[0].r,
                image.as_slice()[0].g,
                image.as_slice()[0].b
            ),
            (0xd3, 0x2f, 0x2f)
        );
    }
}

/// How deep the UV violet of a cured layer is (0 = the profile's normal
/// exposure, bright UV violet; 1 = its longest, the bottom layers, indigo),
/// shared by the 2D layer view and the 3D cut face (Sleptic, 2026-10-30).
pub(crate) fn cure_shade(exposure_s: f64, normal_s: f64, longest_s: f64) -> f32 {
    if !(longest_s > normal_s) || !exposure_s.is_finite() {
        return 0.0;
    }
    ((exposure_s - normal_s) / (longest_s - normal_s)).clamp(0.0, 1.0) as f32
}
