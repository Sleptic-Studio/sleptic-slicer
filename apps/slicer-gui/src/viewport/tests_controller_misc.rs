// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

#[test]
fn unit_warning_distinguishes_small_scaled_and_degenerate_bounds() {
    for (bounds, warning) in [
        ([60.0 / 25.4, 31.0 / 25.4, 48.0 / 25.4], true),
        ([60.0, 31.0, 48.0], false),
        ([3.0, 3.0, 3.0], false),
        ([10.0, 10.0, 0.0], false),
    ] {
        assert_eq!(looks_like_wrong_units(bounds), warning, "{bounds:?}");
    }
}

#[test]
fn transform_display_rounds_without_negative_zero() {
    for (value, expected) in [
        (-0.0, "0.00"),
        (-0.001, "0.00"),
        (-1e-12, "0.00"),
        (-0.82, "-0.82"),
        (-0.006, "-0.01"),
        (100.0, "100.00"),
    ] {
        assert_eq!(format_transform_value(value), expected);
    }
}

#[test]
fn clearing_measurement_does_not_create_an_edit() {
    let controller = ViewController::for_test();
    for point in [[0., 0., 0.], [3., 4., 0.]] {
        controller.measure_point(point);
    }
    assert_eq!(controller.measure.borrow().points.len(), 2);
    controller.select_nothing();
    assert!(controller.measure.borrow().points.is_empty());
    assert!(!controller.scene_doc().has_unsaved_changes());
    assert!(!controller.scene_doc().has_past());
}

#[test]
fn support_summary_identifies_the_selected_raft() {
    let meshes = [sc_mesh_fixtures::unit_cube_mesh()];
    let mut choice =
        sc_print_glue::supports::choose_supports(&meshes, 30000.0, &Default::default());
    choice.lifted = 0;
    assert!(support_choice_text(&choice).contains("kept your support settings"));
    choice.lifted = 1;
    choice.lattice = true;
    choice.raft_share = 0.083;
    let summary = support_choice_text(&choice);
    for fragment in ["mm tips", "lattice raft", "8%"] {
        assert!(summary.contains(fragment), "{summary}");
    }
}

#[test]
fn preview_band_uses_each_layers_own_height() {
    for (first, later) in [(0.05, 0.05), (0.1, 0.03), (0.2, 0.1)] {
        let height = |index: u32| Some(first + f64::from(index) * later);
        for (index, expected, tolerance) in [(0, first, 1e-6), (1, later, 1e-6), (40, later, 1e-5)]
        {
            let actual = layer_band_thickness(height(index).unwrap() as f32, height, index);
            assert!(
                (actual - expected as f32).abs() < tolerance,
                "layer {index}: {actual}"
            );
        }
    }
}

#[test]
fn clipboard_paste_requires_an_open_scene() {
    let controller = ViewController::for_test();
    let fragment = sc_scene_edit::SceneFragment::from_leaf_snapshot(
        std::sync::Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        Transform3D::Identity,
    );
    let clipboard = Clipboard::new(fragment);
    assert!(!controller.paste_at_cursor(&clipboard));
    assert!(!controller.paste_at_pixel(&clipboard, 10.0, 10.0));
}
