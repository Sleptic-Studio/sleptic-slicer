// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Unsupported-island detection: regions of a layer that touch nothing
//! lit in the layer below. In resin printing every layer is peeled off
//! the vat film, so such a region has nothing to hold it to the part and
//! prints as a loose flake (or not at all) unless a support reaches it.
//!
//! The scan works on run-length rows: each layer becomes a list of lit
//! spans per pixel row, spans are joined into regions with union-find
//! (8-connected within the layer, bridging gaps up to the cure bleed, see
//! [`cure_bleed_px`]), and a region is supported when any of
//! its spans is within the self-supporting reach of a lit span of the layer
//! below (see [`self_supporting_reach_px`]). Only
//! lit spans are ever touched, so the cost follows the part's area, not
//! the screen's (Sleptic's C++ slicer measured the full-frame version at 84 of
//! 94 s on a 4K screen; this is the bounded variant its notes asked for).

use sc_sl1cer::contour::ContourStack;
use sc_sl1cer::profile::{LayerSchedule, SliceProfile};
use sc_sl1cer::raster::accuracy::rasterize_accurate;
use sc_sl1cer::raster::pixel_map::PixelMap;
use sc_sl1cer::raster::rle::LayerRaster;

/// How many pixels sideways a lit pixel may sit from the layer below and
/// still be held by it: a new layer cures onto the one under it across an
/// overhang of up to the resin's support angle (45 degrees for rigid
/// resins, i.e. one layer height sideways per layer; a flexible resin's
/// steeper angle allows less, R78).
/// Counting only pixels directly above as support made every one-pixel
/// step on the underside of a horizontal boss an "island" (camera mount:
/// 320 of them, each pinned by its own support). Pixels wider than the
/// layer height give 0, the strict directly-above rule.
pub fn self_supporting_reach_px(profile: &SliceProfile) -> u32 {
    let b = profile.build_volume();
    let r = profile.pixel_resolution();
    let pitch = (b.x_mm() / f64::from(r.x())).max(b.y_mm() / f64::from(r.y()));
    let LayerSchedule::Uniform(grid) = profile.layer_schedule();
    let layer = grid.layer_height().value();
    if !(pitch > 0.0 && layer > 0.0) {
        return 0;
    }
    let angle = profile.support_overhang_angle_deg().to_radians();
    let sideways = layer * angle.cos() / angle.sin();
    ((sideways / pitch + 1e-9).floor() as u32).min(4)
}

/// Layers rasterized together in parallel before they are scanned in order.
const RASTER_BATCH: usize = 64;

/// Resin cures this far (mm) past a lit pixel's edge within its layer.
const CURE_BLEED_MM: f64 = 0.05;

/// A speck island (at most this area, mm^2: four 50 um pixels) that joins
/// supported material within [`SPECK_PERSISTENCE_MM`] of height is raster
/// noise on a sloped or curved edge, not an island: the same rule as a
/// geometric dip that rejoins lower material (supports'
/// MIN_ISLAND_HEIGHT_MM). Larger regions are reported at once, so a
/// ceiling's first layer is never held back.
const SPECK_MAX_MM2: f64 = 0.0101;

/// See [`SPECK_MAX_MM2`].
const SPECK_PERSISTENCE_MM: f64 = 0.1;

/// A lip: an island starting at most this far (mm) from supported material
/// in its own layer, joining it within [`LIP_PERSISTENCE_MM`], is the edge of
/// a surface stepping out a little faster than the self-supporting reach,
/// not a floating piece (Sleptic, R82: a Benchy got columns under its hawse-hole
/// rims, 0.2-0.3 mm off the hull for two layers, and under its wheel's lower
/// edge, 0.15 mm off the cabin wall for a few layers; both print fine on
/// their own).
const LIP_GAP_MM: f64 = 0.3;

/// See [`LIP_GAP_MM`].
const LIP_PERSISTENCE_MM: f64 = 0.25;

/// Most pixels the lip gap may span; 0.3 mm at the finest screens' pixels.
const LIP_GAP_MAX_PX: u32 = 32;

/// [`LIP_GAP_MM`] in pixels across (columns) and along (rows), each at its
/// own pixel size, and [`LIP_PERSISTENCE_MM`] in layers, for `profile`.
/// (R108: one count from the larger pixel side, capped at 8, made the lip
/// 0.13 mm across an M7 Pro's 17 um columns and 0.3 mm on an M3 Premium, so a
/// Benchy's cabin-wall lip was an island on one printer and not the other.)
pub fn lip_limits(profile: &SliceProfile) -> ([u32; 2], usize) {
    let b = profile.build_volume();
    let r = profile.pixel_resolution();
    let pitch = [b.x_mm() / f64::from(r.x()), b.y_mm() / f64::from(r.y())];
    let LayerSchedule::Uniform(grid) = profile.layer_schedule();
    let layer = grid.layer_height().value();
    if !(pitch[0] > 0.0 && pitch[1] > 0.0 && layer > 0.0) {
        return ([0, 0], 0);
    }
    (
        pitch.map(|p| ((LIP_GAP_MM / p + 1e-9).floor() as u32).min(LIP_GAP_MAX_PX)),
        (LIP_PERSISTENCE_MM / layer - 1e-9).ceil().max(0.0) as usize,
    )
}

/// Layers a speck island may take to join supported material.
fn speck_persistence_layers(profile: &SliceProfile) -> usize {
    let LayerSchedule::Uniform(grid) = profile.layer_schedule();
    let layer = grid.layer_height().value();
    if !(layer > 0.0) {
        return 0;
    }
    (SPECK_PERSISTENCE_MM / layer - 1e-9).ceil().max(0.0) as usize
}

/// Lit pixels at most this many unlit pixels apart within one layer count
/// as joined: the resin between them cures too. With anti-aliasing off,
/// a sloped or curved surface lights single pixels one layer before the
/// pixel beside them, and each such speck read as an island (a universal
/// joint at 50 um pixels: 31 extra columns, 13 of them standing on the
/// part). Pixels wider than the bleed give 0, the strict 8-connected rule.
pub fn cure_bleed_px(profile: &SliceProfile) -> u32 {
    let b = profile.build_volume();
    let r = profile.pixel_resolution();
    let pitch = (b.x_mm() / f64::from(r.x())).max(b.y_mm() / f64::from(r.y()));
    if !(pitch > 0.0) {
        return 0;
    }
    ((CURE_BLEED_MM / pitch + 1e-9).floor() as u32).min(2)
}

/// One unsupported region.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    /// Zero-based layer index.
    pub layer: usize,
    /// Area in square millimetres.
    pub area_mm2: f64,
    /// Centroid in output-image pixels (x right, y down, as in the
    /// layer PNGs, after any mirroring the printer profile applies).
    pub centroid_px: (f64, f64),
    /// Centre of the lit pixel in this region nearest its centroid.
    pub contact_px: (f64, f64),
}

/// Rasterize every layer of `stack` the way the writers do and report
/// the unsupported islands, in layer order. Layer 0 rests on the build
/// plate and is never an island.
pub fn find_islands(stack: &ContourStack, profile: &SliceProfile) -> Vec<Island> {
    analyze_islands(stack, profile).islands
}

/// [`analyze_layers`] without the print-risk scan (`risks` comes back
/// empty): volume and islands only. Support completion and auto orient's
/// direct-print check read nothing else, and the risk scan was more than
/// half of each analysis (0.7 of 1.2 s on a 110 mm part).
pub fn analyze_islands(stack: &ContourStack, profile: &SliceProfile) -> LayerAnalysis {
    analyze_with(stack, profile, None, false)
}

/// Whether `stack` has any unsupported island: [`find_islands`]'s answer
/// without its list, stopping at the first island. Auto orient asks this of
/// every pose that might print without supports, and the full scan was
/// most of its time (R90).
pub fn has_islands(stack: &ContourStack, profile: &SliceProfile) -> bool {
    !analyze_with(stack, profile, None, true).islands.is_empty()
}

/// Volume and islands from one rasterization pass over the stack.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerAnalysis {
    /// Resin volume in millilitres (anti-aliased coverage included), the
    /// same figure as [`crate::raster_volume_ml`].
    pub volume_ml: f64,
    /// Unsupported islands, in layer order.
    pub islands: Vec<Island>,
    /// Unified, geometry-based pre-print diagnostics.
    pub risks: Vec<crate::PrintRisk>,
}

/// Rasterize each layer once and derive both the resin volume and the
/// islands from it. The GUI uses this after every slice, so islands cost
/// no second rasterization pass.
pub fn analyze_layers(stack: &ContourStack, profile: &SliceProfile) -> LayerAnalysis {
    crate::risks::analyze_layers_with_settings(stack, profile, crate::RiskSettings::default())
}

pub(crate) fn analyze_with(
    stack: &ContourStack,
    profile: &SliceProfile,
    settings: Option<crate::RiskSettings>,
    stop_at_first_island: bool,
) -> LayerAnalysis {
    let b = profile.build_volume();
    let r = profile.pixel_resolution();
    let map =
        PixelMap::new(b.x(), b.y(), *r, profile.mirror()).with_calibration(profile.calibration());
    let pixel_area_mm2 = b.x_mm() * b.y_mm() / (f64::from(r.x()) * f64::from(r.y()));
    let LayerSchedule::Uniform(grid) = profile.layer_schedule();
    let mut scanner = IslandScanner::new(
        pixel_area_mm2,
        self_supporting_reach_px(profile),
        cure_bleed_px(profile),
    );
    scanner.persistence = speck_persistence_layers(profile);
    (scanner.lip_gap, scanner.lip_persistence) = lip_limits(profile);
    let mut risks = settings.map(|settings| crate::risks::RiskScanner::new(profile, settings));
    let mut volume_mm3 = 0.0;
    let profiling = std::env::var_os("SLEPTIC_PROFILE_ANALYSIS").is_some();
    let mut timings = [std::time::Duration::ZERO; 3];
    // Layers are rasterized, turned into spans and labelled against the layer
    // below in parallel, a batch at a time (each is independent of the
    // scan); the scan then runs in order, since it carries open specks from
    // layer to layer. A batch bounds how much is held at once (R90: serial
    // rasterizing and labelling were most of auto supports' and auto
    // orient's time).
    use rayon::prelude::*;
    let layers = stack.layers();
    let (reach, bleed) = (scanner.reach, scanner.bleed);
    let mut batch: Vec<(LayerRaster, Prepared)> = Vec::new();
    let mut carried: Option<std::sync::Arc<Spans>> = None;
    for i in 0..layers.len() {
        if i % RASTER_BATCH == 0 {
            let started = profiling.then(std::time::Instant::now);
            let end = (i + RASTER_BATCH).min(layers.len());
            let rasters: Vec<LayerRaster> = (i..end)
                .into_par_iter()
                .map(|k| {
                    rasterize_accurate(
                        &layers[k],
                        &map,
                        profile.aa_level(),
                        0.0,
                        profile.accuracy(),
                        k,
                    )
                })
                .collect();
            if let Some(started) = started {
                timings[0] += started.elapsed();
            }
            let started = profiling.then(std::time::Instant::now);
            let spans: Vec<std::sync::Arc<Spans>> = rasters
                .par_iter()
                .map(|raster| std::sync::Arc::new(Spans::from_raster(raster)))
                .collect();
            let mut prepared: Vec<Prepared> = (0..spans.len())
                .into_par_iter()
                .map(|k| {
                    let below = if k == 0 {
                        carried.clone()
                    } else {
                        Some(spans[k - 1].clone())
                    };
                    let infos = below
                        .as_ref()
                        .map(|below| regions(&spans[k], below, reach, bleed, &[]));
                    Prepared {
                        spans: spans[k].clone(),
                        infos,
                    }
                })
                .collect();
            carried = spans.last().cloned();
            if let Some(started) = started {
                timings[1] += started.elapsed();
            }
            prepared.reverse();
            batch = rasters.into_iter().rev().zip(prepared).collect();
        }
        let (raster, prepared) = batch.pop().expect("one raster per layer of the batch");
        let coverage: f64 = raster
            .runs()
            .iter()
            .map(|run| f64::from(run.value) * f64::from(run.length) / 255.0)
            .sum();
        let height = if i == 0 {
            grid.first_layer_height().value()
        } else {
            grid.layer_height().value()
        };
        volume_mm3 += coverage * pixel_area_mm2 * height;
        let started = profiling.then(std::time::Instant::now);
        scanner.push_prepared(prepared);
        if stop_at_first_island && !scanner.islands.is_empty() {
            break;
        }
        if let Some(started) = started {
            timings[1] += started.elapsed();
        }
        let started = profiling.then(std::time::Instant::now);
        if let Some(risks) = risks.as_mut() {
            risks.push(&raster, coverage * pixel_area_mm2);
        }
        if let Some(started) = started {
            timings[2] += started.elapsed();
        }
    }
    if profiling {
        eprintln!(
            "R16 analysis phases: raster={:.6}s islands={:.6}s risks={:.6}s",
            timings[0].as_secs_f64(),
            timings[1].as_secs_f64(),
            timings[2].as_secs_f64()
        );
    }
    let islands = scanner.finish();
    let Some(risks) = risks else {
        return LayerAnalysis {
            volume_ml: volume_mm3 / 1000.0,
            islands,
            risks: Vec::new(),
        };
    };
    let mut report = risks.finish();
    for island in &islands {
        report.push(crate::PrintRisk::island(island, profile));
    }
    report.sort_by_key(|r| (r.kind, r.first_layer));
    LayerAnalysis {
        volume_ml: volume_mm3 / 1000.0,
        islands,
        risks: report,
    }
}

/// [`find_islands`] over already rasterized layers (bottom first), with
/// support counted only directly above (no sideways reach).
pub fn islands_in_rasters(
    rasters: impl IntoIterator<Item = LayerRaster>,
    pixel_area_mm2: f64,
) -> Vec<Island> {
    let mut scanner = IslandScanner::new(pixel_area_mm2, 0, 0);
    for raster in rasters {
        scanner.push(&raster);
    }
    scanner.finish()
}

/// Incremental scan: feed layers bottom first; keeps only the layer below.
struct IslandScanner {
    pixel_area_mm2: f64,
    reach: u32,
    bleed: u32,
    /// Layers a speck may take to join supported material; 0 reports
    /// every island at once.
    persistence: usize,
    /// A lip's distance (pixels) from supported material in its layer, and
    /// the layers it may take to join it ([`LIP_GAP_MM`]); 0 disables.
    /// The lip gap in pixels: across (columns), along (rows).
    lip_gap: [u32; 2],
    lip_persistence: usize,
    layer: usize,
    below: Option<std::sync::Arc<Spans>>,
    /// Per span of `below`: the open speck it continues, if any.
    below_specks: Vec<Option<usize>>,
    specks: Vec<Speck>,
    islands: Vec<Island>,
}

/// A layer's spans and, when labelled ahead in parallel, its regions
/// against the layer below assuming no open speck there.
struct Prepared {
    spans: std::sync::Arc<Spans>,
    infos: Option<Vec<RegionInfo>>,
}

/// A small island waiting to see whether it joins supported material.
struct Speck {
    island: Island,
    open: bool,
    /// Layers it may take to join supported material.
    limit: usize,
}

impl IslandScanner {
    fn new(pixel_area_mm2: f64, reach: u32, bleed: u32) -> Self {
        Self {
            pixel_area_mm2,
            reach,
            bleed,
            persistence: 0,
            lip_gap: [0, 0],
            lip_persistence: 0,
            layer: 0,
            below: None,
            below_specks: Vec::new(),
            specks: Vec::new(),
            islands: Vec::new(),
        }
    }

    fn push(&mut self, raster: &LayerRaster) {
        let spans = std::sync::Arc::new(Spans::from_raster(raster));
        self.push_prepared(Prepared { spans, infos: None });
    }

    /// One layer, its spans and (when computed ahead) its regions labelled
    /// as if no span below were an open speck. With an open speck below,
    /// the regions are labelled again here, with the specks.
    fn push_prepared(&mut self, prepared: Prepared) {
        let Prepared { spans, infos } = prepared;
        let mut specks = vec![None; spans.spans.len()];
        if let Some(below) = &self.below {
            let mut continued = vec![false; self.specks.len()];
            let infos = match infos {
                Some(infos) if self.below_specks.iter().all(Option::is_none) => infos,
                _ => regions(&spans, below, self.reach, self.bleed, &self.below_specks),
            };
            // Supported spans of this layer by row, for the lip distance;
            // only needed when some region is not supported (R90: building
            // it every layer was a third of the scan).
            let mut held: std::collections::BTreeMap<u32, Vec<(u32, u32)>> = Default::default();
            if self.lip_gap != [0, 0] && infos.iter().any(|info| !info.supported) {
                for info in infos.iter().filter(|info| info.supported) {
                    for &span in &info.spans {
                        let (row, x0, x1) = spans.spans[span];
                        held.entry(row).or_default().push((x0, x1));
                    }
                }
            }
            let [across, along] = self.lip_gap;
            let near_held = |info: &RegionInfo| {
                (across > 0 || along > 0)
                    && info.spans.iter().any(|&span| {
                        let (row, x0, x1) = spans.spans[span];
                        held.range(row.saturating_sub(along)..=row.saturating_add(along))
                            .any(|(_, xs)| {
                                xs.iter().any(|&(a, b)| {
                                    a <= x1.saturating_add(across) && x0 <= b.saturating_add(across)
                                })
                            })
                    })
            };
            for info in infos {
                let open: Vec<usize> = info
                    .touched
                    .iter()
                    .copied()
                    .filter(|&id| self.specks[id].open)
                    .collect();
                if info.supported {
                    // Joined supported material in time: noise.
                    for id in open {
                        self.specks[id].open = false;
                    }
                    continue;
                }
                let island = Island {
                    contact_px: info.region.contact_px,
                    layer: self.layer,
                    area_mm2: info.region.pixels as f64 * self.pixel_area_mm2,
                    centroid_px: (
                        info.region.sum_x / info.region.pixels as f64,
                        info.region.sum_y / info.region.pixels as f64,
                    ),
                };
                let id = if let Some(&keep) = open.iter().min() {
                    // Still apart from supported material: the speck goes on
                    // (specks that met are one island; the first one stands).
                    for &other in &open {
                        if other != keep {
                            self.specks[other].open = false;
                        }
                    }
                    keep
                } else if let Some(limit) = (self.persistence > 0
                    && island.area_mm2 <= SPECK_MAX_MM2)
                    .then_some(self.persistence)
                    .or_else(|| {
                        (self.lip_persistence > 0 && near_held(&info))
                            .then_some(self.lip_persistence)
                    })
                {
                    self.specks.push(Speck {
                        island,
                        open: true,
                        limit,
                    });
                    continued.push(false);
                    self.specks.len() - 1
                } else {
                    self.islands.push(island);
                    continue;
                };
                continued[id] = true;
                for &span in &info.spans {
                    specks[span] = Some(id);
                }
            }
            // A speck that stayed apart for the whole persistence height,
            // or ended there, is a real island.
            for (id, speck) in self.specks.iter_mut().enumerate() {
                if speck.open && (!continued[id] || self.layer - speck.island.layer >= speck.limit)
                {
                    speck.open = false;
                    self.islands.push(speck.island.clone());
                    for class in specks.iter_mut().filter(|c| **c == Some(id)) {
                        *class = None;
                    }
                }
            }
        }
        self.below = Some(spans);
        self.below_specks = specks;
        self.layer += 1;
    }

    /// The islands found, in layer order (open specks count).
    fn finish(mut self) -> Vec<Island> {
        for speck in self.specks.iter().filter(|s| s.open) {
            self.islands.push(speck.island.clone());
        }
        self.islands.sort_by_key(|island| island.layer);
        self.islands
    }
}

/// A layer's lit pixels as half-open spans `[x0, x1)` per row, in
/// row-major order, plus an index of where each occupied row starts.
pub(crate) struct Spans {
    pub(crate) spans: Vec<(u32, u32, u32)>,
    /// `(row, first span, one past last span)` for every occupied row,
    /// ascending by row.
    pub(crate) rows: Vec<(u32, usize, usize)>,
}

impl Spans {
    pub(crate) fn from_raster(raster: &LayerRaster) -> Self {
        let width = u64::from(raster.width().max(1));
        let mut spans = Vec::new();
        let mut pos: u64 = 0;
        for run in raster.runs() {
            let end = pos + u64::from(run.length);
            if run.value > 0 {
                // A run may cross row breaks; split it per row.
                let mut start = pos;
                while start < end {
                    let row = start / width;
                    let row_end = ((row + 1) * width).min(end);
                    let span = (
                        row as u32,
                        (start % width) as u32,
                        (row_end - row * width) as u32,
                    );
                    if let Some(last) = spans
                        .last_mut()
                        .filter(|last: &&mut (u32, u32, u32)| last.0 == span.0 && last.2 == span.1)
                    {
                        last.2 = span.2;
                    } else {
                        spans.push(span);
                    }
                    start = row_end;
                }
            }
            pos = end;
        }
        let mut rows: Vec<(u32, usize, usize)> = Vec::new();
        for (i, &(row, _, _)) in spans.iter().enumerate() {
            match rows.last_mut() {
                Some(last) if last.0 == row => last.2 = i + 1,
                _ => rows.push((row, i, i + 1)),
            }
        }
        Self { spans, rows }
    }

    pub(crate) fn row(&self, row: u32) -> &[(u32, u32, u32)] {
        self.row_indexed(row).1
    }

    /// A row's spans with the index of its first span in `spans`.
    fn row_indexed(&self, row: u32) -> (usize, &[(u32, u32, u32)]) {
        match self.rows.binary_search_by_key(&row, |r| r.0) {
            Ok(i) => (self.rows[i].1, &self.spans[self.rows[i].1..self.rows[i].2]),
            Err(_) => (0, &[]),
        }
    }
}

struct Region {
    pixels: u64,
    sum_x: f64,
    sum_y: f64,
    contact_px: (f64, f64),
}

/// The lit spans of `layer` that belong to unsupported islands given the
/// layer below, as `(row, x0, x1)` half-open spans in raster row order.
/// For highlighting islands on a layer image; pass the same `reach` and
/// `bleed` as the analysis ([`self_supporting_reach_px`],
/// [`cure_bleed_px`]) so the overlay matches it.
pub fn island_spans(
    layer: &LayerRaster,
    below: &LayerRaster,
    reach: u32,
    bleed: u32,
) -> Vec<(u32, u32, u32)> {
    let spans = Spans::from_raster(layer);
    let (mut parent, supported, _) =
        label_spans(&spans, &Spans::from_raster(below), reach, bleed, &[]);
    let mut region_supported = vec![false; spans.spans.len()];
    for (i, &span_supported) in supported.iter().enumerate() {
        let root = find(&mut parent, i);
        region_supported[root] |= span_supported;
    }
    (0..spans.spans.len())
        .filter(|&i| !region_supported[find(&mut parent, i)])
        .map(|i| spans.spans[i])
        .collect()
}

/// Union-find labelling of `layer`'s spans (8-connected, joined across
/// gaps of up to `bleed` unlit pixels) plus, per span, whether a lit span
/// of `below` lies within `reach` pixels of it (rows and columns; 0 =
/// directly below only). Spans of `below` marked in `below_specks` hold
/// nothing up: a span touching one lists the speck instead.
fn label_spans(
    layer: &Spans,
    below: &Spans,
    reach: u32,
    bleed: u32,
    below_specks: &[Option<usize>],
) -> (Vec<usize>, Vec<bool>, Vec<Vec<usize>>) {
    let n = layer.spans.len();
    let mut parent: Vec<usize> = (0..n).collect();

    // Spans in one row with at most `bleed` unlit pixels between them.
    for &(_, first, end) in &layer.rows {
        for i in first + 1..end {
            if layer.spans[i].1 - layer.spans[i - 1].2 <= bleed {
                union(&mut parent, i - 1, i);
            }
        }
    }
    // 8-connectivity between spans of nearby occupied rows (up to `bleed`
    // empty rows between), each widened by `bleed`.
    for (k, &upper) in layer.rows.iter().enumerate() {
        for &lower in layer.rows[k + 1..]
            .iter()
            .take_while(|lower| lower.0 <= upper.0 + 1 + bleed)
        {
            let (mut i, mut j) = (upper.1, lower.1);
            while i < upper.2 && j < lower.2 {
                let (_, a0, a1) = layer.spans[i];
                let (_, b0, b1) = layer.spans[j];
                if a0 <= b1 + bleed && b0 <= a1 + bleed {
                    union(&mut parent, i, j);
                }
                if a1 < b1 { i += 1 } else { j += 1 }
            }
        }
    }

    // A span within reach of lit pixels below supports its region. Each
    // below span is widened by `reach` on both sides; widened spans stay
    // sorted by both ends, so the two-pointer sweep still finds any overlap.
    let mut supported = vec![false; n];
    let mut touched = vec![Vec::new(); n];
    for &(row, start, end) in &layer.rows {
        for under_row in row.saturating_sub(reach)..=row.saturating_add(reach) {
            let (base, under) = below.row_indexed(under_row);
            let (mut i, mut j) = (start, 0);
            while i < end && j < under.len() {
                let (_, a0, a1) = layer.spans[i];
                let (_, b0, b1) = under[j];
                let (c0, c1) = (b0.saturating_sub(reach), b1.saturating_add(reach));
                if a0 < c1 && c0 < a1 {
                    match below_specks.get(base + j).copied().flatten() {
                        Some(id) => touched[i].push(id),
                        None => supported[i] = true,
                    }
                }
                if a1 < c1 { i += 1 } else { j += 1 }
            }
        }
    }
    (parent, supported, touched)
}

/// One connected region of a layer: whether lit material below holds it,
/// the open specks it touches below, and its spans.
struct RegionInfo {
    region: Region,
    supported: bool,
    touched: Vec<usize>,
    spans: Vec<usize>,
}

fn regions(
    layer: &Spans,
    below: &Spans,
    reach: u32,
    bleed: u32,
    below_specks: &[Option<usize>],
) -> Vec<RegionInfo> {
    let n = layer.spans.len();
    let (mut parent, supported, touched) = label_spans(layer, below, reach, bleed, below_specks);
    let mut regions: Vec<Option<RegionInfo>> = (0..n).map(|_| None).collect();
    for (i, &(row, x0, x1)) in layer.spans.iter().enumerate() {
        let root = find(&mut parent, i);
        let len = u64::from(x1 - x0);
        let entry = regions[root].get_or_insert(RegionInfo {
            region: Region {
                pixels: 0,
                sum_x: 0.0,
                sum_y: 0.0,
                contact_px: (0.0, 0.0),
            },
            supported: false,
            touched: Vec::new(),
            spans: Vec::new(),
        });
        entry.region.pixels += len;
        // Sum of pixel-centre x over the span, and its row centre.
        entry.region.sum_x += (f64::from(x0) + f64::from(x1)) / 2.0 * len as f64;
        entry.region.sum_y += (f64::from(row) + 0.5) * len as f64;
        entry.supported |= supported[i];
        entry.touched.extend(&touched[i]);
        entry.spans.push(i);
    }
    let mut distances = vec![f64::INFINITY; n];
    for (i, &(row, x0, x1)) in layer.spans.iter().enumerate() {
        let root = find(&mut parent, i);
        let info = regions[root].as_mut().unwrap();
        if info.supported {
            continue;
        }
        let region = &mut info.region;
        let cx = region.sum_x / region.pixels as f64;
        let cy = region.sum_y / region.pixels as f64;
        let x = (cx - 0.5).round().clamp(f64::from(x0), f64::from(x1 - 1)) + 0.5;
        let y = f64::from(row) + 0.5;
        let distance = (x - cx).powi(2) + (y - cy).powi(2);
        if distance < distances[root] {
            distances[root] = distance;
            region.contact_px = (x, y);
        }
    }
    regions
        .into_iter()
        .flatten()
        .filter(|info| info.region.pixels > 0)
        .map(|mut info| {
            info.touched.sort_unstable();
            info.touched.dedup();
            info
        })
        .collect()
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

#[cfg(test)]
mod tests {
    use sc_sl1cer::raster::rle::{LayerRaster, RleRun};

    use super::islands_in_rasters;

    /// Build a `w`x`h` raster from rows of '#' (lit) and '.' (dark).
    pub(super) fn raster(rows: &[&str]) -> LayerRaster {
        let w = rows[0].len() as u32;
        let mut runs: Vec<RleRun> = Vec::new();
        for row in rows {
            for c in row.chars() {
                let value = if c == '#' { 255 } else { 0 };
                match runs.last_mut() {
                    Some(last) if last.value == value => last.length += 1,
                    _ => runs.push(RleRun { value, length: 1 }),
                }
            }
        }
        LayerRaster::new(0.0, w, rows.len() as u32, runs)
    }

    #[test]
    fn a_region_with_nothing_below_is_an_island() {
        let base = raster(&["####....", "####....", "........"]);
        let next = raster(&["####....", "####..##", "......##"]);
        let islands = islands_in_rasters([base, next], 0.25);
        assert_eq!(islands.len(), 1);
        let island = &islands[0];
        assert_eq!(island.layer, 1);
        assert_eq!(island.area_mm2, 4.0 * 0.25);
        assert_eq!(island.centroid_px, (7.0, 2.0));
    }

    #[test]
    fn layer_zero_is_never_an_island_and_overlap_supports() {
        let base = raster(&["##......", "......##"]);
        let next = raster(&["#.......", ".......#"]);
        assert!(islands_in_rasters([base, next], 1.0).is_empty());
    }

    #[test]
    fn diagonal_contact_joins_a_region_to_a_supported_one() {
        // The right pixel touches the supported one only diagonally
        // within the layer (8-connectivity), so it is not an island.
        let base = raster(&["#...", "...."]);
        let next = raster(&["#...", ".#.."]);
        assert!(islands_in_rasters([base, next], 1.0).is_empty());
    }

    /// One pixel sideways per layer is a 45 degree overhang, which cures
    /// onto the layer below; two pixels is not. With reach 0 (pixels wider
    /// than the layer) even the one-pixel step is an island.
    #[test]
    fn a_one_pixel_step_is_held_within_the_self_supporting_reach() {
        let below = raster(&["....", ".#..", "...."]);
        let diagonal = raster(&["....", "....", "..#."]);
        let two_over = raster(&["....", "....", "...#"]);
        assert!(super::island_spans(&diagonal, &below, 1, 0).is_empty());
        assert_eq!(super::island_spans(&diagonal, &below, 0, 0).len(), 1);
        assert_eq!(super::island_spans(&two_over, &below, 1, 0).len(), 1);
    }

    /// A lit speck one unlit pixel from supported material in its layer is
    /// joined to it within a one-pixel cure bleed; two pixels away it is
    /// still an island, and with no bleed (coarse pixels) so is the first.
    #[test]
    fn a_speck_within_the_cure_bleed_joins_the_material_beside_it() {
        let below = raster(&["##......", "##......", "........"]);
        let gap_one = raster(&["##......", "##.#....", "........"]);
        let gap_two = raster(&["##......", "##..#...", "........"]);
        let row_gap = raster(&["##......", "........", ".#......"]);
        assert!(super::island_spans(&gap_one, &below, 0, 1).is_empty());
        assert_eq!(super::island_spans(&gap_one, &below, 0, 0), vec![(1, 3, 4)]);
        assert_eq!(super::island_spans(&gap_two, &below, 0, 1), vec![(1, 4, 5)]);
        assert!(super::island_spans(&row_gap, &below, 0, 1).is_empty());
    }

    /// Speck persistence (two layers here): a one-pixel speck that joins
    /// supported material by the next layer is noise; one still apart two
    /// layers later is an island, reported at its own first layer; a larger
    /// region is reported at once.
    #[test]
    fn a_speck_that_joins_supported_material_soon_is_not_an_island() {
        let scan = |layers: &[&[&str]]| {
            let mut scanner = super::IslandScanner::new(0.0025, 0, 0);
            scanner.persistence = 2;
            for rows in layers {
                scanner.push(&raster(rows));
            }
            scanner.finish()
        };
        let base: &[&str] = &["##......", "........"];
        let joins: &[&str] = &["##......", "...#...."];
        let joined: &[&str] = &["####....", "...#...."];
        assert!(scan(&[base, joins, joined]).is_empty());
        let apart: &[&str] = &["##......", ".....#.."];
        let still: &[&str] = &["##......", ".....##."];
        let islands = scan(&[base, apart, still, still, still]);
        assert_eq!(islands.len(), 1);
        assert_eq!(islands[0].layer, 1);
        let big: &[&str] = &["##......", "...#####"];
        let islands = scan(&[base, big, joined]);
        assert_eq!(islands.len(), 1, "five pixels (0.0125 mm2) are not a speck");
    }

    /// R82, a lip: a strip a few pixels off supported material in its own
    /// layer that joins it a layer or two up (a Benchy's wheel edge beside
    /// the cabin wall) is not an island. Farther off, or still apart after
    /// the lip height, it is.
    #[test]
    fn a_lip_beside_supported_material_that_joins_soon_is_not_an_island() {
        let scan = |layers: &[&[&str]]| {
            let mut scanner = super::IslandScanner::new(0.0025, 0, 0);
            scanner.persistence = 2;
            scanner.lip_gap = [3, 3];
            scanner.lip_persistence = 3;
            for rows in layers {
                scanner.push(&raster(rows));
            }
            scanner.finish()
        };
        let wall: &[&str] = &["##..........", "##..........", "##.........."];
        let lip: &[&str] = &["##...###....", "##...###....", "##...###...."];
        let closing: &[&str] = &["##..####....", "##..####....", "##..####...."];
        let joined: &[&str] = &["########....", "########....", "########...."];
        assert!(scan(&[wall, lip, closing, joined]).is_empty());
        let far: &[&str] = &["##.......###", "##.......###", "##.......###"];
        let far_joined: &[&str] = &["############", "############", "############"];
        assert_eq!(
            scan(&[wall, far, far_joined]).len(),
            1,
            "7 px off is not a lip"
        );
        assert_eq!(
            scan(&[wall, lip, lip, lip, lip, joined]).len(),
            1,
            "apart for longer than the lip height"
        );
    }

    /// R108: the lip gap is 0.3 mm on every printer, across and along its
    /// pixels separately (an M7 Pro's are 17 x 25 um, an M3 Premium's 38 um).
    #[test]
    fn the_lip_gap_is_the_same_distance_on_every_printer() {
        use sc_sl1cer::profile::{
            AaLevel, BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
        };
        let profile = |w: f64, d: f64, x: u32, y: u32| {
            SliceProfile::new(
                BuildVolume::new(w, d, 200.0).unwrap(),
                PixelResolution::new(x, y).unwrap(),
                LayerSchedule::uniform(0.05, 0.05, 5).unwrap(),
                Mirror::default(),
                AaLevel::Off,
                PackingMode::None,
            )
        };
        for (name, p) in [
            ("M7 Pro", profile(223.6, 126.5, 13312, 5120)),
            ("M3 Premium", profile(218.88, 123.12, 5760, 3240)),
        ] {
            let ([across, along], layers) = super::lip_limits(&p);
            let (b, r) = (p.build_volume(), p.pixel_resolution());
            let pitch = [b.x_mm() / f64::from(r.x()), b.y_mm() / f64::from(r.y())];
            for (px, pitch) in [(across, pitch[0]), (along, pitch[1])] {
                let mm = f64::from(px) * pitch;
                assert!(mm > 0.3 - pitch - 1e-9 && mm <= 0.3 + 1e-9, "{name}: {px} px = {mm:.3} mm");
            }
            assert_eq!(layers, 5, "{name}");
        }
    }

    #[test]
    fn runs_crossing_row_breaks_are_split_per_row() {
        // One run of 6 lit pixels spans rows 0 and 1 of a 4-wide raster
        // (last two pixels of row 0, first four of row 1).
        let base = LayerRaster::new(
            0.0,
            4,
            2,
            vec![
                RleRun {
                    value: 0,
                    length: 2,
                },
                RleRun {
                    value: 255,
                    length: 6,
                },
            ],
        );
        let empty = LayerRaster::new(
            0.0,
            4,
            2,
            vec![RleRun {
                value: 0,
                length: 8,
            }],
        );
        let islands = islands_in_rasters([empty, base], 1.0);
        assert_eq!(islands.len(), 1, "one connected region, unsupported");
        assert_eq!(islands[0].area_mm2, 6.0);
    }

    #[test]
    fn island_spans_marks_only_the_unsupported_region() {
        let base = raster(&["####....", "####....", "........"]);
        let next = raster(&["####....", "####..##", "......##"]);
        assert_eq!(
            super::island_spans(&next, &base, 0, 0),
            vec![(1, 6, 8), (2, 6, 8)]
        );
    }
}

#[cfg(test)]
mod contact_tests {
    use super::tests::raster;
    #[test]
    fn concave_region_contact_is_lit_and_other_region_stays_separate() {
        let islands = super::islands_in_rasters(
            [
                raster(&["........", "........", "........"]),
                raster(&["###....#", "#......#", "###....#"]),
            ],
            1.0,
        );
        assert_eq!(islands.len(), 2);
        assert_eq!(islands[0].centroid_px, (9.5 / 7.0, 1.5));
        assert_eq!(islands[0].contact_px, (0.5, 1.5));
        assert_eq!(islands[1].contact_px, (7.5, 1.5));
    }
}
