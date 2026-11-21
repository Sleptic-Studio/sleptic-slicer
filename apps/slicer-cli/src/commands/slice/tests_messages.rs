// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::indexed_mesh::{MeshFaceError, MeshFix, RepairRefusal};
use sc_mesh_core::primitives::{FaceIndex, TriVertexSlot, VertexIndex};
use sc_mesh_core::transform::Transform3D;
use sc_mesh_formats::messages::LoadThreeMfWarning;
use sc_print_job::{Island, PrintRisk, RiskKind, Severity};
use sc_project_formats::{ForeignDialect, ReadProjectWarning};
use sc_scene::mesh_targets::MeshTarget;
use sc_scene::repair::SceneRepairOutcome;
use sc_scene::{NodeId, Scene};
use sc_sl1cer::error::{InvalidLeafGeometry, LeafValidationFailure, SliceSceneError};

use super::*;

fn printed(write: impl FnOnce(&mut Vec<u8>)) -> String {
    let mut bytes = Vec::new();
    write(&mut bytes);
    String::from_utf8(bytes).unwrap()
}

fn node() -> NodeId {
    Scene::new().add_group(None, Transform3D::Identity)
}

fn island(layer: usize, area_mm2: f64) -> Island {
    Island {
        layer,
        area_mm2,
        centroid_px: (10.4, 20.6),
        contact_px: (10.0, 20.0),
    }
}

#[test]
fn islands_are_counted_and_the_first_ten_listed_from_layer_one() {
    assert_eq!(
        island_report(&[]),
        "Islands: none (every region touches the layer below).\n"
    );
    assert_eq!(
        island_report(&[island(0, 0.12345)]),
        "Islands: 1 unsupported region(s) need supports:\n  layer 1: 0.123 mm2 at pixel (10, 21)\n"
    );
    let many: Vec<Island> = (0..12).map(|at| island(at, 1.0)).collect();
    let text = island_report(&many);
    assert!(text.starts_with("Islands: 12 unsupported region(s) need supports:\n"));
    assert_eq!(text.lines().filter(|line| line.starts_with("  layer ")).count(), 10);
    assert!(text.contains("  layer 10: "));
    assert!(!text.contains("  layer 11: "));
    assert!(text.ends_with("  ... and 2 more\n"));
}

fn risk(kind: RiskKind, severity: Severity, first_layer: usize) -> PrintRisk {
    PrintRisk {
        kind,
        severity,
        first_layer,
        last_layer: first_layer,
        position_mm: (1.0, 2.0),
        bounds_px: [0, 0, 1, 1],
        worst_layer: first_layer,
        measured: 0.0,
    }
}

#[test]
fn risks_group_by_kind_in_fixed_order_and_sort_by_layer() {
    let mut found = vec![
        risk(RiskKind::Overlap, Severity::Error, 0),
        risk(RiskKind::ThinWall, Severity::Warning, 9),
        risk(RiskKind::UnsupportedIsland, Severity::Error, 4),
        risk(RiskKind::ThinWall, Severity::Warning, 2),
    ];
    let row = |kind, severity, layer| format!("  {}\n", risk(kind, severity, layer).summary());
    assert_eq!(
        risk_report(&mut found),
        format!(
            "Print risks: 2 errors, 2 warnings\nUnsupported islands (1)\n{}Thin walls / features (2)\n{}{}Overlapping models (1)\n{}",
            row(RiskKind::UnsupportedIsland, Severity::Error, 4),
            row(RiskKind::ThinWall, Severity::Warning, 2),
            row(RiskKind::ThinWall, Severity::Warning, 9),
            row(RiskKind::Overlap, Severity::Error, 0),
        )
    );
    assert_eq!(risk_report(&mut []), "Print risks: 0 errors, 0 warnings\n");
}

fn outcome(fixes: Vec<MeshFix>, refusal: Option<RepairRefusal>) -> SceneRepairOutcome {
    SceneRepairOutcome {
        target: MeshTarget::InlineLeaf(node()),
        fixes,
        refusal,
        edge_manifold_after: true,
    }
}

#[test]
fn repair_prints_one_line_per_change_kind_and_repeats_are_counted_once() {
    let face = |at: usize| FaceIndex::from(at);
    let removed = MeshFix::RemovedZeroLengthEdgeFace { face_idx: face(1) };
    let flipped = MeshFix::FlippedFaceOrientation { face_idx: face(2) };
    let outcomes = [
        outcome(vec![removed.clone(), removed.clone(), flipped.clone()], None),
        // The same fix on another mesh is a separate change.
        outcome(vec![removed.clone()], None),
    ];
    assert_eq!(
        printed(|out| repair_summary(&outcomes, false, out)),
        "Repair: 2 degenerate faces removed, 1 faces flipped.\n"
    );
    let verbose = printed(|out| repair_summary(&outcomes, true, out));
    assert!(verbose.starts_with("Repair: 2 degenerate faces removed, 1 faces flipped.\nApplied fixes (3):\n"), "{verbose}");

    let shells = [outcome(
        vec![
            MeshFix::RemovedDuplicateFace { face_idx: face(3) },
            MeshFix::DetachedSharedVertex { vertex: VertexIndex::from(4_u32) },
            MeshFix::DetachedSharedVertex { vertex: VertexIndex::from(5_u32) },
        ],
        None,
    )];
    assert_eq!(
        printed(|out| repair_summary(&shells, false, out)),
        "Repair: 0 degenerate faces removed, 0 faces flipped. Separated touching shells: 1 duplicate faces removed, 2 shared vertices detached.\n"
    );

    // Nothing changed: no summary line, but refusals are still reported.
    let refused = [outcome(vec![], Some(RepairRefusal::UnfixableFaceErrors(vec![])))];
    assert_eq!(
        printed(|out| repair_summary(&refused, false, out)),
        "warning: Unable to fix mesh. Face errors correction is not implemented (yet).\n"
    );
    assert_eq!(printed(|out| repair_summary(&[outcome(vec![], None)], true, out)), "");
}

fn zero_edge(face: usize) -> MeshFaceError {
    MeshFaceError::ZeroLengthEdge {
        face_idx: face.into(),
        vertex_pos_a: TriVertexSlot::V0,
        vertex_pos_b: TriVertexSlot::V1,
        vertex_idx: VertexIndex::from(0_u32),
    }
}

fn out_of_range(face: usize) -> MeshFaceError {
    MeshFaceError::OutOfRangeVertexIndex {
        face_idx: face.into(),
        vertex_pos: TriVertexSlot::V2,
        vertex_idx: VertexIndex::from(99_u32),
    }
}

#[test]
fn validation_failures_summarise_and_list_everything_only_when_verbose() {
    let leaf = node();
    let error = SliceSceneError::LeavesValidationFailed(vec![LeafValidationFailure {
        leaf,
        leaf_name: Some("cracked".into()),
        geometry: InvalidLeafGeometry::Faces(vec![zero_edge(2), out_of_range(7), zero_edge(5)]),
    }]);
    assert_eq!(
        printed(|out| validation_failure(&error, false, out)),
        format!("{error}\n\nRun `sc-slicer-cli fix` to repair the mesh, or re-run with --verbose for the full error list.\n")
    );
    assert_eq!(
        printed(|out| validation_failure(&error, true, out)),
        format!(
            "{error}\n\nleaf #{} (\"cracked\"):\nZeroLengthEdge (2):\n  - {}\n  - {}\n\nOutOfRangeVertexIndex (1):\n  - {}\n\nRun `sc-slicer-cli fix` to repair the mesh.\n",
            leaf.get(),
            zero_edge(2),
            zero_edge(5),
            out_of_range(7),
        )
    );
    let unnamed = SliceSceneError::LeavesValidationFailed(vec![LeafValidationFailure {
        leaf,
        leaf_name: None,
        geometry: InvalidLeafGeometry::Faces(vec![zero_edge(1)]),
    }]);
    assert!(printed(|out| validation_failure(&unnamed, true, out)).contains(&format!("\nleaf #{}:\n", leaf.get())));
}

#[test]
fn only_mesh_warnings_of_a_project_are_printed() {
    let text = printed(|out| {
        project_warnings(
            &[
                ReadProjectWarning::NewerProjectData { version: 99 },
                ReadProjectWarning::ThreeMf(LoadThreeMfWarning::EmptyBuild),
                ReadProjectWarning::UnknownProfileFile {
                    part_name: "/Profiles/x.bin".into(),
                },
                ReadProjectWarning::OtherSlicersData {
                    dialect: ForeignDialect::Cura,
                },
            ],
            out,
        )
    });
    assert_eq!(text, "WARNING: The 3MF build section lists no items\n");
}

#[test]
fn a_scene_below_the_plate_is_told_to_move_up() {
    let text = unsliceable(UnsliceableSelection::Scene(UnsliceableScene::TopAtOrBelowPlate {
        z_max_mm: -1.5,
    }))
    .to_string();
    assert_eq!(
        text,
        "input scene has its top at z = -1.5 mm. The slicer slices upward from the build plate at z = 0; reposition your model(s) above z = 0 before slicing."
    );
}
