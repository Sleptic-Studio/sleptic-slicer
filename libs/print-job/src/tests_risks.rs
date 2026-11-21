// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_sl1cer::{
    contour::{ClosedRing, ContourLayer, Polygon},
    geom2d::Point2D,
    profile::{AaLevel, BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution},
};
fn profile() -> SliceProfile {
    SliceProfile::new(
        BuildVolume::new(10.0, 10.0, 10.0).unwrap(),
        PixelResolution::new(100, 100).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 5).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    )
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> ClosedRing {
    ClosedRing::new(vec![
        Point2D::new(x, y),
        Point2D::new(x + w, y),
        Point2D::new(x + w, y + h),
        Point2D::new(x, y + h),
    ])
}
fn solid() -> Polygon {
    Polygon::new(rect(-3.0, -3.0, 6.0, 6.0), vec![]).unwrap()
}
fn shell() -> Polygon {
    let mut hole = rect(-2.0, -2.0, 4.0, 4.0);
    hole.reverse();
    Polygon::new(rect(-3.0, -3.0, 6.0, 6.0), vec![hole]).unwrap()
}
fn analyze(polys: Vec<Vec<Polygon>>) -> LayerAnalysis {
    // The topology fixtures use sub-millimetre voids on purpose; the size
    // floor has its own test below.
    analyze_sized(polys, 0.0)
}
fn analyze_sized(polys: Vec<Vec<Polygon>>, minimum_void_mm3: f64) -> LayerAnalysis {
    analyze_layers_with_settings(
        &ContourStack::new_unchecked(
            polys
                .into_iter()
                .enumerate()
                .map(|(i, p)| ContourLayer::new((i as f64 + 0.5) * 0.1, p))
                .collect(),
        ),
        &profile(),
        RiskSettings {
            minimum_void_mm3,
            ..RiskSettings::default()
        },
    )
}
// Regression: every pinhole where a support tip met the hull was reported
// (a supported Benchy listed 44 suction cups, tree supports 200). Voids
// under the default 1 mm^3 floor are dropped; real cups stay.
#[test]
fn pinhole_voids_are_not_reported_but_real_cups_are() {
    let cup = |hole: f64| {
        let mut h = rect(-hole / 2.0, -hole / 2.0, hole, hole);
        h.reverse();
        Polygon::new(rect(-3.0, -3.0, 6.0, 6.0), vec![h]).unwrap()
    };
    let base = Polygon::new(rect(-3.0, -3.0, 6.0, 6.0), vec![]).unwrap();
    let count = |a: &LayerAnalysis| {
        a.risks
            .iter()
            .filter(|r| matches!(r.kind, RiskKind::SuctionCup | RiskKind::TrappedCavity))
            .count()
    };
    let default_floor = RiskSettings::default().minimum_void_mm3;
    // 0.2 mm square pinhole over 3 layers: 0.012 mm^3.
    let pinhole = analyze_sized(
        vec![
            vec![base.clone()],
            vec![cup(0.2)],
            vec![cup(0.2)],
            vec![cup(0.2)],
            vec![base.clone()],
        ],
        default_floor,
    );
    assert_eq!(count(&pinhole), 0);
    // 4 mm square cup over 3 layers: 4.8 mm^3, sealed above -> cup + trapped.
    let real = analyze_sized(
        vec![
            vec![base.clone()],
            vec![cup(4.0)],
            vec![cup(4.0)],
            vec![cup(4.0)],
            vec![base],
        ],
        default_floor,
    );
    assert!(count(&real) >= 1);
}
#[test]
fn clean_cube_has_no_risks() {
    assert!(analyze(vec![vec![solid()]; 5]).risks.is_empty());
}
#[test]
fn naturally_open_top_cup_is_suction_but_not_trapped() {
    let a = analyze(vec![
        vec![solid()],
        vec![shell()],
        vec![shell()],
        vec![shell()],
        vec![shell()],
    ]);
    assert!(
        a.risks
            .iter()
            .any(|r| r.kind == RiskKind::SuctionCup && r.first_layer == 1)
    );
    assert!(!a.risks.iter().any(|r| r.kind == RiskKind::TrappedCavity));
}
#[test]
fn closed_box_traps_resin() {
    let a = analyze(vec![
        vec![solid()],
        vec![shell()],
        vec![shell()],
        vec![solid()],
    ]);
    assert!(
        a.risks
            .iter()
            .any(|r| r.kind == RiskKind::TrappedCavity && r.first_layer == 1 && r.last_layer == 2)
    );
}
#[test]
fn bottom_open_tube_is_vented() {
    let a = analyze(vec![vec![shell()]; 5]);
    assert!(
        !a.risks
            .iter()
            .any(|r| matches!(r.kind, RiskKind::TrappedCavity | RiskKind::SuctionCup))
    );
}
#[test]
fn point_two_mm_fin_disappears_below_default_minimum() {
    let fin = Polygon::new(rect(-2.0, -0.1, 4.0, 0.2), vec![]).unwrap();
    let a = analyze(vec![vec![fin]; 5]);
    assert!(a.risks.iter().any(|r| r.kind == RiskKind::ThinWall));
}
#[test]
fn minimum_is_configurable() {
    let stack = ContourStack::new_unchecked(vec![ContourLayer::new(
        0.05,
        vec![Polygon::new(rect(-2.0, -0.1, 4.0, 0.2), vec![]).unwrap()],
    )]);
    let a = analyze_layers_with_settings(
        &stack,
        &profile(),
        RiskSettings {
            minimum_feature_mm: 0.1,
            ..RiskSettings::default()
        },
    );
    assert!(a.risks.is_empty());
}
#[test]
fn peel_spikes_keep_worst_layer() {
    let mut s = RiskScanner::new(
        &profile(),
        RiskSettings {
            peel_area_mm2: 20.0,
            ..RiskSettings::default()
        },
    );
    use sc_sl1cer::raster::rle::RleRun;
    let r = LayerRaster::new(
        0.05,
        1,
        1,
        vec![RleRun {
            value: 255,
            length: 1,
        }],
    );
    s.push(&r, 21.0);
    s.push(&r, 30.0);
    let risks = s.finish();
    let p = risks
        .iter()
        .find(|r| r.kind == RiskKind::LargePeelArea)
        .unwrap();
    assert_eq!((p.first_layer, p.last_layer, p.worst_layer), (0, 1, 1));
}
#[test]
fn floating_region_is_error() {
    let a = analyze(vec![vec![], vec![solid()]]);
    assert!(
        a.risks
            .iter()
            .any(|r| r.kind == RiskKind::UnsupportedIsland && r.severity == Severity::Error)
    );
}

#[test]
fn off_plate_bounds_are_reported_before_raster_clipping() {
    let stack = ContourStack::new_unchecked(vec![]);
    let m = PlacedModel {
        min_mm: [4.0, 0.0, 0.0],
        max_mm: [6.0, 1.0, 1.0],
        stack: &stack,
    };
    let r = placement_risks(&[m], &profile());
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].kind, RiskKind::OutOfBounds);
    assert_eq!(r[0].severity, Severity::Error);
}
#[test]
fn overlap_is_material_intersection_not_shared_bounds() {
    let a = solid();
    let b = shell();
    assert!(polygons_overlap(&a, &b));
    let inside_hole = Polygon::new(rect(-1.0, -1.0, 2.0, 2.0), vec![]).unwrap();
    assert!(!polygons_overlap(&inside_hole, &b));
    assert!(polygons_overlap(&a, &a));
    let touch = Polygon::new(rect(3.0, -3.0, 2.0, 2.0), vec![]).unwrap();
    assert!(!polygons_overlap(&a, &touch));
}
#[test]
fn two_independent_cups_remain_two_after_both_open_to_exterior() {
    let other = |p: &Polygon| {
        let translate = |r: &ClosedRing| {
            ClosedRing::new(
                r.vertices()
                    .iter()
                    .map(|p| Point2D::new(p.x + 3.5, p.y))
                    .collect(),
            )
        };
        Polygon::new(
            translate(p.outer()),
            p.holes().iter().map(translate).collect(),
        )
        .unwrap()
    };
    let small = Polygon::new(rect(-4.0, -1.0, 2.0, 2.0), vec![]).unwrap();
    let mut h = rect(-3.5, -0.5, 1.0, 1.0);
    h.reverse();
    let cup = Polygon::new(rect(-4.0, -1.0, 2.0, 2.0), vec![h]).unwrap();
    let a = analyze(vec![
        vec![small.clone(), other(&small)],
        vec![cup.clone(), other(&cup)],
        vec![],
    ]);
    assert_eq!(
        a.risks
            .iter()
            .filter(|r| r.kind == RiskKind::SuctionCup)
            .count(),
        2
    );
}
#[test]
fn top_or_side_openings_prevent_trapped_resin() {
    let left = Polygon::new(rect(-3.0, -3.0, 1.0, 6.0), vec![]).unwrap();
    let right = Polygon::new(rect(2.0, -3.0, 1.0, 6.0), vec![]).unwrap();
    let a = analyze(vec![
        vec![solid()],
        vec![shell()],
        vec![left, right],
        vec![solid()],
    ]);
    assert!(!a.risks.iter().any(|r| r.kind == RiskKind::TrappedCavity));
}

#[test]
fn grayscale_edges_are_one_solid_span_for_thin_features() {
    use sc_sl1cer::raster::rle::RleRun;
    let raster = LayerRaster::new(
        0.05,
        5,
        1,
        vec![
            RleRun {
                value: 128,
                length: 1,
            },
            RleRun {
                value: 255,
                length: 3,
            },
            RleRun {
                value: 128,
                length: 1,
            },
        ],
    );
    let spans = Spans::from_raster(&raster);
    assert_eq!(spans.spans, vec![(0, 0, 5)]);
    assert!(thin_spans(&spans, 1, 0).is_empty());
}

#[test]
fn island_positions_are_world_coordinates_under_all_mirrors() {
    let stack = ContourStack::new_unchecked(vec![
        ContourLayer::new(0.05, vec![]),
        ContourLayer::new(
            0.15,
            vec![Polygon::new(rect(1.0, 2.0, 1.0, 1.0), vec![]).unwrap()],
        ),
    ]);
    for x in [false, true] {
        for y in [false, true] {
            let mut p = profile();
            p = SliceProfile::new(
                *p.build_volume(),
                *p.pixel_resolution(),
                *p.layer_schedule(),
                Mirror { x, y },
                AaLevel::Off,
                PackingMode::None,
            );
            let a = crate::analyze_layers(&stack, &p);
            let r = a
                .risks
                .iter()
                .find(|r| r.kind == RiskKind::UnsupportedIsland)
                .unwrap();
            assert!((r.position_mm.0 - 1.5).abs() < 0.1);
            assert!((r.position_mm.1 - 2.5).abs() < 0.1);
        }
    }
}

#[test]
fn a_vented_large_cup_does_not_promote_a_later_pinhole() {
    let mut hole = rect(-2.8, 0.0, 0.2, 0.2);
    hole.reverse();
    let pinhole = Polygon::new(rect(-3., -3., 6., 6.), vec![hole]).unwrap();
    let left_wall = Polygon::new(rect(-3., -3., 1., 6.), vec![]).unwrap();
    let analysis = analyze_sized(
        vec![
            vec![solid()],
            vec![shell()],
            vec![left_wall],
            vec![pinhole],
            vec![],
        ],
        1.0,
    );
    let cups: Vec<_> = analysis
        .risks
        .iter()
        .filter(|r| r.kind == RiskKind::SuctionCup)
        .collect();
    assert_eq!(cups.len(), 1, "{cups:?}");
    assert_eq!(cups[0].first_layer, 1);
    assert!((cups[0].measured - 1.6).abs() < 1e-9);
}

#[test]
fn thin_reconstruction_matches_independent_pixel_opening() {
    use sc_sl1cer::raster::rle::{LayerRaster, RleRunAccumulator};
    for seed in 0..20 {
        let (w, h) = (24usize, 18usize);
        let pixels: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                (x > 2 && x < 20 && y > 2 && y < 15 && (x + seed) % 7 != 0)
                    || (y == 8 && x > 1 && x < 23)
            })
            .collect();
        let mut runs = RleRunAccumulator::new();
        for &lit in &pixels {
            runs.push(if lit { 255 } else { 0 }, 1);
        }
        let raster = LayerRaster::new(0.1, w as u32, h as u32, runs.into_runs());
        let spans = Spans::from_raster(&raster);
        for (rx, ry) in [(1, 1), (2, 1), (0, 2), (2, 0)] {
            let mut eroded = vec![false; w * h];
            for y in ry..h - ry {
                for x in rx..w - rx {
                    eroded[y * w + x] =
                        (y - ry..=y + ry).all(|j| (x - rx..=x + rx).all(|i| pixels[j * w + i]));
                }
            }
            let expected: Vec<_> = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    pixels[i]
                        && !(y.saturating_sub(ry)..=(y + ry).min(h - 1)).any(|j| {
                            (x.saturating_sub(rx)..=(x + rx).min(w - 1)).any(|k| eroded[j * w + k])
                        })
                })
                .collect();
            let mut actual = vec![false; w * h];
            for (y, a, b) in thin_spans(&spans, rx as u32, ry as u32) {
                for x in a..b {
                    actual[y as usize * w + x as usize] = true;
                }
            }
            assert_eq!(actual, expected, "seed={seed} radius={rx},{ry}");
        }
    }
}

/// R78: a rigid resin holds a one-pixel step per layer (45 degrees); a
/// flexible resin needing supports under 70 degree overhangs holds none.
#[test]
fn a_flexible_resin_reaches_less_far_sideways() {
    assert_eq!(crate::self_supporting_reach_px(&profile()), 1);
    let flexible = profile().with_support_overhang_angle(70.0);
    assert_eq!(crate::self_supporting_reach_px(&flexible), 0);
}
