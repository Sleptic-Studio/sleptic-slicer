// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::config::args::{Command, parser};

use super::*;

fn args(target: &Path, extra: &[&str]) -> SliceArgs {
    let target = target.to_str().unwrap();
    let mut words = vec![
        "slice",
        "-s",
        "part.stl",
        "-t",
        target,
        "--printer-profile",
        "p",
        "--material-profile",
        "m",
    ];
    words.extend_from_slice(extra);
    match parser().run_inner(words.as_slice()) {
        Ok(Command::Slice(args)) => *args,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_output_format_comes_from_the_flag_or_the_target_name() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("part.stl");
    std::fs::write(&source, b"solid").unwrap();

    let (format, path) = output(&args(&dir.path().join("job.zip"), &[]), &source).unwrap();
    assert_eq!(format, Format::GcodeZip);
    assert_eq!(path.file_name().unwrap(), "job.zip");

    let named = args(&dir.path().join("job.bin"), &["--target-format", "3mf"]);
    assert_eq!(output(&named, &source).unwrap().0, Format::ThreeMf);

    let unknown = output(&args(&dir.path().join("job.bin"), &[]), &source).unwrap_err();
    assert!(unknown.to_string().starts_with("could not infer target format from path"), "{unknown}");
}

#[test]
fn a_file_target_may_not_be_the_source_but_a_folder_target_is_left_to_its_writer() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("part.3mf");
    std::fs::write(&source, b"x").unwrap();
    assert!(output(&args(&source, &[]), &source).is_err());

    let layers = dir.path().join("missing").join("layers");
    let (format, path) =
        output(&args(&layers, &["--target-format", "png-per-layer"]), &source).unwrap();
    assert_eq!(format, Format::PngPerLayer);
    assert_eq!(path, layers, "folder targets are passed through unchecked");
}

#[test]
fn all_plates_refuses_a_plate_number() {
    let dir = tempfile::tempdir().unwrap();
    let libraries = ProfileLibraries::new(Some(dir.path().to_path_buf()), Vec::new());
    let both = args(&dir.path().join("out"), &["--all-plates", "--plate", "2"]);
    assert_eq!(
        slice(both, &libraries).unwrap_err().to_string(),
        "--plate and --all-plates cannot be combined"
    );
}
