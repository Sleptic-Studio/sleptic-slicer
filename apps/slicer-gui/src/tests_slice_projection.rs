// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Why slicing refuses a scene is tested in sc-print-glue; these check the
//! reason each refusal is filed under and the words the user reads.
use std::error::Error as _;

use sc_mesh_core::transform::Transform3D;
use sc_print_glue::UnschedulableHeight;
use sc_print_profiles::{ResolveError, ResolvedField};
use sc_scene::error::SceneError;

use super::*;

#[test]
fn refusals_are_filed_under_their_own_reason_with_their_cause() {
    let node = Scene::new().add_group(None, Transform3D::Identity);
    let cases: Vec<(SliceFailure, Reason, bool)> = vec![
        (UnsliceableScene::Aabb(SceneError::DanglingNodeId { id: node }).into(), Reason::SceneBounds, true),
        (UnsliceableSelection::Scene(UnsliceableScene::NoGeometry).into(), Reason::NoGeometry, false),
        (
            UnsliceableScene::Unschedulable(UnschedulableHeight::NonFiniteHeight { z_max_mm: f64::INFINITY }).into(),
            Reason::LayerCount,
            true,
        ),
        (
            UnsliceableSelection::LayerHeight(ResolveError::MissingRequiredField { field: ResolvedField::LayerHeightMm })
                .into(),
            Reason::PrintSettings,
            true,
        ),
    ];
    for (failure, reason, has_cause) in cases {
        assert_eq!(failure.reason(), &reason);
        assert_eq!(failure.source().is_some(), has_cause, "{reason:?}");
    }
}

#[test]
fn the_user_reads_what_failed_then_the_library_words() {
    let settings = SliceFailure::from(UnsliceableSelection::LayerHeight(ResolveError::MissingRequiredField {
        field: ResolvedField::LayerHeightMm,
    }));
    assert!(settings.to_string().starts_with("Could not resolve the print configuration: "));
    let below = SliceFailure::from(UnsliceableScene::TopAtOrBelowPlate { z_max_mm: -2.25 });
    assert_eq!(below.reason(), &Reason::BelowPlate { top_mm: -2.25 });
    assert_eq!(
        below.to_string(),
        "The scene's top is at z = -2.25 mm; nothing sits above the build plate (z = 0). Move the model above the plate before slicing."
    );
    let fixed = [
        (Reason::Panic("bad index".into()), "The slicer panicked: bad index"),
        (Reason::LayerGrids, "The slice layers could not be merged (internal layer-grid mismatch)."),
        (Reason::NoGeometry, "The scene has no geometry to slice."),
        (Reason::Slicer, "Slicing failed"),
    ];
    for (reason, words) in fixed {
        assert_eq!(SliceFailure::plain(reason).to_string(), words);
    }
}
