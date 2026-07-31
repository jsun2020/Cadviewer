use crate::encoding::{Codepage, bigfont_code};
use crate::geom::Point;
use crate::shx::ShxFont;
use crate::text::ttf::TtfFont;

/// One loaded font, or the absence of one.
pub enum FontHandle {
    Shx(Box<ShxFont>),
    Ttf(Box<TtfFont>),
    /// The style names no font for this half, or nothing could be found.
    None,
}

/// A style's two fonts: group 3 for single-byte characters, group 4 for
/// double-byte ones (R-TXT-1.3).
pub struct FontPair {
    pub primary: FontHandle,
    pub bigfont: FontHandle,
}

/// One character's outlines, normalised so that 1.0 is the text height.
///
/// Normalising here — rather than in the caller — is what stops a Chinese
/// character drawn from a 64-unit big font sitting at a different size
/// from the Latin text drawn from a 21-unit Latin font beside it.
pub struct CharGeom {
    pub contours: Vec<Vec<Point>>,
    pub advance: f64,
    /// SHX glyphs are pen strokes; TrueType glyphs are closed contours.
    pub fill: bool,
}

fn scale_contours(source: &[Vec<Point>], factor: f64) -> Vec<Vec<Point>> {
    source
        .iter()
        .map(|contour| contour.iter().map(|p| Point::new(p.x * factor, p.y * factor)).collect())
        .collect()
}

impl FontHandle {
    /// Draw a character from this font alone, if it has one.
    ///
    /// `as_bigfont` selects the key space, because the two font kinds do
    /// not share one: a unifont is indexed by the character's own code
    /// point, a big font by its codepage bytes. Using either font's key
    /// on the other is how a degree sign becomes nothing.
    fn char_geom(&mut self, ch: char, cp: Codepage, as_bigfont: bool) -> Option<CharGeom> {
        match self {
            FontHandle::None => None,
            FontHandle::Ttf(font) => {
                let em = font.em();
                let glyph = font.glyph(ch)?;
                Some(CharGeom {
                    contours: scale_contours(&glyph.contours, 1.0 / em),
                    advance: glyph.advance / em,
                    fill: true,
                })
            }
            FontHandle::Shx(font) => {
                let code = if as_bigfont {
                    bigfont_code(ch, cp)?
                } else {
                    u16::try_from(ch as u32).ok()?
                };
                if !font.has(code) {
                    return None;
                }
                let em = font.em();
                let outline = font.glyph(code);
                Some(CharGeom {
                    contours: scale_contours(&outline.strokes, 1.0 / em),
                    advance: outline.advance / em,
                    fill: false,
                })
            }
        }
    }
}

impl FontPair {
    /// R-TXT-1.3: the big font draws what the drawing's codepage encodes
    /// as two bytes; the primary draws everything else.
    ///
    /// That is the actual rule, and it is not "is it ASCII". In a GBK
    /// drawing the degree sign encodes to two bytes (0xA1E3) and lives in
    /// gbcbig, while simplex.shx carries it at its own code point 0xB0 —
    /// so a 0x80 boundary reached neither font and dropped the character.
    ///
    /// The second attempt is not a fall-through to nonsense: each font is
    /// queried in its own key space, so a miss is a genuine miss.
    pub fn char_geom(&mut self, ch: char, cp: Codepage) -> Option<CharGeom> {
        let double_byte = bigfont_code(ch, cp).is_some();
        if double_byte {
            if let Some(found) = self.bigfont.char_geom(ch, cp, true) {
                return Some(found);
            }
            return self.primary.char_geom(ch, cp, false);
        }
        if let Some(found) = self.primary.char_geom(ch, cp, false) {
            return Some(found);
        }
        self.bigfont.char_geom(ch, cp, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::Codepage;
    use std::path::Path;

    fn autocad_font(name: &str) -> Option<Vec<u8>> {
        let path = Path::new("C:\\Program Files\\Autodesk\\AutoCAD 2021\\Fonts").join(name);
        if !path.is_file() {
            eprintln!("SKIPPED: {} not present", path.display());
            return None;
        }
        std::fs::read(path).ok()
    }

    fn shx(name: &str) -> Option<FontHandle> {
        let bytes = autocad_font(name)?;
        Some(FontHandle::Shx(Box::new(crate::shx::ShxFont::load(&bytes).expect("should load"))))
    }

    /// Both fonts are normalised to the same em, so a 1.0-unit advance is
    /// one text height. Without this a Chinese character sits at a
    /// different size from the Latin text beside it.
    #[test]
    fn glyph_geometry_is_normalised_to_the_text_height() {
        let Some(primary) = shx("simplex.shx") else { return };
        let mut pair = FontPair { primary, bigfont: FontHandle::None };
        let a = pair.char_geom('A', Codepage::Gbk).expect("simplex has an A");
        let mut b = crate::geom::Bounds::empty();
        for c in &a.contours {
            for p in c {
                b.add(*p);
            }
        }
        // simplex 'A' is exactly one em tall and advances 22/21 of an em.
        assert!((b.height() - 1.0).abs() < 0.01, "height {} is not one em", b.height());
        assert!((a.advance - 22.0 / 21.0).abs() < 0.01, "advance {}", a.advance);
        assert!(!a.fill, "SHX outlines are strokes, not fills");
    }

    /// R-TXT-1.3: ASCII takes the primary font, double-byte takes the big
    /// font.
    #[test]
    fn ascii_goes_to_the_primary_and_chinese_to_the_bigfont() {
        let (Some(primary), Some(bigfont)) = (shx("simplex.shx"), shx("gbcbig.shx")) else {
            return;
        };
        let mut pair = FontPair { primary, bigfont };
        assert!(pair.char_geom('A', Codepage::Gbk).is_some());
        let cjk = pair.char_geom('图', Codepage::Gbk).expect("gbcbig has 图");
        assert!(!cjk.contours.is_empty(), "the bigfont lookup produced nothing");
        // A full-width character advances about one em.
        assert!(cjk.advance > 0.7, "advance {} is too small for a CJK glyph", cjk.advance);
    }

    /// A style with no big font must not draw Chinese from the Latin font,
    /// where those codes are other characters entirely.
    #[test]
    fn chinese_without_a_bigfont_draws_nothing_rather_than_the_wrong_glyph() {
        let Some(primary) = shx("simplex.shx") else { return };
        let mut pair = FontPair { primary, bigfont: FontHandle::None };
        assert!(pair.char_geom('图', Codepage::Gbk).is_none());
    }

    /// A TTF face serves every character itself — there is no dual-font
    /// split — and its outlines are filled.
    #[test]
    fn a_truetype_primary_serves_every_character_and_is_filled() {
        let path = Path::new("C:\\Windows\\Fonts\\simhei.ttf");
        if !path.is_file() {
            eprintln!("SKIPPED: simhei.ttf not present");
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let font = crate::text::ttf::TtfFont::load(bytes, 0).unwrap();
        let mut pair =
            FontPair { primary: FontHandle::Ttf(Box::new(font)), bigfont: FontHandle::None };
        let g = pair.char_geom('图', Codepage::Gbk).expect("simhei has 图");
        assert!(g.fill, "TrueType contours are closed and must be filled");
        let mut b = crate::geom::Bounds::empty();
        for c in &g.contours {
            for p in c {
                b.add(*p);
            }
        }
        assert!(b.height() > 0.5, "normalised height {}", b.height());
    }

    #[test]
    fn a_space_advances_without_drawing() {
        let Some(primary) = shx("simplex.shx") else { return };
        let mut pair = FontPair { primary, bigfont: FontHandle::None };
        let g = pair.char_geom(' ', Codepage::Gbk).expect("a space still advances");
        assert!(g.contours.is_empty(), "a space drew ink");
        assert!(g.advance > 0.0, "a space did not advance");
    }

    /// The degree sign is what Task 8 emits for `%%d`, and CAD dimension
    /// text is full of it. Every Latin SHX font carries it at its own
    /// code point, and gbcbig carries it at its GBK code — a routing rule
    /// that reaches neither drops it silently.
    #[test]
    fn extended_ascii_symbols_resolve_from_one_font_or_the_other() {
        let (Some(primary), Some(bigfont)) = (shx("simplex.shx"), shx("gbcbig.shx")) else {
            return;
        };
        let mut pair = FontPair { primary, bigfont };
        for ch in ['\u{00B0}', '\u{00B1}'] {
            let g = pair.char_geom(ch, Codepage::Gbk)
                .unwrap_or_else(|| panic!("{ch:?} resolved to no glyph at all"));
            assert!(!g.contours.is_empty(), "{ch:?} produced no geometry");
        }
    }

    /// The same symbol must still resolve when the style has no big font,
    /// this time from the primary's own code point.
    #[test]
    fn extended_ascii_resolves_from_the_primary_when_there_is_no_bigfont() {
        let Some(primary) = shx("simplex.shx") else { return };
        let mut pair = FontPair { primary, bigfont: FontHandle::None };
        let g = pair.char_geom('\u{00B0}', Codepage::Gbk).expect("simplex has the degree sign");
        assert!(!g.contours.is_empty());
    }
}
