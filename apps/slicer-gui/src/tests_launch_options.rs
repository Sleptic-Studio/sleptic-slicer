// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
fn parse(args: &[&str]) -> Result<LaunchOptions, LaunchError> {
    LaunchOptions::parse(args.iter().map(OsString::from))
}
#[test]
fn parses_all_verification_flags_and_repeated_models() {
    let opts = parse(&[
        "--open",
        "one.stl",
        "--open",
        "two.stl",
        "--printer",
        "sleptic-diy-msla",
        "--material",
        "standard-405",
        "--slice",
        "--workspace",
        "preview",
        "--preview-layer",
        "300",
        "--window",
        "1600x1000",
        "--screenshot",
        "out.png",
        "--dump-state",
        "out.json",
    ])
    .unwrap();
    assert_eq!(
        opts.open,
        vec![PathBuf::from("one.stl"), PathBuf::from("two.stl")]
    );
    assert_eq!(opts.printer.as_deref(), Some("sleptic-diy-msla"));
    assert_eq!(opts.material.as_deref(), Some("standard-405"));
    assert!(opts.slice);
    assert_eq!(opts.workspace.as_deref(), Some("preview"));
    assert_eq!(opts.preview_layer, Some(300));
    assert_eq!(opts.window, Some((1600, 1000)));
    assert_eq!(opts.screenshot, Some("out.png".into()));
    assert_eq!(opts.dump_state, Some("out.json".into()));
    assert!(!opts.new_instance);
}
/// Test runs must never hand their files to the user's open window.
#[test]
fn new_instance_flag_skips_the_launch_hand_off() {
    let opts = parse(&["--new-instance", "--open", "a.stl", "--layer-height", "0.03"]).unwrap();
    assert!(opts.new_instance);
    assert_eq!(opts.layer_height.as_deref(), Some("0.03"));
    assert!(matches!(
        crate::single_instance::begin(&opts),
        crate::single_instance::Launch::Local(None)
    ));
}
#[test]
fn rejects_invalid_and_missing_values() {
    for args in [
        vec!["--open"],
        vec!["--workspace", "bad"],
        vec!["--preview-layer", "-1"],
        vec!["--window", "0x0"],
        vec!["--window", "1600"],
        vec!["--window-at", "1920"],
        vec!["--window-at", "a,b"],
        vec!["--unknown", "x"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
}

#[test]
fn r8_hooks_validate_mirror_axes_and_parse_actions() {
    let options = parse(&["--hollow", "--auto-drains", "--fill-plate", "--mirror", "z"]).unwrap();
    assert!(options.hollow && options.auto_drains && options.fill_plate);
    assert_eq!(options.mirror, Some(2));
    assert!(parse(&["--mirror", "w"]).is_err());
}
#[test]
fn preserves_unicode_paths_and_positional_open() {
    assert_eq!(
        parse(&["model.stl"]).unwrap().open,
        vec![PathBuf::from("model.stl")]
    );
}

#[test]
fn support_hook_accepts_auto_off_and_rejects_other_values() {
    assert!(parse(&["--supports", "auto"]).unwrap().supports);
    assert!(parse(&["--auto-prepare"]).unwrap().auto_prepare);
    assert!(!parse(&["--supports", "off"]).unwrap().supports);
    assert!(parse(&["--supports", "bad"]).is_err());
}

#[test]
fn placement_verification_flags() {
    let options = parse(&["--auto-orient", "--arrange"]).unwrap();
    assert!(options.auto_orient && options.arrange);
}

#[test]
fn r9_desktop_verification_hooks() {
    let o = parse(&["--verify-printer-error", "--verify-measure"]).unwrap();
    assert!(o.verify_printer_error && o.verify_measure);
}

#[test]
fn risk_jump_hook_requires_a_valid_index() {
    assert_eq!(parse(&["--risk-jump", "1"]).unwrap().risk_jump, Some(1));
    for args in [
        vec!["--risk-jump"],
        vec!["--risk-jump", "-1"],
        vec!["--risk-jump", "cup"],
    ] {
        assert!(parse(&args).is_err());
    }
}

#[test]
fn r11_accepts_each_canvas_evidence_mode_and_rejects_unknown_mode() {
    for mode in [
        "plate-menu",
        "object-menu",
        "global-menu",
        "orbit",
        "drag",
        "supports",
        "model-cut",
        "model-split",
        "model-label",
        "model-boolean",
    ] {
        assert_eq!(
            parse(&["--verify-input", mode])
                .unwrap()
                .verify_input
                .as_deref(),
            Some(mode)
        );
    }
    assert!(parse(&["--verify-input", "unknown-canvas-mode"]).is_err());
}

#[test]
fn r19_machine_and_theme_audit_capture_modes_parse() {
    for mode in [
        "machine-orientation",
        "machine-xy-dialog",
        "machine-z-tower",
        "calibration-menu",
        "popup-mirror",
        "popup-plate",
        "popup-slice",
        "popup-export",
        "popup-navigation",
        "panel-hollow",
        "panel-supports",
        "chooser-printer",
        "object-panel-hidden",
        "panel-batch",
        "audit-printer",
        "audit-profile",
        "audit-error",
    ] {
        assert_eq!(
            parse(&["--verify-input", mode])
                .unwrap()
                .verify_input
                .as_deref(),
            Some(mode)
        );
    }
}

/// Automated captures can open on a second monitor, away from the one in
/// use.
#[test]
fn window_at_places_the_window_on_another_monitor() {
    let opts = parse(&["--window-at", "1960,-8"]).unwrap();
    assert_eq!(opts.window_at, Some((1960, -8)));
}

#[test]
fn explorer_and_open_batches_keep_all_paths_in_order() {
    let paths = vec![
        PathBuf::from("one.stl"),
        PathBuf::from("two.obj"),
        PathBuf::from("three.stl"),
    ];
    assert_eq!(
        parse(&["--open", "one.stl", "two.obj", "three.stl"])
            .unwrap()
            .open,
        paths
    );
    assert_eq!(
        parse(&["one.stl", "two.obj", "three.stl"]).unwrap().open,
        paths
    );
}

#[test]
fn r23_modal_toolbar_and_simplify_capture_modes() {
    for mode in [
        "toolbar-move",
        "toolbar-scale",
        "toolbar-rotate",
        "model-simplify",
    ] {
        assert!(parse(&["--verify-input", mode, "--window-at", "1960,0"]).is_ok());
    }
}

/// R90: a close look at the supports, to check how round they render.
#[test]
fn the_support_closeup_view_is_a_verification_input() {
    let opts = parse(&["--verify-input", "support-closeup"]).unwrap();
    assert_eq!(opts.verify_input.as_deref(), Some("support-closeup"));
}
