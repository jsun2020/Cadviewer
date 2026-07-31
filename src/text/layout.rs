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
    fn char_geom(&mut self, ch: char, cp: Codepage) -> Option<CharGeom> {
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
                // Single-byte characters index by their own code; anything
                // else needs its codepage bytes (see `bigfont_code`).
                let code = if (ch as u32) < 0x100 {
                    ch as u16
                } else {
                    bigfont_code(ch, cp)?
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
    /// R-TXT-1.3: single-byte characters come from the primary font,
    /// double-byte characters from the big font.
    ///
    /// A missing glyph returns `None` rather than falling through to the
    /// other font: the same numeric code means a different character in
    /// each, so a fall-through draws confident nonsense.
    pub fn char_geom(&mut self, ch: char, cp: Codepage) -> Option<CharGeom> {
        if (ch as u32) < 0x80 {
            // A space has no outline but still advances, so let the
            // primary font answer even when it draws nothing.
            if let Some(found) = self.primary.char_geom(ch, cp) {
                return Some(found);
            }
            // A TrueType primary that lacks the character, or an SHX font
            // missing an ASCII code, may still be covered by the big font.
            return self.bigfont.char_geom(ch, cp);
        }
        if let Some(found) = self.bigfont.char_geom(ch, cp) {
            return Some(found);
        }
        // A TrueType primary covers CJK by itself; an SHX primary does not
        // and will simply return None here.
        match &self.primary {
            FontHandle::Ttf(_) => self.primary.char_geom(ch, cp),
            _ => None,
        }
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
}
