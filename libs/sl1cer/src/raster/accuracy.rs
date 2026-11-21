// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::{
    pixel_map::PixelMap,
    rasterize_layer,
    rle::{LayerRaster, RleRunAccumulator},
};
use crate::{contour::*, profile::AaLevel};
use i_overlay::mesh::float::{
    outline::offset::OutlineOffset,
    style::{LineJoin, OutlineStyle},
};
#[cfg(test)]
thread_local! {
    pub(crate) static DENSE_PIXELS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accuracy {
    pub bottom_compensation_mm: f64,
    pub xy_compensation_mm: f64,
    pub aa_levels: u16,
    pub bottom_layers: u32,
    pub transition_layers: u32,
}
impl Default for Accuracy {
    fn default() -> Self {
        Self {
            bottom_compensation_mm: 0.,
            xy_compensation_mm: 0.,
            aa_levels: 256,
            bottom_layers: 0,
            transition_layers: 0,
        }
    }
}
impl Accuracy {
    pub fn erosion_mm(&self, index: usize) -> f64 {
        erosion_mm(self, index)
    }
}
pub fn erosion_mm(a: &Accuracy, index: usize) -> f64 {
    let index = index as u64;
    let bottom = u64::from(a.bottom_layers);
    let transitions = u64::from(a.transition_layers);
    if index < bottom {
        a.bottom_compensation_mm
    } else if index < bottom + transitions {
        // Fade between full bottom correction and the first uncorrected layer.
        let t = index - bottom;
        a.bottom_compensation_mm * (1. - (t + 1) as f64 / (transitions + 1) as f64)
    } else {
        0.
    }
}
fn compensate(layer: &ContourLayer, d: f64) -> ContourLayer {
    let paths: Vec<Vec<Vec<[f64; 2]>>> = layer
        .polygons()
        .iter()
        .map(|p| {
            std::iter::once(p.outer())
                .chain(p.holes())
                .map(|r| r.vertices().iter().map(|p| [p.x, p.y]).collect())
                .collect()
        })
        .collect();
    let style = OutlineStyle::new(d).line_join(LineJoin::Miter(2. * 0.25f64.asin()));
    let shapes = paths.outline(&style);
    ContourLayer::new(
        layer.z_mm(),
        shapes
            .into_iter()
            .filter_map(|s| {
                let mut it = s.into_iter().map(|r| {
                    ClosedRing::new(
                        r.into_iter()
                            .map(|p| crate::geom2d::Point2D::new(p[0], p[1]))
                            .collect(),
                    )
                });
                let outer = it.next()?;
                Polygon::new(outer, it.collect()).ok()
            })
            .collect(),
    )
}
// Sliding minima make rectangular erosion independent of the requested radius.
fn minimum_line(input: &[u8], output: &mut [u8], radius: usize) {
    let mut q = std::collections::VecDeque::<usize>::new();
    let mut end = 0;
    for (i, o) in output.iter_mut().enumerate() {
        while end < input.len() && end <= i.saturating_add(radius) {
            while q.back().is_some_and(|&j| input[j] >= input[end]) {
                q.pop_back();
            }
            q.push_back(end);
            end += 1
        }
        while q.front().is_some_and(|&j| j < i.saturating_sub(radius)) {
            q.pop_front();
        }
        *o = if i < radius || i.saturating_add(radius) >= input.len() {
            0
        } else {
            input[*q.front().unwrap()]
        }
    }
}
pub fn rasterize_accurate(
    layer: &ContourLayer,
    map: &PixelMap,
    aa: AaLevel,
    min_alpha: f32,
    accuracy: Accuracy,
    index: usize,
) -> LayerRaster {
    let adjusted =
        (accuracy.xy_compensation_mm != 0.).then(|| compensate(layer, accuracy.xy_compensation_mm));
    let raster = rasterize_layer(adjusted.as_ref().unwrap_or(layer), map, aa, min_alpha);
    let erosion = erosion_mm(&accuracy, index);
    let levels = accuracy.aa_levels.clamp(2, 256);
    if erosion <= 0. && levels == 256 {
        return raster;
    }
    let w = raster.width() as usize;
    let h = raster.height() as usize;
    let rx = (erosion.max(0.) * w as f64 / map.plate_x_mm())
        .round()
        .min(w as f64) as usize;
    let ry = (erosion.max(0.) * h as f64 / map.plate_y_mm())
        .round()
        .min(h as f64) as usize;
    // Quantization is pointwise: keep long black and solid runs compressed.
    // Positive physical compensation can still round to zero printer pixels.
    if rx == 0 && ry == 0 {
        let mut out = RleRunAccumulator::new();
        for run in raster.runs() {
            out.push(quantize(run.value, levels), run.length);
        }
        return LayerRaster::new(layer.z_mm(), w as u32, h as u32, out.into_runs());
    }
    #[cfg(test)]
    DENSE_PIXELS.with(|c| c.set(c.get() + w * h));
    let mut pixels = Vec::with_capacity(w * h);
    for r in raster.runs() {
        pixels.resize(pixels.len() + r.length as usize, r.value)
    }
    if erosion > 0. {
        let mut tmp = vec![0; pixels.len()];
        for (a, b) in pixels.chunks(w).zip(tmp.chunks_mut(w)) {
            minimum_line(a, b, rx)
        }
        let mut col = vec![0; h];
        let mut dst = vec![0; h];
        for x in 0..w {
            for y in 0..h {
                col[y] = tmp[y * w + x]
            }
            minimum_line(&col, &mut dst, ry);
            for y in 0..h {
                pixels[y * w + x] = dst[y]
            }
        }
    }
    let mut out = RleRunAccumulator::new();
    for v in pixels {
        out.push(quantize(v, levels), 1)
    }
    LayerRaster::new(layer.z_mm(), w as u32, h as u32, out.into_runs())
}

fn quantize(value: u8, levels: u16) -> u8 {
    let m = u32::from(levels) - 1;
    let b = (u32::from(value) * m + 127) / 255;
    ((255 * b + m / 2) / m) as u8
}
