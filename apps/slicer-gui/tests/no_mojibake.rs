// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Guards the GUI's sources against mojibake: UTF-8 text that was decoded as
//! Windows-1252 and re-encoded (possibly several times) by an editing tool.
//! It happened once: every "…" and "·" in `ui/main-window.slint` turned into
//! strings like "Ãƒ…" that showed on screen as garbage (menus, load, save and
//! support-progress panels). This test fails the build if it comes back.

use std::fs;
use std::path::Path;

/// Byte patterns that only appear in mis-decoded UTF-8: "Ã" (U+00C3) or "â"
/// (U+00E2) followed by a Windows-1252 punctuation glyph that UTF-8 lead/
/// continuation bytes map to. Written as escapes so the scanned sources never
/// need to contain them.
const MARKERS: [&str; 3] = [
    "\u{c3}\u{192}",  // "Ãƒ"
    "\u{c3}\u{201a}", // "Ã‚"
    "\u{e2}\u{20ac}", // "â€"
];

fn looks_mojibaked(text: &str) -> Option<&'static str> {
    MARKERS.iter().copied().find(|m| text.contains(m))
}

fn scan(dir: &Path, extensions: &[&str], hits: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, extensions, hits);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| extensions.contains(&e))
            && let Ok(text) = fs::read_to_string(&path)
        {
            for (number, line) in text.lines().enumerate() {
                if let Some(marker) = looks_mojibaked(line) {
                    hits.push(format!("{}:{} ({marker:?})", path.display(), number + 1));
                }
            }
        }
    }
}

#[test]
fn gui_sources_contain_no_mojibake() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut hits = Vec::new();
    scan(&root.join("ui"), &["slint"], &mut hits);
    scan(&root.join("src"), &["rs"], &mut hits);
    scan(&root.join("i18n"), &["pot", "po"], &mut hits);
    assert!(
        hits.is_empty(),
        "mis-encoded text (UTF-8 decoded as Windows-1252) in:\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_detector_catches_the_real_case() {
    // The exact corruption found in main-window.slint ("…" encoded 3 times).
    let corrupted = "Open\u{c3}\u{192}\u{c2}\u{a2}\u{c3}\u{a2}\u{e2}\u{20ac}\u{161}\u{c2}\u{ac}";
    assert!(looks_mojibaked(corrupted).is_some());
    assert!(looks_mojibaked("Open\u{2026} \u{b7} 45\u{b0} mm\u{b2}").is_none());
}

#[test]
fn translation_template_has_unique_context_and_message_keys() {
    use std::collections::HashSet;
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("i18n/sc-slicer-gui.pot"))
            .unwrap();
    let mut keys = HashSet::new();
    for block in source.split("\n\n") {
        let mut context = String::new();
        let mut message = String::new();
        let mut field = 0;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("msgctxt ") {
                context = serde_json::from_str::<String>(value).unwrap();
                field = 1;
            } else if let Some(value) = line.strip_prefix("msgid ") {
                message = serde_json::from_str::<String>(value).unwrap();
                field = 2;
            } else if line.starts_with('"') {
                let value = serde_json::from_str::<String>(line).unwrap();
                match field {
                    1 => context.push_str(&value),
                    2 => message.push_str(&value),
                    _ => {}
                }
            } else if !line.starts_with('#') {
                field = 0;
            }
        }
        if !message.is_empty() {
            assert!(
                keys.insert((context.clone(), message.clone())),
                "duplicate translation template key: {context:?}, {message:?}"
            );
        }
    }
}
