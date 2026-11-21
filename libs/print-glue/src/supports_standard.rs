// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! The standard resin support structure (R86), as Lychee, Chitubox and
//! PrusaSlicer's SLA mode build it: a short tip cone aimed along the surface
//! it holds, a ball joint under it, a vertical pillar on a wide foot, and
//! pillars close together merged into one thicker trunk as high as the
//! branch angle allows, so the branches are short and near the part and the
//! trunk carries the load straight down.

use super::{
    BranchPath, COLUMN_GAP_MM, HeightTree, SupportError, SupportPoint, ball, bar_clear,
    closed_rings, merge_meshes,
};
use glam::DVec3;
use sc_mesh_core::{IndexedMesh, Vertex};
use sc_print_profiles::support_settings::SupportSettings;

/// The furthest a tip leans from straight down to follow its surface.
const HEAD_MAX_TILT_DEG: f64 = 45.0;
/// A trunk below a junction is at least this tall (mm); lower contacts keep
/// their own pillars.
const MIN_TRUNK_MM: f64 = 2.0;
/// A junction sits at least this far (mm) below the nodes it joins.
const MIN_DROP_MM: f64 = 0.6;
/// A pillar taller than this many diameters is thickened (as far as the
/// largest column allows).
const PILLAR_MAX_SLENDERNESS: f64 = 35.0;
/// The most contacts one trunk carries: past this a single trunk breaking
/// would drop too much of the part.
const MAX_TREE_CONTACTS: usize = 8;
/// Ring sides of every round part.
const SIDES: u32 = super::ROUND_SIDES;
/// A branch of a tree is this share of the shaft diameter: only the trunk
/// carries the sum, so the crown stays light (Lychee and Chitubox draw the
/// parts near the model thinner than the shaft).
const BRANCH_SHARE: f64 = 0.7;
/// How far sideways (mm) a support may reach to find a clear way down
/// (Lychee's "search for ground" goes to 30 mm).
const ELBOW_REACH_MM: [f64; 14] = [
    1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 15.0, 18.0, 24.0, 30.0,
];
/// Directions tried for the angled arm.
const ELBOW_AZIMUTHS: usize = 16;
/// The pillar under an angled arm is at least this tall (mm).
const ELBOW_MIN_PILLAR_MM: f64 = 1.0;

/// A contact whose tip and pillar are clear all the way to the floor.
pub(super) struct Free {
    /// Index into the caller's contacts.
    pub(super) contact: usize,
    point: DVec3,
    /// Unit axis of the tip, from the surface towards the joint.
    dir: DVec3,
    joint: DVec3,
    /// The top of the pillar: the joint, or the lower end of an angled arm
    /// that carries the tip clear of the part.
    knee: DVec3,
    diameter: f64,
}

/// One finished pillar or tree.
pub(super) struct Built {
    /// The contacts it holds (indices into the caller's contacts).
    pub(super) contacts: Vec<usize>,
    pub(super) mesh: IndexedMesh,
    /// XY, shaft bottom, shaft top, diameter: what bracing needs.
    pub(super) shaft: ([f64; 2], f64, f64, f64),
    pub(super) paths: Vec<BranchPath>,
}

/// The tip for `point` and a clear pillar under it, or `None` when the part
/// is in the way (the caller then routes around it).
#[allow(clippy::too_many_arguments)]
pub(super) fn head(
    solid: &crate::hollow::Tree,
    heights: &HeightTree,
    tris: &[[Vertex; 3]],
    contact: usize,
    point: &SupportPoint,
    wanted: f64,
    thinnest: f64,
    settings: &SupportSettings,
    floor: f64,
    plate: Option<[f64; 2]>,
) -> Option<Free> {
    let p = DVec3::from_array(point.position.map(f64::from));
    let n = DVec3::from_array(point.normal.map(f64::from)).normalize_or_zero();
    let tip_length = super::tip_length_above(settings, p.z - floor);
    let base_height = settings.base_height_mm.min((p.z - floor) / 3.0);
    let bottom = floor + base_height;
    // Along the surface normal, no further from straight down than the limit;
    // then straight down.
    let flat = DVec3::new(n.x, n.y, 0.0);
    let tilt = (-n.z)
        .clamp(-1.0, 1.0)
        .acos()
        .min(HEAD_MAX_TILT_DEG.to_radians());
    let along = if flat.length() < 1e-6 {
        DVec3::NEG_Z
    } else {
        flat.normalize() * tilt.sin() - DVec3::Z * tilt.cos()
    };
    let mut tried = [Some(along), Some(DVec3::NEG_Z)];
    if along.z < -1.0 + 1e-9 {
        tried[1] = None;
    }
    let under = |at: DVec3| heights.any(tris, at.x, at.y, &|h| h > floor + 1e-5 && h < at.z - 1e-3);
    for dir in tried.into_iter().flatten() {
        let joint = p + dir * tip_length;
        // A model under the joint: the pillar would stand on it or run
        // through it.
        if under(joint) {
            continue;
        }
        let mut diameter = wanted;
        loop {
            if tip_clear(solid, p, n, dir, joint, diameter / 2.0)
                && pillar_clear(solid, joint, bottom, diameter / 2.0)
            {
                return Some(Free {
                    contact,
                    point: p,
                    dir,
                    joint,
                    knee: joint,
                    diameter,
                });
            }
            if diameter <= thinnest + 1e-9 {
                break;
            }
            diameter = (diameter * 0.8).max(thinnest);
        }
    }
    // The part is in the way below: an angled arm carries the support
    // sideways to the nearest clear drop, then a pillar goes straight down
    // (the standard "search for ground").
    let arm = branch_diameter(settings).min(wanted) / 2.0;
    let prefer = if flat.length() < 1e-6 {
        0.0
    } else {
        flat.y.atan2(flat.x)
    };
    // At the branch angle, then at the most any member may lean: an arm at
    // 55 or 65 degrees (R87) had an overhanging underside (R91).
    let limit = super::MAX_LEAN_DEG;
    let mut angles = vec![settings.branch_angle_deg.min(limit)];
    if settings.branch_angle_deg < limit {
        angles.push(limit);
    }
    // The tip leaves the surface along the normal or straight down when it
    // can; each is checked once, not for every arm. Both are kept: an arm
    // that cannot leave from one joint may leave from the other (R89).
    let fixed: Vec<(DVec3, DVec3)> = [along, DVec3::NEG_Z]
        .into_iter()
        .filter(|dir| dir.dot(n) >= 0.15)
        .map(|dir| (dir, p + dir * tip_length))
        .filter(|&(dir, joint)| tip_clear(solid, p, n, dir, joint, arm))
        .collect();
    for angle in angles {
        let tan = angle.to_radians().tan();
        let longest = ELBOW_REACH_MM[ELBOW_REACH_MM.len() - 1] / angle.to_radians().sin();
        // How far each arm direction stays clear from each fixed joint: a
        // longer arm the same way is the shorter one extended, so it is
        // marched once per direction instead of once per reach.
        let mut clear_to = [[f64::NAN; ELBOW_AZIMUTHS]; 2];
        for reach in ELBOW_REACH_MM {
            let drop = reach / tan;
            if p.z - tip_length - drop < bottom + ELBOW_MIN_PILLAR_MM {
                break;
            }
            for step in 0..ELBOW_AZIMUTHS {
                // Fan out from the way the surface faces: 0, +1, -1, +2, ...
                let turn = (step as f64 / 2.0).ceil() * if step % 2 == 1 { 1.0 } else { -1.0 };
                let az = prefer + turn * std::f64::consts::TAU / ELBOW_AZIMUTHS as f64;
                let h = DVec3::new(az.cos(), az.sin(), 0.0);
                let way = (h * tan - DVec3::Z).normalize();
                // Without a clear tip along the normal or straight down, the
                // tip itself leans the way the arm goes.
                let toward = (h - DVec3::Z).normalize();
                let tips: Vec<(DVec3, DVec3, Option<usize>)> = if fixed.is_empty() {
                    if toward.dot(n) < 0.15 {
                        continue;
                    }
                    vec![(toward, p + toward * tip_length, None)]
                } else {
                    fixed
                        .iter()
                        .enumerate()
                        .map(|(k, &(d, j))| (d, j, Some(k)))
                        .collect()
                };
                for (dir, joint, k) in tips {
                    let knee = joint + h * reach - DVec3::Z * drop;
                    // Cheapest tests first: off the plate, or a part under
                    // the knee (a contact in a pocket fails every arm here,
                    // without marching one, R90).
                    if let Some(half) = plate
                        && (knee.x.abs() > half[0] - settings.base_diameter_mm / 2.0
                            || knee.y.abs() > half[1] - settings.base_diameter_mm / 2.0)
                    {
                        continue;
                    }
                    if under(knee) {
                        continue;
                    }
                    if let Some(k) = k {
                        if clear_to[k][step].is_nan() {
                            clear_to[k][step] = clear_length(solid, joint, way, longest, arm);
                        }
                        if (knee - joint).length() > clear_to[k][step] {
                            continue;
                        }
                    }
                    // Then the short arm, which fails within a few samples
                    // for a contact boxed in by the part; the long pillar
                    // last. The ball rounding the knee is as wide as the
                    // pillar, all round it, so it keeps the pillar's gap too
                    // (R89).
                    if !leg_clear(solid, joint, knee, arm)
                        || solid.near(knee, wanted / 2.0 + COLUMN_GAP_MM)
                        || !pillar_clear(solid, knee, bottom, wanted / 2.0)
                        || (k.is_none() && !tip_clear(solid, p, n, dir, joint, arm))
                    {
                        continue;
                    }
                    return Some(Free {
                        contact,
                        point: p,
                        dir,
                        joint,
                        knee,
                        diameter: wanted,
                    });
                }
            }
        }
    }
    None
}

/// How far a bar of `radius` from `from` along unit `dir` stays clear of the
/// models, up to `max`, with the same clearances as [`leg_clear`].
fn clear_length(
    solid: &crate::hollow::Tree,
    from: DVec3,
    dir: DVec3,
    max: f64,
    radius: f64,
) -> f64 {
    let step = (radius * 0.45).max(0.05);
    let mut t = 0.0;
    while t <= max {
        let gap = if t < 2.0 * radius {
            radius * 0.9
        } else {
            radius + COLUMN_GAP_MM
        };
        if solid.near(from + dir * t, gap) {
            return (t - step).max(0.0);
        }
        t += step;
    }
    f64::INFINITY
}

/// The diameter of a tree's branches and of angled arms.
fn branch_diameter(settings: &SupportSettings) -> f64 {
    (settings.shaft_diameter_mm * BRANCH_SHARE).max(settings.tip_contact_diameter_mm * 1.5)
}

/// Whether a tip from `p` along `dir` keeps clear of the models. Next to the
/// surface the tip holds, the clearance grows with the distance from it, as
/// the tip itself does.
fn tip_clear(
    solid: &crate::hollow::Tree,
    p: DVec3,
    n: DVec3,
    dir: DVec3,
    joint: DVec3,
    radius: f64,
) -> bool {
    let away = dir.dot(n).max(0.2);
    let length = (joint - p).length();
    let mut s = length * 0.5;
    while s <= length + 1e-9 {
        if solid.near(p + dir * s, (0.9 * s * away).min(radius + COLUMN_GAP_MM)) {
            return false;
        }
        s += 0.1;
    }
    true
}

/// Whether the pillar from `top` down to `bottom` keeps clear of the models.
/// Just under its top it is as close to the held surface as the tip was;
/// below that it keeps the full gap.
fn pillar_clear(solid: &crate::hollow::Tree, top: DVec3, bottom: f64, radius: f64) -> bool {
    let full = top.z - 2.0 * radius;
    (full <= bottom
        || bar_clear(
            solid,
            [top.x, top.y, bottom],
            [top.x, top.y, full],
            radius + COLUMN_GAP_MM,
        ))
        && bar_clear(
            solid,
            [top.x, top.y, full.max(bottom)],
            top.to_array(),
            radius * 0.9,
        )
}

struct Node {
    pos: DVec3,
    diameter: f64,
    children: Vec<usize>,
    /// Contacts held at or above this node.
    contacts: usize,
    /// The contact a leaf holds.
    leaf: Option<usize>,
    alive: bool,
}

impl Node {
    /// Its diameter as a branch of a bigger tree: a leaf slims to the branch
    /// diameter, a junction keeps its own.
    fn limb(&self, branch: f64) -> f64 {
        if self.leaf.is_some() {
            branch.min(self.diameter)
        } else {
            self.diameter
        }
    }
}

/// A candidate junction of nodes `a` and `b`, the highest first.
struct Merge {
    z: f64,
    a: usize,
    b: usize,
    at: DVec3,
    diameter: f64,
}

impl PartialEq for Merge {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Merge {}
impl PartialOrd for Merge {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Merge {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.z
            .total_cmp(&other.z)
            .then_with(|| (other.a, other.b).cmp(&(self.a, self.b)))
    }
}

/// Where nodes `a` and `b` can join: the highest point both reach within the
/// branch angle and reach, with a trunk under it. Not yet checked against
/// the models.
fn junction(
    nodes: &[Node],
    a: usize,
    b: usize,
    settings: &SupportSettings,
    bottom: f64,
) -> Option<Merge> {
    let (na, nb) = (&nodes[a], &nodes[b]);
    let reach = settings.spacing_mm * 1.25;
    let span = (nb.pos - na.pos).truncate().length();
    if span < 0.05 || span > 2.0 * reach {
        return None;
    }
    if na.contacts + nb.contacts > MAX_TREE_CONTACTS {
        return None;
    }
    // The trunk grows with the contacts it carries, from the shaft diameter
    // to the largest column. Its tips are far weaker than it is (eight
    // 0.4 mm tips have less section than one 1.2 mm pillar), so adding the
    // branches' whole sections only spent resin: 20% more on the prepare
    // benchmark.
    let branch = branch_diameter(settings);
    let carried = (na.contacts + nb.contacts) as f64;
    let diameter = (settings.shaft_diameter_mm * (1.0 + 0.15 * (carried - 1.0)))
        .min(settings.max_column_diameter_mm)
        .max(na.limb(branch))
        .max(nb.limb(branch));
    let tan = settings
        .branch_angle_deg
        .min(super::MAX_LEAN_DEG)
        .to_radians()
        .tan();
    // The point between them that both reach at the limit angle.
    let from_a = ((span + (na.pos.z - nb.pos.z) * tan) / 2.0).clamp(0.0, span);
    let from_b = span - from_a;
    if from_a > reach || from_b > reach {
        return None;
    }
    let z = (na.pos.z - from_a / tan)
        .min(nb.pos.z - from_b / tan)
        .min(na.pos.z.min(nb.pos.z) - MIN_DROP_MM);
    if z - bottom < MIN_TRUNK_MM {
        return None;
    }
    let xy = na.pos.truncate() + (nb.pos - na.pos).truncate() * (from_a / span);
    Some(Merge {
        z,
        a,
        b,
        at: xy.extend(z),
        diameter,
    })
}

/// A branch from `from` down to `to`: clear of the models, a little closer
/// where it leaves the joint under the part.
fn leg_clear(solid: &crate::hollow::Tree, from: DVec3, to: DVec3, radius: f64) -> bool {
    let length = (to - from).length();
    let near = from + (to - from) * (2.0 * radius / length).min(1.0);
    bar_clear(solid, from.to_array(), near.to_array(), radius * 0.9)
        && bar_clear(
            solid,
            near.to_array(),
            to.to_array(),
            radius + COLUMN_GAP_MM,
        )
}

/// Pillars and trees for the free contacts. With trees off every contact
/// keeps its own pillar.
pub(super) fn grow(
    free: &[Free],
    solid: &crate::hollow::Tree,
    settings: &SupportSettings,
    floor: f64,
) -> Result<Vec<Built>, SupportError> {
    let bottom = floor + settings.base_height_mm;
    let branch = branch_diameter(settings);
    let mut nodes: Vec<Node> = free
        .iter()
        .enumerate()
        .map(|(i, f)| Node {
            pos: f.knee,
            diameter: f.diameter,
            children: Vec::new(),
            contacts: 1,
            leaf: Some(i),
            alive: true,
        })
        .collect();
    if settings.tree_supports {
        let mut heap = std::collections::BinaryHeap::new();
        for a in 0..nodes.len() {
            for b in a + 1..nodes.len() {
                heap.extend(junction(&nodes, a, b, settings, bottom));
            }
        }
        while let Some(merge) = heap.pop() {
            if !nodes[merge.a].alive || !nodes[merge.b].alive {
                continue;
            }
            if [merge.a, merge.b]
                .iter()
                .any(|&i| !leg_clear(solid, nodes[i].pos, merge.at, nodes[i].limb(branch) / 2.0))
            {
                continue;
            }
            // Beside a wall the trunk slims to fit, never thinner than its
            // thickest branch.
            let thinnest = nodes[merge.a].limb(branch).max(nodes[merge.b].limb(branch));
            let trunk_foot = [merge.at.x, merge.at.y, bottom];
            let mut diameter = merge.diameter;
            let fits = loop {
                if bar_clear(
                    solid,
                    trunk_foot,
                    merge.at.to_array(),
                    diameter / 2.0 + COLUMN_GAP_MM,
                ) {
                    break true;
                }
                if diameter <= thinnest + 1e-9 {
                    break false;
                }
                diameter = (diameter * 0.8).max(thinnest);
            };
            if !fits {
                continue;
            }
            nodes[merge.a].alive = false;
            nodes[merge.b].alive = false;
            let new = nodes.len();
            nodes.push(Node {
                pos: merge.at,
                diameter,
                children: vec![merge.a, merge.b],
                contacts: nodes[merge.a].contacts + nodes[merge.b].contacts,
                leaf: None,
                alive: true,
            });
            for other in 0..new {
                if nodes[other].alive {
                    heap.extend(junction(&nodes, other, new, settings, bottom));
                }
            }
        }
    }
    let mut built = Vec::new();
    for root in 0..nodes.len() {
        if !nodes[root].alive {
            continue;
        }
        let mut parts = Vec::new();
        let mut contacts = Vec::new();
        let mut paths = Vec::new();
        let mut stack = vec![root];
        while let Some(i) = stack.pop() {
            let node = &nodes[i];
            // A pillar of its own keeps its full width up to the tip; as a
            // branch it is slim.
            let radius = if i == root {
                node.diameter / 2.0
            } else {
                node.limb(branch) / 2.0
            };
            if let Some(leaf) = node.leaf {
                let f = &free[leaf];
                contacts.push(f.contact);
                let tip_length = (f.joint - f.point).length();
                let elbow = (f.knee - f.joint).length() > 1e-6;
                // Under the tip: the pillar's width, or the arm's when an
                // arm carries it sideways first.
                let neck = if elbow {
                    radius.min(branch / 2.0)
                } else {
                    radius
                };
                // The cone, its ball and an arm leaving it meet at one radius
                // (R104: the cone ended wider than its ball, a lip). The
                // ball still stays clear of the surface the tip holds (a
                // 3 mm pillar's ball reached into the part).
                let end = super::tip_end_radius(settings, tip_length, neck).min(tip_length * 0.6);
                parts.push(tube(
                    f.point - f.dir * settings.contact_depth_mm,
                    settings.tip_contact_diameter_mm / 2.0,
                    f.joint,
                    end,
                )?);
                parts.push(ball(f.joint.to_array(), end)?);
                if elbow {
                    parts.push(tube(f.joint, end, f.knee, neck)?);
                    // The arm is a member like a branch (R91).
                    let delta = f.joint - f.knee;
                    paths.push(BranchPath {
                        length_mm: delta.length(),
                        rise_mm: delta.z.abs(),
                        inclination_deg: delta
                            .truncate()
                            .length()
                            .atan2(delta.z.abs())
                            .to_degrees(),
                    });
                    parts.push(ball(f.knee.to_array(), radius)?);
                }
            } else {
                // Where branches meet, the ball is a little wider than the
                // trunk, so the joint is not its weak point (R84).
                parts.push(ball(node.pos.to_array(), radius * 1.15)?);
            }
            for &child in &node.children {
                let from = nodes[child].pos;
                let limb = nodes[child].limb(branch) / 2.0;
                parts.push(tube(node.pos, limb, from, limb)?);
                let delta = from - node.pos;
                paths.push(BranchPath {
                    length_mm: delta.length(),
                    rise_mm: delta.z.abs(),
                    inclination_deg: delta.truncate().length().atan2(delta.z.abs()).to_degrees(),
                });
                stack.push(child);
            }
        }
        // The pillar: thickened when it is slender, where that still clears
        // the models; its foot narrows beside a part standing on the plate.
        let top = nodes[root].pos;
        let base_height = settings.base_height_mm.min((top.z - floor) / 2.0);
        let foot_top = floor + base_height;
        let mut radius = nodes[root].diameter / 2.0;
        let sturdy = ((top.z - foot_top) / PILLAR_MAX_SLENDERNESS / 2.0)
            .min(settings.max_column_diameter_mm / 2.0);
        if sturdy > radius
            && bar_clear(
                solid,
                [top.x, top.y, foot_top],
                [top.x, top.y, top.z - 2.0 * sturdy],
                sturdy + COLUMN_GAP_MM,
            )
        {
            radius = sturdy;
        }
        let mut base = (settings.base_diameter_mm / 2.0).max(radius);
        while base > radius + 1e-9
            && base_height > 0.02
            && !bar_clear(
                solid,
                [top.x, top.y, floor + 0.01],
                [top.x, top.y, foot_top],
                base + COLUMN_GAP_MM,
            )
        {
            base = (base * 0.8).max(radius);
        }
        // A thickened pillar narrows back to its own width under the joint.
        let own = nodes[root].diameter / 2.0;
        let taper = (top.z - 2.0 * (radius - own)).max(foot_top);
        let mut rings = vec![(floor, base), (foot_top, radius)];
        if radius > own + 1e-9 && taper > foot_top + 1e-6 {
            rings.push((taper, radius));
        }
        rings.push((top.z, if taper > foot_top + 1e-6 { own } else { radius }));
        let vertices = rings
            .iter()
            .flat_map(|&(z, r)| {
                (0..SIDES).map(move |i| {
                    let a = f64::from(i) * std::f64::consts::TAU / f64::from(SIDES);
                    Vertex::new([
                        (top.x + r * a.cos()) as f32,
                        (top.y + r * a.sin()) as f32,
                        z as f32,
                    ])
                })
            })
            .collect();
        // The pillar first: its lowest ring is the foot the raft is laid under.
        parts.insert(0, closed_rings(vertices, SIDES)?);
        contacts.sort_unstable();
        built.push(Built {
            contacts,
            mesh: merge_meshes(parts.iter()).ok_or(SupportError::UnrepresentableGeometry)?,
            shaft: ([top.x, top.y], foot_top, top.z, radius * 2.0),
            paths,
        });
    }
    Ok(built)
}

/// A closed round bar from `from` (radius `r0`) to `to` (radius `r1`).
fn tube(from: DVec3, r0: f64, to: DVec3, r1: f64) -> Result<IndexedMesh, SupportError> {
    let sides = super::sides_for(r0.max(r1));
    let w = (to - from).normalize_or_zero();
    if w == DVec3::ZERO {
        return Err(SupportError::UnrepresentableGeometry);
    }
    let helper = if w.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    // u x v = w: each ring winds counter-clockwise about the axis, as the
    // vertical pillars' rings do about +Z.
    let u = helper.cross(w).normalize();
    let v = w.cross(u);
    let vertices = [(from, r0), (to, r1)]
        .iter()
        .flat_map(|&(c, r)| {
            (0..sides).map(move |i| {
                let (s, k) = (f64::from(i) * std::f64::consts::TAU / f64::from(sides)).sin_cos();
                let p = c + (u * k + v * s) * r;
                Vertex::new([p.x as f32, p.y as f32, p.z as f32])
            })
        })
        .collect();
    closed_rings(vertices, sides)
}
