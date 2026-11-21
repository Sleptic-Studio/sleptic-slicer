// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Acceptance is resolved before any main window exists.
use crate::{app_settings::AppSettings, launch_options::LaunchOptions, LicenseDialog};
use slint::ComponentHandle;
use std::{cell::Cell, error::Error, rc::Rc, time::{SystemTime, UNIX_EPOCH}};
include!(concat!(env!("OUT_DIR"), "/license.rs"));

fn required(required: bool, version: &str, accepted: Option<&str>, automated: bool) -> bool {
    required && !automated && accepted != Some(version)
}

pub(crate) fn confirm(options: &LaunchOptions) -> Result<bool, Box<dyn Error>> {
    if cfg!(test) { return Ok(true); }
    let mut settings = AppSettings::load();
    if !required(LICENSE_REQUIRED, LICENSE_VERSION, settings.accepted_license_version.as_deref(), options.automated) { return Ok(true); }
    let dialog = LicenseDialog::new()?;
    dialog.set_license_text(LICENSE_TEXT.into());
    let accepted = Rc::new(Cell::new(false));
    let result = accepted.clone();
    let weak = dialog.as_weak();
    dialog.on_accept_requested(move || { result.set(true); if let Some(dialog) = weak.upgrade() { let _ = dialog.hide(); } });
    let weak = dialog.as_weak();
    dialog.on_quit_requested(move || { if let Some(dialog) = weak.upgrade() { let _ = dialog.hide(); } });
    dialog.run()?;
    if accepted.get() {
        settings.accepted_license_version = Some(LICENSE_VERSION.to_owned());
        settings.accepted_license_date = Some(utc_date()?);
        settings.save()?;
    }
    Ok(accepted.get())
}

// Convert Unix days to a calendar date in UTC.
fn utc_date() -> Result<String, Box<dyn Error>> {
    let days = (SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 86400) as i64;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

#[cfg(test)]
#[path = "tests_license_acceptance.rs"]
mod tests;
