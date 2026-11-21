// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{contour::ClosedRing, geom2d::Point2D};
pub fn point_in_ring(p: Point2D, r: &ClosedRing) -> bool {
    r.vertices().get(2).is_some() && {
        let mut inside = false;
        for (a, b) in r.vertices().iter().zip(r.vertices().iter().cycle().skip(1)) {
            if (a.y > p.y) != (b.y > p.y) && (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x > p.x {
                inside = !inside
            }
        }
        inside
    }
}
