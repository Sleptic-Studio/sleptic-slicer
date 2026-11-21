// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Runs the 3MF Consortium conformance suites through the 3MF reader.
//!
//! The suites are third-party and 2.7 GB, so they are not in the repository:
//! clone https://github.com/3MFConsortium/test_suites and point
//! `SC_3MF_SUITES` at it, then
//! `cargo test -p sc-mesh-formats --test conformance -- --ignored --nocapture`.
//! Each file is read on its own, a panic counted as an outcome; the results
//! go to `local/conformance/results.tsv` (suite, file, expected P/N, outcome,
//! error kind, message, warnings, milliseconds) and a summary per suite is
//! printed.
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn every_3mf(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            every_3mf(&path, into);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("3mf")) {
            into.push(path);
        }
    }
}

/// The error's variant path, e.g. `ThreeMf(Model(UnsupportedRequiredExtension`.
fn kind(debug: &str) -> String {
    debug.split(|c: char| c == '{' || c == ' ').next().unwrap_or("").to_owned()
}

#[test]
#[ignore = "needs the 3MF Consortium suites; set SC_3MF_SUITES"]
fn consortium_suites() {
    let Some(root) = std::env::var_os("SC_3MF_SUITES").map(PathBuf::from) else {
        eprintln!("SC_3MF_SUITES is not set; skipping");
        return;
    };
    let mut files = Vec::new();
    every_3mf(&root, &mut files);
    files.sort();
    std::panic::set_hook(Box::new(|_| {}));
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/conformance");
    fs::create_dir_all(&out_dir).unwrap();
    let mut table = fs::File::create(out_dir.join("results.tsv")).unwrap();
    writeln!(table, "suite\tfile\texpected\toutcome\tkind\tmessage\twarnings\tms").unwrap();
    // suite -> (expected, outcome) -> count
    let mut summary: BTreeMap<String, BTreeMap<(char, String), usize>> = BTreeMap::new();
    for path in &files {
        let relative = path.strip_prefix(&root).unwrap_or(path);
        let suite = relative.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let expected = name.chars().next().unwrap_or('?').to_ascii_uppercase();
        let started = Instant::now();
        let result = std::panic::catch_unwind(|| {
            let mut reader = BufReader::new(fs::File::open(path).unwrap());
            sc_mesh_formats::threemf::read_threemf(&mut reader)
        });
        let ms = started.elapsed().as_millis();
        let (outcome, kind_text, message, warnings) = match result {
            Err(_) => ("PANIC".to_owned(), String::new(), String::new(), 0),
            Ok(Ok((_, warnings))) => ("opened".to_owned(), String::new(), String::new(), warnings.len()),
            Ok(Err(error)) => {
                let message = error.to_string().replace(['\t', '\n'], " ");
                let unsupported = message.contains("unsupported 3MF extension") || message.contains("not yet supported");
                let outcome = if unsupported { "refused-unsupported" } else { "refused" };
                (outcome.to_owned(), kind(&format!("{error:?}")), message, 0)
            }
        };
        writeln!(table, "{suite}\t{name}\t{expected}\t{outcome}\t{kind_text}\t{message}\t{warnings}\t{ms}").unwrap();
        *summary.entry(suite).or_default().entry((expected, outcome)).or_default() += 1;
    }
    let _ = std::panic::take_hook();
    for (suite, counts) in &summary {
        let line: Vec<String> = counts.iter().map(|((e, o), n)| format!("{e} {o}: {n}")).collect();
        println!("{suite}: {}", line.join(", "));
    }
    println!("{} files", files.len());
}
