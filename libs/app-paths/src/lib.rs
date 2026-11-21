// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Shared application identity keeps every frontend's storage in the same place.

use directories::{BaseDirs, ProjectDirs};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Use the platform's application directories so frontend settings are shared.
pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from(
        "com",
        "Sleptic",
        if cfg!(target_os = "linux") {
            "sleptic-slicer"
        } else {
            "Slicer"
        },
    )
}

/// File dialogs start at the user's home rather than the process working directory.
pub fn home_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

/// Source builds can find bundled resources without an installation step.
pub fn dev_assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// Installed resources take precedence over the source checkout's fallback assets.
pub fn bundled_data_dirs() -> Vec<PathBuf> {
    discover_roots(installed_candidates(), dev_assets_dir())
}

#[cfg(target_os = "linux")]
fn installed_candidates() -> Vec<PathBuf> {
    let bases =
        env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    env::split_paths(&bases)
        .map(|base| base.join("slicer"))
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn installed_candidates() -> Vec<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .map_or_else(Vec::new, |parent| {
            vec![parent.join("slicer"), parent.join("share/slicer")]
        })
}

fn discover_roots(candidates: Vec<PathBuf>, assets: PathBuf) -> Vec<PathBuf> {
    candidates
        .into_iter()
        .chain([assets])
        .filter_map(|path| std::path::absolute(path).ok())
        .filter(|path| path.is_dir())
        .collect()
}

/// Preserve the previous installation's files as a backup while adopting the new identity.
pub fn migrate_legacy_dirs() -> Vec<PathBuf> {
    let Some(new) = project_dirs() else {
        return Vec::new();
    };
    let Some(old) = ProjectDirs::from(
        "it",
        "soulcrafted",
        if cfg!(target_os = "linux") {
            "soulcrafted-slicer"
        } else {
            "slicer"
        },
    ) else {
        return Vec::new();
    };
    migrate_pairs(&[
        (old.config_dir(), new.config_dir()),
        (old.data_dir(), new.data_dir()),
        (old.data_local_dir(), new.data_local_dir()),
    ])
}

fn migrate_pairs(pairs: &[(&Path, &Path)]) -> Vec<PathBuf> {
    let mut created = Vec::new();
    for &(source, destination) in pairs {
        if destination.symlink_metadata().is_ok() || !source.is_dir() {
            continue;
        }
        if fs::create_dir_all(destination).is_ok() {
            created.push(destination.to_path_buf());
            copy_contents(source, destination);
        }
    }
    created
}

/// Ignore links to avoid cycles and copying files outside the legacy directory tree.
fn copy_contents(source: &Path, destination: &Path) {
    let Ok(entries) = fs::read_dir(source) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            if fs::create_dir(&target).is_ok() {
                copy_contents(&entry.path(), &target);
            }
        } else if kind.is_file() {
            let _ = fs::copy(entry.path(), target);
        }
    }
}

#[cfg(test)]
mod tests;
