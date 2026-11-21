// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Which project format a file is, from its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectFileType {
    ThreeMf,
}

#[derive(Debug)]
pub enum InferProjectFileTypeError {
    UnknownExtension { path: std::path::PathBuf },
}
impl std::fmt::Display for InferProjectFileTypeError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self::UnknownExtension { path } = self;
        write!(out, "cannot infer a project file format from the extension of {}", path.display())
    }
}
impl std::error::Error for InferProjectFileTypeError {}
/// The project format a file name says: `.3mf` in any case, nothing else.
pub fn infer_project_file_type_from_extension(
    path: &std::path::Path,
) -> Result<ProjectFileType, InferProjectFileTypeError> {
    let suffix = path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or_default();
    match suffix.eq_ignore_ascii_case("3mf") {
        true => Ok(ProjectFileType::ThreeMf),
        false => Err(InferProjectFileTypeError::UnknownExtension { path: path.to_path_buf() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_3mf_in_any_case_is_a_project() {
        for name in ["a.3mf", "b.3MF", "dir.x/c.3Mf"] {
            assert_eq!(infer_project_file_type_from_extension(name.as_ref()).unwrap(), ProjectFileType::ThreeMf);
        }
        for name in ["a.stl", "3mf", "a.3mf.bak", "a."] {
            let refused = infer_project_file_type_from_extension(name.as_ref()).unwrap_err();
            let expected = format!("cannot infer a project file format from the extension of {name}");
            assert_eq!(refused.to_string(), expected);
        }
    }
}
