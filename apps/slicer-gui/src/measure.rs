// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Two world-space surface points, in millimetres.
#[derive(Default, Debug)]
pub(crate) struct Measure {
    pub points: Vec<[f32; 3]>,
}
impl Measure {
    pub(crate) fn click(&mut self, point: [f32; 3]) -> Option<[f64; 4]> {
        if self.points.len() == 2 {
            self.points.clear();
        }
        self.points.push(point);
        if self.points.len() != 2 {
            return None;
        }
        let d =
            std::array::from_fn::<_, 3, _>(|i| f64::from(point[i]) - f64::from(self.points[0][i]));
        Some([
            (d.iter().map(|x| x * x).sum::<f64>()).sqrt(),
            d[0],
            d[1],
            d[2],
        ])
    }
    pub(crate) fn clear(&mut self) {
        self.points.clear();
    }
}
#[cfg(test)]
#[path = "tests_measure.rs"]
mod tests;
