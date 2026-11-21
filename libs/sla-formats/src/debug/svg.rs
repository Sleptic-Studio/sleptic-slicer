// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{WriterError, WriterOptions};
use sc_sl1cer::contour::{ContourLayer, ContourStack};
use std::{
    io::{BufWriter, Write},
    path::Path,
};
fn header(w: &mut impl Write, o: &WriterOptions) -> std::io::Result<()> {
    let b = o.build_volume();
    let r = o.pixel_resolution();
    let c = o.calibration();
    let (x, y) = if c.rotation.swaps_axes() {
        (b.y_mm(), b.x_mm())
    } else {
        (b.x_mm(), b.y_mm())
    };
    let sc_sl1cer::profile::LayerSchedule::Uniform(s) = o.layer_schedule();
    writeln!(w, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
    writeln!(
        w,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:sc=\"urn:sleptic:slicer:svg\" viewBox=\"{} {} {x} {y}\">",
        -x / 2.0,
        -y / 2.0
    )?;
    writeln!(
        w,
        "<metadata><sc:slicer-run><sc:slicer-version>{}</sc:slicer-version><sc:build-volume x-mm=\"{}\" y-mm=\"{}\" z-mm=\"{}\"/><sc:pixel-resolution x-px=\"{}\" y-px=\"{}\"/><sc:layer-schedule kind=\"uniform\" layer-height-mm=\"{}\" first-layer-height-mm=\"{}\" total-layers=\"{}\"/><sc:aa-level>{}</sc:aa-level><sc:mirror x=\"{}\" y=\"{}\"/><sc:min-alpha-pct>{}</sc:min-alpha-pct></sc:slicer-run></metadata>",
        o.slicer_version(),
        b.x_mm(),
        b.y_mm(),
        b.z_mm(),
        r.x(),
        r.y(),
        s.layer_height().value(),
        s.first_layer_height().value(),
        s.total_layers(),
        crate::options::aa_text(o.aa()),
        o.mirror().x,
        o.mirror().y,
        o.min_alpha_pct()
    )?;
    if wrapper(o) {
        let [mut sx, mut sy] = c.xy_scale();
        if o.mirror().x {
            sx = -sx
        }
        if o.mirror().y {
            sy = -sy
        }
        let v = match u16::from(c.rotation) {
            0 => [sx, 0.0, 0.0, sy],
            90 => [0.0, sx, -sy, 0.0],
            180 => [-sx, 0.0, 0.0, -sy],
            _ => [0.0, -sx, sy, 0.0],
        };
        writeln!(
            w,
            "<g transform=\"matrix({} {} {} {} 0 0)\">",
            v[0], v[1], v[2], v[3]
        )?;
    }
    Ok(())
}
fn wrapper(o: &WriterOptions) -> bool {
    !o.calibration().is_default() || o.mirror().x || o.mirror().y
}
fn layer(w: &mut impl Write, l: &ContourLayer, i: usize) -> std::io::Result<()> {
    writeln!(
        w,
        "<g id=\"layer-{i:04}\" data-z-mm=\"{}\" data-layer-index=\"{i}\" transform=\"scale(1 -1)\">",
        l.z_mm()
    )?;
    for p in l.polygons() {
        write!(w, "<path d=\"")?;
        for (j, r) in std::iter::once(p.outer()).chain(p.holes()).enumerate() {
            if j > 0 {
                write!(w, " ")?
            }
            for (k, v) in r.vertices().iter().enumerate() {
                if k == 0 {
                    write!(w, "M {} {}", v.x, v.y)?
                } else {
                    write!(w, " L {} {}", v.x, v.y)?
                }
            }
            write!(w, " Z")?;
        }
        writeln!(w, "\" fill-rule=\"evenodd\"/>")?;
    }
    writeln!(w, "</g>")
}
fn footer(w: &mut impl Write, o: &WriterOptions) -> std::io::Result<()> {
    if wrapper(o) {
        writeln!(w, "</g>")?
    }
    writeln!(w, "</svg>")
}
pub fn write_combined(s: &ContourStack, p: &Path, o: &WriterOptions) -> Result<(), WriterError> {
    let mut w = BufWriter::new(std::fs::File::create(p)?);
    header(&mut w, o)?;
    for (i, l) in s.layers().iter().enumerate() {
        layer(&mut w, l, i)?
    }
    footer(&mut w, o)?;
    w.flush()?;
    Ok(())
}
pub fn write_per_layer(s: &ContourStack, p: &Path, o: &WriterOptions) -> Result<(), WriterError> {
    for (i, l) in s.layers().iter().enumerate() {
        let mut w = BufWriter::new(std::fs::File::create(p.join(format!("layer_{i:04}.svg")))?);
        header(&mut w, o)?;
        layer(&mut w, l, i)?;
        footer(&mut w, o)?;
        w.flush()?;
    }
    Ok(())
}
