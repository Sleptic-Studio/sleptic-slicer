// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Device tab, "Send to printer": removable drives to export straight to,
//! and an optional command run after every export (an upload script, a
//! copy to a network share) while Sleptic's printer controller is undecided.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A removable drive (USB stick, SD card) an export can be written to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Drive {
    /// Drive root, e.g. `E:\`.
    pub root: PathBuf,
    /// "SDCARD (E:)", or "Removable drive (E:)" without a volume label.
    pub label: String,
}

/// Removable drives currently mounted. Empty on other platforms.
pub(crate) fn removable_drives() -> Vec<Drive> {
    #[cfg(windows)]
    {
        windows_drives()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

// Win32 drive queries need FFI.
#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_drives() -> Vec<Drive> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    const DRIVE_REMOVABLE: u32 = 2;
    // SAFETY: plain Win32 queries; every buffer is sized and outlives the call.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| {
            let letter = char::from(b'A' + bit);
            let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
            if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_REMOVABLE {
                return None;
            }
            let mut name = [0u16; 261];
            let ok = unsafe {
                GetVolumeInformationW(
                    root.as_ptr(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            // An empty card reader slot reports no volume: skip it.
            if ok == 0 {
                return None;
            }
            let len = name.iter().position(|&c| c == 0).unwrap_or(0);
            let volume = String::from_utf16_lossy(&name[..len]);
            let label = if volume.trim().is_empty() {
                crate::i18n::tr!("Removable drive ({}:)", letter).to_string()
            } else {
                format!("{} ({letter}:)", volume.trim())
            };
            Some(Drive {
                root: PathBuf::from(format!("{letter}:\\")),
                label,
            })
        })
        .collect()
}

/// `dir/stem.ext`, or `dir/stem (2).ext` ... when that name is taken, so an
/// export to a drive never overwrites an earlier job silently. `ext` empty
/// names a folder (PNG layers).
pub(crate) fn unique_target(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let name = |n: u32| {
        let base = if n < 2 { stem.to_string() } else { format!("{stem} ({n})") };
        if ext.is_empty() { base } else { format!("{base}.{ext}") }
    };
    (1..)
        .map(|n| dir.join(name(n)))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

/// Settings of the Send to printer card, kept beside the print history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SendSettings {
    /// Run after every successful export; `{file}` is the exported path.
    #[serde(default)]
    pub post_export_command: String,
}

fn settings_path() -> Option<PathBuf> {
    crate::print_history::data_dir().map(|d| d.join("send_to_printer.json"))
}

impl SendSettings {
    pub(crate) fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save(&self) {
        if let Some(path) = settings_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, text);
            }
        }
    }
}

/// The shell command line for `template` and the exported `file`: every
/// `{file}` becomes the quoted path, and a template without `{file}` gets
/// the quoted path appended. None for a blank template.
pub(crate) fn command_line(template: &str, file: &Path) -> Option<String> {
    let template = template.trim();
    if template.is_empty() {
        return None;
    }
    let quoted = format!("\"{}\"", file.display());
    Some(if template.contains("{file}") {
        template.replace("{file}", &quoted)
    } else {
        format!("{template} {quoted}")
    })
}

/// Runs the post-export command and waits for it (call off the UI thread).
/// Returns a one-line result for the export summary.
pub(crate) fn run_post_export(template: &str, file: &Path) -> Option<Result<String, String>> {
    let line = command_line(template, file)?;
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .arg("/C")
            .raw_arg(&line)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };
    #[cfg(not(windows))]
    let output = std::process::Command::new("sh").arg("-c").arg(&line).output();
    Some(match output {
        Ok(out) if out.status.success() => Ok(crate::i18n::tr!("After-export command finished.").to_string()),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
            Err(crate::i18n::tr!(
                "After-export command failed ({}). {}",
                out.status.code().map_or_else(|| "?".into(), |c| c.to_string()),
                last.trim()
            )
            .to_string())
        }
        Err(e) => Err(crate::i18n::tr!("After-export command could not start: {}", e).to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quotes_the_file_and_appends_it_when_not_placed() {
        let file = Path::new(r"E:\job one.zip");
        assert_eq!(command_line("  ", file), None);
        assert_eq!(
            command_line("copy {file} \\\\printer\\jobs", file).as_deref(),
            Some(r#"copy "E:\job one.zip" \\printer\jobs"#)
        );
        assert_eq!(
            command_line("upload.bat", file).as_deref(),
            Some(r#"upload.bat "E:\job one.zip""#)
        );
    }

    #[test]
    fn unique_target_never_reuses_an_existing_name() {
        let dir = std::env::temp_dir().join(format!("sleptic-send-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_target(&dir, "benchy_plate1", "zip");
        assert_eq!(first, dir.join("benchy_plate1.zip"));
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unique_target(&dir, "benchy_plate1", "zip"), dir.join("benchy_plate1 (2).zip"));
        std::fs::create_dir(dir.join("layers")).unwrap();
        assert_eq!(unique_target(&dir, "layers", ""), dir.join("layers (2)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn post_export_command_reports_success_and_failure() {
        let file = Path::new("C:\\nothing.zip");
        assert!(matches!(run_post_export("echo {file}", file), Some(Ok(_))));
        assert!(matches!(run_post_export("exit /b 3 & rem", file), Some(Err(e)) if e.contains('3')));
        assert!(run_post_export("", file).is_none());
    }
}
