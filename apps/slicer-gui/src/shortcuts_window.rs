// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Content and window driver for the read-only "Keyboard Shortcuts"
//! dialog (Help > Keyboard Shortcuts / Ctrl+/).
//!
//! [`shortcut_sections`] is the single source of truth for what the
//! dialog shows -- kept as a plain Rust data structure (not built
//! directly into Slint `ModelRc` values) so it stays unit-testable
//! without a live window, and so a future shortcut this app adds has
//! one obvious place to also add here. It deliberately only lists
//! bindings that actually exist elsewhere in the app (menu
//! `shortcut:` bindings, and the mesh viewer's own `key-pressed`
//! handler) rather than aspirational ones: every row here should be
//! something the user can press right now and see happen.

use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::preferences_bus::PreferenceHub;
use crate::window_prefs::SideWindow;
use crate::{ShortcutGroup, ShortcutRow, ShortcutsDialog};

/// One shortcut row, before conversion to the Slint struct.
pub(crate) struct ShortcutEntry {
    pub action: &'static str,
    pub keys: String,
}

/// One titled group of rows, before conversion to the Slint struct.
pub(crate) struct ShortcutSection {
    pub title: &'static str,
    pub entries: Vec<ShortcutEntry>,
}

/// The modifier key label for a plain-Ctrl binding: "Cmd" on macOS,
/// "Ctrl" everywhere else. Every menu shortcut in this app already
/// makes this same substitution structurally (`@keys(Control + ...)`
/// renders as the platform's own convention), so the dialog's display
/// text needs to make it explicitly since it is plain strings, not
/// real key-binding expressions.
fn ctrl_label(is_macos: bool) -> &'static str {
    if is_macos { "Cmd" } else { "Ctrl" }
}

/// Builds the full shortcut list. `is_macos` only affects display
/// labels (Cmd vs Ctrl, and the platform-specific Fullscreen binding
/// mirrored from `main-window.slint`'s own `root.is-macos` branch);
/// every binding listed is real on every platform.
pub(crate) fn shortcut_sections(is_macos: bool) -> Vec<ShortcutSection> {
    let ctrl = ctrl_label(is_macos);
    let entry = |action: &'static str, keys: String| ShortcutEntry { action, keys };

    vec![
        ShortcutSection {
            title: "File",
            entries: vec![
                entry("New window", format!("{ctrl}+Shift+N")),
                entry("Open…", format!("{ctrl}+O")),
                entry("Close", format!("{ctrl}+W")),
                entry("Save", format!("{ctrl}+S")),
                entry("Save As…", format!("{ctrl}+Shift+S")),
                entry("Export As…", format!("{ctrl}+Shift+E")),
                entry("Slice plate", format!("{ctrl}+R")),
                entry("Export printer job…", format!("{ctrl}+G")),
            ],
        },
        ShortcutSection {
            title: "Edit",
            entries: vec![
                entry("Undo", format!("{ctrl}+Z")),
                entry("Redo", format!("{ctrl}+Shift+Z")),
                entry("Duplicate selected object", format!("{ctrl}+D")),
                entry("Delete selected object", "Delete".to_owned()),
                entry("Preferences", format!("{ctrl}+,")),
            ],
        },
        ShortcutSection {
            title: "View",
            entries: vec![
                entry("Show/hide object list", format!("{ctrl}+L")),
                entry(
                    "Fullscreen",
                    if is_macos {
                        format!("{ctrl}+Control+F")
                    } else {
                        "F11".to_owned()
                    },
                ),
            ],
        },
        ShortcutSection {
            title: "Viewport camera",
            entries: vec![
                entry(
                    "Rotate",
                    "Left-drag, Shift+right-drag or Arrow keys".to_owned(),
                ),
                entry(
                    "Pan",
                    "Right-drag, middle-drag, or Shift+Arrow keys".to_owned(),
                ),
                entry("Zoom", "Scroll, +/-, or Page Up/Down".to_owned()),
                entry("Frame selection / default view", "F, Home or 0".to_owned()),
                entry("Front / Right / Top view", "1 / 3 / 7".to_owned()),
                entry(
                    "Back / Left / Bottom view",
                    format!("{ctrl}+1 / {ctrl}+3 / {ctrl}+7"),
                ),
            ],
        },
        ShortcutSection {
            title: "Selection & transform",
            entries: vec![
                entry("Translate gizmo focus", "T".to_owned()),
                entry("Rotate gizmo focus", "R".to_owned()),
                entry("Scale gizmo focus", "S".to_owned()),
                entry(
                    "Nudge focused gizmo axis",
                    "Arrow keys / Page Up / Page Down".to_owned(),
                ),
                entry("Cycle selection", "Tab / Shift+Tab".to_owned()),
                entry("Copy / paste selection", format!("{ctrl}+C / {ctrl}+V")),
                entry("Clear selection", "Escape".to_owned()),
            ],
        },
        ShortcutSection {
            title: "Help",
            entries: vec![entry(
                "Keyboard shortcuts (this dialog)",
                format!("{ctrl}+/ or ?"),
            )],
        },
    ]
}

/// Converts [`shortcut_sections`]'s output into the `ModelRc` the
/// dialog's `groups` property takes.
pub(crate) fn shortcut_groups_model(is_macos: bool) -> ModelRc<ShortcutGroup> {
    let groups = shortcut_sections(is_macos)
        .into_iter()
        .map(|section| ShortcutGroup {
            title: SharedString::from(section.title),
            rows: ModelRc::new(VecModel::from(
                section
                    .entries
                    .into_iter()
                    .map(|entry| ShortcutRow {
                        action: SharedString::from(entry.action),
                        keys: SharedString::from(entry.keys),
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect::<Vec<_>>();
    ModelRc::new(VecModel::from(groups))
}

/// Help > Keyboard Shortcuts. The list never changes while the app runs;
/// only theme and language follow the preferences.
pub(crate) fn build_shortcuts_window(
    bus: &Rc<PreferenceHub>,
) -> Result<SideWindow<ShortcutsDialog>, slint::PlatformError> {
    let dialog = ShortcutsDialog::new()?;
    dialog.set_groups(shortcut_groups_model(cfg!(target_os = "macos")));
    Ok(SideWindow::follow(bus, dialog, |w, done| w.on_close_requested(done)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn every_section_has_at_least_one_row() {
        for section in shortcut_sections(false) {
            assert!(
                !section.entries.is_empty(),
                "section {:?} has no rows",
                section.title
            );
        }
    }

    #[test]
    fn non_macos_uses_ctrl_label() {
        let sections = shortcut_sections(false);
        let file = sections.iter().find(|s| s.title == "File").unwrap();
        let open = file.entries.iter().find(|e| e.action == "Open…").unwrap();
        assert_eq!(open.keys, "Ctrl+O");
    }

    #[test]
    fn macos_uses_cmd_label() {
        let sections = shortcut_sections(true);
        let file = sections.iter().find(|s| s.title == "File").unwrap();
        let open = file.entries.iter().find(|e| e.action == "Open…").unwrap();
        assert_eq!(open.keys, "Cmd+O");
    }

    #[test]
    fn fullscreen_binding_differs_by_platform() {
        let non_macos = shortcut_sections(false);
        let macos = shortcut_sections(true);
        let find_fullscreen = |sections: &[ShortcutSection]| -> String {
            sections
                .iter()
                .find(|s| s.title == "View")
                .unwrap()
                .entries
                .iter()
                .find(|e| e.action == "Fullscreen")
                .unwrap()
                .keys
                .clone()
        };
        assert_eq!(find_fullscreen(&non_macos), "F11");
        assert_eq!(find_fullscreen(&macos), "Cmd+Control+F");
    }

    #[test]
    fn shortcut_groups_model_preserves_section_count_and_titles() {
        let model = shortcut_groups_model(false);
        let sections = shortcut_sections(false);
        assert_eq!(model.row_count(), sections.len());
        for (i, section) in sections.iter().enumerate() {
            let group = model.row_data(i).unwrap();
            assert_eq!(group.title.as_str(), section.title);
            assert_eq!(group.rows.row_count(), section.entries.len());
        }
    }
}
