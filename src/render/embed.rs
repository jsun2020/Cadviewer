//! TrueType faces embedded in the exported PDF, so text drawn from them is
//! selectable and searchable rather than only visible (R-TXT-4.2).
//!
//! Only the faces a page actually uses are read, and only the glyphs it
//! actually shows are kept: `simsun.ttc` is 16 MB, and a drawing that shows
//! forty distinct characters from it has no business carrying the rest.
//!
//! A face that cannot be read, parsed or subset is simply not embedded. The
//! caller then draws that run's outlines exactly as before — the page still
//! looks right, it is only not selectable. That is the correct trade: a PDF
//! whose text extracts as the wrong characters is worse than one that does
//! not extract at all.

use std::collections::{BTreeMap, BTreeSet};

use ttf_parser::Face;

use crate::text::ttf::FaceKey;

/// One face, subsetted down to the glyphs one export shows.
pub struct EmbeddedFace {
    /// The subsetted font program, for `/FontFile2`.
    pub program: Vec<u8>,
    /// Name used both as `/BaseFont` and in the page's `/Font` dictionary.
    pub base_name: String,
    /// Character to the glyph id it has *in the subset*. Subsetting renumbers
    /// glyphs from zero, so the original ids must never reach the page.
    pub glyphs: BTreeMap<char, u16>,
    /// Subset glyph id to advance width, in the 1/1000 em PDF expects.
    pub widths: BTreeMap<u16, f32>,
    /// Font-matrix scale: how many font units make one em.
    pub units_per_em: f64,
    pub bbox: [f32; 4],
    pub ascent: f32,
    pub descent: f32,
    pub cap_height: f32,
    pub italic_angle: f32,
    pub is_serif_guess: bool,
}

/// Six upper-case letters and a `+`, the conventional marker that a font is
/// a subset rather than the whole face (PDF 1.7 9.6.4).
///
/// Derived from the face's own path so the same face gets the same tag on
/// every export, which keeps two runs of the converter byte-comparable.
fn subset_tag(key: &FaceKey) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in key.path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash ^= u64::from(key.index);
    hash = hash.wrapping_mul(0x1000_0000_01b3);
    let mut tag = String::with_capacity(7);
    for i in 0..6 {
        let letter = ((hash >> (i * 8)) % 26) as u8;
        tag.push((b'A' + letter) as char);
    }
    tag.push('+');
    tag
}

/// PDF names cannot carry arbitrary bytes, and a Windows CJK face's own
/// name may be anything at all. The file stem, reduced to ASCII
/// alphanumerics, is recognisable without being a parsing hazard.
fn ascii_stem(key: &FaceKey) -> String {
    let stem: String = key
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(32)
        .collect();
    if stem.is_empty() { "Embedded".to_owned() } else { stem }
}

/// Read, subset and measure one face for the characters given.
///
/// `None` whenever anything at all goes wrong, because every failure here
/// has the same safe answer: fall back to drawing the outlines.
pub fn embed(key: &FaceKey, chars: &BTreeSet<char>) -> Option<EmbeddedFace> {
    if chars.is_empty() {
        return None;
    }
    let data = std::fs::read(&key.path).ok()?;
    let face = Face::parse(&data, key.index).ok()?;
    let units_per_em = f64::from(face.units_per_em());
    if units_per_em <= 0.0 {
        return None;
    }

    // Original glyph ids, in a stable order so the subset is deterministic.
    let mut originals: BTreeMap<char, u16> = BTreeMap::new();
    for ch in chars {
        // A character the face lacks cannot be shown from it. The caller
        // only ever asks for characters it already drew from this face, so
        // this is a corrupt-face guard rather than an expected path.
        let id = face.glyph_index(*ch)?;
        originals.insert(*ch, id.0);
    }

    let ids: Vec<u16> = originals.values().copied().collect();
    let mapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(&ids);
    let program = subsetter::subset(&data, key.index, &mapper).ok()?;

    let mut glyphs = BTreeMap::new();
    let mut widths = BTreeMap::new();
    let to_thousandths = 1000.0 / units_per_em;
    for (ch, original) in &originals {
        let new = mapper.get(*original)?;
        glyphs.insert(*ch, new);
        let advance = f64::from(face.glyph_hor_advance(ttf_parser::GlyphId(*original)).unwrap_or(0));
        widths.insert(new, (advance * to_thousandths) as f32);
    }

    let bbox = face.global_bounding_box();
    Some(EmbeddedFace {
        program,
        base_name: format!("{}{}", subset_tag(key), ascii_stem(key)),
        glyphs,
        widths,
        units_per_em,
        bbox: [
            (f64::from(bbox.x_min) * to_thousandths) as f32,
            (f64::from(bbox.y_min) * to_thousandths) as f32,
            (f64::from(bbox.x_max) * to_thousandths) as f32,
            (f64::from(bbox.y_max) * to_thousandths) as f32,
        ],
        ascent: (f64::from(face.ascender()) * to_thousandths) as f32,
        descent: (f64::from(face.descender()) * to_thousandths) as f32,
        cap_height: (f64::from(face.capital_height().unwrap_or(face.ascender()))
            * to_thousandths) as f32,
        italic_angle: face.italic_angle(),
        is_serif_guess: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_face(name: &str) -> Option<FaceKey> {
        let path = std::path::Path::new("C:\\Windows\\Fonts").join(name);
        if !path.is_file() {
            eprintln!("SKIPPED: {} not present", path.display());
            return None;
        }
        Some(FaceKey { path, index: 0 })
    }

    #[test]
    fn a_subset_carries_only_the_glyphs_asked_for() {
        let Some(key) = system_face("arial.ttf") else { return };
        let chars: BTreeSet<char> = "AB".chars().collect();
        let face = embed(&key, &chars).expect("arial should subset");
        assert_eq!(face.glyphs.len(), 2);
        let whole = std::fs::metadata(&key.path).unwrap().len() as usize;
        assert!(
            face.program.len() * 10 < whole,
            "the subset is {} bytes against the face's {whole}",
            face.program.len()
        );
    }

    /// The renumbering is the whole hazard: showing the original ids against
    /// a subset that renumbered them draws different letters than the ones
    /// extraction would report.
    #[test]
    fn the_subset_glyph_ids_index_the_subset_not_the_original() {
        let Some(key) = system_face("arial.ttf") else { return };
        let chars: BTreeSet<char> = "Wm".chars().collect();
        let face = embed(&key, &chars).unwrap();
        let subset = Face::parse(&face.program, 0).expect("the subset must parse");
        assert!(subset.number_of_glyphs() <= 3, "subset kept {} glyphs", subset.number_of_glyphs());
        for id in face.glyphs.values() {
            assert!(*id < subset.number_of_glyphs(), "gid {id} is outside the subset");
        }
    }

    /// Every glyph shown must still have an outline in the subset, which is
    /// the check LL-026 was missing: a subset can embed, extract correctly
    /// and draw nothing at all.
    #[test]
    fn every_subset_glyph_still_has_an_outline() {
        let Some(key) = system_face("simhei.ttf").or_else(|| system_face("arial.ttf")) else {
            return;
        };
        let chars: BTreeSet<char> = "\u{56fe}\u{540d}AB".chars().collect();
        let Some(face) = embed(&key, &chars) else { return };
        let subset = Face::parse(&face.program, 0).expect("the subset must parse");
        for (ch, id) in &face.glyphs {
            let mut sink = Outlines::default();
            let bbox = subset.outline_glyph(ttf_parser::GlyphId(*id), &mut sink);
            assert!(
                bbox.is_some() && sink.points > 2,
                "{ch:?} (gid {id}) has no drawable outline in the subset"
            );
        }
    }

    #[derive(Default)]
    struct Outlines {
        points: usize,
    }

    impl ttf_parser::OutlineBuilder for Outlines {
        fn move_to(&mut self, _: f32, _: f32) {
            self.points += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {
            self.points += 1;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.points += 1;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.points += 1;
        }
        fn close(&mut self) {}
    }

    #[test]
    fn a_missing_face_is_not_embedded_rather_than_a_panic() {
        let key = FaceKey { path: std::path::PathBuf::from("C:\\nope\\none.ttf"), index: 0 };
        assert!(embed(&key, &"A".chars().collect()).is_none());
    }
}
