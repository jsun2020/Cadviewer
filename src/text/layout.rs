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

use crate::dxf::entities::RawEntity;
use crate::dxf::tables::StyleRecord;
use crate::geom::Affine;
use crate::text::mtext;

/// Characters one entity may lay out.
///
/// The longest string in the reference drawing is a few dozen characters.
/// A cap six orders of magnitude above that costs nothing on real files
/// and stops a corrupt length field from turning one entity into minutes
/// of work.
const MAX_CHARS: usize = 100_000;

/// Line spacing as a multiple of the text height, matching AutoCAD's
/// default MTEXT spacing factor.
const LINE_SPACING: f64 = 1.667;

#[derive(Clone, Debug, Default)]
pub struct TextGeom {
    /// Pen strokes in drawing units, already rotated and positioned.
    pub stroked: Vec<Vec<Point>>,
    /// Closed contours to fill, in the same coordinate system.
    pub filled: Vec<Vec<Point>>,
}

/// One laid-out line: outlines in em units with the origin at the line's
/// own baseline start, plus the width it occupies.
#[derive(Default)]
struct Line {
    stroked: Vec<Vec<Point>>,
    filled: Vec<Vec<Point>>,
    width: f64,
}

/// Place one string's glyphs along a baseline starting at x = 0.
///
/// `line.width` reports the rightmost ink actually drawn, not the summed
/// advance cursor. SHX letters carry real trailing space after their own
/// strokes (simplex 'M' advances 1.14 em but its ink stops at 0.76), so a
/// right- or centre-justified run anchored on the advance total overshoots
/// past the true glyph edge by exactly that trailing gap. Anchoring on the
/// rendered extent instead lands the visible edge on the alignment point.
fn run_line(
    text: &str,
    fonts: &mut FontPair,
    cp: Codepage,
    width_factor: f64,
    height_factor: f64,
) -> Line {
    let mut line = Line::default();
    let mut x = 0.0f64;
    let mut ink_max_x = 0.0f64;
    for ch in text.chars().take(MAX_CHARS) {
        let Some(glyph) = fonts.char_geom(ch, cp) else {
            // A character no font can draw advances by a blank so the rest
            // of the line does not shift left.
            x += 0.5 * width_factor;
            continue;
        };
        for contour in &glyph.contours {
            let placed: Vec<Point> = contour
                .iter()
                .map(|p| Point::new(x + p.x * width_factor * height_factor, p.y * height_factor))
                .collect();
            for p in &placed {
                if p.x.is_finite() {
                    ink_max_x = ink_max_x.max(p.x);
                }
            }
            if glyph.fill {
                line.filled.push(placed);
            } else {
                line.stroked.push(placed);
            }
        }
        x += glyph.advance * width_factor * height_factor;
    }
    // A blank line (spaces only, or empty) has no ink to anchor on; fall
    // back to the advance cursor so it still occupies its true space.
    line.width = if ink_max_x > 0.0 { ink_max_x } else { x };
    line
}

/// Read a group code from the entity, falling back to the style's value
/// and then to a default.
fn positive_or(entity: &RawEntity, code: i32, fallback: f64, default: f64) -> f64 {
    let value = entity.f64(code, f64::NAN);
    if value.is_finite() && value > 0.0 {
        return value;
    }
    if fallback.is_finite() && fallback > 0.0 { fallback } else { default }
}

pub fn lay_out(
    entity: &RawEntity,
    style: &StyleRecord,
    fonts: &mut FontPair,
    cp: Codepage,
) -> Option<TextGeom> {
    // R-TXT-3.4: ATTDEF is the template AutoCAD does not plot.
    let is_mtext = match entity.kind.as_str() {
        "TEXT" | "ATTRIB" => false,
        "MTEXT" => true,
        _ => return None,
    };

    // A style's non-zero fixed height wins over the entity's own.
    let height = if style.fixed_height.is_finite() && style.fixed_height > 0.0 {
        style.fixed_height
    } else {
        let own = entity.f64(40, 0.0);
        if own.is_finite() && own > 0.0 { own } else { return Some(TextGeom::default()) }
    };
    let width_factor = positive_or(entity, 41, style.width_factor, 1.0);
    let oblique = {
        let value = entity.f64(51, f64::NAN);
        let chosen = if value.is_finite() { value } else { style.oblique };
        if chosen.is_finite() { chosen.clamp(-85.0, 85.0) } else { 0.0 }
    };
    let rotation = {
        let value = entity.f64(50, 0.0);
        if value.is_finite() { value } else { 0.0 }
    };

    let insertion = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
    if !insertion.x.is_finite() || !insertion.y.is_finite() {
        return Some(TextGeom::default());
    }

    // MTEXT's body arrives as any number of group 3 continuation chunks
    // followed by the final group 1.
    let raw = if is_mtext {
        let mut joined = String::new();
        for (code, value) in &entity.codes {
            if matches!(code, 3 | 1)
                && let Some(bytes) = value.as_bytes()
            {
                joined.push_str(&crate::encoding::decode(bytes, cp));
            }
        }
        joined
    } else {
        entity.text(1, cp).unwrap_or_default()
    };

    // Build the lines, in em units.
    let mut lines: Vec<Line> = Vec::new();
    if is_mtext {
        // R-TXT-3.3: spans carry the surviving formatting; every control
        // code we do not understand has already been deleted.
        let mut pending = String::new();
        let mut pending_height = 1.0f64;
        let mut pending_width = 1.0f64;
        for span in mtext::parse(&raw) {
            pending.push_str(&span.text);
            pending_height = span.height;
            pending_width = span.width;
            if span.break_after {
                lines.push(run_line(
                    &pending,
                    fonts,
                    cp,
                    width_factor * pending_width,
                    pending_height,
                ));
                pending.clear();
            }
        }
        if !pending.is_empty() || lines.is_empty() {
            lines.push(run_line(&pending, fonts, cp, width_factor * pending_width, pending_height));
        }
    } else {
        lines.push(run_line(&raw, fonts, cp, width_factor, 1.0));
    }

    let widest = lines.iter().map(|l| l.width).fold(0.0f64, f64::max);
    let line_count = lines.len() as f64;

    // Justification, in em units relative to the run's own origin.
    let (dx, dy) = if is_mtext {
        // Group 71: 1..3 top row, 4..6 middle, 7..9 bottom;
        // 1/4/7 left, 2/5/8 centre, 3/6/9 right.
        let attach = entity.int(71, 1).clamp(1, 9);
        let column = (attach - 1) % 3;
        let row = (attach - 1) / 3;
        let block_height = (line_count - 1.0) * LINE_SPACING + 1.0;
        (
            -widest * f64::from(column) / 2.0,
            match row {
                0 => -1.0,             // top: the first baseline hangs one em down
                1 => block_height / 2.0 - 1.0,
                _ => block_height - 1.0,
            },
        )
    } else {
        // TEXT: group 72 horizontal, group 73 vertical. Anything other
        // than plain bottom-left is measured from the alignment point in
        // groups 11/21.
        let horizontal = entity.int(72, 0);
        let vertical = entity.int(73, 0);
        let column = match horizontal {
            1 | 4 => 1.0, // centre, middle
            2 => 2.0,     // right
            _ => 0.0,
        };
        let dy = match vertical {
            1 => 0.0,   // bottom
            2 => -0.5,  // middle
            3 => -1.0,  // top
            _ => 0.0,   // baseline
        };
        // 72 = 4 is "middle", vertically centred on the cap height.
        let dy = if horizontal == 4 { -0.5 } else { dy };
        (-widest * column / 2.0, dy)
    };

    // The origin every offset is measured from.
    let uses_alignment_point = if is_mtext {
        false
    } else {
        entity.int(72, 0) != 0 || entity.int(73, 0) != 0
    };
    let origin = if uses_alignment_point {
        let p = Point::new(entity.f64(11, insertion.x), entity.f64(21, insertion.y));
        if p.x.is_finite() && p.y.is_finite() { p } else { insertion }
    } else {
        insertion
    };

    // em units -> drawing units -> oblique -> rotation -> position. The
    // shear is applied before the rotation so a slanted, rotated run
    // slants along its own baseline rather than along the world axis.
    let shear = Affine {
        a: 1.0,
        b: 0.0,
        c: oblique.to_radians().tan(),
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };
    // Group 71 generation flags: bit 2 backward, bit 4 upside down.
    let generation = entity.int(71, style.generation);
    let mirror = if is_mtext {
        Affine::identity()
    } else {
        Affine::scale(
            if generation & 2 != 0 { -1.0 } else { 1.0 },
            if generation & 4 != 0 { -1.0 } else { 1.0 },
        )
    };
    let transform = Affine::translation(dx, dy)
        .then(mirror)
        .then(shear)
        .then(Affine::scale(height, height))
        .then(Affine::rotation(rotation))
        .then(Affine::translation(origin.x, origin.y));

    let mut out = TextGeom::default();
    for (index, line) in lines.iter().enumerate() {
        let baseline = Affine::translation(0.0, -(index as f64) * LINE_SPACING).then(transform);
        for contour in &line.stroked {
            out.stroked.push(contour.iter().map(|p| baseline.apply(*p)).collect());
        }
        for contour in &line.filled {
            out.filled.push(contour.iter().map(|p| baseline.apply(*p)).collect());
        }
    }

    // A non-finite parameter that slipped through must not reach the
    // scene, where one NaN invalidates every bounds computation.
    out.stroked.retain(|c| c.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    out.filled.retain(|c| c.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    Some(out)
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

    use crate::dxf::entities::RawEntity;
    use crate::dxf::lexer::Value;
    use crate::dxf::tables::StyleRecord;

    fn entity(kind: &str, codes: &[(i32, Value)]) -> RawEntity {
        RawEntity { kind: kind.to_owned(), codes: codes.to_vec() }
    }

    fn text_entity(content: &str, codes: &[(i32, Value)]) -> RawEntity {
        let mut all = vec![(1i32, Value::Str(content.as_bytes().to_vec()))];
        all.extend_from_slice(codes);
        entity("TEXT", &all)
    }

    fn simplex_pair() -> Option<FontPair> {
        Some(FontPair { primary: shx("simplex.shx")?, bigfont: FontHandle::None })
    }

    fn bounds(g: &TextGeom) -> crate::geom::Bounds {
        let mut b = crate::geom::Bounds::empty();
        for contour in g.stroked.iter().chain(g.filled.iter()) {
            for p in contour {
                b.add(*p);
            }
        }
        b
    }

    /// R-TXT-3.1: the height is group 40 and the glyphs land at the
    /// insertion point.
    #[test]
    fn text_is_drawn_at_its_height_and_insertion_point() {
        let Some(mut fonts) = simplex_pair() else { return };
        let ent = text_entity(
            "A",
            &[(40, Value::F64(100.0)), (10, Value::F64(500.0)), (20, Value::F64(300.0))],
        );
        let g = lay_out(&ent, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap();
        let b = bounds(&g);
        assert!((b.height() - 100.0).abs() < 1.0, "height {}", b.height());
        assert!((b.min_x - 500.0).abs() < 1.0, "min_x {}", b.min_x);
        assert!((b.min_y - 300.0).abs() < 1.0, "min_y {}", b.min_y);
    }

    /// R-TXT-3.1: a style's non-zero group 40 overrides the entity's own
    /// height. Measured values in the reference drawing: 2.5, 3.0, 3.5,
    /// 200.0, 250.0.
    #[test]
    fn a_styles_fixed_height_overrides_the_entitys() {
        let Some(mut fonts) = simplex_pair() else { return };
        let ent = text_entity("A", &[(40, Value::F64(100.0))]);
        let style = StyleRecord { fixed_height: 250.0, ..StyleRecord::default() };
        let g = lay_out(&ent, &style, &mut fonts, Codepage::Gbk).unwrap();
        assert!((bounds(&g).height() - 250.0).abs() < 2.0, "{:?}", bounds(&g));
    }

    /// R-TXT-6.4: the measured width factors are 0.707, 0.7 and 0.8.
    #[test]
    fn the_width_factor_narrows_the_text_without_changing_its_height() {
        let Some(mut fonts) = simplex_pair() else { return };
        let wide = text_entity("MMM", &[(40, Value::F64(100.0))]);
        let narrow = text_entity("MMM", &[(40, Value::F64(100.0)), (41, Value::F64(0.707))]);
        let a = bounds(&lay_out(&wide, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        let b =
            bounds(&lay_out(&narrow, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        assert!((b.width() / a.width() - 0.707).abs() < 0.02, "ratio {}", b.width() / a.width());
        assert!((b.height() - a.height()).abs() < 1.0, "the height changed too");
    }

    /// The entity's own group 41 wins over the style's.
    #[test]
    fn an_entity_width_factor_overrides_the_styles() {
        let Some(mut fonts) = simplex_pair() else { return };
        let ent = text_entity("MMM", &[(40, Value::F64(100.0)), (41, Value::F64(1.0))]);
        let style = StyleRecord { width_factor: 0.5, ..StyleRecord::default() };
        let plain = text_entity("MMM", &[(40, Value::F64(100.0))]);
        let a = bounds(&lay_out(&ent, &style, &mut fonts, Codepage::Gbk).unwrap());
        let b = bounds(&lay_out(&plain, &style, &mut fonts, Codepage::Gbk).unwrap());
        assert!(a.width() > b.width() * 1.8, "{} vs {}", a.width(), b.width());
    }

    /// R-TXT-3.1: rotation is group 50, in degrees, about the insertion
    /// point.
    #[test]
    fn rotation_turns_the_run_about_its_insertion_point() {
        let Some(mut fonts) = simplex_pair() else { return };
        let flat = text_entity("MMMM", &[(40, Value::F64(100.0))]);
        let turned = text_entity("MMMM", &[(40, Value::F64(100.0)), (50, Value::F64(90.0))]);
        let a = bounds(&lay_out(&flat, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        let b =
            bounds(&lay_out(&turned, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        assert!(a.width() > a.height(), "the flat run should be wide");
        assert!(b.height() > b.width(), "the turned run should be tall");
    }

    /// R-TXT-3.2: horizontal justification 1 (centre) and 2 (right) are
    /// measured from the alignment point in groups 11/21, not from 10/20.
    #[test]
    fn centred_and_right_aligned_text_uses_the_alignment_point() {
        let Some(mut fonts) = simplex_pair() else { return };
        let make = |code72: i32| {
            text_entity(
                "MMMM",
                &[
                    (40, Value::F64(100.0)),
                    (10, Value::F64(0.0)),
                    (20, Value::F64(0.0)),
                    (11, Value::F64(1000.0)),
                    (21, Value::F64(0.0)),
                    (72, Value::I32(code72)),
                ],
            )
        };
        let left = bounds(
            &lay_out(&make(0), &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap(),
        );
        let centre = bounds(
            &lay_out(&make(1), &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap(),
        );
        let right = bounds(
            &lay_out(&make(2), &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap(),
        );
        // Left alignment ignores 11/21 and starts at the insertion point.
        assert!(left.min_x.abs() < 1.0, "left run started at {}", left.min_x);
        // Centred text straddles x = 1000.
        assert!(centre.min_x < 1000.0 && centre.max_x > 1000.0, "{centre:?}");
        // Right-aligned text ends there.
        assert!((right.max_x - 1000.0).abs() < 2.0, "{right:?}");
    }

    /// R-TXT-3.2: vertical justification 3 is "top", so the text hangs
    /// below the alignment point.
    #[test]
    fn top_justified_text_hangs_below_its_alignment_point() {
        let Some(mut fonts) = simplex_pair() else { return };
        let ent = text_entity(
            "M",
            &[
                (40, Value::F64(100.0)),
                (11, Value::F64(0.0)),
                (21, Value::F64(0.0)),
                (72, Value::I32(0)),
                (73, Value::I32(3)),
            ],
        );
        let b = bounds(&lay_out(&ent, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        assert!(b.max_y <= 1.0, "top-justified text rose above its point: {b:?}");
    }

    /// R-TXT-3.4: ATTRIB is drawn from its own text attributes; ATTDEF is
    /// the template and AutoCAD does not plot it.
    #[test]
    fn attribs_are_drawn_and_attdefs_are_not() {
        let Some(mut fonts) = simplex_pair() else { return };
        let attrib = entity(
            "ATTRIB",
            &[(1, Value::Str(b"A1".to_vec())), (40, Value::F64(100.0))],
        );
        let attdef = entity(
            "ATTDEF",
            &[(1, Value::Str(b"A1".to_vec())), (40, Value::F64(100.0))],
        );
        assert!(lay_out(&attrib, &StyleRecord::default(), &mut fonts, Codepage::Gbk).is_some());
        assert!(lay_out(&attdef, &StyleRecord::default(), &mut fonts, Codepage::Gbk).is_none());
    }

    /// MTEXT joins its group 3 continuation chunks with the final group 1.
    #[test]
    fn mtext_continuation_chunks_are_joined_in_order() {
        let Some(mut fonts) = simplex_pair() else { return };
        let ent = entity(
            "MTEXT",
            &[
                (3, Value::Str(b"AAA".to_vec())),
                (3, Value::Str(b"BBB".to_vec())),
                (1, Value::Str(b"CC".to_vec())),
                (40, Value::F64(100.0)),
            ],
        );
        let single = entity(
            "MTEXT",
            &[(1, Value::Str(b"AAABBBCC".to_vec())), (40, Value::F64(100.0))],
        );
        let a = bounds(&lay_out(&ent, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        let b =
            bounds(&lay_out(&single, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        assert!((a.width() - b.width()).abs() < 1.0, "{} vs {}", a.width(), b.width());
    }

    /// R-TXT-3.3 end to end: the alignment code must not reach the page.
    /// `\A1;2000` appears 226 times in the reference drawing.
    #[test]
    fn an_mtext_control_code_does_not_become_visible_text() {
        let Some(mut fonts) = simplex_pair() else { return };
        let coded = entity(
            "MTEXT",
            &[(1, Value::Str(b"\\A1;2000".to_vec())), (40, Value::F64(100.0))],
        );
        let plain =
            entity("MTEXT", &[(1, Value::Str(b"2000".to_vec())), (40, Value::F64(100.0))]);
        let a = bounds(&lay_out(&coded, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        let b = bounds(&lay_out(&plain, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        assert!(
            (a.width() - b.width()).abs() < 1.0,
            "the control code was drawn: {} vs {}",
            a.width(),
            b.width()
        );
    }

    /// A hard break stacks lines downward.
    #[test]
    fn mtext_hard_breaks_stack_lines() {
        let Some(mut fonts) = simplex_pair() else { return };
        let ent = entity(
            "MTEXT",
            &[(1, Value::Str(b"AA\\PBB".to_vec())), (40, Value::F64(100.0))],
        );
        let b = bounds(&lay_out(&ent, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        assert!(b.height() > 150.0, "two lines should be taller than one: {b:?}");
    }

    /// MTEXT's group 71 attachment point places the block; 1 is top-left.
    #[test]
    fn mtext_attachment_places_the_block_relative_to_its_insertion_point() {
        let Some(mut fonts) = simplex_pair() else { return };
        let make = |attach: i32| {
            entity(
                "MTEXT",
                &[
                    (1, Value::Str(b"MM".to_vec())),
                    (40, Value::F64(100.0)),
                    (10, Value::F64(0.0)),
                    (20, Value::F64(0.0)),
                    (71, Value::I32(attach)),
                ],
            )
        };
        let top_left =
            bounds(&lay_out(&make(1), &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        let bottom_left =
            bounds(&lay_out(&make(7), &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap());
        assert!(top_left.max_y <= 1.0, "top-left should hang below y=0: {top_left:?}");
        assert!(bottom_left.min_y >= -1.0, "bottom-left should sit above y=0: {bottom_left:?}");
    }

    /// Untrusted input: absurd or non-finite numbers must not put NaN into
    /// the scene, where they poison every bounds computation downstream.
    #[test]
    fn non_finite_and_absurd_parameters_produce_no_geometry() {
        let Some(mut fonts) = simplex_pair() else { return };
        for codes in [
            vec![(40i32, Value::F64(f64::NAN))],
            vec![(40, Value::F64(0.0))],
            vec![(40, Value::F64(-50.0))],
            vec![(40, Value::F64(100.0)), (50, Value::F64(f64::INFINITY))],
            vec![(40, Value::F64(100.0)), (41, Value::F64(0.0))],
        ] {
            let ent = text_entity("A", &codes);
            let g = lay_out(&ent, &StyleRecord::default(), &mut fonts, Codepage::Gbk);
            if let Some(g) = g {
                for contour in g.stroked.iter().chain(g.filled.iter()) {
                    for p in contour {
                        assert!(p.x.is_finite() && p.y.is_finite(), "NaN reached the scene: {p:?}");
                    }
                }
            }
        }
    }

    /// A very long string must not be able to consume unbounded time.
    #[test]
    fn an_absurdly_long_string_is_bounded() {
        let Some(mut fonts) = simplex_pair() else { return };
        let long = "M".repeat(200_000);
        let ent = text_entity(&long, &[(40, Value::F64(100.0))]);
        let start = std::time::Instant::now();
        let g = lay_out(&ent, &StyleRecord::default(), &mut fonts, Codepage::Gbk).unwrap();
        assert!(start.elapsed().as_secs() < 10, "took {:?}", start.elapsed());
        assert!(g.stroked.len() < 2_000_000);
    }
}
