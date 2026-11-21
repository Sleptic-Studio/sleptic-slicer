// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Placement checks for frontend slice results, before the plate is merged.
use sc_scene::{Scene, error::SceneError};
use sc_sl1cer::{LeafSlices, profile::SliceProfile};
/// Inspect model bounds and vector slice interiors, ignoring intentional supports.
pub fn scene_placement_risks(
    scene: &Scene,
    leaves: &[LeafSlices],
    profile: &SliceProfile,
) -> Result<Vec<sc_print_job::PrintRisk>, SceneError> {
    let mut models = Vec::new();
    for leaf in leaves {
        if scene
            .node(leaf.leaf())
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .is_some_and(|s| s.starts_with("sc-generated-support"))
        {
            continue;
        }
        let mesh = sc_scene::bake::bake_leaf_to_world(scene, leaf.leaf())?;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &mesh.vertices {
            let xyz = v.coords();
            for a in 0..3 {
                lo[a] = lo[a].min(f64::from(xyz[a]));
                hi[a] = hi[a].max(f64::from(xyz[a]));
            }
        }
        if !mesh.vertices.is_empty() {
            models.push(sc_print_job::risks::PlacedModel {
                min_mm: lo,
                max_mm: hi,
                stack: leaf.stack(),
            });
        }
    }
    Ok(sc_print_job::risks::placement_risks(&models, profile))
}
