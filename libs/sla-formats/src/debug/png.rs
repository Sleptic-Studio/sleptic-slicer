// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{Format, WriterError, WriterOptions};
use rayon::prelude::*;
use sc_sl1cer::contour::{ContourLayer, ContourStack};
use std::{io::Write, path::Path};
pub(crate) fn pixels(l: &ContourLayer, i: usize, o: &WriterOptions) -> Vec<u8> {
    let r = sc_sl1cer::raster::accuracy::rasterize_accurate(
        l,
        &o.pixel_map(),
        o.aa(),
        o.min_alpha_pct(),
        o.accuracy(),
        i,
    );
    let w = r.width() as usize;
    let h = r.height() as usize;
    let mut data = vec![0; w * h];
    let mut at = 0;
    // Raster rows are bottom-up; PNG scanlines are top-down.
    for run in r.runs() {
        let end = at + run.length as usize;
        while at < end {
            let y = at / w;
            let x = at % w;
            let n = (end - at).min(w - x);
            data[(h - 1 - y) * w + x..(h - 1 - y) * w + x + n].fill(run.value);
            at += n;
        }
    }
    data
}
pub(crate) fn encode(
    w: impl Write,
    width: u32,
    height: u32,
    rgb: bool,
    data: &[u8],
    metadata: Vec<(String, String)>,
    format: Format,
) -> Result<(), WriterError> {
    let mut e = png::Encoder::new(w, width, height);
    e.set_color(if rgb {
        png::ColorType::Rgb
    } else {
        png::ColorType::Grayscale
    });
    e.set_depth(png::BitDepth::Eight);
    for (k, v) in metadata {
        e.add_text_chunk(k, v).map_err(|e| png_error(e, format))?;
    }
    let mut writer = e.write_header().map_err(|e| png_error(e, format))?;
    writer
        .write_image_data(data)
        .map_err(|e| png_error(e, format))?;
    writer.finish().map_err(|e| png_error(e, format))?;
    Ok(())
}
fn png_error(e: png::EncodingError, f: Format) -> WriterError {
    match e {
        png::EncodingError::IoError(e) => WriterError::Io(e),
        e => crate::error::encoding(f, e),
    }
}
pub(crate) fn metadata(o: &WriterOptions) -> Vec<(String, String)> {
    let b = o.build_volume();
    let r = o.pixel_resolution();
    let sc_sl1cer::profile::LayerSchedule::Uniform(s) = o.layer_schedule();
    vec![
        ("slicer-version", o.slicer_version().into()),
        ("build-volume-x-mm", b.x_mm().to_string()),
        ("build-volume-y-mm", b.y_mm().to_string()),
        ("build-volume-z-mm", b.z_mm().to_string()),
        ("pixel-resolution-x-px", r.x().to_string()),
        ("pixel-resolution-y-px", r.y().to_string()),
        (
            "layer-schedule",
            format!(
                "uniform layer-height-mm={} first-layer-height-mm={} total-layers={}",
                s.layer_height().value(),
                s.first_layer_height().value(),
                s.total_layers()
            ),
        ),
        ("aa-level", crate::options::aa_text(o.aa()).into()),
        ("mirror-x", o.mirror().x.to_string()),
        ("mirror-y", o.mirror().y.to_string()),
        ("min-alpha-pct", o.min_alpha_pct().to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect()
}
pub fn write_per_layer(s: &ContourStack, p: &Path, o: &WriterOptions) -> Result<(), WriterError> {
    s.layers().par_iter().enumerate().try_for_each(|(i, l)| {
        let map = o.pixel_map();
        let r = map.pixel_resolution();
        let mut m = metadata(o);
        m.push(("layer-z-mm".into(), l.z_mm().to_string()));
        m.push(("layer-index".into(), i.to_string()));
        let mut sink =
            std::io::BufWriter::new(std::fs::File::create(p.join(format!("layer_{i:04}.png")))?);
        encode(
            &mut sink,
            r.x(),
            r.y(),
            false,
            &pixels(l, i, o),
            m,
            Format::PngPerLayer,
        )?;
        sink.flush()?;
        Ok(())
    })
}
