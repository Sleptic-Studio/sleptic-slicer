// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

#[test]
fn an_error_with_no_cause_has_no_chain_to_show() {
    assert_eq!(error_with_causes(&anyhow::Error::msg("alone")), "");
}

#[test]
fn each_cause_follows_on_its_own_line_outermost_first() {
    let two = anyhow::Error::msg("inner").context("outer");
    assert_eq!(error_with_causes(&two), "outer\n  caused by: inner");
    let five = anyhow::Error::msg("L0")
        .context("L1")
        .context("L2")
        .context("L3")
        .context("L4");
    assert_eq!(
        error_with_causes(&five),
        "L4\n  caused by: L3\n  caused by: L2\n  caused by: L1\n  caused by: L0"
    );
}

#[test]
fn short_plain_details_show_inline_and_technical_ones_do_not() {
    assert!(inline_details("Part A\nPart B"));
    assert!(inline_details("  The file is not a mesh.  "));
    assert!(!inline_details(""));
    assert!(!inline_details("   \n "));
    // Long: more than 360 characters or 8 lines.
    assert!(inline_details(&"a".repeat(360)));
    assert!(!inline_details(&"a".repeat(361)));
    assert!(inline_details(&["x"; 8].join("\n")));
    assert!(!inline_details(&["x"; 9].join("\n")));
    // Characters count, not bytes.
    assert!(inline_details(&"\u{e9}".repeat(360)));
    for technical in [
        "sc_mesh::io::Error",
        "Error { kind: 1 }",
        "C:\\models\\a.stl",
        "Access denied (os error 5)",
        "thread 'main' panicked",
        "Load failed\n    at src/main.rs:1",
        "Err(NotFound)",
    ] {
        assert!(!inline_details(technical), "{technical}");
    }
}
