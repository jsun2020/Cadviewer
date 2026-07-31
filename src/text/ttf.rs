use std::collections::HashMap;
use std::sync::Arc;

use ttf_parser::{Face, OutlineBuilder};

use crate::geom::Point;

/// Segments per quadratic or cubic curve.
///
/// Text is small on a plotted sheet — the reference drawing's heights map
/// to a few millimetres — so eight segments per curve is already below
/// plotter resolution, and the count matters: a page carries thousands of
/// glyphs and every segment becomes a `l` operator in the PDF.
const CURVE_SEGMENTS: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct TtfGlyph {
    /// Closed contours in font units, y up. Curves already flattened.
    pub contours: Vec<Vec<Point>>,
    pub advance: f64,
}

pub struct TtfFont {
    /// Owned so the `Face` borrowed from it can be rebuilt per lookup
    /// without the caller having to keep the bytes alive.
    data: Vec<u8>,
    face_index: u32,
    em: f64,
    /// `Arc` rather than `Rc` for the same reason as `ShxFont`'s cache: the
    /// viewer moves the whole text engine into its rebuild thread.
    cache: HashMap<char, Option<Arc<TtfGlyph>>>,
}

/// Collects `ttf-parser`'s outline callbacks into polylines.
#[derive(Default)]
struct Builder {
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
    at: Point,
}

impl Builder {
    fn flush(&mut self) {
        if self.current.len() > 1 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.at = Point::new(f64::from(x), f64::from(y));
        self.current.push(self.at);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.at = Point::new(f64::from(x), f64::from(y));
        self.current.push(self.at);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.at;
        let c = Point::new(f64::from(x1), f64::from(y1));
        let p1 = Point::new(f64::from(x), f64::from(y));
        for step in 1..=CURVE_SEGMENTS {
            let t = step as f64 / CURVE_SEGMENTS as f64;
            let u = 1.0 - t;
            self.current.push(Point::new(
                u * u * p0.x + 2.0 * u * t * c.x + t * t * p1.x,
                u * u * p0.y + 2.0 * u * t * c.y + t * t * p1.y,
            ));
        }
        self.at = p1;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.at;
        let c1 = Point::new(f64::from(x1), f64::from(y1));
        let c2 = Point::new(f64::from(x2), f64::from(y2));
        let p1 = Point::new(f64::from(x), f64::from(y));
        for step in 1..=CURVE_SEGMENTS {
            let t = step as f64 / CURVE_SEGMENTS as f64;
            let u = 1.0 - t;
            self.current.push(Point::new(
                u * u * u * p0.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p1.x,
                u * u * u * p0.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p1.y,
            ));
        }
        self.at = p1;
    }

    fn close(&mut self) {
        // A closed contour is filled, so the renderers close it themselves;
        // repeating the first point here would only add a zero-length edge.
        self.flush();
    }
}

impl TtfFont {
    pub fn load(data: Vec<u8>, face_index: u32) -> Result<TtfFont, String> {
        let em = {
            let face = Face::parse(&data, face_index)
                .map_err(|e| format!("无法解析 TrueType 字库：{e}"))?;
            f64::from(face.units_per_em())
        };
        if em <= 0.0 {
            return Err("TrueType 字库的 unitsPerEm 无效".to_owned());
        }
        Ok(TtfFont { data, face_index, em, cache: HashMap::new() })
    }

    pub fn em(&self) -> f64 {
        self.em
    }

    pub fn glyph(&mut self, ch: char) -> Option<Arc<TtfGlyph>> {
        if let Some(hit) = self.cache.get(&ch) {
            return hit.clone();
        }
        let built = self.build(ch);
        self.cache.insert(ch, built.clone());
        built
    }

    fn build(&self, ch: char) -> Option<Arc<TtfGlyph>> {
        let face = Face::parse(&self.data, self.face_index).ok()?;
        let id = face.glyph_index(ch)?;
        let advance = f64::from(face.glyph_hor_advance(id).unwrap_or(0));
        let mut builder = Builder::default();
        // A glyph with no outline (a space) still has an advance, so an
        // empty outline is a success, not a miss.
        face.outline_glyph(id, &mut builder);
        builder.flush();
        Some(Arc::new(TtfGlyph { contours: builder.contours, advance }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn system_font(name: &str) -> Option<Vec<u8>> {
        let path = Path::new("C:\\Windows\\Fonts").join(name);
        if !path.is_file() {
            eprintln!("SKIPPED: {} not present", path.display());
            return None;
        }
        std::fs::read(path).ok()
    }

    #[test]
    fn a_latin_glyph_yields_closed_contours() {
        let Some(bytes) = system_font("arial.ttf") else { return };
        let mut f = TtfFont::load(bytes, 0).expect("arial should load");
        assert!(f.em() >= 1000.0, "unexpected units per em {}", f.em());
        let g = f.glyph('A').expect("arial has an A");
        assert!(!g.contours.is_empty());
        // 'A' has an outer contour and the counter inside it.
        assert!(g.contours.len() >= 2, "got {} contours", g.contours.len());
        assert!(g.advance > 0.0);
    }

    /// The whole reason TTF is here: `simhei.ttf` draws the title block.
    #[test]
    fn a_cjk_glyph_is_found_through_the_unicode_cmap() {
        let Some(bytes) = system_font("simhei.ttf") else { return };
        let mut f = TtfFont::load(bytes, 0).expect("simhei should load");
        let g = f.glyph('图').expect("simhei has 图");
        assert!(!g.contours.is_empty());
        let mut b = crate::geom::Bounds::empty();
        for c in &g.contours {
            for p in c {
                b.add(*p);
            }
        }
        // A full-width CJK glyph fills most of the em box.
        assert!(b.height() > f.em() * 0.5, "height {} against em {}", b.height(), f.em());
    }

    /// `.ttc` collections hold several faces; picking the wrong index
    /// silently draws the wrong typeface.
    #[test]
    fn a_collection_can_be_opened_by_face_index() {
        let Some(bytes) = system_font("simsun.ttc") else { return };
        assert!(TtfFont::load(bytes.clone(), 0).is_ok());
        // Index 99 does not exist and must be an error, not a panic.
        assert!(TtfFont::load(bytes, 99).is_err());
    }

    #[test]
    fn curves_are_flattened_into_polylines_not_dropped() {
        let Some(bytes) = system_font("arial.ttf") else { return };
        let mut f = TtfFont::load(bytes, 0).unwrap();
        let o = f.glyph('O').expect("arial has an O");
        // A circle approximated by line segments needs many points; four
        // would mean the quadratic segments were dropped.
        let points: usize = o.contours.iter().map(|c| c.len()).sum();
        assert!(points > 20, "'O' flattened to only {points} points");
    }

    #[test]
    fn a_character_the_face_lacks_returns_none() {
        let Some(bytes) = system_font("arial.ttf") else { return };
        let mut f = TtfFont::load(bytes, 0).unwrap();
        assert!(f.glyph('\u{10FFFF}').is_none());
    }

    #[test]
    fn glyphs_are_cached() {
        let Some(bytes) = system_font("arial.ttf") else { return };
        let mut f = TtfFont::load(bytes, 0).unwrap();
        let a = f.glyph('A').unwrap();
        let b = f.glyph('A').unwrap();
        assert!(std::sync::Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn garbage_bytes_are_an_error_not_a_panic() {
        assert!(TtfFont::load(vec![0u8; 64], 0).is_err());
        assert!(TtfFont::load(Vec::new(), 0).is_err());
    }
}
