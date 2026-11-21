// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Streaming diagnostics over the exact corrected export rasters. Empty space is
//! 6-connected in the stack (4-connected in XY); only adjacent span planes are held.
use crate::islands::Spans;
use crate::{Island, LayerAnalysis};
use sc_sl1cer::{contour::ContourStack, profile::SliceProfile, raster::rle::LayerRaster};

/// Reserved semantic colours: errors block a reliable print; warnings need review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}
/// Stable grouping order shared by the frontends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskKind {
    UnsupportedIsland,
    SuctionCup,
    TrappedCavity,
    ThinWall,
    LargePeelArea,
    OutOfBounds,
    Overlap,
}
impl RiskKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::UnsupportedIsland => "Unsupported islands",
            Self::SuctionCup => "Suction cups",
            Self::TrappedCavity => "Trapped resin cavities",
            Self::ThinWall => "Thin walls / features",
            Self::LargePeelArea => "Large peel area",
            Self::OutOfBounds => "Outside build volume",
            Self::Overlap => "Overlapping models",
        }
    }
    pub fn explanation(self) -> &'static str {
        match self {
            Self::UnsupportedIsland => "This region has no contact with the layer below.",
            Self::SuctionCup => {
                "An enclosed void starts above a sealed floor and can pull a vacuum during peeling."
            }
            Self::TrappedCavity => {
                "This void has no connection to the outside anywhere in the layer stack."
            }
            Self::ThinWall => {
                "Material disappears under minimum-feature erosion and reconstruction."
            }
            Self::LargePeelArea => {
                "Lit area exceeds the peel limit or changes abruptly from the previous layer."
            }
            Self::OutOfBounds => "Model geometry extends beyond the printer's build envelope.",
            Self::Overlap => "Separate model volumes intersect.",
        }
    }
    pub fn fix(self) -> &'static str {
        match self {
            Self::UnsupportedIsland => "Add supports.",
            Self::SuctionCup | Self::TrappedCavity => "Add drain / vent holes.",
            Self::ThinWall => "Thicken the feature or lower the minimum after calibration.",
            Self::LargePeelArea => "Tilt the model or reduce the cross-section.",
            Self::OutOfBounds => "Move or scale the model inside the build volume.",
            Self::Overlap => "Arrange the models apart.",
        }
    }
}
/// A localized diagnostic; indices are zero-based, positions are world XY mm.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintRisk {
    pub kind: RiskKind,
    pub severity: Severity,
    pub first_layer: usize,
    pub last_layer: usize,
    pub position_mm: (f64, f64),
    /// Half-open image-pixel rectangle, image Y points down.
    pub bounds_px: [u32; 4],
    /// Worst measured layer for navigation (e.g. maximum peel spike).
    pub worst_layer: usize,
    pub measured: f64,
}
impl PrintRisk {
    pub(crate) fn island(i: &Island, p: &SliceProfile) -> Self {
        let x = i.contact_px.0.floor() as u32;
        let y = p.pixel_map().pixel_resolution().y() - 1 - i.contact_px.1.floor() as u32;
        let mut r = Self::new(
            RiskKind::UnsupportedIsland,
            i.layer,
            [x, y, x + 1, y + 1],
            p,
        );
        r.measured = i.area_mm2;
        r
    }
    pub fn new(kind: RiskKind, layer: usize, bounds: [u32; 4], p: &SliceProfile) -> Self {
        let map = p.pixel_map();
        let centre = map.pixel_to_mm((
            (f64::from(bounds[0]) + f64::from(bounds[2])) / 2.0,
            f64::from(map.pixel_resolution().y())
                - (f64::from(bounds[1]) + f64::from(bounds[3])) / 2.0,
        ));
        Self {
            kind,
            severity: if matches!(
                kind,
                RiskKind::UnsupportedIsland | RiskKind::OutOfBounds | RiskKind::Overlap
            ) {
                Severity::Error
            } else {
                Severity::Warning
            },
            first_layer: layer,
            last_layer: layer,
            worst_layer: layer,
            position_mm: (centre.x, centre.y),
            bounds_px: bounds,
            measured: 0.0,
        }
    }
    /// Plain-text report row, shared by command-line and diagnostic tooling.
    pub fn summary(&self) -> String {
        format!(
            "{:?}: {} | layers {}-{} | ({:.2}, {:.2}) mm | {} Fix: {}",
            self.severity,
            self.kind.title(),
            self.first_layer + 1,
            self.last_layer + 1,
            self.position_mm.0,
            self.position_mm.1,
            self.kind.explanation(),
            self.kind.fix()
        )
    }
}
/// Physical thresholds; defaults deliberately leave ordinary small cubes clean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiskSettings {
    pub minimum_feature_mm: f64,
    pub peel_area_mm2: f64,
    pub peel_change_mm2: f64,
    /// Enclosed voids smaller than this (mm^3) are not reported as suction
    /// cups or trapped cavities. Sub-millimetre pockets where a support tip
    /// meets the hull, or where tree branches cross, hold no meaningful resin
    /// and cannot pull a vacuum; reporting them buried the real warnings
    /// (a supported Benchy listed 44 "suction cups", tree supports 200).
    pub minimum_void_mm3: f64,
}
impl Default for RiskSettings {
    fn default() -> Self {
        Self {
            minimum_feature_mm: 0.3,
            peel_area_mm2: 2000.0,
            peel_change_mm2: 500.0,
            minimum_void_mm3: 1.0,
        }
    }
}
/// Volume, islands and risks from a single rasterization pass.
pub fn analyze_layers_with_settings(
    stack: &ContourStack,
    profile: &SliceProfile,
    settings: RiskSettings,
) -> LayerAnalysis {
    crate::islands::analyze_with(stack, profile, Some(settings), false)
}

type Span = (u32, u32, u32);
#[derive(Clone)]
struct Void {
    parent: usize,
    outside: bool,
    cups: Vec<usize>,
    contact: (f64, f64),
    first: usize,
    last: usize,
    bounds: [u32; 4],
    /// Enclosed volume so far, in pixel-layers (empty pixels summed over layers).
    pixels: u64,
}
pub(crate) struct RiskScanner {
    profile: SliceProfile,
    settings: RiskSettings,
    layer: usize,
    previous_area: f64,
    previous_bounds: [u32; 4],
    nodes: Vec<Void>,
    below: Vec<(Span, usize)>,
    report: Vec<PrintRisk>,
}
fn root(nodes: &mut [Void], mut i: usize) -> usize {
    while nodes[i].parent != i {
        nodes[i].parent = nodes[nodes[i].parent].parent;
        i = nodes[i].parent;
    }
    i
}
fn bounds_union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}
fn join(nodes: &mut [Void], a: usize, b: usize) {
    let a = root(nodes, a);
    let b = root(nodes, b);
    if a == b {
        return;
    }
    let (a, b) = (a.min(b), a.max(b));
    let other = nodes[b].clone();
    nodes[b].parent = a;
    let n = &mut nodes[a];
    n.outside |= other.outside;
    if n.outside {
        // A vented component cannot become sealed again in the already printed
        // volume. Cup measurements have been frozen in the report.
        n.cups.clear();
    } else {
        n.cups.extend(other.cups);
    }
    if other.first < n.first {
        n.contact = other.contact;
    }
    n.first = n.first.min(other.first);
    n.last = n.last.max(other.last);
    n.bounds = bounds_union(n.bounds, other.bounds);
    n.pixels += other.pixels;
}
impl RiskScanner {
    pub(crate) fn new(p: &SliceProfile, settings: RiskSettings) -> Self {
        Self {
            profile: p.clone(),
            settings,
            layer: 0,
            previous_area: 0.0,
            previous_bounds: [0; 4],
            nodes: Vec::new(),
            below: Vec::new(),
            report: Vec::new(),
        }
    }
    pub(crate) fn push(&mut self, r: &LayerRaster, area: f64) {
        let lit = Spans::from_raster(r);
        let w = r.width();
        let h = r.height();
        // Complement spans cover empty rows too, so narrow side drains connect
        // to the same exterior component instead of being mistaken for cavities.
        let mut empty = Vec::new();
        for y in 0..h {
            let mut x = 0;
            for &(_, a, b) in lit.row(y) {
                if a > x {
                    empty.push((y, x, a));
                }
                x = x.max(b);
            }
            if x < w {
                empty.push((y, x, w));
            }
        }
        let mut parents: Vec<usize> = (0..empty.len()).collect();
        fn find(p: &mut [usize], i: usize) -> usize {
            if p[i] != i {
                p[i] = find(p, p[i]);
            }
            p[i]
        }
        let mut prev_start = 0;
        let mut start = 0;
        while start < empty.len() {
            let y = empty[start].0;
            let mut end = start + 1;
            while end < empty.len() && empty[end].0 == y {
                end += 1;
            }
            if start > 0 && empty[prev_start].0 + 1 == y {
                let (mut a, mut b) = (prev_start, start);
                while a < start && b < end {
                    let x = empty[a];
                    let z = empty[b];
                    if x.1 < z.2 && z.1 < x.2 {
                        let ra = find(&mut parents, a);
                        let rb = find(&mut parents, b);
                        parents[rb] = ra;
                    }
                    if x.2 < z.2 {
                        a += 1;
                    } else {
                        b += 1;
                    }
                }
            }
            prev_start = start;
            start = end;
        }
        let current_base = self.nodes.len();
        let mut ids = vec![usize::MAX; empty.len()];
        let mut current = Vec::with_capacity(empty.len());
        for (i, &s) in empty.iter().enumerate() {
            let rt = find(&mut parents, i);
            if ids[rt] == usize::MAX {
                let id = self.nodes.len();
                self.nodes.push(Void {
                    parent: id,
                    outside: false,
                    cups: Vec::new(),
                    contact: (0.0, 0.0),
                    first: usize::MAX,
                    last: 0,
                    bounds: [u32::MAX, u32::MAX, 0, 0],
                    pixels: 0,
                });
                ids[rt] = id;
            }
            let id = ids[rt];
            let n = &mut self.nodes[id];
            n.outside |= self.layer == 0 || s.0 == 0 || s.0 + 1 == h || s.1 == 0 || s.2 == w;
            current.push((s, id));
        }
        let mut touched = vec![false; self.nodes.len() - current_base];
        let (mut a, mut b) = (0, 0);
        while a < current.len() && b < self.below.len() {
            let (s, id) = current[a];
            let (t, j) = self.below[b];
            if s.0 == t.0 && s.1 < t.2 && t.1 < s.2 {
                touched[id - current_base] = true;
                join(&mut self.nodes, id, j);
            }
            if (s.0, s.2) < (t.0, t.2) {
                a += 1;
            } else {
                b += 1;
            }
        }
        // Track enclosure geometry only, excluding the giant exterior rectangle.
        let mut changed = Vec::new();
        for &(s, id) in &current {
            let rt = root(&mut self.nodes, id);
            if !self.nodes[rt].outside {
                let n = &mut self.nodes[rt];
                if n.first == usize::MAX {
                    n.contact = (
                        (f64::from(s.1) + f64::from(s.2)) / 2.0,
                        f64::from(h - s.0) - 0.5,
                    );
                }
                if !touched[id - current_base] && self.layer > 0 && n.cups.is_empty() {
                    let index = self.report.len();
                    self.report.push(PrintRisk::new(
                        RiskKind::SuctionCup,
                        self.layer,
                        [s.1, h - s.0 - 1, s.2, h - s.0],
                        &self.profile,
                    ));
                    n.cups.push(index);
                }
                n.first = n.first.min(self.layer);
                n.last = self.layer;
                n.bounds = bounds_union(n.bounds, [s.1, h - s.0 - 1, s.2, h - s.0]);
                n.pixels += u64::from(s.2 - s.1);
                changed.push(rt);
            }
        }
        changed.sort_unstable();
        changed.dedup();
        for &rt in &changed {
            if self.nodes[rt].first == self.layer {
                let bounds = self.nodes[rt].bounds;
                let cx = (f64::from(bounds[0]) + f64::from(bounds[2])) / 2.0;
                let cy = (f64::from(bounds[1]) + f64::from(bounds[3])) / 2.0;
                let mut distance = f64::INFINITY;
                for &(s, id) in &current {
                    if root(&mut self.nodes, id) != rt {
                        continue;
                    }
                    let x = (cx - 0.5).round().clamp(f64::from(s.1), f64::from(s.2 - 1)) + 0.5;
                    let y = f64::from(h - s.0) - 0.5;
                    let d = (x - cx).powi(2) + (y - cy).powi(2);
                    if d < distance {
                        distance = d;
                        self.nodes[rt].contact = (x, y);
                    }
                }
            }
        }
        for rt in changed {
            let n = &self.nodes[rt];
            for &id in &n.cups {
                // Freeze cup volume when its component vents to the exterior.
                // Later exterior mergers must not promote sub-mm pockets.
                self.report[id].measured = n.pixels as f64;
                self.report[id].last_layer = n.last;
                self.report[id].bounds_px = n.bounds;
                set_contact(&mut self.report[id], n.contact, &self.profile);
            }
        }
        self.below = current;
        let bounds = lit
            .spans
            .iter()
            .fold([u32::MAX, u32::MAX, 0, 0], |b, &(y, a, z)| {
                bounds_union(b, [a, h - y - 1, z, h - y])
            });
        if area > self.settings.peel_area_mm2
            || (self.layer > 0 && (area - self.previous_area).abs() > self.settings.peel_change_mm2)
        {
            self.add(
                RiskKind::LargePeelArea,
                if area > 0.0 {
                    bounds
                } else {
                    self.previous_bounds
                },
                area.max((area - self.previous_area).abs()),
            );
        }
        self.previous_area = area;
        if area > 0.0 {
            self.previous_bounds = bounds;
        }
        let b = self.profile.build_volume();
        let rx = ((self.settings.minimum_feature_mm * f64::from(w) / b.x_mm() / 2.0).floor()
            as u32)
            .min(w);
        let ry = ((self.settings.minimum_feature_mm * f64::from(h) / b.y_mm() / 2.0).floor()
            as u32)
            .min(h);
        if rx > 0 || ry > 0 {
            let thin = thin_spans(&lit, rx, ry);
            if !thin.is_empty() {
                let bounds = thin
                    .iter()
                    .fold([u32::MAX, u32::MAX, 0, 0], |b, &(y, a, z)| {
                        bounds_union(b, [a, h - y - 1, z, h - y])
                    });
                self.add(RiskKind::ThinWall, bounds, self.settings.minimum_feature_mm);
            }
        }
        self.layer += 1;
    }
    fn add(&mut self, kind: RiskKind, bounds: [u32; 4], measured: f64) {
        if let Some(r) = self
            .report
            .iter_mut()
            .rev()
            .find(|r| r.kind == kind && r.last_layer + 1 == self.layer)
        {
            r.last_layer = self.layer;
            r.bounds_px = bounds_union(r.bounds_px, bounds);
            if measured > r.measured {
                r.measured = measured;
                r.worst_layer = self.layer;
            }
        } else {
            let mut r = PrintRisk::new(kind, self.layer, bounds, &self.profile);
            r.measured = measured;
            self.report.push(r);
        }
    }
    pub(crate) fn finish(mut self) -> Vec<PrintRisk> {
        // Minimum reportable void, converted to pixel-layers.
        let volume = self.profile.build_volume();
        let pixels = self.profile.pixel_resolution();
        let voxel_mm3 =
            volume.x_mm() / f64::from(pixels.x()) * (volume.y_mm() / f64::from(pixels.y())) * {
                // Average slab height (the schedule may use a thicker first layer).
                let schedule = self.profile.layer_schedule();
                schedule.iter_slab_tops().last().unwrap_or(0.0)
                    / f64::from(schedule.total_layers().max(1))
            };
        let min_pixels = (self.settings.minimum_void_mm3 / voxel_mm3).ceil().max(0.0) as u64;
        let tiny: Vec<_> = self
            .report
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                (r.kind == RiskKind::SuctionCup && r.measured < min_pixels as f64).then_some(i)
            })
            .collect();
        for &(_, id) in &self.below {
            let rt = root(&mut self.nodes, id);
            self.nodes[rt].outside = true;
        }
        for i in 0..self.nodes.len() {
            if root(&mut self.nodes, i) != i {
                continue;
            }
            let n = &self.nodes[i];
            if n.first == usize::MAX {
                continue;
            }
            if n.pixels < min_pixels {
                continue;
            }
            if !n.outside {
                let mut r =
                    PrintRisk::new(RiskKind::TrappedCavity, n.first, n.bounds, &self.profile);
                r.last_layer = n.last;
                set_contact(&mut r, n.contact, &self.profile);
                self.report.push(r);
            }
        }
        // Drop the suction cups raised (eagerly, layer by layer) for voids
        // that turned out too small to matter.
        let mut index = 0;
        self.report.retain(|_| {
            let keep = tiny.binary_search(&index).is_err();
            index += 1;
            keep
        });
        for report in &mut self.report {
            if report.kind == RiskKind::SuctionCup {
                report.measured *= voxel_mm3;
            }
        }
        self.report
    }
}
/// Rectangular physical erosion followed by reconstruction. The difference
/// identifies vanishing fins and necks, rather than every ordinary wall edge.
fn thin_spans(lit: &Spans, rx: u32, ry: u32) -> Vec<Span> {
    let mut eroded: Vec<Vec<(u32, u32)>> =
        vec![Vec::new(); lit.rows.last().map_or(0, |r| r.0 as usize + 1)];
    for &(y, start, end) in &lit.rows {
        let mut spans: Vec<(u32, u32)> = lit.spans[start..end]
            .iter()
            .filter_map(|&(_, a, b)| (b - a > 2 * rx).then_some((a + rx, b.saturating_sub(rx))))
            .collect();
        if y < ry {
            continue;
        }
        for row in y - ry..=y + ry {
            if row == y {
                continue;
            }
            let other = lit.row(row);
            let mut next = Vec::new();
            let (mut a, mut b) = (0, 0);
            while a < spans.len() && b < other.len() {
                let x = spans[a];
                let z = other[b];
                let lo = x.0.max(z.1.saturating_add(rx));
                let hi = x.1.min(z.2.saturating_sub(rx));
                if lo < hi {
                    next.push((lo, hi));
                }
                if x.1 < z.2.saturating_sub(rx) {
                    a += 1;
                } else {
                    b += 1;
                }
            }
            spans = next;
            if spans.is_empty() {
                break;
            }
        }
        eroded[y as usize] = spans;
    }
    let mut thin = Vec::new();
    let mut cover = Vec::new();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for &(y, start, end) in &lit.rows {
        cover.clear();
        merged.clear();
        for row in y.saturating_sub(ry)..=(y + ry).min(eroded.len().saturating_sub(1) as u32) {
            for &(x, z) in &eroded[row as usize] {
                cover.push((x.saturating_sub(rx), z.saturating_add(rx)));
            }
        }
        cover.sort_unstable();
        for &(lo, hi) in &cover {
            if let Some(last) = merged.last_mut()
                && lo <= last.1
            {
                last.1 = last.1.max(hi);
            } else {
                merged.push((lo, hi));
            }
        }
        // Reconstruct once per occupied row, then subtract from all lit spans.
        // The old implementation rebuilt and sorted the same cover per span.
        let mut cursor = 0;
        for &(_, a, b) in &lit.spans[start..end] {
            let mut x = a;
            while cursor < merged.len() && merged[cursor].1 <= x {
                cursor += 1;
            }
            let mut i = cursor;
            while i < merged.len() && merged[i].0 < b {
                let (lo, hi) = merged[i];
                if lo > x {
                    thin.push((y, x, lo.min(b)));
                }
                x = x.max(hi).min(b);
                if x == b {
                    break;
                }
                i += 1;
            }
            if x < b {
                thin.push((y, x, b));
            }
        }
    }
    thin
}

/// Unclipped model bounds paired with its slice stack. Supports should be
/// excluded by the caller: their intentional model contacts are not collisions.
#[derive(Debug)]
pub struct PlacedModel<'a> {
    pub min_mm: [f64; 3],
    pub max_mm: [f64; 3],
    pub stack: &'a ContourStack,
}
/// Placement diagnostics use original world bounds before raster clipping and
/// polygon interiors for overlap (AABB intersection alone is insufficient).
pub fn placement_risks(models: &[PlacedModel<'_>], p: &SliceProfile) -> Vec<PrintRisk> {
    let b = p.build_volume();
    let mut out = Vec::new();
    for (i, m) in models.iter().enumerate() {
        let range = |r: &mut PrintRisk| {
            let layers = m.stack.layers();
            r.first_layer = layers
                .iter()
                .position(|l| !l.polygons().is_empty())
                .unwrap_or(0);
            r.last_layer = layers
                .iter()
                .rposition(|l| !l.polygons().is_empty())
                .unwrap_or(r.first_layer);
            r.worst_layer = r.first_layer;
        };
        if m.min_mm[0] < -b.x_mm() / 2.0 - 1e-5
            || m.max_mm[0] > b.x_mm() / 2.0 + 1e-5
            || m.min_mm[1] < -b.y_mm() / 2.0 - 1e-5
            || m.max_mm[1] > b.y_mm() / 2.0 + 1e-5
            || m.min_mm[2] < -1e-5
            || m.max_mm[2] > b.z_mm() + 1e-5
        {
            let mut r = world_risk(RiskKind::OutOfBounds, 0, m.min_mm, m.max_mm, p);
            range(&mut r);
            out.push(r);
        }
        for n in &models[..i] {
            if (0..3).any(|a| m.min_mm[a] >= n.max_mm[a] || n.min_mm[a] >= m.max_mm[a]) {
                continue;
            }
            let mut collision = None;
            for (layer, (a, b)) in m.stack.layers().iter().zip(n.stack.layers()).enumerate() {
                if a.polygons()
                    .iter()
                    .any(|a| b.polygons().iter().any(|b| polygons_overlap(a, b)))
                {
                    collision.get_or_insert((layer, layer)).1 = layer;
                }
            }
            if let Some((first, last)) = collision {
                let lo = std::array::from_fn(|a| m.min_mm[a].max(n.min_mm[a]));
                let hi = std::array::from_fn(|a| m.max_mm[a].min(n.max_mm[a]));
                let mut r = world_risk(RiskKind::Overlap, first, lo, hi, p);
                r.last_layer = last;
                out.push(r);
            }
        }
    }
    out
}
fn world_risk(
    kind: RiskKind,
    layer: usize,
    lo: [f64; 3],
    hi: [f64; 3],
    p: &SliceProfile,
) -> PrintRisk {
    let map = p.pixel_map();
    let res = map.pixel_resolution();
    let project = |x: f64, y: f64| {
        let (x, y) = map.mm_to_pixel(sc_sl1cer::geom2d::Point2D::new(x, y));
        [
            x.clamp(0.0, f64::from(res.x())) as u32,
            (f64::from(res.y()) - y).clamp(0.0, f64::from(res.y())) as u32,
        ]
    };
    let a = project(lo[0], lo[1]);
    let z = project(hi[0], hi[1]);
    let mut r = PrintRisk::new(
        kind,
        layer,
        [
            a[0].min(z[0]),
            a[1].min(z[1]),
            a[0].max(z[0]),
            a[1].max(z[1]),
        ],
        p,
    );
    r.position_mm = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    r
}
fn polygons_overlap(a: &sc_sl1cer::contour::Polygon, b: &sc_sl1cer::contour::Polygon) -> bool {
    use sc_sl1cer::{
        contour::{ClosedRing, Polygon},
        geom2d::Point2D,
    };
    fn cross(a: Point2D, b: Point2D, c: Point2D) -> f64 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }
    fn edges(r: &ClosedRing) -> impl Iterator<Item = (Point2D, Point2D)> + '_ {
        let v = r.vertices();
        v.iter()
            .copied()
            .zip(v.iter().copied().cycle().skip(1))
            .take(v.len())
    }
    fn inside(r: &ClosedRing, p: Point2D) -> Option<bool> {
        let mut yes = false;
        for (a, b) in edges(r) {
            if cross(a, b, p).abs() < 1e-9
                && p.x >= a.x.min(b.x) - 1e-9
                && p.x <= a.x.max(b.x) + 1e-9
                && p.y >= a.y.min(b.y) - 1e-9
                && p.y <= a.y.max(b.y) + 1e-9
            {
                return None;
            }
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                yes = !yes;
            }
        }
        Some(yes)
    }
    fn material(poly: &Polygon, p: Point2D) -> bool {
        inside(poly.outer(), p) == Some(true)
            && poly.holes().iter().all(|h| inside(h, p) == Some(false))
    }
    for (x, y) in [(a, b), (b, a)] {
        for (u, v) in edges(x.outer()) {
            if material(y, u) || material(y, Point2D::new((u.x + v.x) / 2.0, (u.y + v.y) / 2.0)) {
                return true;
            }
        }
    }
    for ra in std::iter::once(a.outer()).chain(a.holes()) {
        for rb in std::iter::once(b.outer()).chain(b.holes()) {
            for (u, v) in edges(ra) {
                for (x, y) in edges(rb) {
                    if cross(u, v, x) * cross(u, v, y) < -1e-12
                        && cross(x, y, u) * cross(x, y, v) < -1e-12
                    {
                        return true;
                    }
                }
            }
        }
    }
    // Coincident boundaries: an interior sample just left of a CCW outer edge.
    for (u, v) in edges(a.outer()) {
        let d = ((v.x - u.x).powi(2) + (v.y - u.y).powi(2)).sqrt();
        if d > 0.0 {
            let q = Point2D::new(
                (u.x + v.x) / 2.0 - (v.y - u.y) / d * 1e-6,
                (u.y + v.y) / 2.0 + (v.x - u.x) / d * 1e-6,
            );
            if material(a, q) && material(b, q) {
                return true;
            }
        }
    }
    false
}

fn set_contact(r: &mut PrintRisk, contact: (f64, f64), p: &SliceProfile) {
    let map = p.pixel_map();
    let centre = map.pixel_to_mm((contact.0, f64::from(map.pixel_resolution().y()) - contact.1));
    r.position_mm = (centre.x, centre.y);
}

/// Exact selected-layer thin-feature spans, in bottom-to-top raster rows.
/// This is for on-demand highlighting; stack analysis uses the same operation.
pub fn thin_feature_spans(
    raster: &LayerRaster,
    pixel_mm: (f64, f64),
    minimum_mm: f64,
) -> Vec<(u32, u32, u32)> {
    let rx = ((minimum_mm / pixel_mm.0 / 2.0).floor().max(0.0) as u32).min(raster.width());
    let ry = ((minimum_mm / pixel_mm.1 / 2.0).floor().max(0.0) as u32).min(raster.height());
    if rx == 0 && ry == 0 {
        return Vec::new();
    }
    thin_spans(&Spans::from_raster(raster), rx, ry)
}

#[cfg(test)]
#[path = "tests_risks.rs"]
mod tests;
