// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::IndexedMesh;
pub fn render_mesh(m: &IndexedMesh, width: u32, height: u32, background: [u8; 3]) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut out = background.repeat(w * h);
    if w == 0 || h == 0 || m.vertices.is_empty() {
        return out;
    }
    let mut points = Vec::with_capacity(m.vertices.len());
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for v in &m.vertices {
        let [x, y, z] = v.coords().map(f64::from);
        let p = [
            (x - y) / std::f64::consts::SQRT_2,
            0.408248290463863 * (x + y) - 0.816496580927726 * z,
            0.5773502691896258 * (x + y + z),
        ];
        for i in 0..2 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
        points.push(p);
    }
    let scale =
        (w as f64 / (max[0] - min[0]).max(1e-9)).min(h as f64 / (max[1] - min[1]).max(1e-9)) * 0.88;
    for p in &mut points {
        p[0] = (p[0] - (min[0] + max[0]) * 0.5) * scale + w as f64 * 0.5;
        p[1] = (p[1] - (min[1] + max[1]) * 0.5) * scale + h as f64 * 0.5;
        p[2] *= scale;
    }
    let mut depth = vec![f64::NEG_INFINITY; w * h];
    for f in &m.faces {
        let [a, b, c] = f.vertices.map(|i| points[i.get() as usize]);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 {
            continue;
        }
        let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let t = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            e[1] * t[2] - e[2] * t[1],
            e[2] * t[0] - e[0] * t[2],
            e[0] * t[1] - e[1] * t[0],
        ];
        let norm = n.iter().map(|x| x * x).sum::<f64>().sqrt();
        let brightness =
            0.45 + 0.55 * ((n[0] * 0.3 - n[1] * 0.4 + n[2] * 0.8660254037844386) / norm).abs();
        let color = [75., 155., 220.].map(|c| (c * brightness) as u8);
        let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.) as usize;
        let x1 = a[0].max(b[0]).max(c[0]).ceil().min(w as f64) as usize;
        let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.) as usize;
        let y1 = a[1].max(b[1]).max(c[1]).ceil().min(h as f64) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let u = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                let v = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                let t = 1. - u - v;
                if u < -1e-9 || v < -1e-9 || t < -1e-9 {
                    continue;
                }
                let z = u * a[2] + v * b[2] + t * c[2];
                let i = y * w + x;
                if z > depth[i] {
                    depth[i] = z;
                    out[i * 3..i * 3 + 3].copy_from_slice(&color);
                }
            }
        }
    }
    out
}
