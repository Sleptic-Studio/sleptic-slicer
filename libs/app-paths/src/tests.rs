// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;

#[test]
fn resource_roots_keep_precedence_and_filter_non_directories() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("installed/slicer");
    let second = temp.path().join("share/slicer");
    let file = temp.path().join("file/slicer");
    let assets = temp.path().join("assets");
    for dir in [&first, &second, &assets, file.parent().unwrap()] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(&file, b"not a directory").unwrap();
    let roots = discover_roots(
        vec![
            first.clone(),
            file,
            temp.path().join("missing"),
            second.clone(),
        ],
        assets.clone(),
    );
    assert_eq!(roots, vec![first, second, assets]);
    assert!(roots.iter().all(|path| path.is_absolute()));
}

#[test]
fn relative_resource_roots_become_absolute() {
    let relative = PathBuf::from(".");
    let roots = discover_roots(vec![relative.clone()], relative);
    assert_eq!(roots.len(), 2);
    assert!(roots.iter().all(|path| path.is_absolute()));
}

#[test]
fn platform_paths_use_sleptic_identity() {
    if let Some(dirs) = project_dirs() {
        for path in [dirs.config_dir(), dirs.data_dir(), dirs.data_local_dir()] {
            let text = path.to_string_lossy();
            if cfg!(target_os = "linux") {
                assert!(text.contains("sleptic-slicer"));
            } else if cfg!(target_os = "macos") {
                assert!(text.contains("com.Sleptic.Slicer"));
            } else {
                assert!(
                    path.ends_with("Sleptic/Slicer/config")
                        || path.ends_with("Sleptic/Slicer/data")
                );
            }
        }
    }
    assert!(dev_assets_dir().is_absolute());
    assert!(dev_assets_dir().ends_with("assets"));
}

#[test]
fn migration_copies_nested_tree_and_preserves_backup() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old");
    let new = temp.path().join("new");
    fs::create_dir_all(old.join("nested/empty")).unwrap();
    fs::write(old.join("nested/settings"), b"saved settings").unwrap();
    let pairs = [(old.as_path(), new.as_path())];
    assert_eq!(migrate_pairs(&pairs), vec![new.clone()]);
    assert_eq!(
        fs::read(new.join("nested/settings")).unwrap(),
        b"saved settings"
    );
    assert!(new.join("nested/empty").is_dir());
    assert!(old.join("nested/settings").is_file());
    assert!(migrate_pairs(&pairs).is_empty());
}

#[test]
fn migration_keeps_existing_destination() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old");
    let new = temp.path().join("new");
    fs::create_dir(&old).unwrap();
    fs::create_dir(&new).unwrap();
    fs::write(old.join("settings"), b"legacy").unwrap();
    fs::write(new.join("settings"), b"current").unwrap();
    assert!(migrate_pairs(&[(&old, &new)]).is_empty());
    assert_eq!(fs::read(new.join("settings")).unwrap(), b"current");
}

#[test]
fn migration_ignores_missing_sources_and_duplicate_pairs() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old");
    let new = temp.path().join("new");
    assert!(migrate_pairs(&[(&old, &new)]).is_empty());
    assert!(!new.exists());
    fs::create_dir(&old).unwrap();
    assert_eq!(migrate_pairs(&[(&old, &new), (&old, &new)]), vec![new]);
}
