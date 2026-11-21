// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The 3D cut's coverage raster, pinned exactly: its size on square and
//! oblong plates and a fingerprint of its texels.
use sc_sl1cer::contour::{ClosedRing, ContourLayer, Polygon};
use sc_sl1cer::geom2d::Point2D;
use sc_sl1cer::profile::AaLevel;
use sc_sl1cer::raster::accuracy::Accuracy;

use crate::build_volume::BuildVolume;
use crate::slice_cap::{CutImage, build_cap_accurate};

/// An L shape off the plate's centre, with a square hole.
fn layer() -> ContourLayer {
    let ring = |points: &[(f64, f64)]| ClosedRing::new(points.iter().map(|&(x, y)| Point2D::new(x, y)).collect());
    let outer = ring(&[(-40.0, -30.0), (25.0, -30.0), (25.0, -5.0), (-10.0, -5.0), (-10.0, 35.0), (-40.0, 35.0)]);
    let hole = ring(&[(-30.0, -20.0), (-30.0, 10.0), (-20.0, 10.0), (-20.0, -20.0)]);
    ContourLayer::new(0.05, vec![Polygon::new(outer, vec![hole]).expect("a valid L with a hole")])
}

/// FNV-1a over the texels.
fn fingerprint(cap: &CutImage) -> u64 {
    cap.texels.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}

fn cap(x: f32, y: f32, accuracy: Accuracy, aa: AaLevel) -> Option<CutImage> {
    build_cap_accurate(&layer(), &BuildVolume::new(x, y, 100.0), accuracy, aa, 0)
}

#[test]
fn the_cut_raster_is_pinned() {
    let mut seen = Vec::new();
    for (x, y) in [(223.6, 126.5), (80.0, 80.0), (126.5, 223.6), (0.4, 0.3)] {
        for aa in [AaLevel::Off, AaLevel::X4] {
            let c = cap(x, y, Accuracy::default(), aa).expect("a cap");
            assert_eq!(c.texels.len(), (c.width * c.height) as usize);
            seen.push((c.width, c.height, fingerprint(&c)));
        }
    }
    assert_eq!(
        seen,
        [
            (2048, 1159, 4370453290852086788),
            (2048, 1159, 14178094032564749130),
            (2048, 2048, 15668040621698147109),
            (2048, 2048, 15668040621698147109),
            (1159, 2048, 2987222517485562447),
            (1159, 2048, 10076247838419032582),
            (2048, 1536, 4602708827301880613),
            (2048, 1536, 4602708827301880613),
        ]
    );
}
