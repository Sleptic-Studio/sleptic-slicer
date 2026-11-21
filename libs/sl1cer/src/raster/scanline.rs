// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Row buckets and constant-coverage intervals avoid full-frame pixel storage.
use super::{
    pixel_map::PixelMap,
    rle::{LayerRaster, RleRunAccumulator},
};
use crate::{contour::ContourLayer, geom2d::Point2D, profile::AaLevel};
#[cfg(test)]
thread_local! {
    pub(crate) static EDGE_ROW_SAMPLES:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};
    pub(crate) static COVERAGE_SPAN_SAMPLES:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};
}
#[derive(Clone)]
struct Edge {
    a: Point2D,
    b: Point2D,
    rows: std::ops::Range<usize>,
}
#[derive(Clone, Copy)]
struct Span {
    a: f64,
    b: f64,
}
pub fn rasterize_layer(
    layer: &ContourLayer,
    map: &PixelMap,
    aa: AaLevel,
    min_alpha: f32,
) -> LayerRaster {
    let w = map.width() as usize;
    let h = map.height() as usize;
    let n = match aa {
        AaLevel::Off => 1,
        AaLevel::X2 => 2,
        AaLevel::X4 => 4,
        AaLevel::X8 => 8,
    };
    let mut edges = Vec::new();
    let mut offsets = vec![0usize; h + 1];
    for p in layer.polygons() {
        for r in std::iter::once(p.outer()).chain(p.holes()) {
            for (a, b) in r.vertices().iter().zip(r.vertices().iter().cycle().skip(1)) {
                let (x, y) = map.mm_to_pixel(*a);
                let a = Point2D::new(x, y);
                let (x, y) = map.mm_to_pixel(*b);
                let b = Point2D::new(x, y);
                if a.y == b.y {
                    continue;
                }
                let lo = a.y.min(b.y).floor().max(0.).min(h as f64) as usize;
                let hi = a.y.max(b.y).ceil().max(0.).min(h as f64) as usize;
                edges.push(Edge { a, b, rows: lo..hi });
                for count in &mut offsets[lo + 1..hi + 1] {
                    *count += 1;
                }
            }
        }
    }
    // A contiguous row index avoids thousands of tiny allocations per layer.
    // Filling in edge order also preserves the stable crossing order at ties.
    for row in 1..offsets.len() {
        offsets[row] += offsets[row - 1];
    }
    let mut cursors = offsets[..h].to_vec();
    let mut indices = vec![0usize; offsets[h]];
    for (index, edge) in edges.iter().enumerate() {
        for cursor in &mut cursors[edge.rows.clone()] {
            indices[*cursor] = index;
            *cursor += 1;
        }
    }
    let mut out = RleRunAccumulator::new();
    let mut crossings = Vec::<(f64, i32)>::new();
    let mut spans = Vec::<Span>::new();
    let mut boundaries = Vec::<usize>::new();
    let threshold = (min_alpha as f64 * 255. / 100.).round().clamp(0., 256.) as u16;
    let mut empty = 0u64;
    for row in 0..h {
        let ids = &indices[offsets[row]..offsets[row + 1]];
        if ids.is_empty() {
            empty += w as u64;
            continue;
        }
        black(&mut out, &mut empty);
        spans.clear();
        boundaries.clear();
        boundaries.extend([0, w]);
        let mut cursors = [0usize; 8];
        let mut ends = [0usize; 8];
        for k in 0..n {
            cursors[k] = spans.len();
            let y = row as f64 + (k as f64 + 0.5) / n as f64;
            crossings.clear();
            for &i in ids {
                #[cfg(test)]
                EDGE_ROW_SAMPLES.with(|c| c.set(c.get() + 1));
                let e = &edges[i];
                if (e.a.y > y) != (e.b.y > y) {
                    let x = (e.b.x - e.a.x) * (y - e.a.y) / (e.b.y - e.a.y) + e.a.x;
                    crossings.push((x, if e.b.y > e.a.y { 1 } else { -1 }))
                }
            }
            crossings.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut winding = 0;
            let mut start = 0.;
            for &(x, d) in &crossings {
                let before = winding;
                winding += d;
                if before == 0 && winding != 0 {
                    start = x
                } else if before != 0 && winding == 0 {
                    let a = start.max(0.).min(w as f64);
                    let b = x.max(0.).min(w as f64);
                    if !a.is_finite() || !b.is_finite() || b <= a {
                        continue;
                    }
                    if aa == AaLevel::Off {
                        let l = (a - 0.5).ceil().max(0.) as usize;
                        let r = (b - 0.5).ceil().max(0.).min(w as f64) as usize;
                        if l < r {
                            spans.push(Span {
                                a: l as f64,
                                b: r as f64,
                            });
                            boundaries.extend([l, r]);
                        }
                    } else {
                        spans.push(Span { a, b });
                        let l = a.floor() as usize;
                        let r = b.floor() as usize;
                        boundaries.extend([l, (l + 1).min(w), r, (r + 1).min(w)]);
                    }
                }
            }
            ends[k] = spans.len();
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        for pair in boundaries.windows(2) {
            let x = pair[0];
            let coverage = if aa == AaLevel::Off {
                if spans.iter().any(|s| s.a <= x as f64 && (x as f64) < s.b) {
                    1.
                } else {
                    0.
                }
            } else {
                let mut sum = 0.;
                if spans.len() <= 16 {
                    // A short flat sum costs less than setting up a sweep for
                    // ordinary single-shell contours.
                    for s in &spans {
                        #[cfg(test)]
                        COVERAGE_SPAN_SAMPLES.with(|c| c.set(c.get() + 1));
                        sum += (s.b.min(x as f64 + 1.) - s.a.max(x as f64)).max(0.);
                    }
                } else {
                    // Spans are ordered and disjoint within each supersample.
                    // Advance only past zero contributions, keeping the original
                    // summation order (and therefore exact grayscale rounding).
                    for k in 0..n {
                        while cursors[k] < ends[k] && spans[cursors[k]].b <= x as f64 {
                            cursors[k] += 1;
                        }
                        for s in &spans[cursors[k]..ends[k]] {
                            if s.a >= x as f64 + 1. {
                                break;
                            }
                            #[cfg(test)]
                            COVERAGE_SPAN_SAMPLES.with(|c| c.set(c.get() + 1));
                            sum += (s.b.min(x as f64 + 1.) - s.a.max(x as f64)).max(0.);
                        }
                    }
                }
                sum / n as f64
            };
            let mut v = (coverage * 255.).round().clamp(0., 255.) as u8;
            if aa != AaLevel::Off && v > 0 && (v as u16) < threshold {
                v = 0
            }
            out.push(v, (pair[1] - x) as u32);
        }
    }
    black(&mut out, &mut empty);
    LayerRaster::new(layer.z_mm(), w as u32, h as u32, out.into_runs())
}
fn black(out: &mut RleRunAccumulator, count: &mut u64) {
    while *count > 0 {
        let n = (*count).min(u32::MAX as u64) as u32;
        out.push(0, n);
        *count -= n as u64
    }
}
