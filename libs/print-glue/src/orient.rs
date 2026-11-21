// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Deterministic resin orientation, shared by both frontends.
//!
//! All objectives use estimated user minutes; see [`Rates`]. Bed-contact
//! faces contribute support contacts only when the pose requires automatic
//! support lift. Direct-bed eligibility always inspects every face.
//! Flat surfaces have zero stair-step penalty.
use glam::{DQuat, DVec2, DVec3};
use rayon::prelude::*;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_print_primitives::BuildVolume;
use sc_scene::{Scene, node::NodeId};

pub use sc_print_profiles::orient::Preset;
/// Measured orientation costs (areas in square millimetres).
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    /// Conservative contact floor for tall, narrow supported poses.
    pub stability_contacts: f64,
    /// All-face geometric direct-bed eligibility, including adhesion and islands.
    pub zero_support: bool,
    /// Downward projected area above the build plate.
    pub support_area: f64,
    /// Estimated contact footprint for default 0.4 mm tips and 4 mm spacing.
    pub contact_area: f64,
    /// Curvature-weighted downward area; contacts on detailed surfaces cost more.
    pub detail_contact_area: f64,
    /// Topological local-minimum patches requiring at least one contact.
    pub minima: usize,
    /// Surface area exposed to stair stepping, weighted by slope.
    pub stair_area: f64,
    /// Largest of 24 sampled horizontal sections.
    pub peak_area: f64,
    /// Axis-aligned XY area after yaw optimization.
    pub footprint: f64,
    /// Height in millimetres.
    pub height: f64,
    /// XY dimensions after yaw.
    pub width: f64,
    /// Y extent after yaw.
    pub depth: f64,
    /// Enclosed model volume as a resin mass proxy, in cubic millimetres.
    pub volume_mm3: f64,
    /// Calibrated support contacts (geometric sampler on finalists).
    pub contacts: f64,
    /// Contacts shadowed below by model geometry.
    pub blocked_contacts: usize,
    /// Contacts without a lateral tool approach within 8 mm.
    pub recessed_contacts: usize,
    /// Expected generated model-start contacts from obstructed candidates.
    pub expected_blocked_contacts: f64,
    /// Expected generated contacts recessed for removal.
    pub expected_recessed_contacts: f64,
    /// Recessed geometric contacts with an unobstructed drop to the plate.
    pub plate_recessed_contacts: usize,
    /// Bed face or support/raft contact area in mm?.
    pub bed_contact_area: f64,
    /// Identical copies under the Fill plate shelf packing rule.
    pub copies: usize,
    /// Estimated print duration including support lift, in minutes.
    pub print_minutes: f64,
    /// Removal difficulty of long or inclined paths, in minutes.
    pub path_minutes: f64,
    /// Whether this orientation fits the complete build envelope.
    pub fits: bool,
    /// Pose keeps a real flat base on the plate: supports go only under its
    /// overhangs, with no lift and no raft (see `supports::Placement`).
    pub anchored: bool,
}
/// An orientation proposal in baked world coordinates.
#[derive(Clone, Debug)]
pub struct Proposal {
    /// At least one fitting checked candidate can print directly on the bed.
    pub zero_support_available: bool,
    /// World-space delta. Identity when retained.
    pub delta: Transform3D,
    /// Original costs.
    pub original: Metrics,
    /// Chosen costs.
    pub chosen: Metrics,
    /// Shortest fully verified fitting finalist, in estimated print minutes.
    pub shortest_minutes: f64,
    /// Metrics of the shortest verified fitting finalist, for tradeoff audits.
    pub shortest: Metrics,
    /// Whether the original transform was retained.
    pub retained: bool,
}
#[derive(Clone)]
struct Candidate {
    rotation: DQuat,
    min: DVec3,
    max: DVec3,
    metrics: Metrics,
    footprint_min: DVec3,
    footprint_max: DVec3,
    /// Position in the model-relative candidate order: the last tie-break,
    /// so equal poses (mirror images) resolve the same way from any start.
    source: usize,
}
fn bounds(points: impl Iterator<Item = DVec3>) -> Option<(DVec3, DVec3)> {
    points.fold(None, |b, p| {
        Some(match b {
            None => (p, p),
            Some((a, b)) => (a.min(p), b.max(p)),
        })
    })
}
fn hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_unstable_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let mut result = Vec::<DVec2>::new();
    for &p in &points {
        while result.len() >= 2
            && (result[result.len() - 1] - result[result.len() - 2])
                .perp_dot(p - result[result.len() - 1])
                <= 0.0
        {
            result.pop();
        }
        result.push(p);
    }
    let lower = result.len();
    for &p in points.iter().rev().skip(1) {
        while result.len() > lower
            && (result[result.len() - 1] - result[result.len() - 2])
                .perp_dot(p - result[result.len() - 1])
                <= 0.0
        {
            result.pop();
        }
        result.push(p);
    }
    result.pop();
    result
}
/// Evaluate hull-edge rectangles plus exact plate-boundary angles within every
/// caliper interval. A long part may fit diagonally even though its minimum-area
/// rectangle does not fit either plate-axis assignment.
fn yaw(points: &[DVec3], rotation: DQuat, volume: BuildVolume) -> DQuat {
    let polygon = hull(points.iter().map(|p| (rotation * *p).truncate()).collect());
    if polygon.len() < 2 {
        return rotation;
    }
    let mut breaks = vec![0.0, std::f64::consts::PI];
    for i in 0..polygon.len() {
        let edge = polygon[(i + 1) % polygon.len()] - polygon[i];
        let a = (-edge.y.atan2(edge.x)).rem_euclid(std::f64::consts::FRAC_PI_2);
        breaks.extend([a, a + std::f64::consts::FRAC_PI_2]);
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut angles = breaks.clone();
    for interval in breaks.windows(2) {
        let mid = (interval[0] + interval[1]) * 0.5;
        let (s, c) = mid.sin_cos();
        let x = |p: &DVec2| c * p.x - s * p.y;
        let y = |p: &DVec2| s * p.x + c * p.y;
        let dx = *polygon.iter().max_by(|a, b| x(a).total_cmp(&x(b))).unwrap()
            - *polygon.iter().min_by(|a, b| x(a).total_cmp(&x(b))).unwrap();
        let dy = *polygon.iter().max_by(|a, b| y(a).total_cmp(&y(b))).unwrap()
            - *polygon.iter().min_by(|a, b| y(a).total_cmp(&y(b))).unwrap();
        for (a, b, limit) in [(dx.x, -dx.y, volume.x_mm()), (dy.y, dy.x, volume.y_mm())] {
            let radius = a.hypot(b);
            if radius < limit || radius <= 1e-12 {
                continue;
            }
            let phi = b.atan2(a);
            let offset = (limit / radius).clamp(-1.0, 1.0).acos();
            for root in [phi - offset, phi + offset] {
                for turn in [-std::f64::consts::TAU, 0.0, std::f64::consts::TAU] {
                    let angle = root + turn;
                    if angle >= interval[0] - 1e-12 && angle <= interval[1] + 1e-12 {
                        angles.push(angle);
                    }
                }
            }
        }
    }
    let mut best = (false, f64::INFINITY, 0.0f64);
    for a in angles {
        let (s, c) = a.sin_cos();
        let (lo, hi) = bounds(
            polygon
                .iter()
                .map(|p| DVec3::new(c * p.x - s * p.y, s * p.x + c * p.y, 0.0)),
        )
        .unwrap();
        let size = hi - lo;
        let fits = size.x <= volume.x_mm() + 1e-5 && size.y <= volume.y_mm() + 1e-5;
        let area = size.x * size.y;
        // Equal areas (a box turned 90°, within f32 world-bake noise) keep the
        // heading nearest the incoming, model-relative one; an exact compare
        // let the starting angle pick between them (R50).
        let tol = 1e-6 * best.1.min(area).max(1.0);
        let (bs, bc) = best.2.sin_cos();
        let nearer = c > bc + 1e-9 || ((c - bc).abs() <= 1e-9 && s > bs + 1e-9);
        if (fits && !best.0)
            || (fits == best.0 && (area < best.1 - tol || ((area - best.1).abs() <= tol && nearer)))
        {
            best = (fits, area, a);
        }
    }
    DQuat::from_rotation_z(best.2) * rotation
}

fn evaluate(
    points: &[DVec3],
    triangles: &[([DVec3; 3], DVec3, f64)],
    rotation: DQuat,
    volume: BuildVolume,
    threshold: f64,
    optimize_yaw: bool,
) -> Candidate {
    let rotation = if optimize_yaw {
        yaw(points, rotation, volume)
    } else {
        rotation
    };
    let (min, max) = bounds(points.iter().map(|p| rotation * *p)).unwrap();
    let size = max - min;
    let mut metrics = Metrics {
        height: size.z,
        width: size.x,
        depth: size.y,
        footprint: size.x * size.y,
        fits: size.x <= volume.x_mm() + 1e-5
            && size.y <= volume.y_mm() + 1e-5
            && size.z <= volume.z_mm() + 1e-5,
        ..Default::default()
    };
    let mut sections = [0.0f64; 24];
    let centre = (min + max) * 0.5;
    for (tri, normal, detail) in triangles {
        let t = tri.map(|p| rotation * p);
        let normal = rotation * *normal;
        let area = normal.length() * 0.5;
        let sample_weight = normal.length() / (t[1] - t[0]).cross(t[2] - t[0]).length();
        let nz = normal.z / normal.length();
        let lo = t.iter().map(|v| v.z).fold(f64::INFINITY, f64::min);
        let hi = t.iter().map(|v| v.z).fold(f64::NEG_INFINITY, f64::max);
        if nz < -0.999999 && hi <= min.z + 0.02 {
            metrics.bed_contact_area += area;
        }
        // Faces resting on the plate are the base, not overhangs: the support
        // generator keeps a flat base on the plate (R38).
        if -nz > threshold + 1e-10 && hi > min.z + 0.02 {
            metrics.support_area += area * -nz;
            metrics.contact_area += area * -nz * std::f64::consts::PI * 0.04 / 16.0;
            metrics.detail_contact_area += area * -nz * detail;
        }
        if nz.abs() < 1.0 - 1e-8 {
            metrics.stair_area += area * nz.abs().powi(4);
        }
        for (i, section) in sections.iter_mut().enumerate() {
            let z = min.z + size.z * (i as f64 + 0.5) / 24.0;
            if z < lo || z >= hi {
                continue;
            }
            let mut hits = [DVec3::ZERO; 2];
            let mut count = 0;
            for j in 0..3 {
                let a = t[j];
                let b = t[(j + 1) % 3];
                if (a.z <= z && b.z > z) || (b.z <= z && a.z > z) {
                    if count < 2 {
                        hits[count] = a + (b - a) * ((z - a.z) / (b.z - a.z));
                    }
                    count += 1;
                }
            }
            if count == 2 {
                // Relative to the part's own centre: the sampled triangles
                // do not close each section, so the shoelace sum over them
                // changed with where the part sat on the plate (the same
                // Benchy pose read 3666 or 4633 mm2 of peel area, enough to
                // flip Auto prepare between upright and tilted, R79).
                let [a, b] = hits.map(|p| p - centre);
                let sign = if (b - a).dot(DVec3::Z.cross(normal)) >= 0.0 {
                    1.0
                } else {
                    -1.0
                };
                *section += sign * (a.x * b.y - b.x * a.y) * 0.5 * sample_weight;
            }
        }
    }
    metrics.peak_area = sections.into_iter().map(f64::abs).fold(0.0, f64::max);
    metrics.anchored = metrics.bed_contact_area >= 2.0 + 0.5 * metrics.height
        && metrics.bed_contact_area >= 0.1 * metrics.footprint;
    Candidate {
        footprint_min: min,
        footprint_max: max,
        rotation,
        min,
        max,
        metrics,
        source: usize::MAX,
    }
}
/// Conversion rates in hands-on-equivalent minutes. These are estimates, not
/// measured failure probabilities or promises about manual finishing time.
/// Kept in one place so presets/sliders change rates, never candidate ranges.
#[derive(Debug)]
pub struct Rates;
impl Rates {
    /// Removal and sanding per ordinary contact: 21 seconds.
    pub const CONTACT: f64 = 0.35;
    /// Unattended minute costs 0.12 manual minutes after a five-minute dead zone.
    pub const UNATTENDED: f64 = 0.12;
    /// Extra long jobs grow mildly convex: 0.04 manual minutes per hour squared.
    pub const LONG_JOB: f64 = 2.4;
    /// Fraction of blocked proposals becoming generated model-start contacts:
    /// 78 / 2607 = 0.02992 on the 36-pose completed-support calibration audit.
    pub const BLOCKED_COMPLETION_PROBABILITY: f64 = 0.03;
    /// Inspecting a pose with shadowed support proposals, once per part.
    pub const ROUTING_REVIEW: f64 = 3.0;
    /// A generated model-start contact needs six minutes for access/repair to remove/repair.
    pub const BLOCKED: f64 = 6.0;
    /// A recessed contact needs three extra removal/finishing minutes.
    pub const RECESSED: f64 = 3.0;
    /// Finishing a square millimetre of slope: 0.12 seconds.
    pub const FINISH: f64 = 0.002;
    /// Batch-oriented users value a full-plate print cycle at 120 manual minutes.
    pub const BATCH: f64 = 120.0;
    /// Retain the original within 30 seconds of estimated user effort.
    pub const NOISE: f64 = 0.5;
    /// Dead zone before unattended time costs anything, in minutes.
    pub const TIME_DEAD_ZONE: f64 = 5.0;
    /// Stair/detail finishing minutes per weighted square millimetre.
    pub const DETAIL_FINISH: f64 = 0.0005;
    /// Extra finishing per detailed contact, in minutes.
    pub const DETAIL_MARK: f64 = 0.15;
    /// Base adhesion target, in square millimetres.
    pub const ADHESION_BASE: f64 = 20.0;
    /// Target adhesion area per square millimetre of peak peel section.
    pub const ADHESION_PEEL: f64 = 0.15;
    /// Target adhesion area per millimetre of part height.
    pub const ADHESION_HEIGHT: f64 = 0.5;
    /// Failure prior before geometry terms.
    pub const FAILURE_BASE: f64 = 0.01;
    /// Failure probability per square millimetre of peak section.
    pub const PEEL_PROBABILITY: f64 = 0.00001;
    /// Failure probability per square millimetre of solid bed/raft area.
    pub const BOTTOM_PEEL_PROBABILITY: f64 = 0.000005;
    /// Extra adhesion target area per cubic millimetre of model volume.
    pub const ADHESION_VOLUME: f64 = 0.01;
    /// Extra contact effort on broad smooth surfaces (relative to ordinary).
    pub const SMOOTH_CONTACT: f64 = 0.25;
    /// Maximum extra failure probability for insufficient bed adhesion.
    pub const ADHESION_FAILURE: f64 = 0.20;
    /// Cup/recess proxy's maximum extra failure probability.
    pub const CUP_FAILURE: f64 = 0.05;
    /// Upper bound of the failure prior.
    pub const FAILURE_CAP: f64 = 0.6;
    /// Wash/reload/setup effort when a job fails, in minutes.
    pub const REPRINT_SETUP: f64 = 15.0;
    /// Maximum simplicity preference, in user minutes.
    pub const COMPOUND_TILT: f64 = 1.5;
    /// Long columns begin costing removal effort above this height, in mm.
    pub const LONG_PATH_MM: f64 = 60.0;
    /// Additional removal minute per this many mm of excess column length.
    pub const PATH_MM_PER_MINUTE: f64 = 120.0;
    /// Extra time rate when the model only fits tall in the searched poses.
    pub const STANDING_TIME_RATE: f64 = 3.0;
    /// Coarse area/grid occupancy calibrated on the 36-run library audit.
    pub const GRID_OCCUPANCY: f64 = 0.5;
    /// L1 contact calibration of accepted geometric samples versus completed
    /// supports is identity (MAE 2.72 contacts across 36 poses).
    pub const SAMPLED_CONTACT_FACTOR: f64 = 1.0;
    /// Reference inclination for angled-branch removal work, in degrees.
    pub const BRANCH_REFERENCE_DEG: f64 = 30.0;
    /// Extra removal work for a 30-degree branch, growing quadratically: six
    /// seconds. A branch snaps off at its tip like a column; at a full
    /// minute (five contacts' worth) tree supports pushed auto orient into
    /// tall poses with an unreachable contact (prepare benchmark, R68).
    pub const BRANCH_WORK: f64 = 0.1;
    /// Default only for callers without a resolved profile: 6 s / 0.05 mm.
    pub const DEFAULT_SECONDS_PER_MM: f64 = 120.0;
}
/// Print/support context, shared by GUI, batch and recipe callers.
#[derive(Clone, Copy, Debug)]
pub struct Context {
    /// Active printer raster geometry for verifying direct-bed finalists.
    pub hardware: Option<sc_print_profiles::PrinterHardware>,
    /// Resolved export compensation for direct-bed verification.
    pub accuracy: sc_sl1cer::raster::accuracy::Accuracy,
    /// Raster anti-aliasing used for direct-bed island verification.
    pub aa: sc_sl1cer::profile::AaLevel,
    /// First exposure slab height; zero uses the normal layer height.
    pub first_layer_height_mm: f64,
    /// Resolved layer height for shortest-print comparisons.
    pub layer_height_mm: f64,
    /// Extra raw copies a supported Fill plate pose must overcome.
    pub zero_support_head_start: usize,
    /// Normal exposure and resolved motion seconds per millimetre.
    pub seconds_per_mm: f64,
    /// Fixed bottom/transition overhead in minutes; zero without a profile.
    pub startup_minutes: f64,
    /// Arrangement clearance in millimetres.
    pub gap_mm: f64,
    /// Whether the model will be lifted and supported.
    pub supports: bool,
    /// Additional multiplier for awkward removal paths; one is Balanced.
    pub convenience_rate: f64,
    /// Support process footprint and sampling settings.
    pub support: sc_print_profiles::support_settings::SupportSettings,
}
impl Default for Context {
    fn default() -> Self {
        Self {
            hardware: None,
            accuracy: Default::default(),
            aa: sc_sl1cer::profile::AaLevel::X4,
            first_layer_height_mm: 0.0,
            layer_height_mm: 0.05,
            zero_support_head_start: sc_print_profiles::orient::ZERO_SUPPORT_HEAD_START_COPIES,
            seconds_per_mm: Rates::DEFAULT_SECONDS_PER_MM,
            startup_minutes: 0.0,
            gap_mm: 3.0,
            supports: true,
            convenience_rate: 1.0,
            support: Default::default(),
        }
    }
}
impl Context {
    /// Resolve the same exposure/motion cycle as slicing, including overrides.
    pub fn for_profiles(
        printer: &sc_print_profiles::PrinterProfile,
        material: &sc_print_profiles::EffectiveMaterial,
        overrides: &sc_print_profiles::SceneOverrides,
    ) -> Result<Self, sc_print_profiles::ResolveError> {
        let config = sc_print_profiles::resolve(printer, material, overrides)?;
        let m = &config.material_resolved;
        let seconds_per_mm = sc_print_job::seconds_per_mm(m);
        let startup_layers = m.bottom_layer_count.saturating_add(m.transition_layers);
        let startup_minutes = (sc_print_job::estimated_seconds(m, startup_layers)
            - f64::from(startup_layers) * m.layer_height_mm.value() * seconds_per_mm)
            / 60.0;
        let (bottom_compensation_mm, xy_compensation_mm, aa_levels) =
            sc_print_profiles::resolve_accuracy(printer, material, overrides);
        Ok(Self {
            accuracy: sc_sl1cer::raster::accuracy::Accuracy {
                bottom_compensation_mm,
                xy_compensation_mm,
                aa_levels,
                bottom_layers: m.bottom_layer_count,
                transition_layers: m.transition_layers,
            },
            hardware: Some(printer.hardware),
            layer_height_mm: m.layer_height_mm.value(),
            seconds_per_mm,
            startup_minutes,
            ..Self::default()
        })
    }
}
/// Number of identical axis-aligned bounding rectangles under Fill plate's
/// left-to-right shelf rule. No leading/trailing gap; capped like Fill plate.
pub fn copies_per_plate(width: f64, depth: f64, volume: BuildVolume, gap: f64) -> usize {
    if !width.is_finite() || !depth.is_finite() || width <= 0.0 || depth <= 0.0 {
        return 0;
    }
    let gap = if gap.is_finite() { gap.max(0.0) } else { 3.0 };
    // A micron of slack: models are baked to world in f32, so an exact fit
    // otherwise flips with the starting angle (R50).
    let nx = ((volume.x_mm() + gap + 1e-3) / (width + gap)).floor() as usize;
    let ny = ((volume.y_mm() + gap + 1e-3) / (depth + gap)).floor() as usize;
    nx.saturating_mul(ny).min(10001)
}
/// Unattended cost is small for a few minutes, significant for hours.
pub fn time_cost(extra_minutes: f64) -> f64 {
    let t = (extra_minutes - Rates::TIME_DEAD_ZONE).max(0.0);
    Rates::UNATTENDED * t + Rates::LONG_JOB * (t / 60.0).powi(2)
}
/// Estimated user costs: support work/convenience, unattended time, finishing
/// and expected reprint work, batch throughput. Balanced weights are unit rates.
pub fn user_costs(m: Metrics, shortest_minutes: f64) -> [f64; 4] {
    let detail = (m.detail_contact_area / m.support_area.max(1.0)).clamp(0.0, 1.0);
    let support =
        m.contacts * Rates::CONTACT * (1.0 + detail + Rates::SMOOTH_CONTACT * (1.0 - detail))
            + convenience_minutes(m);
    // Adequacy grows with peel load and leverage; above it adhesion saturates.
    let adequate = (Rates::ADHESION_BASE
        + m.peak_area * Rates::ADHESION_PEEL
        + m.height * Rates::ADHESION_HEIGHT
        + m.volume_mm3 * Rates::ADHESION_VOLUME)
        .max(Rates::ADHESION_BASE);
    let adhesion_deficit = (1.0 - m.bed_contact_area / adequate).clamp(0.0, 1.0);
    // Conservative engineering prior, not a physically calibrated failure model.
    let probability = (Rates::FAILURE_BASE
        + m.peak_area * Rates::PEEL_PROBABILITY
        + m.bed_contact_area * Rates::BOTTOM_PEEL_PROBABILITY
        + Rates::CUP_FAILURE * (m.expected_recessed_contacts / m.contacts.max(1.0)).min(1.0)
        + Rates::ADHESION_FAILURE * adhesion_deficit.powi(2))
    .min(Rates::FAILURE_CAP);
    let risk = probability * (Rates::REPRINT_SETUP + m.print_minutes * Rates::UNATTENDED);
    [
        support,
        time_cost(m.print_minutes - shortest_minutes),
        m.stair_area * Rates::FINISH
            + m.detail_contact_area * Rates::DETAIL_FINISH
            + m.contacts * detail * Rates::DETAIL_MARK
            + risk,
        Rates::BATCH / m.copies.max(1) as f64,
    ]
}
/// Fill plate values each additional copy at one user minute (before weights).
/// Ten direct-bed bonus copies are the default; unlike a reciprocal batch cost,
/// a gain of ten copies has identical value at every production volume.
pub fn fill_copy_cost(m: Metrics, head_start: usize) -> f64 {
    -(m.copies
        .saturating_add(if m.zero_support { head_start } else { 0 }) as f64)
}

// A supported candidate must first clear the raw-copy head start. Only then
// may the remaining weighted score decide whether its extra work is worthwhile.
fn direct_fill_order(a: Metrics, b: Metrics, head_start: usize) -> Option<std::cmp::Ordering> {
    if a.zero_support && b.zero_support {
        return Some(
            b.copies
                .cmp(&a.copies)
                .then(a.print_minutes.total_cmp(&b.print_minutes)),
        );
    }
    if a.zero_support != b.zero_support {
        let (direct, supported) = if a.zero_support { (a, b) } else { (b, a) };
        if supported.copies <= direct.copies.saturating_add(head_start) {
            return Some(b.zero_support.cmp(&a.zero_support));
        }
    }
    None
}

/// Estimated manual removal, access and finishing effort; excludes unattended
/// time and expected reprints. Useful for explaining a longer fitting pose.
pub fn hands_on_minutes(m: Metrics) -> f64 {
    let detail = (m.detail_contact_area / m.support_area.max(1.0)).clamp(0.0, 1.0);
    user_costs(m, m.print_minutes)[0]
        + m.stair_area * Rates::FINISH
        + m.detail_contact_area * Rates::DETAIL_FINISH
        + m.contacts * detail * Rates::DETAIL_MARK
}
fn convenience_minutes(m: Metrics) -> f64 {
    m.expected_blocked_contacts * Rates::BLOCKED
        + if m.blocked_contacts > 0 {
            Rates::ROUTING_REVIEW
        } else {
            0.0
        }
        + m.expected_recessed_contacts * Rates::RECESSED
        + m.path_minutes
}
fn costs(c: &Candidate, shortest: f64) -> [f64; 4] {
    user_costs(c.metrics, shortest)
}
fn contextualize(c: &mut Candidate, volume: BuildVolume, context: Context) {
    let context = Context {
        supports: context.supports && !c.metrics.zero_support,
        ..context
    };
    // Lift and raft only apply to a pose without a usable base.
    let lifted = context.supports && !c.metrics.anchored;
    let m = &mut c.metrics;
    // R18 library: area/minimum proxy overcounts fine detail. Compress the
    // minimum component; finalists replace this with the actual spaced sampler.
    m.contacts = m.support_area * Rates::GRID_OCCUPANCY / context.support.spacing_mm.powi(2)
        + (m.minima as f64).sqrt();
    let margin = if lifted {
        context.support.raft_margin_mm + context.support.base_diameter_mm / 2.0
    } else {
        0.0
    };
    m.copies = copies_per_plate(
        m.width + 2.0 * margin,
        m.depth + 2.0 * margin,
        volume,
        context.gap_mm,
    );
    m.print_minutes = (m.height + if lifted { context.support.lift_mm } else { 0.0 })
        * context.seconds_per_mm
        / 60.0
        + context.startup_minutes;
    if !context.supports {
        m.detail_contact_area = 0.0;
        m.contacts = 0.0;
    }
}
/// Search several hundred sphere directions and large face normals, then refine
/// the best four. Scoring samples at most 4096 triangles; final bounds and yaw
/// use every vertex. Original fitting placement is always available as fallback.
/// A finite directional search cannot prove that an arbitrary mesh never fits.
pub fn search(
    scene: &Scene,
    id: NodeId,
    volume: BuildVolume,
    support_angle_deg: f64,
    weights: [f64; 4],
) -> Option<Proposal> {
    search_with_context(
        scene,
        id,
        volume,
        support_angle_deg,
        weights,
        Context::default(),
    )
}
/// Search with the selected printer/resin timing and support process.
pub fn search_with_context(
    scene: &Scene,
    id: NodeId,
    volume: BuildVolume,
    support_angle_deg: f64,
    weights: [f64; 4],
    context: Context,
) -> Option<Proposal> {
    search_impl(scene, id, volume, support_angle_deg, weights, context, true)
}

fn search_impl(
    scene: &Scene,
    id: NodeId,
    volume: BuildVolume,
    support_angle_deg: f64,
    weights: [f64; 4],
    context: Context,
    lazy_direct: bool,
) -> Option<Proposal> {
    if weights.iter().any(|v| !v.is_finite() || *v < 0.0) || weights.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let mut mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    // Imported STL meshes may contain collapsed-index faces before the user
    // requests repair. They have no surface and must not disable sampling.
    // Filter only the private analysis copy; never repair the user's scene.
    mesh.faces.retain(|f| {
        let [a, b, c] = f.vertices;
        a != b
            && b != c
            && c != a
            && [a, b, c]
                .iter()
                .all(|i| usize::from(*i) < mesh.vertices.len())
    });
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let (old_min, old_max) = bounds(vertices.iter().copied())?;
    // World directions back into the model's own file coordinates.
    let local_from_world = match scene.world_transform_of(id).ok()? {
        Transform3D::Identity => glam::DMat3::IDENTITY,
        Transform3D::Affine(a) => {
            let l = a.linear();
            glam::DMat3::from_cols_array_2d(l).transpose().inverse()
        }
    };
    if !local_from_world.is_finite() {
        return None;
    }
    let world_from_local = local_from_world.inverse();
    if vertices.iter().any(|v| !v.is_finite()) || !old_min.is_finite() || !old_max.is_finite() {
        return None;
    }
    let triangles = triangle_sample(&mesh, &vertices);
    let model_volume = enclosed_volume(&mesh, &vertices);
    let access_tree = crate::hollow::Tree::from_mesh(&mesh);
    if triangles.is_empty() {
        return None;
    }
    let sample_stride = vertices.len().div_ceil(2048).max(1);
    let sample: Vec<_> = vertices.iter().step_by(sample_stride).copied().collect();
    let threshold = support_angle_deg.clamp(0.0, 90.0).to_radians().cos();
    let mut original = evaluate(
        &vertices,
        &triangles,
        DQuat::IDENTITY,
        volume,
        threshold,
        false,
    );
    original.metrics.volume_mm3 = model_volume;
    // Face sizes and directions are ranked on the model's own mesh, before
    // its pose is applied: those numbers are identical however the model was
    // turned. Ranked on the world-baked copy, f32 noise reordered faces of
    // equal size (a universal joint's mirror-image yokes) and with them the
    // model frame and every candidate pose, so the part landed a few degrees
    // differently depending on how it started (R79; R50 had only bucketed it).
    let local = local_faces(scene, id, &mesh, local_from_world);
    let stride = mesh.faces.len().div_ceil(4096).max(1);
    let mut faces: Vec<_> = local.iter().step_by(stride).filter(|f| f.2 > 0.0).collect();
    faces.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.3.cmp(&b.3)));
    let mut clusters = Vec::<DVec3>::new();
    for &&(n, _, _, _) in &faces {
        if clusters.iter().all(|c| c.dot(n) < 0.985) {
            clusters.push(n);
            if clusters.len() == 16 {
                break;
            }
        }
    }
    // The search samples directions in the MODEL's frame (its largest faces),
    // not the world's: the same part started at any angle tries the same poses
    // relative to itself and lands on the same result (Sleptic, R50).
    let frame = model_frame(&clusters);
    let mut directions: Vec<DVec3> = [
        DVec3::Z,
        -DVec3::Z,
        DVec3::X,
        -DVec3::X,
        DVec3::Y,
        -DVec3::Y,
    ]
    .map(|d| frame * d)
    .to_vec();
    for i in 0..320 {
        let z = 1.0 - 2.0 * (i as f64 + 0.5) / 320.0;
        let angle = i as f64 * std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
        let r = (1.0 - z * z).sqrt();
        directions.push(frame * DVec3::new(r * angle.cos(), r * angle.sin(), z));
    }
    // Every distinct face-down direction is eligible, including small faces
    // omitted from the scoring sample. Quantization only merges parallel normals.
    // Coplanar faces grouped and ranked on the model's own mesh (see above).
    let mut flat = std::collections::BTreeMap::<[i64; 3], (DVec3, f64, f64, usize)>::new();
    for (index, &(n, area, local_area, key)) in local.iter().enumerate() {
        if local_area > 0.0 {
            let entry = flat.entry(key).or_insert((n, 0.0, 0.0, index));
            entry.1 += area;
            entry.2 += local_area;
        }
    }
    let mut flat: Vec<_> = flat.into_values().collect();
    flat.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.3.cmp(&b.3)));
    for (normal, area, _, _) in &flat {
        if *area < 2.0 {
            continue;
        }
        let rotation = DQuat::from_rotation_arc(-*normal, DVec3::Z);
        let (lo, hi) = bounds(sample.iter().map(|p| rotation * *p)).unwrap();
        // Sample height is a lower bound: this rejection cannot hide an
        // adequately adhered flat pose. Check eligibility on every face before
        // paying for the sampled R20 score of an additional direct candidate.
        if *area >= 2.0 + 0.5 * (hi.z - lo.z)
            && direct_metrics(&mesh, rotation, threshold).is_some()
        {
            directions.push(-*normal);
        }
    }
    for n in clusters {
        for n in [n, -n] {
            directions.push(n);
            // The tilt axis is picked in the model's own axes: picked from
            // the world normal it turned with the starting pose, so a
            // universal joint was tried at different tilts (R79).
            // Snapped so f32 noise around an exact zero (a CAD face) cannot
            // flip the side any_orthonormal_vector picks; + 0.0 clears -0.
            let local = DVec3::from_array(
                (local_from_world * n)
                    .normalize_or_zero()
                    .to_array()
                    .map(|v| (v * 1e6).round() / 1e6 + 0.0),
            );
            let tangent = (world_from_local * local.any_orthonormal_vector())
                .reject_from(n)
                .try_normalize()
                .unwrap_or_else(|| n.any_orthonormal_vector());
            for degrees in [5.0f64, 10.0, 20.0] {
                for sign in [-1.0, 1.0] {
                    directions
                        .push(DQuat::from_axis_angle(tangent, sign * degrees.to_radians()) * n);
                }
            }
        }
    }
    let evaluate_pose = |points: &[DVec3], rotation: DQuat| {
        let mut c = evaluate(points, &triangles, rotation, volume, threshold, true);
        c.metrics.volume_mm3 = model_volume;
        let direct = direct_metrics(&mesh, c.rotation, threshold).is_some();
        c.metrics.zero_support = direct;
        if weights[3] <= 1.0 && !direct {
            return c;
        }
        let packing_context = Context {
            supports: context.supports
                && !direct
                && weights != sc_print_profiles::orient::FILL_PLATE_WEIGHTS,
            ..context
        };
        let rotation = packing_yaw(points, c.rotation, volume, packing_context);
        let mut c = evaluate(points, &triangles, rotation, volume, threshold, false);
        c.metrics.volume_mm3 = model_volume;
        c.metrics.zero_support = direct;
        c
    };
    let mut candidates: Vec<_> = directions
        .par_iter()
        .enumerate()
        .map(|(i, up)| Candidate {
            source: i,
            ..evaluate_pose(&sample, canonical_pose(*up, frame))
        })
        .collect();
    let reference = reference_pose(
        &candidates,
        &sample,
        &vertices,
        &triangles,
        &mesh.faces,
        volume,
        threshold,
    );
    for c in &mut candidates {
        contextualize(c, volume, context);
    }
    let shortest = candidates
        .iter()
        .filter(|c| c.metrics.fits && c.metrics.copies > 0)
        .map(|c| c.metrics.print_minutes)
        .fold(f64::INFINITY, f64::min);
    let shortest = if shortest.is_finite() { shortest } else { 0.0 };
    let standing = !original.metrics.fits
        && candidates
            .iter()
            .filter(|c| c.metrics.fits)
            .map(|c| c.metrics.height)
            .fold(f64::INFINITY, f64::min)
            > (original.metrics.height * 2.0).max(60.0);
    let fill = weights == sc_print_profiles::orient::FILL_PLATE_WEIGHTS;
    let score = |c: &Candidate, shortest: f64| {
        let mut terms = costs(c, shortest);
        if fill {
            // One raw copy is one user minute before the production multiplier.
            // A fixed value keeps +10 copies equally valuable at any batch size.
            terms[3] = fill_copy_cost(
                Metrics {
                    copies: copies_per_plate(
                        c.metrics.width,
                        c.metrics.depth,
                        volume,
                        context.gap_mm,
                    ),
                    ..c.metrics
                },
                context.zero_support_head_start,
            );
        }
        terms.iter().enumerate()
            .map(|(i, v)| weights[i] * v * if i == 1 && standing { Rates::STANDING_TIME_RATE } else { 1.0 }).sum::<f64>()
            // A clean single-axis tilt gets no extra pose cost. Compound
            // tilts need a measurable benefit (up to 90 seconds).
            + context.convenience_rate * Rates::COMPOUND_TILT * compound_tilt(local_from_world * (c.rotation.inverse() * DVec3::Z))
            + weights[0] * (context.convenience_rate - 1.0) * convenience_minutes(c.metrics)
    };
    let rank = |a: &Candidate, b: &Candidate| {
        b.metrics.fits.cmp(&a.metrics.fits).then_with(|| {
            if a.metrics.fits {
                (b.metrics.copies > 0)
                    .cmp(&(a.metrics.copies > 0))
                    .then_with(|| {
                        tie_safe(score(a, shortest)).total_cmp(&tie_safe(score(b, shortest)))
                    })
            } else {
                fit_ratio(a.metrics, volume).total_cmp(&fit_ratio(b.metrics, volume))
            }
        })
    };
    candidates.sort_by(rank);
    let mut refined = Vec::new();
    for c in candidates.iter().take(4) {
        for (k, degrees) in [-5.0f64, -2.0, 2.0, 5.0].into_iter().enumerate() {
            for (j, axis) in [DVec3::X, DVec3::Y].into_iter().enumerate() {
                refined.push(Candidate {
                    source: directions
                        .len()
                        .saturating_add(c.source.saturating_mul(8))
                        .saturating_add(k * 2 + j),
                    ..evaluate_pose(
                        &sample,
                        DQuat::from_axis_angle(axis, degrees.to_radians()) * c.rotation,
                    )
                });
            }
        }
    }
    for c in &mut refined {
        contextualize(c, volume, context);
    }
    candidates.extend(refined);
    candidates.sort_by(rank);
    // Never accept approximate extents as evidence of fit. Recheck finalists
    // using the complete mesh, retaining the original as an explicit candidate.
    // Sample vertices are a subset of full vertices. If their exact yaw
    // problem is infeasible, the full projection cannot fit that up-vector.
    let mut shortlist: Vec<_> = candidates
        .iter()
        .filter(|c| c.metrics.fits)
        .take(32)
        .collect();
    // Always verify the shortest fitting directions too: an inexpensive pose
    // must remain available even when the coarse contact estimate is poor.
    let mut by_height: Vec<_> = candidates.iter().filter(|c| c.metrics.fits).collect();
    by_height.sort_by(|a, b| {
        (a.metrics.height / context.layer_height_mm.max(0.001) - 1e-6)
            .ceil()
            .total_cmp(&(b.metrics.height / context.layer_height_mm.max(0.001) - 1e-6).ceil())
    });
    for c in by_height.into_iter().take(4) {
        if !shortlist.iter().any(|p| std::ptr::eq(*p, c)) {
            shortlist.push(c);
        }
    }
    for c in candidates.iter().filter(|c| c.metrics.zero_support) {
        if !shortlist.iter().any(|p| std::ptr::eq(*p, c)) {
            shortlist.push(c);
        }
    }
    let mut finalists: Vec<_> = shortlist
        .par_iter()
        .map(|c| Candidate {
            source: c.source,
            ..evaluate_pose(&vertices, c.rotation)
        })
        .collect();
    if let Some(rotation) = reference {
        let mut c = evaluate(&vertices, &triangles, rotation, volume, threshold, false);
        c.metrics.volume_mm3 = model_volume;
        c.source = usize::MAX - 1;
        finalists.push(c);
    }
    finalists.push(original.clone());
    let topology = crate::supports::ContactTopology::new(&mesh);
    let raster_cache = DirectRasterCache::default();
    let lazy_direct = lazy_direct
        && !fill
        && context.hardware.is_some()
        && context.seconds_per_mm.is_finite()
        && context.seconds_per_mm >= 0.0
        && context.support.lift_mm.is_finite()
        && context.support.lift_mm >= 0.0;
    let unverified = lazy_direct.then(|| finalists.clone());
    // Group before entering nested layer parallelism. Waiting for another
    // finalist's OnceLock inside Rayon could exhaust its workers while that
    // finalist is itself waiting for layer tasks on the same pool.
    if context.hardware.is_some() && !lazy_direct {
        let mut unique = std::collections::HashMap::new();
        for c in &finalists {
            if direct_metrics(&mesh, c.rotation, threshold).is_some() {
                let placed = direct_raster_mesh(c, &mesh, context);
                unique
                    .entry(direct_raster_key(c, &placed))
                    .or_insert((c, placed));
            }
        }
        unique.into_par_iter().for_each(|(key, (c, placed))| {
            raster_cache.get_or_check(key, || direct_raster_clear_placed(c, placed, context));
        });
    }
    let finalize_candidate = |c: &mut Candidate, raster_context: Context| {
        contextualize(c, volume, context);
        finalize_with_topology(
            c,
            &mesh,
            &access_tree,
            volume,
            raster_context,
            support_angle_deg,
            Some(((old_min + old_max) * 0.5).truncate()),
            Some(&raster_cache),
            Some(&topology),
        );
        c.metrics.minima = if c.metrics.zero_support {
            0
        } else {
            minimum_patches(&vertices, &mesh.faces, c.rotation)
        };
        c.metrics.contact_area += c.metrics.minima as f64 * std::f64::consts::PI * 0.04;
    };
    let sampled_rotations: Vec<_> = shortlist.iter().map(|c| c.rotation).collect();
    finalize_unique_poses(&mut finalists, &sampled_rotations, |c| {
        finalize_candidate(
            c,
            if lazy_direct {
                Context {
                    hardware: None,
                    ..context
                }
            } else {
                context
            },
        );
    });
    if let Some(unverified) = unverified {
        resolve_direct_finalists(&mut finalists, context.layer_height_mm, |i| {
            let mut c = unverified[i].clone();
            finalize_candidate(&mut c, context);
            c
        });
    }
    if !finalists.iter().any(|c| c.metrics.fits) {
        for c in candidates.iter().filter(|c| c.metrics.fits).skip(32) {
            let mut c = Candidate {
                source: c.source,
                ..evaluate_pose(&vertices, c.rotation)
            };
            contextualize(&mut c, volume, context);
            finalize_with_topology(
                &mut c,
                &mesh,
                &access_tree,
                volume,
                context,
                support_angle_deg,
                Some(((old_min + old_max) * 0.5).truncate()),
                Some(&raster_cache),
                Some(&topology),
            );
            if c.metrics.fits {
                c.metrics.minima = if c.metrics.zero_support {
                    0
                } else {
                    minimum_patches(&vertices, &mesh.faces, c.rotation)
                };
                c.metrics.contact_area += c.metrics.minima as f64 * std::f64::consts::PI * 0.04;
                finalists.push(c);
                break;
            }
        }
    }
    original.metrics.minima = minimum_patches(&vertices, &mesh.faces, DQuat::IDENTITY);
    original.metrics.contact_area += original.metrics.minima as f64 * std::f64::consts::PI * 0.04;
    contextualize(&mut original, volume, context);
    finalize_with_topology(
        &mut original,
        &mesh,
        &access_tree,
        volume,
        context,
        support_angle_deg,
        Some(((old_min + old_max) * 0.5).truncate()),
        Some(&raster_cache),
        Some(&topology),
    );
    if fill {
        // Fill plate copies the current bare model, before generating supports.
        // Use its exact shelf dimensions, never an imaginary future raft margin.
        for c in &mut finalists {
            c.metrics.copies =
                copies_per_plate(c.metrics.width, c.metrics.depth, volume, context.gap_mm);
        }
        original.metrics.copies = copies_per_plate(
            original.metrics.width,
            original.metrics.depth,
            volume,
            context.gap_mm,
        );
    }
    let shortest_metrics = finalists
        .iter()
        .filter(|c| c.metrics.fits)
        .min_by(|a, b| a.metrics.print_minutes.total_cmp(&b.metrics.print_minutes))
        .map_or(original.metrics, |c| c.metrics);
    let shortest = if shortest_metrics.fits {
        shortest_metrics.print_minutes
    } else {
        f64::INFINITY
    };
    let compare = |a: &Candidate, b: &Candidate| {
        if !fill && (a.metrics.zero_support || b.metrics.zero_support) {
            return direct_pose_order(a, b, context.layer_height_mm);
        }
        if fill
            && let Some(order) =
                direct_fill_order(a.metrics, b.metrics, context.zero_support_head_start)
        {
            return order;
        }
        tie_safe(score(a, shortest))
            .total_cmp(&tie_safe(score(b, shortest)))
            .then(a.source.cmp(&b.source))
    };
    let zero_support_available = finalists
        .iter()
        .any(|c| c.metrics.fits && c.metrics.zero_support);
    let best = finalists
        .iter()
        .filter(|c| c.metrics.fits)
        .min_by(|a, b| compare(a, b));
    let Some(best) = best else {
        return Some(Proposal {
            zero_support_available,
            delta: Transform3D::Identity,
            original: original.metrics,
            chosen: original.metrics,
            shortest: shortest_metrics,
            shortest_minutes: shortest,
            retained: true,
        });
    };
    let (axis, angle) = best.rotation.to_axis_angle();
    let rotation =
        Transform3D::Affine(AffineTransform3D::from_axis_angle(axis.to_array(), angle).ok()?);
    // Preserve existing packing whenever possible. Clamp the old centre only
    // as far as needed to keep the new footprint inside the active plate.
    let old_centre = (old_min + old_max) * 0.5;
    // The renderer downcasts both the matrix and source STL coordinates to
    // f32. Reserve a few source-coordinate ULPs to avoid false bed breaches.
    let precision_guard = vertices
        .iter()
        .map(|p| p.abs().max_element())
        .fold(1.0, f64::max)
        * f64::from(f32::EPSILON)
        * 4.0;
    let model_centre = (best.min + best.max) * 0.5;
    let clamp_centre = |old: f64, axis: usize, extent: f64| {
        let lo = -extent * 0.5 - (best.footprint_min[axis] - model_centre[axis]);
        let hi = extent * 0.5 - (best.footprint_max[axis] - model_centre[axis]);
        let guard = precision_guard.min(((hi - lo) * 0.25).max(0.0));
        if lo <= hi {
            old.clamp(lo + guard, hi - guard)
        } else {
            (lo + hi) * 0.5
        }
    };
    let centre_x = clamp_centre(old_centre.x, 0, volume.x_mm());
    let centre_y = clamp_centre(old_centre.y, 1, volume.y_mm());
    let z_guard = precision_guard.min(
        ((volume.z_mm()
            - best.metrics.height
            - if context.supports {
                context.support.lift_mm
            } else {
                0.0
            })
            * 0.5)
            .max(0.0),
    );
    let delta = Transform3D::Affine(AffineTransform3D::from_translation([
        centre_x - (best.min.x + best.max.x) * 0.5,
        centre_y - (best.min.y + best.max.y) * 0.5,
        -best.min.z + z_guard,
    ]))
    .compose(&rotation);
    Some(Proposal {
        zero_support_available,
        delta,
        original: original.metrics,
        chosen: best.metrics,
        shortest: shortest_metrics,
        shortest_minutes: shortest,
        retained: false,
    })
}

/// The sampled finalists all come from the same full-mesh evaluator. Equal
/// input rotation bits therefore have identical inputs apart from the source
/// tie-break. Key on rotations BEFORE evaluation, including its yaw decisions.
/// Keep every slot (including shortest-pose tie order), but do the expensive
/// support work once. Reference and original candidates are deliberately excluded:
/// they use a different evaluation path and may carry different input metrics.
fn finalize_unique_poses(
    finalists: &mut [Candidate],
    sampled_rotations: &[DQuat],
    finalize: impl Fn(&mut Candidate) + Sync,
) {
    let mut groups = std::collections::HashMap::new();
    let mut unique = Vec::new();
    let slots: Vec<_> = finalists
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i >= sampled_rotations.len() {
                unique.push(c.clone());
                return unique.len() - 1;
            }
            let key = sampled_rotations[i].to_array().map(f64::to_bits);
            *groups.entry(key).or_insert_with(|| {
                unique.push(c.clone());
                unique.len() - 1
            })
        })
        .collect();
    unique.par_iter_mut().for_each(&finalize);
    for (c, slot) in finalists.iter_mut().zip(slots) {
        let source = c.source;
        *c = unique[slot].clone();
        c.source = source;
    }
}

fn direct_pose_order(a: &Candidate, b: &Candidate, layer_height_mm: f64) -> std::cmp::Ordering {
    b.metrics
        .zero_support
        .cmp(&a.metrics.zero_support)
        .then(
            (a.metrics.height / layer_height_mm.max(0.001) - 1e-6)
                .ceil()
                .total_cmp(&(b.metrics.height / layer_height_mm.max(0.001) - 1e-6).ceil()),
        )
        .then(
            (b.metrics.bed_contact_area * 1000.0)
                .round()
                .total_cmp(&(a.metrics.bed_contact_area * 1000.0).round()),
        )
        .then(b.metrics.copies.cmp(&a.metrics.copies))
        .then(a.source.cmp(&b.source))
}

/// For non-fill presets a verified direct pose beats every supported pose.
/// Initially direct poses carry their exact clear-raster metrics. Resolve them
/// in the unchanged direct ranking until the winner is verified, then resolve
/// the shortest fitting pose until its metrics are verified too. A failed check
/// is fully finalized from its original input, including supports and fit.
/// Remaining speculative poses cannot affect either public result: they rank
/// after a verified direct winner (or cannot fit), and after a verified shortest
/// pose. Their original order is retained for the shortest-pose tie rule.
fn resolve_direct_finalists(
    finalists: &mut [Candidate],
    layer_height_mm: f64,
    mut resolve: impl FnMut(usize) -> Candidate,
) {
    let mut pending: Vec<_> = finalists.iter().map(|c| c.metrics.zero_support).collect();
    loop {
        let best = finalists
            .iter()
            .enumerate()
            .filter(|(_, c)| c.metrics.fits && c.metrics.zero_support)
            .min_by(|(_, a), (_, b)| direct_pose_order(a, b, layer_height_mm))
            .map(|(i, _)| i);
        let Some(i) = best.filter(|&i| pending[i]) else {
            break;
        };
        finalists[i] = resolve(i);
        pending[i] = false;
    }
    loop {
        let shortest = finalists
            .iter()
            .enumerate()
            .filter(|(_, c)| c.metrics.fits)
            .min_by(|(_, a), (_, b)| a.metrics.print_minutes.total_cmp(&b.metrics.print_minutes))
            .map(|(i, _)| i);
        let Some(i) = shortest.filter(|&i| pending[i]) else {
            break;
        };
        finalists[i] = resolve(i);
        pending[i] = false;
    }
}

/// Scores compared at 0.1 ms of user time: below that, two poses are a
/// tie (mirror images, the same face reached from two directions) and the
/// candidate order, which follows the model's own frame, decides. Exact
/// comparison let float noise from the starting angle pick the winner (R50).
fn tie_safe(score: f64) -> f64 {
    (score * 1e4).round()
}

/// Rotation putting `up` at +Z with a heading fixed to the model (its frame's
/// first axis, or second when that one points up, lies along +X). The shortest
/// arc gave a heading that depended on the start pose, and with it the copy
/// count and contact grid (R50).
fn canonical_pose(up: DVec3, frame: glam::DMat3) -> DQuat {
    let u = up.normalize();
    let flat = |v: DVec3| v - u * u.dot(v);
    let r = if flat(frame.x_axis).length() >= 0.3 {
        flat(frame.x_axis)
    } else {
        flat(frame.y_axis)
    };
    let Some(r) = r.try_normalize() else {
        return DQuat::from_rotation_arc(u, DVec3::Z);
    };
    DQuat::from_mat3(&glam::DMat3::from_cols(r, u.cross(r), u).transpose()).normalize()
}

/// Per face of the analysis mesh: its world unit normal and area, and from
/// the model's own (unposed) mesh its area and a coplanar-group key. The
/// local numbers do not change with the pose the model starts in. Falls
/// back to the world mesh when the two do not match face for face.
fn local_faces(
    scene: &Scene,
    id: NodeId,
    mesh: &sc_mesh_core::IndexedMesh,
    local_from_world: glam::DMat3,
) -> Vec<(DVec3, f64, f64, [i64; 3])> {
    let corners = |m: &sc_mesh_core::IndexedMesh, f: &sc_mesh_core::IndexedTriangle| {
        let p = f.vertices.map(|i| {
            m.vertices
                .get(usize::from(i))
                .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        });
        Some([p[0]?, p[1]?, p[2]?])
    };
    let cross = |t: [DVec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]);
    let own = scene
        .mesh_at_leaf(id)
        .ok()
        .filter(|m| m.faces.len() >= mesh.faces.len());
    // The analysis copy dropped collapsed faces; drop the same ones here.
    let own: Option<Vec<_>> = own.map(|m| {
        m.faces
            .iter()
            .filter(|f| {
                let [a, b, c] = f.vertices;
                a != b
                    && b != c
                    && c != a
                    && f.vertices
                        .iter()
                        .all(|i| usize::from(*i) < m.vertices.len())
            })
            .map(|f| corners(m, f))
            .collect()
    });
    let own = own.filter(|o| o.len() == mesh.faces.len());
    mesh.faces
        .iter()
        .enumerate()
        .map(|(index, f)| {
            let world = corners(mesh, f).map(cross).unwrap_or(DVec3::ZERO);
            let n = world.normalize_or_zero();
            let area = world.length() * 0.5;
            let (local_area, key_normal) = match own.as_ref().and_then(|o| o[index]) {
                Some(t) => {
                    let c = cross(t);
                    (c.length() * 0.5, c.normalize_or_zero())
                }
                None => (area, local_from_world * n),
            };
            let key_normal = key_normal.normalize_or_zero();
            let key = key_normal.to_array().map(|v| (v * 1e4).round() as i64);
            (
                n,
                area,
                if n == DVec3::ZERO { 0.0 } else { local_area },
                key,
            )
        })
        .collect()
}

/// An orthonormal frame fixed to the model: its largest face direction and
/// the next clearly different one. Identity when the mesh has no such faces
/// (a sphere looks the same from everywhere anyway).
fn model_frame(clusters: &[DVec3]) -> glam::DMat3 {
    let Some(&a) = clusters.first() else {
        return glam::DMat3::IDENTITY;
    };
    let Some(b) = clusters
        .iter()
        .find(|c| c.dot(a).abs() < 0.9)
        .map(|c| (*c - a * c.dot(a)).normalize())
    else {
        let b = a.any_orthonormal_vector();
        return glam::DMat3::from_cols(b, a.cross(b), a);
    };
    glam::DMat3::from_cols(b, a.cross(b), a)
}

// Keep the previously shipped R18 proposal in the search coverage. Its
// historical rank is ONLY a direction generator: every reference is rechecked
// with real contacts, fit, and the same absolute user-minute score as all others.
fn reference_pose(
    coarse: &[Candidate],
    sample: &[DVec3],
    vertices: &[DVec3],
    triangles: &[([DVec3; 3], DVec3, f64)],
    faces: &[sc_mesh_core::IndexedTriangle],
    volume: BuildVolume,
    threshold: f64,
) -> Option<DQuat> {
    let terms = |c: &Candidate| {
        let m = c.metrics;
        [
            m.support_area + m.minima as f64 * 4.0,
            m.height,
            m.stair_area + m.peak_area * 0.5 + m.contact_area * 32.0 + m.detail_contact_area * 0.25,
            m.footprint,
        ]
    };
    let mut low = [f64::INFINITY; 4];
    let mut high = [f64::NEG_INFINITY; 4];
    for c in coarse.iter().filter(|c| c.metrics.fits) {
        for (i, v) in terms(c).into_iter().enumerate() {
            low[i] = low[i].min(v);
            high[i] = high[i].max(v);
        }
    }
    if !low[0].is_finite() {
        return None;
    }
    let score = |c: &Candidate| {
        terms(c)
            .into_iter()
            .enumerate()
            .map(|(i, v)| [0.45, 0.25, 0.20, 0.10][i] * (v - low[i]) / (high[i] - low[i]).max(1e-9))
            .sum::<f64>()
    };
    let rank = |a: &Candidate, b: &Candidate| {
        b.metrics
            .fits
            .cmp(&a.metrics.fits)
            .then_with(|| tie_safe(score(a)).total_cmp(&tie_safe(score(b))))
    };
    let mut candidates = coarse.to_vec();
    candidates.sort_by(rank);
    let mut refined = Vec::new();
    for c in candidates.iter().take(4) {
        for degrees in [-5.0f64, -2.0, 2.0, 5.0] {
            for axis in [DVec3::X, DVec3::Y] {
                refined.push(evaluate(
                    sample,
                    triangles,
                    DQuat::from_axis_angle(axis, degrees.to_radians()) * c.rotation,
                    volume,
                    threshold,
                    true,
                ));
            }
        }
    }
    candidates.extend(refined);
    candidates.sort_by(rank);
    let mut finalists: Vec<_> = candidates
        .iter()
        .filter(|c| c.metrics.fits)
        .take(16)
        .collect::<Vec<_>>()
        .par_iter()
        .map(|c| evaluate(vertices, triangles, c.rotation, volume, threshold, true))
        .collect();
    let mut original = coarse.last()?.clone();
    finalists.push(original.clone());
    finalists.par_iter_mut().for_each(|c| {
        c.metrics.minima = minimum_patches(vertices, faces, c.rotation);
        c.metrics.contact_area += c.metrics.minima as f64 * std::f64::consts::PI * 0.04;
    });
    original.metrics.minima = minimum_patches(vertices, faces, DQuat::IDENTITY);
    original.metrics.contact_area += original.metrics.minima as f64 * std::f64::consts::PI * 0.04;
    let best = finalists
        .iter()
        .filter(|c| c.metrics.fits)
        .min_by(|a, b| tie_safe(score(a)).total_cmp(&tie_safe(score(b))))?;
    Some(
        if original.metrics.fits && score(&original) <= score(best) + 0.005 {
            DQuat::IDENTITY
        } else {
            best.rotation
        },
    )
}

// Count-based rotating calipers for the batch preset, rather than minimizing
// rectangle area. Finalists replace the conservative support margin with the
// actual sampled base/raft bounds before scoring.
fn packing_yaw(points: &[DVec3], rotation: DQuat, volume: BuildVolume, context: Context) -> DQuat {
    let polygon = hull(points.iter().map(|p| (rotation * *p).truncate()).collect());
    let margin = if context.supports {
        context.support.base_diameter_mm / 2.0
            + if context.support.raft {
                context.support.raft_margin_mm
            } else {
                0.0
            }
    } else {
        0.0
    };
    let mut best = (0usize, f64::INFINITY, rotation, (-2.0f64, -2.0f64));
    for i in 0..polygon.len() {
        let edge = polygon[(i + 1) % polygon.len()] - polygon[i];
        for swap in [0.0, std::f64::consts::FRAC_PI_2] {
            let angle = -edge.y.atan2(edge.x) + swap;
            let r = DQuat::from_rotation_z(angle) * rotation;
            let (lo, hi) = bounds(points.iter().map(|p| r * *p)).unwrap();
            let size = hi - lo;
            let count = copies_per_plate(
                size.x + 2.0 * margin,
                size.y + 2.0 * margin,
                volume,
                context.gap_mm,
            );
            // Equal count and area (within f32 world-bake noise) keep the yaw
            // nearest the incoming, model-relative heading, so the result does
            // not depend on which hull edge float noise lists first (R50).
            let area = size.x * size.y;
            let tol = 1e-6 * best.1.min(area).max(1.0);
            let heading = (angle.cos(), angle.sin());
            let nearer = heading.0 > best.3.0 + 1e-9
                || ((heading.0 - best.3.0).abs() <= 1e-9 && heading.1 > best.3.1 + 1e-9);
            if count > best.0
                || (count == best.0
                    && (area < best.1 - tol || ((area - best.1).abs() <= tol && nearer)))
            {
                best = (count, area, r, heading);
            }
        }
    }
    best.2
}

fn branch_work_minutes(paths: &[crate::supports::BranchPath]) -> f64 {
    paths
        .iter()
        .map(|p| {
            let angle = if p.inclination_deg < 0.001 {
                0.0
            } else {
                p.inclination_deg
            };
            Rates::BRANCH_WORK * (angle / Rates::BRANCH_REFERENCE_DEG).powi(2)
                + (p.length_mm - p.rise_mm).max(0.0) / Rates::PATH_MM_PER_MINUTE
        })
        .sum()
}
fn enclosed_volume(mesh: &sc_mesh_core::IndexedMesh, vertices: &[DVec3]) -> f64 {
    let origin = vertices[0];
    mesh.faces
        .iter()
        .map(|f| {
            let [a, b, c] = f.vertices.map(|i| vertices[usize::from(i)] - origin);
            a.dot(b.cross(c)) / 6.0
        })
        .sum::<f64>()
        .abs()
}
/// How far `up` (in the model's own file coordinates) is from lying in a
/// plane of the model's axes: zero for a single-axis tilt of the model as
/// it was drawn. Measured in the file's axes, not the pose the model was in
/// when Auto orient ran, so the starting pose cannot change the result (the
/// same Benchy got a different Auto prepare pose when merely spun, R79).
fn compound_tilt(up: DVec3) -> f64 {
    let up = up.normalize_or_zero();
    let mut a = up.abs().to_array();
    a.sort_by(f64::total_cmp);
    (a[0] * 4.0).min(1.0)
}
fn finalize(
    c: &mut Candidate,
    mesh: &sc_mesh_core::IndexedMesh,
    access_tree: &crate::hollow::Tree,
    volume: BuildVolume,
    context: Context,
    angle: f64,
    desired_xy: Option<DVec2>,
    raster_cache: Option<&DirectRasterCache>,
) {
    finalize_with_topology(
        c,
        mesh,
        access_tree,
        volume,
        context,
        angle,
        desired_xy,
        raster_cache,
        None,
    );
}

fn finalize_with_topology(
    c: &mut Candidate,
    mesh: &sc_mesh_core::IndexedMesh,
    access_tree: &crate::hollow::Tree,
    volume: BuildVolume,
    context: Context,
    angle: f64,
    desired_xy: Option<DVec2>,
    raster_cache: Option<&DirectRasterCache>,
    topology: Option<&crate::supports::ContactTopology>,
) {
    let threshold = angle.clamp(0.0, 90.0).to_radians().cos();
    c.metrics.zero_support = false;
    if let Some(bed) = direct_metrics(mesh, c.rotation, threshold)
        .filter(|_| desired_xy.is_some() || c.min.z.abs() <= 0.02)
        .filter(|_| direct_raster_clear(c, mesh, context, desired_xy, raster_cache))
    {
        c.metrics.zero_support = true;
        c.metrics.bed_contact_area = bed;
        let direct = Context {
            supports: false,
            ..context
        };
        contextualize(c, volume, direct);
        c.metrics.support_area = 0.0;
        c.metrics.contact_area = 0.0;
        c.metrics.minima = 0;
        c.metrics.copies =
            copies_per_plate(c.metrics.width, c.metrics.depth, volume, context.gap_mm);
        return;
    }
    contextualize(c, volume, context);
    if !context.supports {
        return;
    }
    if finalize_anchored(c, mesh, access_tree, volume, context, angle, topology) {
        return;
    }
    if !c.metrics.anchored {
        // The quick estimate may have taken the pose for anchored; score it
        // lifted (lift time, raft margin) now that the real check said no.
        contextualize(c, volume, context);
    }
    c.metrics.fits &= c.metrics.height + context.support.lift_mm <= volume.z_mm() + 1e-5;
    let centre = (c.min + c.max) * 0.5;
    // Score every pose at the same reference spot (plate centre): support
    // contacts are sampled on a world grid, so scoring at the model's current
    // position made the choice depend on where it started (Sleptic, R50).
    let _ = desired_xy;
    let target = DVec2::ZERO;
    let sample_shift = DVec3::new(
        target.x - centre.x,
        target.y - centre.y,
        context.support.lift_mm - c.min.z,
    );
    let mut placed = mesh.clone();
    for v in &mut placed.vertices {
        let p = c.rotation * DVec3::from_array(v.coords().map(f64::from)) + sample_shift;
        *v = sc_mesh_core::Vertex::new(p.to_array().map(|x| x as f32));
    }
    let settings = sc_print_profiles::support_settings::SupportSettings {
        overhang_angle_deg: angle,
        ..context.support
    };
    if let Ok(points) = crate::supports::sample_contacts_placed(
        &placed,
        &settings,
        (access_tree, c.rotation, sample_shift),
        topology,
    ) {
        let analysis = crate::supports::orientation_access_indexed(
            access_tree,
            &points,
            c.rotation,
            sample_shift,
            1e-4,
        );
        let m = &mut c.metrics;
        m.blocked_contacts = analysis.0;
        m.recessed_contacts = analysis.1;
        m.plate_recessed_contacts = analysis.3;
        m.expected_blocked_contacts = analysis.0 as f64 * Rates::BLOCKED_COMPLETION_PROBABILITY;
        m.expected_recessed_contacts = analysis.3 as f64
            + analysis.1.saturating_sub(analysis.3) as f64 * Rates::BLOCKED_COMPLETION_PROBABILITY;
        m.contacts = points.len().saturating_sub(analysis.0) as f64 * Rates::SAMPLED_CONTACT_FACTOR;
        m.path_minutes = points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let probability = if analysis.2[i] {
                    Rates::BLOCKED_COMPLETION_PROBABILITY
                } else {
                    1.0
                };
                probability
                    * ((f64::from(p.position[2]) - Rates::LONG_PATH_MM) / Rates::PATH_MM_PER_MINUTE)
                        .max(0.0)
            })
            .sum();
        let margin = settings.base_diameter_mm / 2.0
            + if settings.raft {
                settings.raft_margin_mm
            } else {
                0.0
            };
        let accepted: Vec<_> = points
            .iter()
            .enumerate()
            .filter(|(i, _)| !analysis.2[*i])
            .map(|(_, p)| DVec3::from_array(p.position.map(f64::from)))
            .collect();
        if let Some((lo, hi)) = bounds(accepted.into_iter()) {
            let horizontal_shift = DVec3::new(sample_shift.x, sample_shift.y, 0.0);
            let lo = lo - horizontal_shift;
            let hi = hi - horizontal_shift;
            let w = hi.x - lo.x + 2.0 * margin;
            let d = hi.y - lo.y + 2.0 * margin;
            m.bed_contact_area = if settings.raft {
                w * d
            } else {
                m.contacts * std::f64::consts::PI * (settings.base_diameter_mm / 2.0).powi(2)
            };
            c.footprint_min = c.min.min(lo - DVec3::new(margin, margin, 0.0));
            c.footprint_max = c.max.max(hi + DVec3::new(margin, margin, 0.0));
            let width = c.footprint_max.x - c.footprint_min.x;
            let depth = c.footprint_max.y - c.footprint_min.y;
            m.copies = copies_per_plate(width, depth, volume, context.gap_mm);
            m.fits &= m.copies > 0 && m.height + settings.lift_mm <= volume.z_mm() + 1e-5;
        } else {
            m.copies = 0;
            m.bed_contact_area = 0.0;
            m.fits = false;
        }
        if settings.tree_supports || settings.lattice_raft || !settings.raft {
            if let Ok(structure) = crate::supports::with_coarse_meshes(|| {
                crate::supports::build_supports(&placed, &points, &settings)
            }) {
                if settings.tree_supports {
                    m.path_minutes +=
                        branch_work_minutes(&crate::supports::branch_paths(&structure));
                }
                m.bed_contact_area = if let Some(raft) = &structure.raft {
                    crate::supports::base_contact_area(raft)
                } else {
                    structure
                        .columns
                        .iter()
                        .map(crate::supports::base_contact_area)
                        .sum()
                };
                let generated = structure
                    .columns
                    .iter()
                    .chain(structure.raft.iter())
                    .flat_map(|mesh| {
                        mesh.vertices.iter().map(|v| {
                            DVec3::from_array(v.coords().map(f64::from))
                                - horizontal_shift_for_sample(sample_shift)
                        })
                    });
                if let Some((lo, hi)) = bounds(generated) {
                    c.footprint_min = c.min.min(lo);
                    c.footprint_max = c.max.max(hi);
                    m.copies = copies_per_plate(
                        c.footprint_max.x - c.footprint_min.x,
                        c.footprint_max.y - c.footprint_min.y,
                        volume,
                        context.gap_mm,
                    );
                    m.fits &= m.copies > 0;
                }
            } else {
                m.fits = false;
            }
        }
    } else {
        c.metrics.fits = false;
    }
    if c.metrics.contacts > 0.0 {
        let aspect = c.metrics.height / c.metrics.width.min(c.metrics.depth).max(0.1);
        // A very slender lifted part should not be advertised as a one-tip job.
        // Three contacts resist rocking; another contact per three aspect units
        // is an explicit engineering prior, capped at twelve. This is distinct
        // from the geometric generator count and is not a physical calibration.
        if aspect > 6.0 {
            c.metrics.stability_contacts = (aspect / 3.0).ceil().clamp(3.0, 12.0);
            c.metrics.contacts = c.metrics.contacts.max(c.metrics.stability_contacts);
        }
        // The full-size v2 audit completed a nominal one-contact pose with
        // four contacts. A tall pose with peel section larger than its base
        // therefore reserves at least four contacts, even if its overall XY
        // envelope is broad enough to hide the narrow bearing in aspect ratio.
        if c.metrics.height > 50.0 && c.metrics.peak_area > c.metrics.bed_contact_area {
            c.metrics.stability_contacts = c.metrics.stability_contacts.max(4.0);
            c.metrics.contacts = c.metrics.contacts.max(c.metrics.stability_contacts);
        }
    }
}
// A geometric proof alone cannot detect a feature narrower than one raster
// pixel. Frontends with a selected printer also check the real slice grid.
/// Score a pose the support generator would keep on the plate (a real flat
/// base): columns only under overhangs high enough for one, no lift, no raft,
/// the base itself as adhesion. Returns false for a pose without such a base.
fn finalize_anchored(
    c: &mut Candidate,
    mesh: &sc_mesh_core::IndexedMesh,
    access_tree: &crate::hollow::Tree,
    volume: BuildVolume,
    context: Context,
    angle: f64,
    topology: Option<&crate::supports::ContactTopology>,
) -> bool {
    // Same reference spot as the lifted path: the plate centre.
    let centre = (c.min + c.max) * 0.5;
    let shift = DVec3::new(-centre.x, -centre.y, -c.min.z);
    let mut placed = mesh.clone();
    for v in &mut placed.vertices {
        let p = c.rotation * DVec3::from_array(v.coords().map(f64::from)) + shift;
        *v = sc_mesh_core::Vertex::new(p.to_array().map(|x| x as f32));
    }
    if !anchored_on_plate(&placed) {
        c.metrics.anchored = false;
        return false;
    }
    let settings = sc_print_profiles::support_settings::SupportSettings {
        overhang_angle_deg: angle,
        raft: false,
        ..context.support
    };
    let sampled = match topology {
        Some(t) => crate::supports::sample_overhang_contacts_indexed(&placed, &settings, t),
        None => crate::supports::sample_overhang_contacts(&placed, &settings),
    };
    let Ok(mut points) = sampled else {
        return false;
    };
    let lowest = settings.lowest_contact_mm();
    points.retain(|p| f64::from(p.position[2]) > lowest);
    let analysis =
        crate::supports::orientation_access_indexed(access_tree, &points, c.rotation, shift, 1e-4);
    let base = c.metrics.bed_contact_area;
    let m = &mut c.metrics;
    m.anchored = true;
    m.blocked_contacts = analysis.0;
    m.recessed_contacts = analysis.1;
    m.plate_recessed_contacts = analysis.3;
    // A contact shadowed by the part itself is usually a bridge between walls
    // (cabin roofs, window tops) that the generator skips; only the raster
    // completion pass turns a few into columns on the model. Same calibrated
    // probability as the lifted path (78 / 2607 on the 36-pose audit).
    m.expected_blocked_contacts = analysis.0 as f64 * Rates::BLOCKED_COMPLETION_PROBABILITY;
    m.expected_recessed_contacts = analysis.3 as f64
        + analysis.1.saturating_sub(analysis.3) as f64 * Rates::BLOCKED_COMPLETION_PROBABILITY;
    // A pose that is geometrically clean but failed the raster check (a
    // sub-pixel neck) still gets completion columns: count at least one.
    m.contacts =
        (points.len().saturating_sub(analysis.0) as f64 * Rates::SAMPLED_CONTACT_FACTOR).max(1.0);
    m.path_minutes = points
        .iter()
        .map(|p| {
            ((f64::from(p.position[2]) - Rates::LONG_PATH_MM) / Rates::PATH_MM_PER_MINUTE).max(0.0)
        })
        .sum();
    m.bed_contact_area =
        base + m.contacts * std::f64::consts::PI * (settings.base_diameter_mm / 2.0).powi(2);
    m.copies = copies_per_plate(m.width, m.depth, volume, context.gap_mm);
    m.fits &= m.copies > 0;
    true
}

/// One search owns one mesh and process context. Only byte-identical baked
/// vertices on the same layer schedule share a verdict; yaw equivalence alone
/// is insufficient because raster connectivity depends on pixel alignment.
#[derive(Default)]
struct DirectRasterCache {
    entries: std::sync::Mutex<
        std::collections::HashMap<Vec<u32>, std::sync::Arc<std::sync::OnceLock<bool>>>,
    >,
}

impl DirectRasterCache {
    fn get_or_check(&self, key: Vec<u32>, check: impl FnOnce() -> bool) -> bool {
        let entry = self.entries.lock().unwrap().entry(key).or_default().clone();
        *entry.get_or_init(check)
    }
}

fn direct_raster_clear(
    c: &Candidate,
    mesh: &sc_mesh_core::IndexedMesh,
    context: Context,
    desired_xy: Option<DVec2>,
    cache: Option<&DirectRasterCache>,
) -> bool {
    if context.hardware.is_none() {
        return true;
    }
    let _ = desired_xy;
    let placed = direct_raster_mesh(c, mesh, context);
    if let Some(cache) = cache {
        return cache.get_or_check(direct_raster_key(c, &placed), || {
            direct_raster_clear_placed(c, placed, context)
        });
    }
    direct_raster_clear_placed(c, placed, context)
}

fn direct_raster_mesh(
    c: &Candidate,
    mesh: &sc_mesh_core::IndexedMesh,
    context: Context,
) -> sc_mesh_core::IndexedMesh {
    let hardware = context.hardware.unwrap();
    let volume = hardware.build_volume();
    let centre = (c.min + c.max) * 0.5;
    // Checked at the plate centre like the rest of the scoring, so the verdict
    // does not change with where the model started (R50).
    let xy = DVec2::ZERO;
    let spare = DVec2::new(
        (volume.x_mm() - c.metrics.width).max(0.0) * 0.5,
        (volume.y_mm() - c.metrics.depth).max(0.0) * 0.5,
    );
    let target = xy.clamp(-spare, spare);
    let shift = DVec3::new(target.x - centre.x, target.y - centre.y, -c.min.z);
    let mut placed = mesh.clone();
    for v in &mut placed.vertices {
        let p = c.rotation * DVec3::from_array(v.coords().map(f64::from)) + shift;
        *v = sc_mesh_core::Vertex::new(p.to_array().map(|v| v as f32));
    }
    placed
}

fn direct_raster_key(c: &Candidate, placed: &sc_mesh_core::IndexedMesh) -> Vec<u32> {
    // Include the exact height bits as well as every vertex bit. Height feeds
    // the layer schedule independently of the f32 baked mesh. Faces and all
    // other profile inputs are fixed for the lifetime of this search's cache.
    let height = c.metrics.height.to_bits();
    [height as u32, (height >> 32) as u32]
        .into_iter()
        .chain(
            placed
                .vertices
                .iter()
                .flat_map(|v| v.coords().map(f32::to_bits)),
        )
        .collect()
}

fn direct_raster_clear_placed(
    c: &Candidate,
    placed: sc_mesh_core::IndexedMesh,
    context: Context,
) -> bool {
    let Some(hardware) = context.hardware else {
        return true;
    };
    let mut scene = Scene::new();
    let Ok(id) = scene.add_leaf(
        None,
        Transform3D::Identity,
        sc_scene::LeafPayload::Mesh(std::sync::Arc::new(placed)),
    ) else {
        return false;
    };
    if scene.add_root(id).is_err() {
        return false;
    }
    let layer = context.layer_height_mm;
    let first = if context.first_layer_height_mm > 0.0 {
        context.first_layer_height_mm
    } else {
        layer
    };
    let Ok(layers) = crate::projection::total_layers_for_height(c.metrics.height, layer, first)
    else {
        return false;
    };
    let Ok(layer) = sc_print_primitives::PositiveFiniteMm::new(layer) else {
        return false;
    };
    let Ok(first) = sc_print_primitives::PositiveFiniteMm::new(first) else {
        return false;
    };
    let profile = crate::projection::hardware_profile(&hardware, crate::projection::LayerPlan { layer_height: layer, first_layer_height: first, layers: layers }, context.aa, sc_sl1cer::profile::PackingMode::None)
    .with_accuracy(context.accuracy)
    .with_support_overhang_angle(context.support.overhang_angle_deg);
    let Ok(sc_sl1cer::SliceOutcome::Completed(leaves)) = sc_sl1cer::api::slice_scene_with_fallback(
        &scene,
        &profile,
        &sc_sl1cer::CancelToken::new(),
        |_| {},
        true,
    ) else {
        return false;
    };
    let Ok(stack) =
        sc_sl1cer::contour::merge_stacks(leaves.into_iter().map(|l| l.into_stack()).collect())
    else {
        return false;
    };
    !sc_print_job::has_islands(&stack, &profile)
}

fn horizontal_shift_for_sample(shift: DVec3) -> DVec3 {
    DVec3::new(shift.x, shift.y, 0.0)
}

#[cfg(test)]
#[path = "tests_orient.rs"]
mod tests;

/// Measure an existing baked orientation without running the search. Uses the
/// same deterministic triangle sample and full vertex bounds as the search.
pub fn measure(
    scene: &Scene,
    id: NodeId,
    volume: BuildVolume,
    support_angle_deg: f64,
) -> Option<Metrics> {
    measure_with_context(scene, id, volume, support_angle_deg, Context::default())
}
/// Measure with the same process context supplied to search.
pub fn measure_with_context(
    scene: &Scene,
    id: NodeId,
    volume: BuildVolume,
    support_angle_deg: f64,
    context: Context,
) -> Option<Metrics> {
    let mut mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    // Imported STL meshes may contain collapsed-index faces before the user
    // requests repair. They have no surface and must not disable sampling.
    // Filter only the private analysis copy; never repair the user's scene.
    mesh.faces.retain(|f| {
        let [a, b, c] = f.vertices;
        a != b
            && b != c
            && c != a
            && [a, b, c]
                .iter()
                .all(|i| usize::from(*i) < mesh.vertices.len())
    });
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    if vertices.is_empty() {
        return None;
    }
    let triangles = triangle_sample(&mesh, &vertices);
    let model_volume = enclosed_volume(&mesh, &vertices);
    let access_tree = crate::hollow::Tree::from_mesh(&mesh);
    let mut c = evaluate(
        &vertices,
        &triangles,
        DQuat::IDENTITY,
        volume,
        support_angle_deg.to_radians().cos(),
        false,
    );
    c.metrics.volume_mm3 = model_volume;
    c.metrics.minima = minimum_patches(&vertices, &mesh.faces, DQuat::IDENTITY);
    c.metrics.contact_area += c.metrics.minima as f64 * std::f64::consts::PI * 0.04;
    contextualize(&mut c, volume, context);
    finalize(
        &mut c,
        &mesh,
        &access_tree,
        volume,
        context,
        support_angle_deg,
        None,
        None,
    );
    Some(c.metrics)
}

/// True only for a model already touching the plate with adequate adhesion.
/// Uses every face; no scoring sample can hide an overhang or a minimum island.
pub fn prints_without_supports(mesh: &sc_mesh_core::IndexedMesh, angle: f64) -> bool {
    let min = mesh
        .vertices
        .iter()
        .map(|v| f64::from(v.coords()[2]))
        .fold(f64::INFINITY, f64::min);
    min.abs() <= 0.02
        && direct_metrics(
            mesh,
            DQuat::IDENTITY,
            angle.clamp(0.0, 90.0).to_radians().cos(),
        )
        .is_some()
}

/// A model resting on the plate with a real flat base: it keeps that base on
/// the plate and only its overhangs get supports, instead of being lifted onto
/// a raft. Requires the base to hold the leverage rule of `direct_metrics`
/// (2 mm2 + 0.5 mm2 per mm of height) and to cover a tenth of the XY extent,
/// so a tall part balanced on a small foot is still lifted.
pub fn anchored_on_plate(mesh: &sc_mesh_core::IndexedMesh) -> bool {
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let Some((min, max)) = bounds(vertices.iter().copied()) else {
        return false;
    };
    if min.z.abs() > 0.02 {
        return false;
    }
    let mut bed = 0.0;
    for f in &mesh.faces {
        let [a, b, c] = f.vertices.map(|i| vertices.get(usize::from(i)).copied());
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            return false;
        };
        let n = (b - a).cross(c - a);
        if a.z.max(b.z).max(c.z) <= min.z + 0.02 && n.z < 0.0 {
            bed += -n.z * 0.5;
        }
    }
    let footprint = (max.x - min.x) * (max.y - min.y);
    bed >= 2.0 + 0.5 * (max.z - min.z) && bed >= 0.1 * footprint
}

fn direct_metrics(
    mesh: &sc_mesh_core::IndexedMesh,
    rotation: DQuat,
    threshold: f64,
) -> Option<f64> {
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let (min, max) = bounds(vertices.iter().map(|p| rotation * *p))?;
    let mut bed = 0.0;
    for f in &mesh.faces {
        let [a, b, c] = f
            .vertices
            .map(|i| vertices.get(usize::from(i)).copied())
            .map(|v| v.map(|p| rotation * p));
        let (a, b, c) = (a?, b?, c?);
        let n = (b - a).cross(c - a);
        let length = n.length();
        if !length.is_finite() {
            return None;
        }
        if length <= 1e-12 {
            continue;
        }
        let on_bed = a.z.max(b.z).max(c.z) <= min.z + 0.02;
        if -n.z / length > threshold + 1e-10 && !on_bed {
            return None;
        }
        if on_bed && n.z < 0.0 {
            bed += -n.z * 0.5;
        }
    }
    // Leverage requires half a square millimetre per millimetre of height,
    // plus 2 mm2. A pin balanced on a tiny end cannot qualify.
    if bed < 2.0 + 0.5 * (max.z - min.z) {
        return None;
    }
    if minimum_patches_above(&vertices, &mesh.faces, rotation, min.z) != 0 {
        return None;
    }
    Some(bed)
}

// Same topological criterion as the real support sampler: flood equal-height
// edges, reject any patch with a lower neighbour, and require a downward face.
fn minimum_patches(
    vertices: &[DVec3],
    faces: &[sc_mesh_core::IndexedTriangle],
    rotation: DQuat,
) -> usize {
    minimum_patches_above(vertices, faces, rotation, f64::NEG_INFINITY)
}

fn minimum_patches_above(
    vertices: &[DVec3],
    faces: &[sc_mesh_core::IndexedTriangle],
    rotation: DQuat,
    bed: f64,
) -> usize {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let up = rotation.conjugate() * DVec3::Z;
    let heights: Vec<_> = vertices.iter().map(|v| v.dot(up)).collect();
    let mut parent: Vec<_> = (0..vertices.len()).collect();
    let mut eligible = vec![true; vertices.len()];
    let mut downward = vec![false; vertices.len()];
    for f in faces {
        let ids = f.vertices.map(|i| i.get() as usize);
        let [a, b, c] = ids.map(|i| vertices[i]);
        let down = (b - a).cross(c - a).dot(up) < -1e-12;
        for &i in &ids {
            downward[i] |= down;
            for &j in &ids {
                if heights[j] < heights[i] - 1e-5 {
                    eligible[i] = false;
                } else if i != j && (heights[j] - heights[i]).abs() <= 1e-5 {
                    let a = root(&mut parent, i);
                    let b = root(&mut parent, j);
                    parent[a] = b;
                }
            }
        }
    }
    let mut minima = vec![true; vertices.len()];
    let mut normals = vec![false; vertices.len()];
    for i in 0..vertices.len() {
        let r = root(&mut parent, i);
        minima[r] &= eligible[i];
        normals[r] |= downward[i];
    }
    (0..vertices.len())
        .filter(|&i| parent[i] == i && minima[i] && normals[i] && heights[i] > bed + 0.02)
        .count()
}

// Area-weighted vertex normals provide a geometric detail proxy without a
// semantic surface map. Sharp changes carry a larger contact penalty than a
// smooth patch. The deterministic face sample keeps all score terms bounded.
fn triangle_sample(
    mesh: &sc_mesh_core::IndexedMesh,
    vertices: &[DVec3],
) -> Vec<([DVec3; 3], DVec3, f64)> {
    let mut normals = vec![DVec3::ZERO; vertices.len()];
    for f in &mesh.faces {
        let [Some(a), Some(b), Some(c)] =
            f.vertices.map(|i| vertices.get(i.get() as usize).copied())
        else {
            continue;
        };
        let n = (b - a).cross(c - a);
        for i in f.vertices {
            normals[i.get() as usize] += n;
        }
    }
    let stride = mesh.faces.len().div_ceil(4096).max(1);
    mesh.faces
        .iter()
        .step_by(stride)
        .filter_map(|f| {
            let [a, b, c] = f.vertices.map(|i| vertices.get(i.get() as usize).copied());
            let t = [a?, b?, c?];
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if !n.is_finite() || n.length_squared() <= 1e-16 {
                return None;
            }
            let direction = n.normalize();
            let detail = f
                .vertices
                .iter()
                .map(|i| {
                    1.0 - direction
                        .dot(normals[i.get() as usize].normalize_or_zero())
                        .abs()
                        .clamp(0.0, 1.0)
                })
                .sum::<f64>()
                / 3.0;
            Some((t, n * stride as f64, detail))
        })
        .collect()
}

// Infeasible candidates refine around the smallest envelope violation instead
// of an undefined normalized score when there are no fitting coarse poses.
fn fit_ratio(metrics: Metrics, volume: BuildVolume) -> f64 {
    (metrics.width / volume.x_mm())
        .max(metrics.depth / volume.y_mm())
        .max(metrics.height / volume.z_mm())
}
