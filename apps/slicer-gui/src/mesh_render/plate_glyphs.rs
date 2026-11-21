// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The plate tool tiles' icons and number as flat triangles on the tiles
//! themselves (R110). They were 2D paths drawn over the 3D image at the
//! tiles' projections, hidden whenever a tile came out small on screen or a
//! model stood in front of it, so they blinked out at shallow angles. As part
//! of the plate mesh they tilt, shrink and hide behind models exactly as the
//! plate does.
//!
//! Strokes (the icons) are turned into outlines by the SVG renderer's own
//! stroker (round caps and joins, as drawn in the toolbar), curves are
//! flattened, and every outline is filled by an exact trapezoid decomposition
//! under the nonzero rule (slabs between consecutive vertex heights, so each
//! crossing edge spans its slab), which handles holes and self-overlap.

use glam::Vec2;

use crate::surface_text::{Segment, TextOutline};

/// Triangles filling `outline` once mapped into the plate plane by `map`
/// (an affine map), stroked `stroke` units wide first when given.
pub(in crate::mesh_render) fn glyph_triangles(
    outline: &TextOutline,
    stroke: Option<f32>,
    map: impl Fn(Vec2) -> Vec2,
) -> Vec<[Vec2; 3]> {
    use usvg::tiny_skia_path as tsp;
    let mut builder = tsp::PathBuilder::new();
    for segment in &outline.segments {
        match *segment {
            Segment::Move(p) => builder.move_to(p.x, p.y),
            Segment::Line(p) => builder.line_to(p.x, p.y),
            Segment::Quad(a, p) => builder.quad_to(a.x, a.y, p.x, p.y),
            Segment::Cubic(a, b, p) => builder.cubic_to(a.x, a.y, b.x, b.y, p.x, p.y),
            Segment::Close => builder.close(),
        }
    }
    let Some(mut path) = builder.finish() else {
        return Vec::new();
    };
    if let Some(width) = stroke {
        let stroke = tsp::Stroke {
            width,
            line_cap: tsp::LineCap::Round,
            line_join: tsp::LineJoin::Round,
            ..Default::default()
        };
        match path.stroke(&stroke, 1.0) {
            Some(outline) => path = outline,
            None => return Vec::new(),
        }
    }
    // Flatten to closed polylines in plate coordinates.
    let mut rings: Vec<Vec<Vec2>> = Vec::new();
    let mut ring: Vec<Vec2> = Vec::new();
    let mut last = Vec2::ZERO;
    let pt = |p: tsp::Point| Vec2::new(p.x, p.y);
    for segment in path.segments() {
        match segment {
            tsp::PathSegment::MoveTo(p) => {
                if ring.len() > 2 {
                    rings.push(std::mem::take(&mut ring));
                }
                ring.clear();
                last = pt(p);
                ring.push(map(last));
            }
            tsp::PathSegment::LineTo(p) => {
                last = pt(p);
                ring.push(map(last));
            }
            tsp::PathSegment::QuadTo(a, p) => {
                let (a, p) = (pt(a), pt(p));
                for k in 1..=4 {
                    let t = k as f32 / 4.0;
                    let u = 1.0 - t;
                    ring.push(map(last * u * u + a * 2.0 * u * t + p * t * t));
                }
                last = p;
            }
            tsp::PathSegment::CubicTo(a, b, p) => {
                let (a, b, p) = (pt(a), pt(b), pt(p));
                for k in 1..=6 {
                    let t = k as f32 / 6.0;
                    let u = 1.0 - t;
                    ring.push(map(
                        last * u * u * u + a * 3.0 * u * u * t + b * 3.0 * u * t * t + p * t * t * t,
                    ));
                }
                last = p;
            }
            tsp::PathSegment::Close => {
                if ring.len() > 2 {
                    rings.push(std::mem::take(&mut ring));
                }
                ring.clear();
            }
        }
    }
    if ring.len() > 2 {
        rings.push(ring);
    }
    fill_nonzero(&rings)
}

/// Triangles covering the nonzero-winding interior of closed `rings`.
pub(in crate::mesh_render) fn fill_nonzero(rings: &[Vec<Vec2>]) -> Vec<[Vec2; 3]> {
    // Every edge, oriented by its winding direction.
    let mut edges: Vec<(Vec2, Vec2, i32)> = Vec::new();
    let mut heights: Vec<f32> = Vec::new();
    for ring in rings {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            heights.push(a.y);
            if (a.y - b.y).abs() > 1e-7 {
                edges.push(if a.y < b.y { (a, b, 1) } else { (b, a, -1) });
            }
        }
    }
    heights.sort_by(f32::total_cmp);
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    let mut out = Vec::new();
    let x_at = |e: &(Vec2, Vec2, i32), y: f32| e.0.x + (e.1.x - e.0.x) * (y - e.0.y) / (e.1.y - e.0.y);
    for slab in heights.windows(2) {
        let (y0, y1) = (slab[0], slab[1]);
        let mid = 0.5 * (y0 + y1);
        // Edges spanning this slab (none ends inside it: slabs break at
        // every vertex height), left to right at its middle.
        let mut crossing: Vec<&(Vec2, Vec2, i32)> =
            edges.iter().filter(|e| e.0.y <= mid && e.1.y >= mid).collect();
        crossing.sort_by(|a, b| x_at(a, mid).total_cmp(&x_at(b, mid)));
        let mut winding = 0;
        for pair in crossing.windows(2) {
            winding += pair[0].2;
            if winding != 0 {
                let (l, r) = (pair[0], pair[1]);
                let (a, b) = (Vec2::new(x_at(l, y0), y0), Vec2::new(x_at(r, y0), y0));
                let (c, d) = (Vec2::new(x_at(r, y1), y1), Vec2::new(x_at(l, y1), y1));
                out.push([a, b, c]);
                out.push([a, c, d]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(triangles: &[[Vec2; 3]]) -> f32 {
        triangles
            .iter()
            .map(|[a, b, c]| 0.5 * ((*b - *a).perp_dot(*c - *a)).abs())
            .sum()
    }

    /// A square with a square hole, either winding: exactly its area.
    #[test]
    fn a_ring_with_a_hole_fills_exactly() {
        let outer = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let hole = vec![Vec2::new(3.0, 3.0), Vec2::new(3.0, 7.0), Vec2::new(7.0, 7.0), Vec2::new(7.0, 3.0)];
        assert!((area(&fill_nonzero(&[outer.clone(), hole])) - 84.0).abs() < 1e-3);
        // Two overlapping squares the same way round: their union.
        let shifted: Vec<Vec2> = outer.iter().map(|p| *p + Vec2::new(5.0, 0.0)).collect();
        assert!((area(&fill_nonzero(&[outer, shifted])) - 150.0).abs() < 1e-3);
    }

    /// Every plate icon becomes triangles inside its 24-unit box, and a
    /// stroked line is about as large as its length times its width.
    #[test]
    fn plate_icons_fill_their_strokes() {
        for slot in 1..=6 {
            let icon = crate::surface_text::plate_icon(slot);
            let tris = glyph_triangles(icon, Some(1.6), |p| p);
            assert!(!tris.is_empty(), "slot {slot}");
            for t in &tris {
                for p in t {
                    assert!(p.x > -1.5 && p.x < 25.5 && p.y > -1.5 && p.y < 25.5, "slot {slot}: {p}");
                }
            }
        }
        let line = TextOutline {
            segments: vec![Segment::Move(Vec2::new(2.0, 12.0)), Segment::Line(Vec2::new(22.0, 12.0))],
            width: 24.0,
        };
        let a = area(&glyph_triangles(&line, Some(2.0), |p| p));
        // 20 x 2 plus two round caps of radius 1.
        assert!((a - (40.0 + std::f32::consts::PI)).abs() < 0.3, "{a}");
    }

    /// Plate numbers, holes and all ("0", "8"), fill without overlap: the
    /// area is the glyph's, not more.
    #[test]
    fn plate_numbers_fill_with_their_holes() {
        let one = area(&glyph_triangles(&crate::surface_text::outline("1"), None, |p| p));
        let eight = area(&glyph_triangles(&crate::surface_text::outline("8"), None, |p| p));
        assert!(one > 0.02 && eight > one, "{one} {eight}");
        // An "8" covers well under its box: its counters are holes.
        let box_area = crate::surface_text::outline("8").width;
        assert!(eight < box_area * 0.8, "{eight} of {box_area}");
    }
}
