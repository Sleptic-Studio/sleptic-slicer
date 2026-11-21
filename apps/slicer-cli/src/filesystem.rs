// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use anyhow::{Context, Result, bail};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) fn validate_sourcefile_path(path: PathBuf) -> Result<PathBuf> {
    let resolved = path
        .canonicalize()
        .with_context(|| format!("Cannot resolve source {}", display_path(&path)))?;
    if !resolved.is_file() {
        bail!("Source {} is not a regular file", display_path(&path));
    }
    Ok(resolved)
}

pub(crate) fn validate_targetfile_path(path: PathBuf) -> Result<PathBuf> {
    let filename = path.file_name().context("Target path needs a file name")?;
    if let Some(info) = optional_metadata(&path)?
        && !info.file_type().is_file()
    {
        bail!(
            "Target {} must not be a directory or symlink",
            display_path(&path)
        );
    }
    let parent = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => parent
            .canonicalize()
            .with_context(|| format!("Cannot resolve target directory {}", display_path(parent)))?,
        None => std::env::current_dir().context("Cannot resolve current directory")?,
    };
    if !parent.is_dir() {
        bail!("Target parent {} is not a directory", display_path(&parent));
    }
    Ok(parent.join(filename))
}

fn optional_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(info) => Ok(Some(info)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Cannot inspect {}", display_path(path))),
    }
}

/// Reject overwrites through path aliases, links and native file identity.
pub(crate) fn ensure_distinct_files(source: &Path, target: &Path) -> Result<()> {
    if source == target {
        bail!("Source file path and target file path are the same");
    }
    let Some(info) = optional_metadata(target)? else {
        return Ok(());
    };
    if info.file_type().is_symlink() {
        bail!("Refusing a symlink at target {}", display_path(target));
    }
    if !info.is_file() {
        bail!("Target {} is not a regular file", display_path(target));
    }
    #[cfg(any(unix, windows))]
    if native_identity(source)? == native_identity(target)? {
        bail!("Source and target are the same underlying file; refusing to overwrite it");
    }
    Ok(())
}

#[cfg(unix)]
fn native_identity(path: &Path) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let info =
        fs::metadata(path).with_context(|| format!("Cannot identify {}", display_path(path)))?;
    Ok((info.dev(), info.ino()))
}

#[cfg(windows)]
fn native_identity(path: &Path) -> Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let file = fs::File::open(path)
        .with_context(|| format!("Cannot open {} for identity check", display_path(path)))?;
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: File owns the live handle for this call. Windows initializes the
    // complete output structure on success; failure leaves it unread.
    #[allow(unsafe_code)]
    let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
    if success == 0 {
        return Err(io::Error::last_os_error()).context("Cannot read native file identity");
    }
    // SAFETY: The successful call above initialized every field.
    #[allow(unsafe_code)]
    let info = unsafe { info.assume_init() };
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(test)]
mod tests_paths {
    use super::*;
    #[test]
    fn validation_resolves_inputs_without_creating_output_or_parent() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.obj");
        fs::write(&source, b"input bytes").unwrap();
        let canonical = validate_sourcefile_path(source.clone()).unwrap();
        assert_eq!(canonical, source.canonicalize().unwrap());
        let out = validate_targetfile_path(dir.path().join("out.obj")).unwrap();
        assert!(!out.exists());
        assert!(validate_sourcefile_path(dir.path().to_owned()).is_err());
        assert!(validate_sourcefile_path(out).is_err());
        assert!(validate_targetfile_path(dir.path().to_owned()).is_err());
        assert!(validate_targetfile_path(dir.path().join("missing/out.obj")).is_err());
        assert!(!dir.path().join("missing").exists());
        assert!(validate_targetfile_path(PathBuf::new()).is_err());
        assert_eq!(fs::read(source).unwrap(), b"input bytes");
    }

    #[test]
    fn identity_and_repeated_spelling_protect_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        let source = validate_sourcefile_path(a.clone()).unwrap();
        let same = validate_targetfile_path(dir.path().join("./a")).unwrap();
        assert!(ensure_distinct_files(&source, &same).is_err());
        assert!(ensure_distinct_files(&source, &b).is_ok());
        assert!(ensure_distinct_files(&source, &dir.path().join("not-created")).is_ok());
        #[cfg(any(unix, windows))]
        {
            assert_eq!(
                native_identity(&source).unwrap(),
                native_identity(&same).unwrap()
            );
            assert_ne!(native_identity(&a).unwrap(), native_identity(&b).unwrap());
            assert!(native_identity(&dir.path().join("missing")).is_err());
        }
        let alias = dir.path().join("alias");
        match fs::hard_link(&a, &alias) {
            Ok(()) => assert!(ensure_distinct_files(&source, &alias).is_err()),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("hard-link creation denied; native identity assertions still ran")
            }
            Err(e) => panic!("hard-link setup: {e}"),
        }
        assert_eq!(fs::read(a).unwrap(), b"a");
    }

    #[test]
    fn a_link_inserted_after_validation_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original");
        fs::write(&source, b"protected").unwrap();
        let target = validate_targetfile_path(dir.path().join("later")).unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&source, &target);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&source, &target);
        #[cfg(any(unix, windows))]
        match linked {
            Ok(()) => {
                assert!(validate_targetfile_path(target.clone()).is_err());
                assert!(ensure_distinct_files(&source, &target).is_err());
                fs::remove_file(&source).unwrap();
                assert!(validate_targetfile_path(target).is_err());
            }
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("symlink creation denied")
            }
            Err(e) => panic!("symlink setup: {e}"),
        }
    }
}

/// Human-readable path spelling; never use this adapter for filesystem access.
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{}", unc);
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

#[cfg(test)]
mod display_path_tests {
    use super::*;
    #[test]
    fn strips_verbatim_drive_and_preserves_unc_root() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\models\boat.stl")),
            r"C:\models\boat.stl"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\boat.stl")),
            r"\\server\share\boat.stl"
        );
        assert_eq!(display_path(Path::new("boat.stl")), "boat.stl");
    }
}
