// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Text as vector outlines that can be laid onto a surface. Labels drawn
//! upright at a projected point always face the camera (billboarding); Sleptic
//! asked for labels fixed to what they are on. Each label becomes the bundled
//! UI font's glyph outlines in a normalised frame, and the caller maps every
//! point onto its surface, so the text turns and foreshortens with it.

use glam::Vec2;

/// The UI font (also imported by the Slint UI as its default family).
const FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Variable.ttf");

/// Semi-bold: thin strokes vanish once a small face is foreshortened.
const WEIGHT: f32 = 600.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Segment {
    Move(Vec2),
    Line(Vec2),
    Quad(Vec2, Vec2),
    Cubic(Vec2, Vec2, Vec2),
    Close,
}

/// A label's outline, centred on the origin with its height (cap height
/// box from baseline to ascender) normalised to 1. `width` is in the same
/// units.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextOutline {
    pub segments: Vec<Segment>,
    pub width: f32,
}

struct Builder<'a> {
    out: &'a mut Vec<Segment>,
    origin_x: f32,
}

impl ttf_parser::OutlineBuilder for Builder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.out
            .push(Segment::Move(Vec2::new(self.origin_x + x, y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.out
            .push(Segment::Line(Vec2::new(self.origin_x + x, y)));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.out.push(Segment::Quad(
            Vec2::new(self.origin_x + x1, y1),
            Vec2::new(self.origin_x + x, y),
        ));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.out.push(Segment::Cubic(
            Vec2::new(self.origin_x + x1, y1),
            Vec2::new(self.origin_x + x2, y2),
            Vec2::new(self.origin_x + x, y),
        ));
    }
    fn close(&mut self) {
        self.out.push(Segment::Close);
    }
}

/// Outline `text` in the UI font. Characters the font lacks are skipped.
pub(crate) fn outline(text: &str) -> TextOutline {
    let Ok(mut face) = ttf_parser::Face::parse(FONT, 0) else {
        return TextOutline::default();
    };
    face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), WEIGHT);
    let mut segments = Vec::new();
    let mut pen = 0.0f32;
    for ch in text.chars() {
        let Some(glyph) = face.glyph_index(ch) else {
            continue;
        };
        let mut builder = Builder {
            out: &mut segments,
            origin_x: pen,
        };
        face.outline_glyph(glyph, &mut builder);
        pen += f32::from(face.glyph_hor_advance(glyph).unwrap_or(0));
    }
    // Centre on the box from the baseline to the cap height and scale it to 1.
    let cap = f32::from(face.capital_height().unwrap_or(face.ascender())).max(1.0);
    let centre = Vec2::new(pen * 0.5, cap * 0.5);
    let norm = |p: Vec2| (p - centre) / cap;
    let segments = segments
        .into_iter()
        .map(|s| match s {
            Segment::Move(p) => Segment::Move(norm(p)),
            Segment::Line(p) => Segment::Line(norm(p)),
            Segment::Quad(a, p) => Segment::Quad(norm(a), norm(p)),
            Segment::Cubic(a, b, p) => Segment::Cubic(norm(a), norm(b), norm(p)),
            Segment::Close => Segment::Close,
        })
        .collect();
    TextOutline {
        segments,
        width: pen / cap,
    }
}

/// An SVG icon's geometry (all paths, shapes resolved to curves, transforms
/// applied) in its own viewBox units, y down. The plate tool icons are line
/// drawings, so callers stroke the result.
pub(crate) fn svg_outline(svg: &str) -> TextOutline {
    let Ok(tree) = usvg::Tree::from_str(svg, &usvg::Options::default()) else {
        return TextOutline::default();
    };
    fn walk(group: &usvg::Group, out: &mut Vec<Segment>) {
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => walk(g, out),
                usvg::Node::Path(path) => {
                    let t = path.abs_transform();
                    let map = |p: usvg::tiny_skia_path::Point| {
                        Vec2::new(
                            t.sx * p.x + t.kx * p.y + t.tx,
                            t.ky * p.x + t.sy * p.y + t.ty,
                        )
                    };
                    use usvg::tiny_skia_path::PathSegment as S;
                    for segment in path.data().segments() {
                        out.push(match segment {
                            S::MoveTo(p) => Segment::Move(map(p)),
                            S::LineTo(p) => Segment::Line(map(p)),
                            S::QuadTo(a, p) => Segment::Quad(map(a), map(p)),
                            S::CubicTo(a, b, p) => Segment::Cubic(map(a), map(b), map(p)),
                            S::Close => Segment::Close,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    let mut segments = Vec::new();
    walk(tree.root(), &mut segments);
    TextOutline {
        segments,
        width: tree.size().width(),
    }
}

/// The brand symbol for a plate label (the manufacturer in capitals), in its
/// own viewBox units, y down; `None` for a brand without one (R120).
fn brand_mark(label: &str) -> Option<&'static TextOutline> {
    use std::sync::OnceLock;
    static ANYCUBIC: OnceLock<TextOutline> = OnceLock::new();
    static ELEGOO: OnceLock<TextOutline> = OnceLock::new();
    static SLEPTIC: OnceLock<TextOutline> = OnceLock::new();
    match label {
        "ANYCUBIC" => Some(ANYCUBIC.get_or_init(|| svg_outline(include_str!("../ui/brands/anycubic.svg")))),
        "ELEGOO" => Some(ELEGOO.get_or_init(|| svg_outline(include_str!("../ui/brands/elegoo.svg")))),
        "SLEPTIC" => Some(SLEPTIC.get_or_init(|| svg_outline(include_str!("../assets/images/sleptic.svg")))),
        _ => None,
    }
}

/// The plate's brand lockup (R120: the maintainer wanted the printer
/// company's own style): the brand's symbol, then its name, normalised like
/// [`outline`] (cap height 1, centred, y up). A brand without a symbol is
/// its name alone, so every manufacturer gets lettering.
pub(crate) fn brand_lockup(label: &str) -> TextOutline {
    let text = outline(label);
    let Some(mark) = brand_mark(label) else {
        return text;
    };
    let mark_h = 1.45;
    let scale = mark_h / mark.width.max(0.01);
    let mark_w = mark.width * scale;
    let gap = 0.45;
    let total = mark_w + gap + text.width;
    // Mark at the left, centred on the cap height; y flips to up.
    let m = |p: Vec2| Vec2::new(p.x * scale - 0.5 * total, (0.5 * mark.width - p.y) * scale);
    let t = |p: Vec2| p + Vec2::new(0.5 * total - 0.5 * text.width, 0.0);
    let mut segments: Vec<Segment> = mark
        .segments
        .iter()
        .map(|s| match *s {
            Segment::Move(p) => Segment::Move(m(p)),
            Segment::Line(p) => Segment::Line(m(p)),
            Segment::Quad(a, p) => Segment::Quad(m(a), m(p)),
            Segment::Cubic(a, b, p) => Segment::Cubic(m(a), m(b), m(p)),
            Segment::Close => Segment::Close,
        })
        .collect();
    segments.extend(text.segments.iter().map(|s| match *s {
        Segment::Move(p) => Segment::Move(t(p)),
        Segment::Line(p) => Segment::Line(t(p)),
        Segment::Quad(a, p) => Segment::Quad(t(a), t(p)),
        Segment::Cubic(a, b, p) => Segment::Cubic(t(a), t(b), t(p)),
        Segment::Close => Segment::Close,
    }));
    TextOutline { segments, width: total }
}

/// The plate tool icon for tile `slot` (1 clear, 2 orient, 3 arrange, 4 lock,
/// 5 settings) in its 24-unit viewBox; slot 0 (the plate number) is text.
pub(crate) fn plate_icon(slot: usize) -> &'static TextOutline {
    static ICONS: std::sync::OnceLock<[TextOutline; 7]> = std::sync::OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        [
            TextOutline::default(),
            svg_outline(include_str!("../ui/icons/delete.svg")),
            svg_outline(include_str!("../ui/icons/rotate.svg")),
            svg_outline(include_str!("../ui/icons/arrange.svg")),
            svg_outline(include_str!("../ui/icons/lock.svg")),
            svg_outline(include_str!("../ui/icons/gear.svg")),
            // A plus framed by a plate outline: a bare plus, turned with the
            // tile, read as a close "x" from the default camera.
            svg_outline(include_str!("../ui/icons/plate-add.svg")),
        ]
    });
    &icons[slot.min(6)]
}

/// The lock tile's icon for a plate's state (R110): a closed padlock when
/// the plate is locked, an open one when it is not.
pub(crate) fn plate_lock_icon(locked: bool) -> &'static TextOutline {
    static OPEN: std::sync::OnceLock<TextOutline> = std::sync::OnceLock::new();
    if locked {
        plate_icon(4)
    } else {
        OPEN.get_or_init(|| svg_outline(include_str!("../ui/icons/lock-open.svg")))
    }
}

/// SVG path commands for `outline` with every point (control points too)
/// passed through `map`. Exact for affine maps, which is what an
/// orthographic view of a flat face is.
pub(crate) fn path_commands(outline: &TextOutline, map: impl Fn(Vec2) -> Vec2) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for segment in &outline.segments {
        let _ = match *segment {
            Segment::Move(p) => {
                let p = map(p);
                write!(out, "M{:.2} {:.2} ", p.x, p.y)
            }
            Segment::Line(p) => {
                let p = map(p);
                write!(out, "L{:.2} {:.2} ", p.x, p.y)
            }
            Segment::Quad(a, p) => {
                let (a, p) = (map(a), map(p));
                write!(out, "Q{:.2} {:.2} {:.2} {:.2} ", a.x, a.y, p.x, p.y)
            }
            Segment::Cubic(a, b, p) => {
                let (a, b, p) = (map(a), map(b), map(p));
                write!(
                    out,
                    "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ",
                    a.x, a.y, b.x, b.y, p.x, p.y
                )
            }
            Segment::Close => write!(out, "Z "),
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_is_centred_and_cap_height_normalised() {
        let o = outline("Top");
        assert!(o.width > 1.0, "three letters are wider than tall");
        let ys: Vec<f32> = o
            .segments
            .iter()
            .filter_map(|s| match s {
                Segment::Move(p) | Segment::Line(p) => Some(p.y),
                _ => None,
            })
            .collect();
        let (lo, hi) = ys
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        // 'T' spans baseline to cap height: -0.5 .. 0.5; 'p' descends below.
        assert!((hi - 0.5).abs() < 0.05, "top {hi}");
        assert!(lo < -0.5, "descender {lo}");
    }

    /// Every plate tool icon parses into geometry inside its 24-unit box,
    /// including the lock's rounded rect and the arcs in orient and gear.
    #[test]
    fn plate_icons_parse_into_paths_inside_their_viewbox() {
        for slot in 1..=6 {
            let icon = plate_icon(slot);
            let points: Vec<Vec2> = icon
                .segments
                .iter()
                .flat_map(|s| match *s {
                    Segment::Move(p) | Segment::Line(p) => vec![p],
                    Segment::Quad(a, p) => vec![a, p],
                    Segment::Cubic(a, b, p) => vec![a, b, p],
                    Segment::Close => vec![],
                })
                .collect();
            assert!(points.len() >= 4, "slot {slot} is empty");
            assert!(
                points
                    .iter()
                    .all(|p| (0.0..=24.0).contains(&p.x) && (0.0..=24.0).contains(&p.y)),
                "slot {slot} leaves its viewBox"
            );
        }
        assert!(plate_icon(0).segments.is_empty());
    }

    #[test]
    fn path_commands_apply_the_map_to_every_point() {
        let o = outline("T");
        let path = path_commands(&o, |p| p * 10.0 + Vec2::new(100.0, 0.0));
        assert!(path.starts_with('M'));
        assert!(path.trim_end().ends_with('Z'));
        // Every x is shifted by +100 +/- 10 * half width.
        for x in path
            .split(|c: char| c.is_ascii_alphabetic())
            .filter_map(|chunk| chunk.split_whitespace().next())
            .filter_map(|v| v.parse::<f32>().ok())
        {
            assert!((90.0..=110.0).contains(&x), "x {x}");
        }
    }
}
