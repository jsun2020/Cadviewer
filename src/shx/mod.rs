use std::collections::HashMap;
use std::sync::Arc;

pub mod container;
pub mod interp;

use container::{RawFont, ShxKind, parse_container};
use interp::{Interp, Outline};

/// Em used when a font record does not supply a usable one.
///
/// `simplex.shx` measures 21 and `txt.shx` 6, so there is no
/// representative default; this exists only so a corrupt record cannot
/// divide a glyph by zero and put NaN into the scene's bounds.
const FALLBACK_EM: f64 = 21.0;

/// Measure `gbcbig.shx`'s em when its font record's `above` is unusable
/// (measured `00 40 02 00`: byte 0 is 0, and byte 1 read directly as an em
/// gives an advance an order of magnitude off — 5.82 against 64).
///
/// Every glyph in the file opens with a call to subshape `0x8E`, which sets
/// the scale the glyph body draws at, and closes with a call to `0x8F`,
/// which restores that scale and performs the pen-up move to the next
/// character's origin. That move *is* the font's one-em advance, in the
/// same coordinate space every glyph is drawn in — so running exactly
/// those two calls back to back measures the em the font itself uses,
/// without guessing at a byte whose meaning this container's header
/// layout does not document.
fn bigfont_em(raw: &RawFont) -> f64 {
    if !raw.glyphs.contains_key(&0x8E) || !raw.glyphs.contains_key(&0x8F) {
        return FALLBACK_EM;
    }
    let mut probe = raw.glyphs.clone();
    let calibration_code = 0xFFFFu16;
    probe.insert(calibration_code, vec![0x07, 0x8E, 0x07, 0x8F, 0x00]);
    let advance = Interp { glyphs: &probe, wide_subshape: false }.run(calibration_code).advance;
    if advance > 0.0 { advance } else { FALLBACK_EM }
}

pub struct ShxFont {
    raw: RawFont,
    em: f64,
    /// `Arc` rather than `Rc` so a loaded font can cross a thread boundary:
    /// the viewer hands its whole text engine to the background rebuild it
    /// runs on every sheet switch, and re-reading `gbcbig.shx` (900 KB,
    /// 7,703 glyph records) per switch is what this cache exists to avoid.
    cache: HashMap<u16, Arc<Outline>>,
}

impl ShxFont {
    pub fn load(bytes: &[u8]) -> Result<ShxFont, String> {
        let raw = parse_container(bytes)?;
        // The font record is `above, below, modes, ...`, and `above` is the
        // em: interpreting simplex 'A' gives a bounding box exactly 21
        // units tall, which is its `above`.
        let em = match raw.font_record.first() {
            Some(above) if *above > 0 => f64::from(*above),
            Some(0) if raw.kind == ShxKind::Bigfont => bigfont_em(&raw),
            _ => FALLBACK_EM,
        };
        Ok(ShxFont { raw, em, cache: HashMap::new() })
    }

    pub fn kind(&self) -> ShxKind {
        self.raw.kind
    }

    pub fn em(&self) -> f64 {
        self.em
    }

    pub fn wide_subshape(&self) -> bool {
        self.raw.kind == ShxKind::Unifont
    }

    pub fn has(&self, code: u16) -> bool {
        self.raw.glyphs.contains_key(&code)
    }

    /// Outlines in raw font units, interpreted once per code.
    ///
    /// A drawing repeats the same few hundred characters thousands of
    /// times — the reference sheet has 1,257 TEXT entities alone — so
    /// re-running the interpreter per occurrence is the difference between
    /// milliseconds and minutes.
    pub fn glyph(&mut self, code: u16) -> Arc<Outline> {
        if let Some(hit) = self.cache.get(&code) {
            return Arc::clone(hit);
        }
        let outline = Arc::new(
            Interp { glyphs: &self.raw.glyphs, wide_subshape: self.raw.kind == ShxKind::Unifont }
                .run(code),
        );
        self.cache.insert(code, Arc::clone(&outline));
        outline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_unifont(above: u8) -> Vec<u8> {
        let mut v = b"AutoCAD-86 unifont 1.0\r\n".to_vec();
        v.push(0x1A);
        v.extend_from_slice(&2u16.to_le_bytes());
        let rec0: &[u8] = &[0x00, above, 7, 2, 0, 0, 0];
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&(rec0.len() as u16).to_le_bytes());
        v.extend_from_slice(rec0);
        let rec_a: &[u8] = &[0x00, 0x01, 0xA0, 0x00];
        v.extend_from_slice(&0x41u16.to_le_bytes());
        v.extend_from_slice(&(rec_a.len() as u16).to_le_bytes());
        v.extend_from_slice(rec_a);
        v
    }

    #[test]
    fn the_em_is_the_font_records_above_value() {
        let f = ShxFont::load(&tiny_unifont(21)).unwrap();
        assert_eq!(f.em(), 21.0);
    }

    /// A font record claiming an em of zero would divide every glyph by
    /// zero. Fall back to a sane em rather than emitting NaN coordinates
    /// that poison the scene's bounds.
    #[test]
    fn a_zero_em_falls_back_instead_of_dividing_by_zero() {
        let f = ShxFont::load(&tiny_unifont(0)).unwrap();
        assert!(f.em() > 0.0, "em was {}", f.em());
    }

    #[test]
    fn glyphs_are_cached_by_code() {
        let mut f = ShxFont::load(&tiny_unifont(21)).unwrap();
        let a = f.glyph(0x41);
        let b = f.glyph(0x41);
        assert!(std::sync::Arc::ptr_eq(&a, &b), "the second lookup re-interpreted the glyph");
    }

    #[test]
    fn a_missing_glyph_yields_an_empty_outline_without_caching_confusion() {
        let mut f = ShxFont::load(&tiny_unifont(21)).unwrap();
        assert!(!f.has(0x4E2D));
        assert!(f.glyph(0x4E2D).strokes.is_empty());
    }

    /// Unifont numbers subshapes in two bytes, bigfont in one. The font
    /// must tell the interpreter which, or every CJK glyph calls a
    /// subroutine that does not exist.
    #[test]
    fn the_subshape_width_follows_the_container_kind() {
        let f = ShxFont::load(&tiny_unifont(21)).unwrap();
        assert_eq!(f.kind(), container::ShxKind::Unifont);
        assert!(f.wide_subshape());
    }
}
