// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::{fs, process::Command};
fn executable() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sc-profiles-converter"))
}

#[test]
fn help_and_version_do_not_need_inputs() {
    let help = executable().arg("--help").output().unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(
        text.contains("--with-images") && text.contains("--input") && text.contains("json")
    );
    let version = executable().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .contains(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn conversion_reports_on_stderr_and_writes_the_library() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.json");
    let out = temp.path().join("output");
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/contract.json")).unwrap();
    fs::write(&input, serde_json::to_vec(&cases[0]["input"]).unwrap()).unwrap();
    let result = executable()
        .args(["-f", "json", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains(&format!(
                "converted 1 printer(s), 0 material(s), 0 image(s) into {}",
                out.display()
            ))
    );
    assert!(out.join("printers/probe-printer.toml").is_file());
}

#[test]
fn invalid_requests_exit_without_creating_output() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("output");
    for format in ["json", "unknown"] {
        let result = executable()
            .args(["-f", format, "-o"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!out.exists());
    }
}

#[test]
fn a_later_unreadable_input_does_not_write_earlier_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let valid = temp.path().join("valid.json");
    let broken = temp.path().join("broken.json");
    let out = temp.path().join("output");
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/contract.json")).unwrap();
    fs::write(&valid, serde_json::to_vec(&cases[0]["input"]).unwrap()).unwrap();
    fs::write(&broken, "[{").unwrap();
    let result = executable()
        .args(["-f", "json", "-i"])
        .arg(&valid)
        .arg("-i")
        .arg(&broken)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!out.exists());
    assert!(String::from_utf8_lossy(&result.stderr).contains("broken.json"));
}
