// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Builds the UI: compiles the Slint files with their translations,
//! writes the table of languages the language selector offers, and on
//! Windows embeds the icon and version.
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use slint_build::{CompilerConfiguration, compile_with_config};

const TRANSLATIONS_DIR: &str = "i18n";
const LANGUAGE_NAMES_TOML: &str = "i18n/locale-names.toml";
const PO_FILE_NAME: &str = "sc-slicer-gui.po";

/// A language with less of its text translated than this (fuzzy listing
/// not counted) is left out of the selector: picking it would show a
/// mostly English UI anyway. `msgmerge` adds new languages at 0%.
const MIN_TRANSLATED_PERCENT: u32 = 50;

/// The language the `msgid`s are written in. It has no `i18n/` folder (a
/// `.po` would translate it to itself), so its selector entry is made
/// here, letting it be chosen explicitly and not only by the system
/// locale.
const UNTRANSLATED_CODE: &str = "en";

#[cfg(windows)]
const APP_ICON_ICO: &str = "assets/icons/sleptic.ico";

fn main() {
    // Slint's resolver recurses deeply; the default 1 MiB main stack
    // overflows on Windows, so the build runs on a thread with more.
    const STACK_BYTES: usize = 8 << 20;
    let worker = std::thread::Builder::new().stack_size(STACK_BYTES).spawn(build_ui);
    let finished = worker.expect("could not start the UI build thread").join();
    finished.expect("the UI build thread panicked");
}

fn build_ui() {
    #[cfg(windows)]
    embed_icon_and_version();
    generate_license();
    generate_language_enum();
    let slint_config =
        CompilerConfiguration::new().with_debug_info(true).with_bundled_translations(TRANSLATIONS_DIR);
    compile_with_config("ui/main-window.slint", slint_config).expect("the Slint UI did not compile");
}

/// The icon and the `CARGO_PKG_*` version, for Explorer, the taskbar and
/// shortcuts.
#[cfg(windows)]
fn embed_icon_and_version() {
    println!("cargo:rerun-if-changed={APP_ICON_ICO}");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(APP_ICON_ICO);
    res.compile().expect("could not embed the icon and version in the executable");
}

/// A language offered in the selector.
struct OfferedLanguage {
    /// Its `BundledLanguage` variant.
    variant: String,
    /// Its folder name under `i18n/`.
    code: String,
    /// Its name in itself, from `locale-names.toml`.
    native_name: String,
}

impl OfferedLanguage {
    fn new(code: &str, native_name: &str) -> Self {
        Self {
            variant: pascal_case_variant(code),
            code: code.to_owned(),
            native_name: native_name.to_owned(),
        }
    }
}

/// Writes `OUT_DIR/language_table.rs`, the `BundledLanguage` enum the app
/// `include!`s: every translated-enough language under `i18n/` plus the
/// source language, by code. A translated language without a name in
/// `locale-names.toml` stops the build; a name without a folder is only
/// warned about, so removing a translation does not break the build.
fn generate_language_enum() {
    println!("cargo:rerun-if-changed={TRANSLATIONS_DIR}");
    println!("cargo:rerun-if-changed={LANGUAGE_NAMES_TOML}");
    let folders = translated_codes(Path::new(TRANSLATIONS_DIR));
    let names = read_language_names(Path::new(LANGUAGE_NAMES_TOML));

    let mut offered = Vec::with_capacity(folders.len());
    for code in &folders {
        let po_file: PathBuf = [TRANSLATIONS_DIR, code.as_str(), "LC_MESSAGES", PO_FILE_NAME].iter().collect();
        let tally = count_translated(&po_file);
        let percent = tally.share_percent();
        if percent < MIN_TRANSLATED_PERCENT {
            println!(
                "cargo:warning=skipping locale '{code}' from the language selector: {percent}% \
                 translated ({}/{}); threshold is {MIN_TRANSLATED_PERCENT}%",
                tally.done, tally.messages,
            );
            continue;
        }
        let Some(name) = names.get(code) else {
            panic!("{TRANSLATIONS_DIR}/{code}/ is translated but {LANGUAGE_NAMES_TOML} has no name for '{code}'; add one");
        };
        offered.push(OfferedLanguage::new(code, name));
    }
    for code in names
        .keys()
        .filter(|code| *code != UNTRANSLATED_CODE && !folders.contains(code))
    {
        println!(
            "cargo:warning={LANGUAGE_NAMES_TOML} has entry '{code}' but no matching {TRANSLATIONS_DIR}/{code}/ directory"
        );
    }
    // Choosing the source language finds no bundle at run time, and
    // `LanguagePreference::apply` then falls back to the untranslated
    // strings, which are that language. A real `i18n/en/` would be
    // offered above instead.
    if !offered
        .iter()
        .any(|locale| locale.code == UNTRANSLATED_CODE)
    {
        let Some(name) = names.get(UNTRANSLATED_CODE) else {
            panic!("{LANGUAGE_NAMES_TOML} needs a name for '{UNTRANSLATED_CODE}', the language the texts are written in");
        };
        offered.push(OfferedLanguage::new(UNTRANSLATED_CODE, name));
    }
    offered.sort_by(|a, b| a.code.cmp(&b.code));
    let generated = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("language_table.rs");
    fs::write(&generated, language_enum_source(&offered)).expect("could not write language_table.rs");
}

/// A locale code as a PascalCase identifier; `-` and `_` start a new
/// word, so `pt-BR` becomes `PtBr`.
fn pascal_case_variant(code: &str) -> String {
    let ident: String = code
        .split(['-', '_'])
        .flat_map(|word| {
            let mut letters = word.chars();
            let first = letters.next().into_iter().flat_map(char::to_uppercase);
            first.chain(letters.flat_map(char::to_lowercase))
        })
        .collect();
    assert!(
        ident.chars().next().is_some_and(char::is_alphabetic),
        "locale code {code:?} does not yield a valid Rust identifier ({ident:?})",
    );
    ident
}

/// The generated module's source.
fn language_enum_source(locales: &[OfferedLanguage]) -> String {
    let mut out = String::from(
        "// Written by apps/slicer-gui/build.rs from the i18n/ folders and\n\
         // i18n/locale-names.toml; edits here are lost on the next build.\n\n\
         /// A language the app ships a translation for (or the untranslated\n\
         /// one), so every `match` on it must handle each.\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n\
         pub(crate) enum BundledLanguage {\n",
    );
    let each = |out: &mut String, line: &dyn Fn(&OfferedLanguage) -> String| {
        for locale in locales {
            out.push_str(&line(locale));
        }
    };
    each(&mut out, &|l| format!("    {},\n", l.variant));
    out.push_str("}\n\nimpl BundledLanguage {\n");
    out.push_str("    /// Every language, sorted by code.\n");
    out.push_str("    pub(crate) const ALL: &'static [BundledLanguage] = &[\n");
    each(&mut out, &|l| format!("        Self::{},\n", l.variant));
    out.push_str("    ];\n\n");
    let accessors: [(&str, &str, fn(&OfferedLanguage) -> &str); 2] = [
        (
            "The locale code, matching the directory name under `i18n/`.",
            "code",
            |l| &l.code,
        ),
        (
            "The language name spelled in the language itself.",
            "native_name",
            |l| &l.native_name,
        ),
    ];
    for (doc, name, field) in accessors {
        let _ = write!(
            out,
            "    /// {doc}\n    pub(crate) const fn {name}(self) -> &'static str {{\n        match self {{\n"
        );
        each(&mut out, &|l| {
            format!(
                "            Self::{} => {},\n",
                l.variant,
                quoted_literal(field(l))
            )
        });
        out.push_str("        }\n    }\n\n");
    }
    out.push_str("    /// The language with this code, if the app has it.\n");
    out.push_str(
        "    pub(crate) fn from_code(code: &str) -> Option<Self> {\n        match code {\n",
    );
    each(&mut out, &|l| {
        format!(
            "            {} => Some(Self::{}),\n",
            quoted_literal(&l.code),
            l.variant
        )
    });
    out.push_str("            _ => None,\n        }\n    }\n}\n");
    out
}

/// The codes of the folders in `root` that hold a `LC_MESSAGES/` `.po`,
/// sorted.
fn translated_codes(root: &Path) -> Vec<String> {
    let listing = fs::read_dir(root).unwrap_or_else(|e| panic!("cannot list {}: {e}", root.display()));
    let mut codes: Vec<String> = listing
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("cannot list {}: {e}", root.display()))
                .path()
        })
        .filter(|dir| dir.is_dir() && dir.join("LC_MESSAGES").join(PO_FILE_NAME).is_file())
        .filter_map(|dir| dir.file_name()?.to_str().map(str::to_owned))
        .collect();
    codes.sort();
    codes
}

fn read_language_names(path: &Path) -> BTreeMap<String, String> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{} is not a table of code = \"name\": {e}", path.display()))
}

/// Messages in a `.po` file, and how many of them are translated.
struct PoTally {
    done: u32,
    messages: u32,
}

impl PoTally {
    /// Whole percent translated; an empty file counts as 0.
    fn share_percent(&self) -> u32 {
        (self.done * 100).checked_div(self.messages).unwrap_or(0)
    }
}

/// Which part of an entry the quoted lines continue.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Neither,
    Source,
    Translation,
}

/// The entry being read.
struct Entry {
    started: bool,
    fuzzy: bool,
    has_source: bool,
    has_translation: bool,
    part: Part,
}

impl Entry {
    const BLANK: Self = Self {
        started: false,
        fuzzy: false,
        has_source: false,
        has_translation: false,
        part: Part::Neither,
    };

    /// Adds `text` (quoted) to the current part.
    fn take(&mut self, part: Part, text: &str) {
        self.part = part;
        self.note(text);
    }

    fn note(&mut self, text: &str) {
        let filled = text.trim() != "\"\"";
        match self.part {
            Part::Source => self.has_source |= filled,
            Part::Translation => self.has_translation |= filled,
            Part::Neither => {}
        }
    }
}

/// Counts a `.po` file's messages the way `msgfmt --statistics` does: the
/// header (empty `msgid`) is not a message, and a message is translated
/// when some `msgstr` of it has text and it is not marked fuzzy. A few
/// lines of parsing, rather than a PO crate or a `msgfmt` that may not
/// be installed.
fn count_translated(po_path: &Path) -> PoTally {
    let text = fs::read_to_string(po_path).unwrap_or_else(|e| panic!("cannot read {}: {e}", po_path.display()));
    let mut tally = PoTally { done: 0, messages: 0 };
    let mut entry = Entry::BLANK;
    let mut finish = |entry: &mut Entry| {
        if entry.started && entry.has_source {
            tally.messages += 1;
            tally.done += u32::from(entry.has_translation && !entry.fuzzy);
        }
        *entry = Entry::BLANK;
    };
    for line in text.lines().map(str::trim_start) {
        if line.is_empty() {
            finish(&mut entry);
        } else if let Some(comment) = line.strip_prefix('#') {
            // Only the `#,` flags line matters; other comments are notes.
            if comment
                .strip_prefix(',')
                .is_some_and(|flags| flags.split(',').any(|f| f.trim() == "fuzzy"))
            {
                entry.fuzzy = true;
            }
        } else if line.starts_with("msgctxt ") {
            entry.started = true;
            entry.part = Part::Neither;
        } else if let Some(text) = line.strip_prefix("msgid_plural ") {
            entry.take(Part::Source, text);
        } else if let Some(text) = line.strip_prefix("msgid ") {
            entry.started = true;
            entry.take(Part::Source, text);
        } else if let Some(text) = line.strip_prefix("msgstr ") {
            entry.take(Part::Translation, text);
        } else if let Some(indexed) = line.strip_prefix("msgstr[") {
            let text = indexed.split_once(']').map_or("", |(_, text)| text.trim());
            entry.take(Part::Translation, text);
        } else if line.starts_with('"') {
            entry.note(line);
        }
    }
    // The last entry may not end in a blank line.
    finish(&mut entry);
    drop(finish);
    tally
}

/// `s` as a Rust string literal, with `\` and `"` escaped (native names
/// have neither today; the generated file must compile regardless).
fn quoted_literal(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn generate_license() {
    let config_path = Path::new("assets/license/ACCEPTANCE.toml");
    println!("cargo:rerun-if-changed={}", config_path.display());
    let config: toml::Value = toml::from_str(&fs::read_to_string(config_path).unwrap()).unwrap();
    let text = config["text_file"].as_str().expect("license text_file");
    let text_path = fs::canonicalize(config_path.parent().unwrap().join(text)).expect("license text file");
    println!("cargo:rerun-if-changed={}", text_path.display());
    let code = format!("const LICENSE_VERSION: &str = {:?};\nconst LICENSE_REQUIRED: bool = {};\nconst LICENSE_TEXT: &str = include_str!({:?});\n",
        config["version"].as_str().expect("license version"), config["required"].as_bool().expect("license required"), text_path.to_string_lossy());
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("license.rs"), code).unwrap();
}
