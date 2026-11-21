// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_profiles_converter::{args, options, pipeline};

fn main() -> anyhow::Result<()> {
    let choices = args::cli_parser().run();
    let job = options::ConversionRequest::from_raw(choices)?;
    eprintln!("{}", job.summary());
    let totals = pipeline::convert(&job)?;
    let destination = job.out().display();
    let (printers, materials, images) = (totals.printers, totals.materials, totals.images);
    eprintln!("converted {printers} printer(s), {materials} material(s), {images} image(s) into {destination}");
    Ok(())
}
