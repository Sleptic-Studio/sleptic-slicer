// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Application preferences with a stable TOML representation.

use crate::language::LanguagePreference;
use crate::panel_place::{Panel, PanelPlace, SavedPanel};
use crate::{MenuLayout, ThemePreference};
use sc_print_primitives::{AaLevel, MaterialId, PrinterId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const SETTINGS_FILE: &str = "settings.toml";
pub(crate) const MAX_RECENT_FILES: usize = 10;
// The same defaults serve new installs and sections absent from older files.
macro_rules! stored_defaults {
    ($ty:ty { $($field:ident: $value:expr),* $(,)? }) => {
        impl Default for $ty {
            fn default() -> Self { Self { $($field: $value),* } }
        }
    };
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum StoredTheme {
    System,
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum StoredMenu {
    #[default]
    Bar,
    Compact,
}

impl StoredMenu {
    fn omitted(&self) -> bool {
        matches!(self, StoredMenu::Bar)
    }
}

// Slint owns the UI enums; only these lowercase variants reach disk.
macro_rules! menu_bridge {
    ($from:ty, $to:ident) => {
        impl From<$from> for $to {
            fn from(menu: $from) -> Self {
                if matches!(menu, <$from>::Compact) {
                    Self::Compact
                } else {
                    Self::Bar
                }
            }
        }
    };
}
menu_bridge!(MenuLayout, StoredMenu);
menu_bridge!(StoredMenu, MenuLayout);
impl From<ThemePreference> for StoredTheme {
    fn from(theme: ThemePreference) -> Self {
        if theme == ThemePreference::Dark {
            Self::Dark
        } else if theme == ThemePreference::Light {
            Self::Light
        } else {
            Self::System
        }
    }
}
impl From<StoredTheme> for ThemePreference {
    fn from(theme: StoredTheme) -> Self {
        match theme {
            StoredTheme::System => Self::System,
            StoredTheme::Light => Self::Light,
            StoredTheme::Dark => Self::Dark,
        }
    }
}

fn currency_fallback() -> String {
    "$".to_owned()
}
fn currency_omitted(symbol: &str) -> bool {
    symbol == "$"
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LookRecord {
    #[serde(default)]
    theme: StoredTheme,
    #[serde(default, skip_serializing_if = "LanguagePreference::is_system")]
    language: LanguagePreference,
    #[serde(default, skip_serializing_if = "StoredMenu::omitted")]
    menu_layout: StoredMenu,
    #[serde(
        default = "currency_fallback",
        skip_serializing_if = "currency_omitted"
    )]
    currency_symbol: String,
}
stored_defaults!(LookRecord {
    theme: StoredTheme::default(),
    language: LanguagePreference::default(),
    menu_layout: StoredMenu::default(),
    currency_symbol: currency_fallback(),
});
pub(crate) const SETTINGS_FORMAT_VERSION: u32 = 1;

fn schema_fallback() -> u32 {
    SETTINGS_FORMAT_VERSION
}
fn aa_fallback() -> AaLevel {
    AaLevel::Off
}
fn aa_omitted(level: &AaLevel) -> bool {
    *level == AaLevel::Off
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AppSettings {
    #[serde(default)]
    pub orient_preset: i32,
    #[serde(default)]
    pub perspective_view: bool,
    #[serde(default)]
    pub setup_complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_license_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_license_date: Option<String>,
    #[serde(default = "schema_fallback")]
    pub(crate) version: u32,
    #[serde(default)]
    pub appearance: LookRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_printer: Option<PrinterId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_resin: Option<MaterialId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    favourite_printers: BTreeSet<PrinterId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    favourite_resins: BTreeSet<MaterialId>,
    #[serde(default = "aa_fallback", skip_serializing_if = "aa_omitted")]
    aa_level: AaLevel,
    #[serde(default, skip_serializing_if = "SavedPanel::is_empty")]
    object_list: SavedPanel,
    #[serde(default, skip_serializing_if = "SavedPanel::is_empty")]
    slice_panel: SavedPanel,
    #[serde(default, skip_serializing_if = "SavedPanel::is_empty")]
    slice_view: SavedPanel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_open_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    recent_files: Vec<PathBuf>,
}

stored_defaults!(AppSettings {
    orient_preset: 0,
    perspective_view: false,
    setup_complete: false,
    accepted_license_version: None,
    accepted_license_date: None,
    version: SETTINGS_FORMAT_VERSION,
    appearance: LookRecord::default(),
    default_printer: None,
    default_resin: None,
    favourite_printers: BTreeSet::new(),
    favourite_resins: BTreeSet::new(),
    aa_level: aa_fallback(),
    object_list: SavedPanel::default(),
    slice_panel: SavedPanel::default(),
    slice_view: SavedPanel::default(),
    last_open_dir: None,
    recent_files: Vec::new(),
});

// Returning owned values lets callers edit snapshots without borrowing settings.
macro_rules! stored_value {
    ($get:ident, $set:ident, $ty:ty, $($field:ident).+) => {
        pub(crate) fn $get(&self) -> $ty { self.$($field).+.clone() }
        pub(crate) fn $set(&mut self, value: $ty) { self.$($field).+ = value; }
    };
}
macro_rules! copied_value {
    ($get:ident, $set:ident, $ty:ty, $($field:ident).+) => {
        pub(crate) fn $get(&self) -> $ty { self.$($field).+ }
        pub(crate) fn $set(&mut self, value: $ty) { self.$($field).+ = value; }
    };
}
impl AppSettings {
    /// Where `panel` is, as saved (its start place for anything unsaved).
    pub(crate) fn panel(&self, panel: Panel) -> PanelPlace {
        self.saved_panel(panel).place(panel)
    }

    pub(crate) fn set_panel(&mut self, panel: Panel, place: PanelPlace) {
        *self.saved_panel_mut(panel) = SavedPanel::record(panel, place);
    }

    fn saved_panel(&self, panel: Panel) -> &SavedPanel {
        match panel {
            Panel::ObjectList => &self.object_list,
            Panel::SliceControls => &self.slice_panel,
            Panel::SliceView => &self.slice_view,
        }
    }

    fn saved_panel_mut(&mut self, panel: Panel) -> &mut SavedPanel {
        match panel {
            Panel::ObjectList => &mut self.object_list,
            Panel::SliceControls => &mut self.slice_panel,
            Panel::SliceView => &mut self.slice_view,
        }
    }

    stored_value!(
        default_printer,
        set_default_printer,
        Option<PrinterId>,
        default_printer
    );
    stored_value!(
        default_resin,
        set_default_resin,
        Option<MaterialId>,
        default_resin
    );
    stored_value!(
        favourite_printers,
        set_favourite_printers,
        BTreeSet<PrinterId>,
        favourite_printers
    );
    stored_value!(
        favourite_resins,
        set_favourite_resins,
        BTreeSet<MaterialId>,
        favourite_resins
    );
    stored_value!(
        last_open_dir,
        set_last_open_dir,
        Option<PathBuf>,
        last_open_dir
    );
    stored_value!(recent_files, set_recent_files, Vec<PathBuf>, recent_files);
    copied_value!(
        language,
        set_language,
        LanguagePreference,
        appearance.language
    );
    copied_value!(aa_level, set_aa_level, AaLevel, aa_level);

    pub(crate) fn theme(&self) -> ThemePreference {
        self.appearance.theme.into()
    }
    pub(crate) fn set_theme(&mut self, value: ThemePreference) {
        self.appearance.theme = value.into();
    }
    pub(crate) fn menu_layout(&self) -> MenuLayout {
        self.appearance.menu_layout.into()
    }
    pub(crate) fn set_menu_layout(&mut self, value: MenuLayout) {
        self.appearance.menu_layout = value.into();
    }
    pub(crate) fn currency_symbol(&self) -> String {
        self.appearance.currency_symbol.clone()
    }
    pub(crate) fn set_currency_symbol(&mut self, mut value: String) {
        if value.is_empty() {
            value = currency_fallback();
        }
        self.appearance.currency_symbol = value;
    }
    pub(crate) fn load() -> Self {
        config_file().map_or_else(Self::default, |file| Self::load_from_path(&file))
    }
    pub(crate) fn load_from_path(path: &Path) -> Self {
        let source = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    eprintln!("Failed to read {}: {error}", path.display());
                }
                return Self::default();
            }
        };
        match toml::from_str::<Self>(&source) {
            Ok(mut settings) if settings.version == SETTINGS_FORMAT_VERSION => {
                // The old System default changes only before setup has been completed.
                if !settings.setup_complete
                    && matches!(settings.appearance.theme, StoredTheme::System)
                {
                    settings.set_theme(ThemePreference::Light);
                }
                return settings;
            }
            Ok(settings) => eprintln!(
                "Settings schema version in {} is {} (current is {}); resetting to defaults",
                path.display(),
                settings.version,
                SETTINGS_FORMAT_VERSION,
            ),
            Err(error) => eprintln!(
                "Failed to parse {}: {error}; resetting to defaults",
                path.display()
            ),
        }
        let replacement = Self::default();
        if let Err(error) = replacement.save_to_path(path) {
            eprintln!(
                "Failed to overwrite {} with fresh defaults: {error}",
                path.display()
            );
        }
        replacement
    }
    pub(crate) fn save(&self) -> io::Result<()> {
        match config_file() {
            Some(file) => self.save_to_path(&file),
            None => Err(io::Error::other("no config directory available")),
        }
    }
    pub(crate) fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        let encoded = toml::to_string_pretty(self).map_err(io::Error::other)?;
        replace_file(path, encoded.as_bytes())
    }
}
fn config_file() -> Option<PathBuf> {
    Some(
        sc_app_paths::project_dirs()?
            .config_dir()
            .join(SETTINGS_FILE),
    )
}
static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn staging_file(target: &Path) -> io::Result<PathBuf> {
    let leaf = target
        .file_name()
        .ok_or_else(|| io::Error::other("settings path has no file name"))?;
    let directory = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(directory.join(format!(
        ".{}.tmp.{}.{}",
        leaf.to_string_lossy(),
        std::process::id(),
        WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )))
}
fn replace_file(target: &Path, contents: &[u8]) -> io::Result<()> {
    let staging = staging_file(target)?;
    let result = (|| {
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)?;
        output.write_all(contents)?;
        output.flush()?;
        output.sync_all()?;
        drop(output);
        fs::rename(&staging, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result
}
#[cfg(test)]
#[path = "tests_app_settings.rs"]
mod tests;
