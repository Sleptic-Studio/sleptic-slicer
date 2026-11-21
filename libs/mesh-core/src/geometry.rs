// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{Normal, Vertex};
pub const DEGENERACY_SIN_SQ_THRESHOLD: f64 = 1e-12;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    DegenerateTriangle,
}
impl std::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("degenerate (near-zero area) triangle")
    }
}
impl std::error::Error for GeometryError {}
pub fn compute_normal_from_triangle_vertices(v: &[Vertex; 3]) -> Result<Normal, GeometryError> {
    let a = v[1].to_f64().diff(&v[0].to_f64());
    let b = v[2].to_f64().diff(&v[0].to_f64());
    let c = a.cross_product(&b);
    let n = c.squared_norm();
    let threshold = DEGENERACY_SIN_SQ_THRESHOLD * a.squared_norm() * b.squared_norm();
    match n.partial_cmp(&threshold) {
        Some(std::cmp::Ordering::Greater) => {
            let length = n.sqrt();
            Ok(Normal::new([
                (c[0] / length) as f32,
                (c[1] / length) as f32,
                (c[2] / length) as f32,
            ]))
        }
        _ => Err(GeometryError::DegenerateTriangle),
    }
}
