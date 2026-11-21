// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_print_profiles::ProfileLibraries;
use std::ffi::OsString;

pub(crate) const LIBRARY_DIR_ENV: &str = "SC_SLICER_LIBRARY_DIR";
pub(crate) const ASSETS_DIR_ENV: &str = "SC_SLICER_ASSETS_DIR";

pub(crate) fn resolve_profile_libraries() -> ProfileLibraries {
    apply_roots(
        ProfileLibraries::platform_default(),
        std::env::var_os(LIBRARY_DIR_ENV),
        std::env::var_os(ASSETS_DIR_ENV),
    )
}

fn apply_roots(
    mut defaults: ProfileLibraries,
    user: Option<OsString>,
    bundled: Option<OsString>,
) -> ProfileLibraries {
    if let Some(root) = user {
        defaults.user = Some(root.into());
    }
    if let Some(root) = bundled {
        defaults.bundled = vec![root.into()];
    }
    defaults
}

#[cfg(test)]
mod tests_roots {
    use super::*;
    #[test]
    fn overrides_are_literal_independent_and_do_not_touch_the_filesystem() {
        let defaults = ProfileLibraries::new(
            Some("original-user".into()),
            vec!["first".into(), "second".into()],
        );
        for user in [None, Some(""), Some("other-user")] {
            for bundled in [None, Some(""), Some("other-assets")] {
                let result = apply_roots(
                    defaults.clone(),
                    user.map(Into::into),
                    bundled.map(Into::into),
                );
                assert_eq!(
                    result.user,
                    user.map(Into::into).or_else(|| defaults.user.clone())
                );
                assert_eq!(
                    result.bundled,
                    bundled
                        .map(|s| vec![s.into()])
                        .unwrap_or_else(|| defaults.bundled.clone())
                );
            }
        }
        let no_user = apply_roots(
            ProfileLibraries::new(None, vec![]),
            None,
            Some("only-assets".into()),
        );
        assert!(no_user.user.is_none());
        assert_eq!(
            no_user.bundled,
            vec![std::path::PathBuf::from("only-assets")]
        );
    }
}
