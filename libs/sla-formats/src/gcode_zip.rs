// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{Format, WriterError, WriterOptions, debug::png::encode};
use rayon::prelude::*;
use sc_sl1cer::{contour::ContourStack, geom2d::Point2D};
use std::{io::Write, path::Path};
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedJob {
    pub regions: Vec<([f64; 2], [f64; 2])>,
    pub layer_z_mm: Vec<f64>,
    pub exposure_s: Vec<f64>,
    pub script: String,
}
fn fail(s: &str) -> WriterError {
    crate::error::encoding(Format::GcodeZip, s)
}
fn validate(s: &ContourStack, j: &PreparedJob) -> Result<(), WriterError> {
    if j.layer_z_mm.len() != s.layers().len() || j.exposure_s.len() != s.layers().len() {
        return Err(fail("prepared job layer count differs from contour stack"));
    }
    if j.layer_z_mm.iter().any(|v| !v.is_finite())
        || j.layer_z_mm.windows(2).any(|v| v[0] >= v[1])
        || j.exposure_s.iter().any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err(fail("prepared job has invalid Z or exposure values"));
    }
    for (i, (a, b)) in j.regions.iter().enumerate() {
        if a.iter().chain(b).any(|v| !v.is_finite())
            || a[0] >= b[0]
            || a[1] >= b[1]
            || j.regions[..i]
                .iter()
                .any(|(c, d)| a[0] < d[0] && c[0] < b[0] && a[1] < d[1] && c[1] < b[1])
        {
            return Err(fail("invalid or overlapping exposure regions"));
        }
    }
    Ok(())
}
fn image(data: &[u8], w: u32, h: u32, rgb: bool) -> Result<Vec<u8>, WriterError> {
    let mut v = Vec::new();
    encode(&mut v, w, h, rgb, data, vec![], Format::GcodeZip)?;
    Ok(v)
}
fn zip_error(e: zip::result::ZipError) -> WriterError {
    match e {
        zip::result::ZipError::Io(e) => WriterError::Io(e),
        e => crate::error::encoding(Format::GcodeZip, e),
    }
}
pub fn write_gcode_zip(
    s: &ContourStack,
    p: &Path,
    o: &WriterOptions,
    j: &PreparedJob,
) -> Result<(), WriterError> {
    validate(s, j)?;
    crate::write::validate(p, Format::GcodeZip)?;
    let map = o.pixel_map();
    let r = map.pixel_resolution();
    let (w, h) = (r.x(), r.y());
    let e = w.max(h).max(256);
    let (pw, ph) = (
        (256_u64 * w as u64 / e as u64).max(1) as u32,
        (256_u64 * h as u64 / e as u64).max(1) as u32,
    );
    let mut preview = vec![0; (pw * ph) as usize];
    let mut z = zip::ZipWriter::new(std::io::BufWriter::new(std::fs::File::create(p)?));
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    let rects: Vec<_> = j
        .regions
        .iter()
        .map(|(a, b)| {
            let a = map.mm_to_pixel(Point2D::new(a[0], a[1]));
            let b = map.mm_to_pixel(Point2D::new(b[0], b[1]));
            (
                a.0.min(b.0).floor().clamp(0.0, w as f64) as usize,
                a.1.min(b.1).floor().clamp(0.0, h as f64) as usize,
                a.0.max(b.0).ceil().clamp(0.0, w as f64) as usize,
                a.1.max(b.1).ceil().clamp(0.0, h as f64) as usize,
            )
        })
        .collect();
    // Bounded batches preserve archive order while allowing rasterization and PNG compression on all workers.
    let batch = rayon::current_num_threads().saturating_mul(2).max(1);
    let mut buffers: Vec<_> = (0..batch).map(|_| LayerBuffers::default()).collect();
    for (start, layers) in s.layers().chunks(batch).enumerate() {
        let results: Vec<_> = layers
            .par_iter()
            .zip(buffers.par_iter_mut())
            .enumerate()
            .map(|(k, (l, buffers))| -> Result<_, WriterError> {
                let i = start * batch + k;
                let raster = sc_sl1cer::raster::accuracy::rasterize_accurate(
                    l,
                    &map,
                    o.aa(),
                    o.min_alpha_pct(),
                    o.accuracy(),
                    i,
                );
                expand_project(
                    &raster,
                    &mut buffers.data,
                    &mut buffers.small,
                    pw as usize,
                    ph as usize,
                );
                let mut images = vec![image(&buffers.data, w, h, false)?];
                for &(x0, y0, x1, y1) in &rects {
                    buffers.tile.resize(buffers.data.len(), 0);
                    buffers.tile.fill(0);
                    for y in y0..y1 {
                        let at = (h as usize - 1 - y) * w as usize;
                        buffers.tile[at + x0..at + x1]
                            .copy_from_slice(&buffers.data[at + x0..at + x1]);
                    }
                    images.push(image(&buffers.tile, w, h, false)?);
                }
                Ok((i, images))
            })
            .collect();
        for (result, buffers) in results.into_iter().zip(&buffers) {
            let (i, images) = result?;
            for (a, &b) in preview.iter_mut().zip(&buffers.small) {
                *a = (*a).max(b)
            }
            for (k, bytes) in images.into_iter().enumerate() {
                let name = if k == 0 {
                    format!("{}.png", i + 1)
                } else {
                    format!("{}-tile{k}.png", i + 1)
                };
                z.start_file(name, opt).map_err(zip_error)?;
                z.write_all(&bytes)?;
            }
        }
    }
    z.start_file("run.gcode", opt).map_err(zip_error)?;
    z.write_all(j.script.as_bytes())?;
    z.start_file("preview.png", opt).map_err(zip_error)?;
    let bytes = if let Some(rgb) = &o.preview {
        image(rgb, 256, 256, true)?
    } else {
        image(&preview, pw, ph, false)?
    };
    z.write_all(&bytes)?;
    let mut sink = z.finish().map_err(zip_error)?;
    sink.flush()?;
    Ok(())
}

#[derive(Default)]
struct LayerBuffers {
    data: Vec<u8>,
    tile: Vec<u8>,
    small: Vec<u8>,
}

// Project constant-valued spans instead of visiting every screen pixel. Splitting
// at row boundaries preserves the top-down PNG orientation and exact max bins.
fn expand_project(
    raster: &sc_sl1cer::raster::rle::LayerRaster,
    data: &mut Vec<u8>,
    small: &mut Vec<u8>,
    pw: usize,
    ph: usize,
) {
    let w = raster.width() as usize;
    let h = raster.height() as usize;
    data.resize(w * h, 0);
    small.resize(pw * ph, 0);
    small.fill(0);
    let mut at = 0;
    for run in raster.runs() {
        let end = at + run.length as usize;
        while at < end {
            let y = h - 1 - at / w;
            let x = at % w;
            let n = (end - at).min(w - x);
            data[y * w + x..y * w + x + n].fill(run.value);
            if run.value != 0 {
                let row = y * ph / h * pw;
                for v in &mut small[row + x * pw / w..=row + (x + n - 1) * pw / w] {
                    *v = (*v).max(run.value);
                }
            }
            at += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::expand_project;
    use sc_sl1cer::raster::rle::{LayerRaster, RleRun};

    #[test]
    fn span_projection_matches_pixels_across_rows_and_reused_buffers() {
        let (w, h, pw, ph) = (517, 293, 256, 145);
        let mut data = Vec::new();
        let mut small = Vec::new();
        for seed in [17usize, 29, 0] {
            let mut bottom_up = vec![0; w * h];
            let mut runs = Vec::new();
            let mut at = 0;
            while at < bottom_up.len() {
                let n = (1 + (at * 13 + 907) % 8191).min(bottom_up.len() - at);
                let value = (at * seed % 256) as u8;
                bottom_up[at..at + n].fill(value);
                runs.push(RleRun {
                    value,
                    length: n as u32,
                });
                at += n;
            }
            let raster = LayerRaster::new(0.0, w as u32, h as u32, runs);
            expand_project(&raster, &mut data, &mut small, pw, ph);
            let mut expected = vec![0u8; pw * ph];
            for y in 0..h {
                for x in 0..w {
                    let value = bottom_up[(h - 1 - y) * w + x];
                    assert_eq!(data[y * w + x], value);
                    let bin = y * ph / h * pw + x * pw / w;
                    expected[bin] = expected[bin].max(value);
                }
            }
            assert_eq!(small, expected);
        }
    }
}
