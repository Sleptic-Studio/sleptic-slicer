// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

#[test]
fn acceptance_round_trips_and_old_preferences_load() {
    let mut settings: AppSettings = toml::from_str("").unwrap();
    assert!(settings.accepted_license_version.is_none());
    assert!(settings.accepted_license_date.is_none());
    settings.accepted_license_version = Some("v1".into());
    settings.accepted_license_date = Some("2026-11-20".into());
    let saved = toml::to_string(&settings).unwrap();
    let loaded: AppSettings = toml::from_str(&saved).unwrap();
    assert_eq!(loaded.accepted_license_version.as_deref(), Some("v1"));
    assert_eq!(loaded.accepted_license_date.as_deref(), Some("2026-11-20"));
}

#[test]
fn requirement_tracks_version_and_automation() {
    assert!(!required(false, "v1", None, false));
    assert!(required(true, "v1", None, false));
    assert!(!required(true, "v1", Some("v1"), false));
    assert!(required(true, "v2", Some("v1"), false));
    for flag in ["--screenshot", "--verify-input", "--benchmark-preview", "--slice"] {
        let args = if flag == "--screenshot" || flag == "--verify-input" { vec![flag, if flag == "--verify-input" { "popup-mirror" } else { "out.png" }] } else { vec![flag] };
        let options = LaunchOptions::parse(args.into_iter().map(Into::into)).unwrap();
        assert!(!required(true, "v1", None, options.automated));
    }
}

#[test]
fn utc_date_format() {
    let date = utc_date().unwrap();
    assert_eq!(date.len(), 10);
    assert_eq!(&date[4..5], "-");
    assert_eq!(&date[7..8], "-");
    let parts: Vec<u32> = date.split('-').map(|p| p.parse().unwrap()).collect();
    assert!(parts[0] >= 2026);
    assert!((1..=12).contains(&parts[1]));
    assert!((1..=31).contains(&parts[2]));
}

#[test]
fn license_dialog_instantiation() {
    // Keep this test on Slint's test backend, like the other window tests.
    i_slint_backend_testing::init_no_event_loop();
    let dialog = LicenseDialog::new().unwrap();
    dialog.set_license_text("Test License Text".into());
    assert_eq!(dialog.get_license_text(), "Test License Text");
}
