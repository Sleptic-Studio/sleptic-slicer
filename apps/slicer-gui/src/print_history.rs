// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Device tab, "Print history": every exported job with its printer, resin,
//! estimate, and the outcome the user records afterwards (M7 #6). Stored as
//! JSON lines in the app data folder so a crash mid-write loses one line.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The app's per-user data folder (`...\AppData\Roaming\...\data`).
pub(crate) fn data_dir() -> Option<PathBuf> {
    sc_app_paths::project_dirs().map(|d| d.data_dir().to_path_buf())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    #[default]
    NotRecorded,
    Succeeded,
    Failed,
}

impl Outcome {
    pub(crate) fn index(self) -> i32 {
        match self {
            Self::NotRecorded => 0,
            Self::Succeeded => 1,
            Self::Failed => 2,
        }
    }

    pub(crate) fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Succeeded,
            2 => Self::Failed,
            _ => Self::NotRecorded,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    /// Seconds since the Unix epoch when the export finished.
    pub exported_at: u64,
    pub file: PathBuf,
    pub printer: String,
    pub material: String,
    pub layers: usize,
    /// The estimate as shown ("1h 14m"), empty when none was held.
    #[serde(default)]
    pub estimate: String,
    #[serde(default)]
    pub outcome: Outcome,
    /// 1-based layer the print failed at, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_layer: Option<u32>,
}

/// The history file; one JSON object per line, oldest first.
pub(crate) fn history_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("print_history.jsonl"))
}

/// Every readable entry, oldest first; malformed lines are skipped.
pub(crate) fn load(path: &Path) -> Vec<Entry> {
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn append(path: &Path, entry: &Entry) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(entry)?)
}

/// Rewrites the file (after an outcome edit) through a temporary file, so
/// an interrupted write leaves the old history intact.
pub(crate) fn save_all(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = String::new();
    for entry in entries {
        text.push_str(&serde_json::to_string(entry)?);
        text.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Seconds to add to UTC for local wall-clock time (0 when unknown).
// The Win32 time zone query needs FFI.
#[allow(unsafe_code)]
pub(crate) fn local_offset_secs() -> i64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        // SAFETY: the struct is plain data the call fills in.
        let mut info: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
        let state = unsafe { GetTimeZoneInformation(&mut info) };
        // 2 = TIME_ZONE_ID_DAYLIGHT: daylight saving is in effect now.
        let bias = info.Bias + if state == 2 { info.DaylightBias } else { info.StandardBias };
        -i64::from(bias) * 60
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// "2026-10-29 14:05" for `unix` shifted by `offset_secs` (pass
/// [`local_offset_secs`] for the user's wall clock).
pub(crate) fn format_time(unix: u64, offset_secs: i64) -> String {
    let local = unix as i64 + offset_secs;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02} {:02}:{:02}", secs / 3600, secs / 60 % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> Entry {
        Entry {
            exported_at: 1_761_746_700,
            file: PathBuf::from(format!("E:\\{name}.zip")),
            printer: "sleptic-diy-msla".into(),
            material: "default-standard-405nm".into(),
            layers: 960,
            estimate: "1h 14m".into(),
            outcome: Outcome::NotRecorded,
            failed_layer: None,
        }
    }

    #[test]
    fn history_appends_edits_and_survives_a_bad_line() {
        let dir = std::env::temp_dir().join(format!("sleptic-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("print_history.jsonl");
        append(&path, &entry("a")).unwrap();
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "{not json\n").unwrap();
        append(&path, &entry("b")).unwrap();
        let mut all = load(&path);
        assert_eq!(all.len(), 2, "the malformed line is skipped, not fatal");
        all[1].outcome = Outcome::Failed;
        all[1].failed_layer = Some(212);
        save_all(&path, &all).unwrap();
        let back = load(&path);
        assert_eq!(back[1].outcome, Outcome::Failed);
        assert_eq!(back[1].failed_layer, Some(212));
        assert_eq!(back[0], entry("a"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn times_format_as_calendar_dates() {
        assert_eq!(format_time(0, 0), "1970-01-01 00:00");
        assert_eq!(format_time(1_761_746_700, 0), "2025-10-29 14:05");
        assert_eq!(format_time(1_761_746_700, -5 * 3600), "2025-10-29 09:05");
        assert_eq!(format_time(951_782_400, 0), "2000-02-29 00:00");
    }
}
