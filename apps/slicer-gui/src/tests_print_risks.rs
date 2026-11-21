// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

#[test]
fn coordinates_normalize_rounded_negative_zero() {
    for value in [-0.0, -0.001, -0.049, 0.0, 0.049] {
        assert_eq!(super::coordinate(value), "0.0");
    }
    assert_eq!(super::coordinate(-0.051), "-0.1");
    assert_eq!(super::coordinate(1.26), "1.3");
}

#[test]
fn risk_rows_have_compact_fixed_spacing_and_plural_status() {
    let ui = include_str!("../ui/main-window.slint");
    assert!(ui.contains("@tr(\"{n} print error\" | \"{n} print errors\" % root.risk-error-count)"));
    // R25: the risk list is a summary per type; a group's rows take no space
    // until it is expanded (the old flat list was dozens of near-identical
    // lines). The real-window audit clicks a group open and a row's jump.
    assert!(ui.contains("height: root.expanded-risk-kind == risk.kind ? 32px : 0px;"));
    assert!(ui.contains("root.expanded-risk-kind = root.expanded-risk-kind == risk.kind ? -1 : risk.kind;"));
}

#[test]
fn support_options_have_separate_full_height_rows() {
    let ui = include_str!("../ui/main-window.slint");
    for label in ["Symmetric supports (detected models)"] {
        // Every checkbox row, in the floating panel and the sidebar (R97).
        let rows: Vec<&str> = ui
            .lines()
            .filter(|line| line.contains("CheckBox") && line.contains(label))
            .collect();
        assert_eq!(rows.len(), 2, "{label}");
        for row in rows {
            assert!(row.contains("min-height: 32px"), "{row}");
        }
    }
}

#[test]
fn model_tools_reserve_space_for_wrapped_boolean_help() {
    let ui = include_str!("../ui/main-window.slint");
    let help = ui
        .lines()
        .find(|line| line.contains("Select two models. Subtract removes"))
        .unwrap();
    assert!(help.contains("min-height: 40px"), "{help}");
}

#[test]
fn vertical_plate_navigation_does_not_share_the_toolbar_or_preview_lane() {
    let ui = include_str!("../ui/main-window.slint");
    assert!(!ui.contains("x: root.settings-width + 12px; y: 112px;"));
    assert!(ui.contains("visible: root.plate-tabs-visible && root.plate-ids.length > 1;"));
    assert!(!ui.contains("y: parent.height - 102px;"));
    let strip = ui
        .split("// Plate thumbnails occupy a vertical viewport rail")
        .nth(1)
        .unwrap()
        .split("// Plate tool glyphs, one per tile")
        .next()
        .unwrap();
    assert!(strip.contains("ScrollView"));
    assert!(!strip.contains("slice-all-requested"));
    assert!(!strip.contains("export-all-requested"));
    assert!(strip.contains("plate-menu := PopupWindow"));
    assert!(ui.contains("if root.supports-stale: VerticalLayout"));
    assert!(ui.contains("text: @tr(\"Regenerate\")"));
}

#[test]
fn preview_graph_has_its_own_column_and_cut_plane_is_not_stale_yellow() {
    let ui = include_str!("../ui/main-window.slint");
    // 2D ends the right panel and 96 px before the right edge (R109: the
    // panel slides away); graph starts 362px before it.
    assert!(ui.contains("width: root.width - root.settings-width - root.right-panel-space - 96px;"));
    assert!(ui.contains("x: -24px;"));
    assert!(ui.contains("stroke: ShellTheme.cut-plane;"));
    let theme = include_str!("../ui/theme.slint");
    assert!(theme.contains("cut-plane: #e46b26"));
    assert!(theme.contains("warning: light ? #8a5b00 : #e5a500"));
}
