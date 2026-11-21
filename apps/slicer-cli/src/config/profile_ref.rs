// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `--printer` and `--material` values: a profile file when the value is
//! clearly a path, otherwise a profile id.
use sc_print_primitives::{InvalidProfileId, MaterialId, PrinterId};
use std::{fmt, path::PathBuf, str::FromStr};

/// A profile named on the command line: a file to read, or an id to look
/// up in the libraries.
#[derive(Clone, Debug)]
pub(crate) enum ProfileRef<Id> {
    Path(PathBuf),
    Id(Id),
}
pub(crate) type PrinterProfileRef = ProfileRef<PrinterId>;
pub(crate) type MaterialProfileRef = ProfileRef<MaterialId>;

/// A value that neither looks like a path nor is a valid id.
#[derive(Debug)]
pub(crate) struct ProfileRefParseError {
    given: String,
    not_an_id: InvalidProfileId,
}

impl fmt::Display for ProfileRefParseError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { given, not_an_id } = self;
        write!(out, "Profile reference {given:?} needs an explicit path prefix or a valid ID: {not_an_id}")
    }
}
impl std::error::Error for ProfileRefParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.not_an_id)
    }
}

/// Whether `value` says it is a path by how it starts: `/`, `./` or `../`
/// anywhere; on Windows also the backslash forms and a drive (`C:/`, `C:\`).
/// A bare `name.toml` is not one: it would be read as a (bad) id.
fn looks_like_path(value: &str) -> bool {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| value.starts_with(prefix));
    if starts(&["/", "./", "../"]) {
        return true;
    }
    if !cfg!(windows) {
        return false;
    }
    let drive = matches!(value.as_bytes(), [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic());
    drive || starts(&["\\", ".\\", "..\\"])
}

impl<Id: FromStr<Err = InvalidProfileId>> FromStr for ProfileRef<Id> {
    type Err = ProfileRefParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if looks_like_path(value) {
            return Ok(Self::Path(PathBuf::from(value)));
        }
        match value.parse() {
            Ok(id) => Ok(Self::Id(id)),
            Err(not_an_id) => Err(ProfileRefParseError { given: value.to_owned(), not_an_id }),
        }
    }
}

#[cfg(test)]
mod tests_references {
    use super::*;
    #[test]
    fn typed_ids_and_literal_paths_remain_disjoint() {
        for value in ["p", "12", "resin-2026"] {
            match value.parse::<PrinterProfileRef>().unwrap() {
                PrinterProfileRef::Id(id) => assert_eq!(id.as_str(), value),
                _ => panic!("expected ID"),
            }
            match value.parse::<MaterialProfileRef>().unwrap() {
                MaterialProfileRef::Id(id) => assert_eq!(id.as_str(), value),
                _ => panic!("expected ID"),
            }
        }
        for value in ["/tmp/file.toml", "./relative.toml", "../file.toml"] {
            assert!(
                matches!(value.parse::<PrinterProfileRef>().unwrap(), PrinterProfileRef::Path(p) if p.as_path()==std::path::Path::new(value))
            );
            assert!(
                matches!(value.parse::<MaterialProfileRef>().unwrap(), MaterialProfileRef::Path(p) if p.as_path()==std::path::Path::new(value))
            );
        }
        for value in [
            "",
            "..",
            ".",
            "name.toml",
            "a/b",
            "a\\b",
            "C:relative",
            "1:/wrong",
            "UPPER",
            "resin name",
            "-prefix",
        ] {
            assert!(value.parse::<PrinterProfileRef>().is_err(), "{value}");
            assert!(value.parse::<MaterialProfileRef>().is_err(), "{value}");
        }
        let error = "name.toml".parse::<PrinterProfileRef>().unwrap_err();
        let why = "name.toml".parse::<PrinterId>().unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Profile reference \"name.toml\" needs an explicit path prefix or a valid ID: {why}")
        );
        assert!(std::error::Error::source(&error).is_some());
    }
    #[test]
    fn native_prefixes_are_classified_only_on_their_platform() {
        for value in [
            "C:/profile.toml",
            "z:\\profile.toml",
            "\\rooted",
            ".\\local",
            "..\\parent",
            "\\\\server\\share",
        ] {
            let p = value.parse::<PrinterProfileRef>();
            let m = value.parse::<MaterialProfileRef>();
            if cfg!(windows) {
                assert!(
                    matches!(p.unwrap(), PrinterProfileRef::Path(path) if path.as_path()==std::path::Path::new(value))
                );
                assert!(
                    matches!(m.unwrap(), MaterialProfileRef::Path(path) if path.as_path()==std::path::Path::new(value))
                );
            } else {
                assert!(p.is_err());
                assert!(m.is_err());
            }
        }
    }
}
