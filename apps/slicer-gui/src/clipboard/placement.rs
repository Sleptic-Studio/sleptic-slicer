// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Where a pasted copy lands: the most compact free spot on its plate.
//! Sleptic: "ctrl c then ctrl v ... the new pasted part just ends up going
//! in a random position usually way off the build plate. It should auto
//! position every paste to be a spot that's the most compact." The old
//! search centred the copy on the cursor's projection onto the plate
//! (anywhere at a grazing view, or from a stale hover over a side
//! panel), never checked the plate bounds, and gave up overlapping. Orca (`Selection::paste_objects_from_clipboard` ->
//! `GLCanvas3D::get_nearest_empty_cell`) starts from the copied
//! object when it is on the current plate (else the plate centre) and
//! takes the nearest free cell on the plate. This search goes one step
//! further toward "most compact": every snug position (against a plate
//! edge, or one arrange gap from an existing part, aligned with or
//! beside it) that lies on the plate and clears every part is scored
//! by the box around the plate's parts with the copy added: its longer
//! side first (smallest area alone grows a long strip, three in a row
//! beating a 2 x 2 block), then its area, then nearness to the anchor.
//! The algorithm is deterministic and side-effect-free.

use sc_mesh_core::Aabb;
use sc_mesh_spatial::SceneSpatial;

/// Pick the translation that puts a fragment in the most compact free
/// spot on a plate. `copy_box` bounds the copy in its own frame;
/// `anchor` is the plate-local XY preferred on ties (the copied parts'
/// centre, a right-clicked point, or the plate centre); the plate spans
/// `-half..+half` in its own coordinates; `gap` is the clearance kept to
/// every part (the arrange gap); `occupied` bounds the parts already on
/// the plate, plate-local. The copy's floor lands at `Z = 0`; with no
/// free spot it is centred on the anchor, clamped onto the plate.
pub(crate) fn compact_paste_translation(
    copy_box: &Aabb,
    anchor: [f64; 2],
    half: [f64; 2],
    gap: f64,
    occupied: &[Aabb],
) -> [f64; 3] {
    let (fmin, fmax) = placement_bounds_as_f64(copy_box);
    let size = [fmax[0] - fmin[0], fmax[1] - fmin[1]];
    let gap = if gap.is_finite() { gap.max(0.0) } else { 3.0 };
    let rects: Vec<([f64; 2], [f64; 2])> = occupied
        .iter()
        .filter(|o| !o.is_empty())
        .map(|o| {
            let (lo, hi) = placement_bounds_as_f64(o);
            ([lo[0], lo[1]], [hi[0], hi[1]])
        })
        .collect();
    // Footprint of the parts already there; the copy's score is the
    // area of this box grown to include it.
    let used = rects.iter().fold(None, |acc: Option<([f64; 2], [f64; 2])>, r| {
        Some(match acc {
            Some((lo, hi)) => (
                [lo[0].min(r.0[0]), lo[1].min(r.0[1])],
                [hi[0].max(r.1[0]), hi[1].max(r.1[1])],
            ),
            None => *r,
        })
    });
    // Candidate positions of the copy's min corner, per axis.
    let axis = |k: usize| {
        let lo = -half[k];
        let hi = half[k] - size[k];
        let mut v = vec![lo, hi, anchor[k] - 0.5 * size[k]];
        for r in &rects {
            v.extend([r.1[k] + gap, r.0[k] - gap - size[k], r.0[k], r.1[k] - size[k]]);
        }
        let mut v: Vec<f64> = v
            .into_iter()
            .filter(|x| x.is_finite())
            .map(|x| if hi < lo { lo } else { x.clamp(lo, hi) })
            .collect();
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        v
    };
    let (xs, ys) = (axis(0), axis(1));
    let fits_plate = size[0] <= 2.0 * half[0] + 1e-6 && size[1] <= 2.0 * half[1] + 1e-6;
    let mut best: Option<((i64, i64, f64), [f64; 2])> = None;
    if fits_plate {
        for &x in &xs {
            for &y in &ys {
                let (lo, hi) = ([x, y], [x + size[0], y + size[1]]);
                // Touching at exactly one gap is free (snug fit).
                let blocked = rects.iter().any(|r| {
                    (0..2).all(|k| lo[k] < r.1[k] + gap - 1e-6 && r.0[k] - gap + 1e-6 < hi[k])
                });
                if blocked {
                    continue;
                }
                let (w, d) = match used {
                    Some((ulo, uhi)) => (
                        uhi[0].max(hi[0]) - ulo[0].min(lo[0]),
                        uhi[1].max(hi[1]) - ulo[1].min(lo[1]),
                    ),
                    None => (0.0, 0.0),
                };
                let centre = [x + 0.5 * size[0], y + 0.5 * size[1]];
                let distance = (centre[0] - anchor[0]).hypot(centre[1] - anchor[1]);
                // Sides within 0.01 mm and areas within 0.01 mm^2 tie,
                // so float noise never outranks the anchor distance.
                let key = (
                    (w.max(d) * 100.0).round() as i64,
                    (w * d * 100.0).round() as i64,
                    distance,
                );
                if best.is_none_or(|(b, _)| {
                    (key.0, key.1) < (b.0, b.1) || ((key.0, key.1) == (b.0, b.1) && key.2 < b.2)
                }) {
                    best = Some((key, [x, y]));
                }
            }
        }
    }
    let [x, y] = best.map_or_else(
        || {
            let clamp = |k: usize| {
                let (lo, hi) = (-half[k], half[k] - size[k]);
                let x = anchor[k] - 0.5 * size[k];
                if hi < lo { lo } else { x.clamp(lo, hi) }
            };
            [clamp(0), clamp(1)]
        },
        |(_, p)| p,
    );
    [x - fmin[0], y - fmin[1], -fmin[2]]
}
/// Bounds of every leaf in the scene, whatever plate it is on; leaves
/// without vertices have no box and are left out. The app asks
/// [`obstacle_aabbs_on_plate`]; tests use this to see where pastes landed.
#[cfg(test)]
pub(crate) fn collect_scene_placement_bounds(spatial: &SceneSpatial) -> Vec<Aabb> {
    spatial
        .leaf_ids()
        .into_iter()
        .filter_map(|id| spatial.leaf(id))
        .filter_map(|leaf| leaf.transformed_aabb(leaf.world_transform()))
        .collect()
}

/// The boxes the paste search must avoid on `plate`. Leaves are stored in
/// plate-local coordinates, so objects on other plates
/// overlap this plate's local space and must not block a placement here.
/// Supports are merged into their model's box: one obstacle per part
/// keeps the paste search small on a plate full of supported models.
pub(crate) fn obstacle_aabbs_on_plate(
    spatial: &SceneSpatial,
    plate: sc_scene::plates::PlateId,
) -> Vec<Aabb> {
    let mut parts: std::collections::BTreeMap<sc_scene::NodeId, Aabb> =
        std::collections::BTreeMap::new();
    for id in spatial.leaf_ids() {
        if spatial.graph().plate_of(id) != plate {
            continue;
        }
        let Some(aabb) = spatial
            .leaf(id)
            .and_then(|q| q.transformed_aabb(q.world_transform()))
        else {
            continue;
        };
        let owner = crate::scene_document::support_owner(spatial.graph(), id).unwrap_or(id);
        parts
            .entry(owner)
            .and_modify(|b| b.union(&aabb))
            .or_insert(aabb);
    }
    parts.into_values().collect()
}


/// The box's corners widened to f64, the precision the search and the
/// scene's transforms work in.
fn placement_bounds_as_f64(b: &Aabb) -> ([f64; 3], [f64; 3]) {
    let wide = |corner: [f32; 3]| corner.map(f64::from);
    (wide(b.min().coords()), wide(b.max().coords()))
}

#[cfg(test)]
mod paste_spot_tests {
    use std::sync::Arc;

    use sc_mesh_core::IndexedMesh;
    use sc_mesh_core::primitives::{IndexedTriangle, Normal, Vertex};
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::Scene;
    use sc_scene::node::LeafPayload;

    use super::*;

    /// Plate half extents shared by the placement tests (a 200 x 120 plate).
    const HALF: [f64; 2] = [100.0, 60.0];

    fn boxed(lo: [f32; 3], hi: [f32; 3]) -> Aabb {
        Aabb::from_min_max(Vertex::new(lo), Vertex::new(hi))
    }

    /// A `w` x `d` part, 2 mm tall, centred on its own origin.
    fn part(w: f32, d: f32) -> Aabb {
        boxed([-w / 2.0, -d / 2.0, 0.0], [w / 2.0, d / 2.0, 2.0])
    }

    /// The copy's XY footprint once moved by `t`.
    fn placed(copy: &Aabb, t: [f64; 3]) -> ([f64; 2], [f64; 2]) {
        let (lo, hi) = placement_bounds_as_f64(copy);
        ([lo[0] + t[0], lo[1] + t[1]], [hi[0] + t[0], hi[1] + t[1]])
    }

    #[test]
    fn empty_plate_centres_the_copy_on_the_anchor() {
        let moved = compact_paste_translation(&part(2.0, 2.0), [5.0, 7.0], HALF, 3.0, &[]);
        assert_eq!(moved, [5.0, 7.0, 0.0], "no parts: the anchor wins outright");
    }

    #[test]
    fn the_copy_always_rests_on_the_plate_floor() {
        let floating = boxed([-1.0, -1.0, 7.5], [1.0, 1.0, 10.0]);
        assert_eq!(compact_paste_translation(&floating, [0.0, 0.0], HALF, 3.0, &[])[2], -7.5);
        let sunk = boxed([-1.0, -1.0, -3.0], [1.0, 1.0, 4.0]);
        assert_eq!(compact_paste_translation(&sunk, [0.0, 0.0], HALF, 3.0, &[])[2], 3.0);
    }

    /// Sleptic: pastes went "way off the build plate". An anchor far off
    /// the plate (a grazing cursor ray) still lands the copy on it.
    #[test]
    fn an_off_plate_anchor_still_lands_on_the_plate() {
        let copy = part(20.0, 20.0);
        let t = compact_paste_translation(&copy, [900.0, -700.0], HALF, 3.0, &[]);
        let (lo, hi) = placed(&copy, t);
        for k in 0..2 {
            assert!(-HALF[k] <= lo[k] && hi[k] <= HALF[k], "axis {k}: {lo:?} {hi:?}");
        }
    }

    /// A copy of a wide, shallow part goes beside it in Y (less added
    /// area than beside it in X), exactly one gap away.
    #[test]
    fn a_copy_lands_snug_beside_the_original() {
        let copy = part(40.0, 10.0);
        let original = boxed([-20.0, -5.0, 0.0], [20.0, 5.0, 2.0]);
        let t = compact_paste_translation(&copy, [0.0, 0.0], HALF, 3.0, &[original]);
        let (lo, hi) = placed(&copy, t);
        assert_eq!(lo[0], -20.0);
        let above = (lo[1] - 8.0).abs() < 1e-9;
        let below = (hi[1] + 8.0).abs() < 1e-9;
        assert!(above || below, "{lo:?} {hi:?}");
    }

    /// Eight pastes after one part: each stays on the plate, keeps the gap
    /// to every part, and the nine end up a block rather than a scatter.
    #[test]
    fn repeated_pastes_pack_into_a_compact_block() {
        const GAP: f64 = 3.0;
        let copy = part(20.0, 20.0);
        let mut on_plate = vec![part(20.0, 20.0)];
        for _ in 0..8 {
            let t = compact_paste_translation(&copy, [0.0, 0.0], HALF, GAP, &on_plate);
            let (lo, hi) = placed(&copy, t);
            for k in 0..2 {
                assert!(lo[k] >= -HALF[k] - 1e-9 && hi[k] <= HALF[k] + 1e-9, "off the plate");
            }
            for other in &on_plate {
                let (olo, ohi) = placement_bounds_as_f64(other);
                let clear = (0..2)
                    .any(|k| hi[k] <= olo[k] - GAP + 1e-9 || lo[k] >= ohi[k] + GAP - 1e-9);
                assert!(clear, "within the gap of a part: {lo:?} {hi:?} vs {olo:?} {ohi:?}");
            }
            on_plate.push(boxed([lo[0] as f32, lo[1] as f32, 0.0], [hi[0] as f32, hi[1] as f32, 2.0]));
        }
        let mut all = Aabb::empty();
        for p in &on_plate {
            all.union(p);
        }
        let (lo, hi) = placement_bounds_as_f64(&all);
        let (w, d) = (hi[0] - lo[0], hi[1] - lo[1]);
        // A 3 x 3 block of 20 mm parts with 3 mm gaps is 66 mm square.
        assert!(w * d <= 66.0 * 66.0 + 1e-6, "footprint {w} x {d}");
    }

    #[test]
    fn a_full_plate_falls_back_onto_the_plate() {
        let copy = part(20.0, 20.0);
        let wall_to_wall = boxed([-100.0, -60.0, 0.0], [100.0, 60.0, 5.0]);
        let t = compact_paste_translation(&copy, [500.0, 0.0], HALF, 3.0, &[wall_to_wall]);
        let (lo, hi) = placed(&copy, t);
        assert_eq!((lo[0], hi[0]), (80.0, 100.0));
        assert_eq!((lo[1], hi[1]), (-10.0, 10.0));
    }

    #[test]
    fn an_empty_box_blocks_nothing() {
        let moved =
            compact_paste_translation(&part(2.0, 2.0), [0.0, 0.0], HALF, 3.0, &[Aabb::empty()]);
        assert_eq!(moved, [0.0; 3], "the empty sentinel must not push the copy away");
    }

    /// A unit cube's eight corners (one triangle is enough for a box; the
    /// corners make the expected bounds obvious).
    fn unit_cube() -> Arc<IndexedMesh> {
        let corner = |i: usize| {
            Vertex::new([0, 1, 2].map(|axis| if (i >> axis) & 1 == 1 { 0.5 } else { -0.5 }))
        };
        let face = IndexedTriangle::from_raw_indices(Normal::new([0.0, 0.0, 1.0]), [0, 1, 2]);
        Arc::new(IndexedMesh::new(None, (0..8).map(corner).collect(), vec![face]))
    }

    fn no_vertices() -> Arc<IndexedMesh> {
        Arc::new(IndexedMesh::new(None, vec![], vec![]))
    }

    /// One root group with a leaf per mesh, each shifted along X.
    fn scene_with(leaves: &[(f64, Arc<IndexedMesh>)]) -> SceneSpatial {
        let mut scene = Scene::new();
        let group = scene.add_group(None, Transform3D::Identity);
        scene.add_root(group).expect("root group");
        for (x, mesh) in leaves {
            let shift = Transform3D::Affine(AffineTransform3D::from_translation([*x, 0.0, 0.0]));
            let leaf = scene
                .add_leaf(None, shift, LeafPayload::Mesh(Arc::clone(mesh)))
                .expect("leaf");
            scene.add_child(group, leaf).expect("child");
        }
        SceneSpatial::primed_graph(scene)
    }

    fn min_xs(boxes: &[Aabb]) -> Vec<f32> {
        let mut xs: Vec<f32> = boxes.iter().map(|b| b.min().coords()[0]).collect();
        xs.sort_by(f32::total_cmp);
        xs
    }

    #[test]
    fn a_scene_without_leaves_has_no_bounds() {
        assert!(collect_scene_placement_bounds(&scene_with(&[])).is_empty());
    }

    #[test]
    fn every_leaf_contributes_its_moved_box() {
        let cube = unit_cube();
        let spatial = scene_with(&[(0.0, cube.clone()), (10.0, cube.clone()), (20.0, cube)]);
        assert_eq!(min_xs(&collect_scene_placement_bounds(&spatial)), [-0.5, 9.5, 19.5]);
    }

    #[test]
    fn leaves_without_vertices_have_no_box() {
        let spatial = scene_with(&[(0.0, unit_cube()), (0.0, no_vertices())]);
        assert_eq!(collect_scene_placement_bounds(&spatial).len(), 1);
    }

    /// What the paste actually avoids: the boxes on the target plate.
    #[test]
    fn plate_obstacles_are_the_leaves_on_that_plate() {
        let spatial = scene_with(&[(0.0, unit_cube()), (10.0, unit_cube()), (5.0, no_vertices())]);
        let plate = spatial.graph().plate_of(spatial.leaf_ids()[0]);
        assert_eq!(min_xs(&obstacle_aabbs_on_plate(&spatial, plate)), [-0.5, 9.5]);
    }
}
