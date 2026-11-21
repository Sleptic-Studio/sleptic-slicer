// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use float_cmp::assert_approx_eq;

use super::*;

fn volume() -> BuildVolume {
    BuildVolume::new(100.0, 60.0, 80.0)
}

/// Points in the slab's outline.
const N: usize = 4 * (PLATE_CORNER_SEGMENTS as usize + 1);
/// Where the volume's corners start: the area twice, the outline three
/// times, the underside's centre.
const CAGE: usize = 8 + 3 * N + 1;
const WALLS: usize = CAGE + 8;
const CENTRE: u32 = 8 + 3 * N as u32;

/// The kind without the plate number.
fn plain(kind: u32) -> u32 {
    kind & ((1 << PLATE_INDEX_SHIFT) - 1)
}

fn is_icon(kind: u32) -> bool {
    (SHADE_GLYPH..=SHADE_GLYPH_ACCENT).contains(&plain(kind))
}

fn count(g: &PlateMesh, kind: u32) -> usize {
    g.plate_corners.iter().filter(|v| v.shade_kind == kind).count()
}

#[test]
fn the_vertices_start_in_the_documented_order() {
    let g = single_plate_mesh(volume());
    let kinds = |range: std::ops::Range<usize>| {
        g.plate_corners[range].iter().map(|v| v.shade_kind).collect::<Vec<_>>()
    };
    assert_eq!(kinds(0..4), [SHADE_FLOOR; 4]);
    assert_eq!(kinds(4..8 + N), vec![SHADE_PLATE_TOP_BORDER; 4 + N]);
    assert_eq!(kinds(8 + N..8 + 3 * N), vec![SHADE_PLATE_SIDE; 2 * N]);
    assert_eq!(g.plate_corners[CENTRE as usize].shade_kind, SHADE_PLATE_BOTTOM);
    assert_eq!(kinds(CAGE..WALLS), [SHADE_CAGE; 8]);
    for (wall, kind) in (SHADE_ENVELOPE_WALL_X_NEG..=SHADE_ENVELOPE_WALL_Z_POS).enumerate() {
        assert_eq!(kinds(WALLS + 4 * wall..WALLS + 4 * wall + 4), [kind; 4]);
    }
    // Everything after the walls up to the first icon: the axes, the edge
    // line, the tab, its lettering and the 7 tiles; then only icons.
    let before_icons = g.plate_corners.iter().take_while(|v| !is_icon(v.shade_kind)).count();
    let tiles = 7 * (1 + 4 * (TILE_CORNER_STEPS as usize + 1));
    let edge_and_tab = 4 * 4 + 3 * 4 * (TAB_SAMPLES - 1);
    assert_eq!(
        before_icons,
        WALLS + 20 + 6 + edge_and_tab + count(&g, SHADE_LABEL) + tiles
    );
    assert!(g.plate_corners[before_icons..].iter().all(|v| is_icon(v.shade_kind)));
}

#[test]
fn the_slab_spans_the_area_plus_its_overhang_and_its_thickness() {
    let g = single_plate_mesh(volume());
    // The area's corners, twice, at z = 0.
    for (floor, border) in g.plate_corners[..4].iter().zip(&g.plate_corners[4..8]) {
        assert_eq!(floor.position, border.position);
        assert_eq!(floor.position.map(f32::abs), [50.0, 30.0, 0.0]);
    }
    let outline = &g.plate_corners[8..8 + N];
    let max = |axis: usize| {
        outline
            .iter()
            .map(|v| v.position[axis])
            .fold(f32::NEG_INFINITY, f32::max)
    };
    assert_approx_eq!(f32, max(0), 50.0 + PLATE_OVERHANG_MM, epsilon = 1e-4);
    assert_approx_eq!(f32, max(1), 30.0 + PLATE_OVERHANG_MM, epsilon = 1e-4);
    for i in 0..N {
        let (top, low) = (
            g.plate_corners[8 + N + i].position,
            g.plate_corners[8 + 2 * N + i].position,
        );
        assert_eq!((top[0], top[1], top[2]), (low[0], low[1], 0.0));
        assert_eq!(low[2], -PLATE_THICKNESS_MM);
    }
    // The volume's corners: four on the floor, four at its height.
    assert!(
        g.plate_corners[CAGE..CAGE + 4]
            .iter()
            .all(|v| v.position[2] == 0.0)
    );
    assert!(
        g.plate_corners[CAGE + 4..WALLS]
            .iter()
            .all(|v| v.position[2] == 80.0)
    );
}

#[test]
fn the_index_lists_hold_what_each_pipeline_draws() {
    let g = single_plate_mesh(volume());
    // Top: the area, then a ring of border-only triangles (a floor vertex
    // in one would show the grid in the ring).
    assert_eq!(g.top_list[..6], [0, 1, 2, 0, 2, 3]);
    let ring = 4 * 3 * (PLATE_CORNER_SEGMENTS as usize + 2);
    assert!(
        g.top_list[6..6 + ring]
            .iter()
            .all(|&i| g.plate_corners[i as usize].shade_kind == SHADE_PLATE_TOP_BORDER)
    );
    assert_ne!(
        g.plate_corners[g.top_list[6 + ring] as usize].shade_kind,
        SHADE_PLATE_TOP_BORDER
    );
    let label = g
        .top_list
        .iter()
        .filter(|&&i| g.plate_corners[i as usize].shade_kind == SHADE_LABEL)
        .count();
    let tiles = 7 * 3 * 4 * (TILE_CORNER_STEPS as usize + 1);
    let before_icons = g
        .top_list
        .iter()
        .take_while(|&&i| !is_icon(g.plate_corners[i as usize].shade_kind))
        .count();
    assert_eq!(
        before_icons,
        6 + ring + 24 + 6 * (TAB_SAMPLES - 1) + label + tiles
    );
    // Sides, and the tab's side in both windings.
    assert_eq!(g.side_list.len(), 6 * N + 2 * 6 * (TAB_SAMPLES - 1));
    // The underside: every triangle starts at its centre, which carries the
    // translucent kind.
    assert_eq!(g.under_list.len(), 3 * N);
    assert!(g.under_list.chunks(3).all(|t| t[0] == CENTRE));
    // 12 edges on the volume's corners, then the 3 axes.
    assert_eq!(g.edge_list.len(), 30);
    assert!(
        g.edge_list[..24]
            .iter()
            .all(|&i| (CAGE..WALLS).contains(&(i as usize)))
    );
    // 5 walls of 2 triangles, on their own vertices, each in its face.
    assert_eq!(g.wall_list.len(), 30);
    assert!(
        g.wall_list
            .iter()
            .all(|&i| (WALLS..WALLS + 20).contains(&(i as usize)))
    );
    let (lo, hi) = (volume().envelope_min_mm(), volume().envelope_max_mm());
    let faces = [(0, lo.x), (0, hi.x), (1, lo.y), (1, hi.y), (2, hi.z)];
    for (wall, (axis, at)) in faces.into_iter().enumerate() {
        for v in &g.plate_corners[WALLS + 4 * wall..WALLS + 4 * wall + 4] {
            assert_approx_eq!(f32, v.position[axis], at);
        }
    }
}

#[test]
fn the_origin_axes_stand_at_the_volumes_corner() {
    let g = single_plate_mesh(BuildVolume::new(192.0, 120.0, 200.0));
    for (axis, line) in g.edge_list[24..].chunks(2).enumerate() {
        let (a, b) = (g.plate_corners[line[0] as usize], g.plate_corners[line[1] as usize]);
        assert_eq!(a.position, [-96.0, -60.0, 0.15]);
        assert_eq!(
            (a.shade_kind, b.shade_kind),
            (SHADE_AXIS_X + axis as u32, SHADE_AXIS_X + axis as u32)
        );
        let mut end = a.position;
        end[axis] += 12.0;
        assert_eq!(b.position, end);
    }
}

#[test]
fn icons_lie_on_their_tiles_coloured_by_the_plates_state() {
    let open = plate_mesh_for(volume(), 1, false, true);
    assert!(count(&open, SHADE_GLYPH) > 0 && count(&open, SHADE_GLYPH_ACCENT) > 0);
    assert!(count(&open, SHADE_GLYPH_DANGER) > 0);
    assert_eq!(
        count(&open, SHADE_PLATE_TILE_DANGER),
        1 + 4 * (TILE_CORNER_STEPS as usize + 1)
    );
    assert_eq!(count(&open, SHADE_GLYPH_DIM), 0);
    // Locked: delete and the other actions dimmed, the padlock shut.
    let shut = plate_mesh_for(volume(), 1, true, true);
    assert_eq!(
        (
            count(&shut, SHADE_GLYPH_DANGER),
            count(&shut, SHADE_PLATE_TILE_DANGER)
        ),
        (0, 0)
    );
    assert!(count(&shut, SHADE_GLYPH_DIM) > 0);
    assert_ne!(count(&open, SHADE_GLYPH), count(&shut, SHADE_GLYPH));
    // Not active: the number is plain.
    assert_eq!(
        count(
            &plate_mesh_for(volume(), 1, false, false),
            SHADE_GLYPH_ACCENT
        ),
        0
    );
    let tiles = volume().plate_tile_rects();
    for v in open.plate_corners.iter().filter(|v| is_icon(v.shade_kind)) {
        let [x, y, z] = v.position;
        assert!((z - GLYPH_LIFT_MM).abs() < 1e-6);
        let on = |[x0, y0, x1, y1]: &[f32; 4]| {
            x >= x0 - 1e-3 && x <= x1 + 1e-3 && y >= y0 - 1e-3 && y <= y1 + 1e-3
        };
        assert!(tiles.iter().any(on), "icon point ({x}, {y}) off its tile");
    }
}

#[test]
fn several_plates_carry_their_numbers_and_only_one_set_of_volume_edges() {
    let one = single_plate_mesh(volume());
    let three = all_plates_mesh(volume(), 3, 1, &[false, true, false]);
    // In plate order, every kind below the number bits.
    let plates: Vec<u32> = three
        .plate_corners
        .iter()
        .map(|v| v.shade_kind >> PLATE_INDEX_SHIFT)
        .collect();
    assert!(plates.windows(2).all(|w| w[1] == w[0] || w[1] == w[0] + 1));
    assert_eq!(plates.last(), Some(&2));
    assert!(three.plate_corners.iter().all(|v| plain(v.shade_kind) <= SHADE_LABEL));
    // R55: later plates add only their axes.
    assert_eq!(
        three.edge_list.len(),
        24 + 3 * (one.edge_list.len() - 24)
    );
    // Plate 2 (index 1) is the active one, locked: its border is
    // highlighted and its actions dimmed.
    let of = |plate: u32, kind: u32| {
        three
            .plate_corners
            .iter()
            .filter(|v| v.shade_kind == kind | plate << PLATE_INDEX_SHIFT)
            .count()
    };
    assert!(of(1, SHADE_ACTIVE_BORDER) > 0 && of(1, SHADE_GLYPH_DIM) > 0);
    assert_eq!(
        (of(0, SHADE_ACTIVE_BORDER), of(2, SHADE_ACTIVE_BORDER)),
        (0, 0)
    );
    // One plate alone has no highlight.
    assert_eq!(
        count(
            &all_plates_mesh(volume(), 1, 0, &[]),
            SHADE_ACTIVE_BORDER
        ),
        0
    );
}

#[test]
fn the_slice_view_shows_only_the_sliced_plate_where_it_stands() {
    let own = plate_mesh_for(volume(), 3, false, true);
    let only = active_plate_mesh(volume(), 3, 2, &[false; 3]);
    assert_eq!(
        (only.plate_corners.len(), only.top_list.len()),
        (own.plate_corners.len(), own.top_list.len())
    );
    let [x, y] = sc_scene::plate_grid_offset(2, 3, volume().width_mm(), volume().depth_mm());
    assert!(x != 0.0 || y != 0.0);
    for (a, b) in own.plate_corners.iter().zip(&only.plate_corners) {
        assert_approx_eq!(f32, b.position[0], a.position[0] + x);
        assert_approx_eq!(f32, b.position[1], a.position[1] + y);
        assert_eq!(b.shade_kind, a.shade_kind | 2 << PLATE_INDEX_SHIFT);
    }
}

#[test]
fn the_lettering_fills_the_front_lip() {
    let v = volume();
    let depth = (0.06 * v.depth_mm() + 3.0).clamp(6.0, 12.0);
    let half = (0.24 * v.width_mm()).clamp(3.0 * depth + 10.0, 60.0);
    let flat = 2.0 * (half - 2.0 * depth);
    for (maker, model) in [("Generic", "Generic Machine"), ("Elegoo", "Saturn 4 Ultra"), ("Creality", "HALOT-MAGE S")] {
        crate::mesh_render::set_plate_printer(maker, model);
        let g = single_plate_mesh(v);
        let label: Vec<[f32; 3]> =
            g.plate_corners.iter().filter(|c| c.shade_kind == SHADE_LABEL).map(|c| c.position).collect();
        assert!(!label.is_empty());
        let (lo_x, hi_x) = label.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
        let (lo_y, hi_y) = label.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        let (w, h) = (hi_x - lo_x, hi_y - lo_y);
        // Inside the lip, and as large as one of its sides allows.
        assert!(h <= depth, "{maker}: {h} tall in a {depth} mm lip");
        assert!(w <= flat + 0.01, "{maker}: {w} wide on a {flat} mm flat");
        assert!(h >= 0.85 * depth || w >= 0.9 * flat, "{maker}: {w} x {h} fills a {flat} x {depth} lip");
    }
    crate::mesh_render::set_plate_printer("Generic", "Generic Machine");
}
