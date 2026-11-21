// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! How wide text is in the app's own font. The resin list wraps its tags by
//! the real width rather than a guess, so it shapes the text with the same
//! Noto Sans file the windows draw with.
use std::cell::RefCell;
use std::collections::HashMap;

use harfrust::{FontRef, ShapeOptions, ShaperData, UnicodeBuffer};

static NOTO_SANS: &[u8] = include_bytes!("../assets/fonts/NotoSans-Variable.ttf");

/// Measures strings in the bundled font. The resin list asks for the same
/// few tag texts on every refresh, so each text's advance is kept, as a
/// share of the em (which does not depend on the size asked for).
pub(crate) struct TextWidths {
    face: FontRef<'static>,
    tables: ShaperData,
    known: RefCell<HashMap<String, f32>>,
}

/// The advance of `text` in ems. harfrust works out script and direction
/// from the text itself; it panics on a buffer without them (right-to-left
/// text included).
fn em_advance(face: &FontRef<'_>, tables: &ShaperData, text: &str) -> f32 {
    let shaper = tables.shaper(face).build();
    let mut line = UnicodeBuffer::new();
    line.push_str(text);
    line.guess_segment_properties();
    let laid_out = shaper.shape(line, ShapeOptions::default());
    let total: i64 = laid_out.glyph_positions().iter().map(|p| i64::from(p.x_advance)).sum();
    total as f32 / shaper.units_per_em() as f32
}

impl TextWidths {
    /// The bundled font; `None` only if it cannot be read, in which case
    /// the windows could not draw text either.
    pub(crate) fn bundled() -> Option<Self> {
        let face = FontRef::new(NOTO_SANS).ok()?;
        let tables = ShaperData::new(&face);
        Some(TextWidths { face, tables, known: RefCell::default() })
    }

    /// Width of `text` in logical pixels at a `font_px` font size.
    pub(crate) fn pixel_width(&self, text: &str, font_px: f32) -> f32 {
        let cached = self.known.borrow().get(text).copied();
        let ems = cached.unwrap_or_else(|| {
            let measured = em_advance(&self.face, &self.tables, text);
            self.known.borrow_mut().insert(text.to_owned(), measured);
            measured
        });
        ems * font_px
    }
}

#[cfg(test)]
mod tests {
    use super::TextWidths;

    #[test]
    fn widths_scale_with_size_and_differ_by_glyph() {
        let fonts = TextWidths::bundled().expect("the bundled font reads");
        assert_eq!(fonts.pixel_width("", 14.0), 0.0);
        let slim = fonts.pixel_width("llll", 14.0);
        let broad = fonts.pixel_width("MMMM", 14.0);
        assert!(slim > 0.0 && broad > slim, "{slim} {broad}");
        let small = fonts.pixel_width("Mono M7 Pro", 9.0);
        let large = fonts.pixel_width("Mono M7 Pro", 27.0);
        assert!((large - 3.0 * small).abs() < 0.01);
        // Hebrew (right to left) is measured rather than panicking.
        assert!(fonts.pixel_width("\u{5e9}\u{5dc}\u{5d5}\u{5dd}", 14.0) > 0.0);
        // A second request comes from the cache with the same answer.
        assert_eq!(fonts.pixel_width("MMMM", 14.0), broad);
        assert_eq!(fonts.known.borrow().len(), 5);
    }
}
