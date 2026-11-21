// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::options::RawCliArgs;
use bpaf::{OptionParser, Parser, construct, long};
use std::path::PathBuf;

fn source_format() -> impl Parser<String> {
    long("format")
        .short('f')
        .help("Input format: json")
        .argument::<String>("FORMAT")
        .guard(
            |name| name == "json",
            "Unsupported profile format; choose json",
        )
}

fn source_paths() -> impl Parser<Vec<PathBuf>> {
    long("input")
        .short('i')
        .help("Source file or directory; may be repeated")
        .argument::<PathBuf>("PATH")
        .many()
}

fn destination() -> impl Parser<PathBuf> {
    long("out")
        .short('o')
        .help("Destination Sleptic profile library")
        .argument::<PathBuf>("DIR")
}

pub fn cli_parser() -> OptionParser<RawCliArgs> {
    let images = long("with-images")
        .help("Copy printer preview images into the library")
        .switch();
    construct!(source_format(), source_paths(), destination(), images)
        .map(|(format, inputs, out, with_images)| RawCliArgs {
            format,
            inputs,
            out,
            with_images,
        })
        .to_options()
        .descr("Convert printer and resin profiles into Sleptic TOML libraries")
        .version(env!("CARGO_PKG_VERSION"))
}
