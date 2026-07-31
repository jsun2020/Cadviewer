use std::path::{Path, PathBuf};

use cadviewer::shx::ShxFont;
use cadviewer::shx::container::{ShxKind, parse_container};

/// AutoCAD's font directory. Local-only: these files are licensed assets
/// and are never committed or redistributed (R-TXT-5.1).
fn fonts_dir() -> Option<PathBuf> {
    let dir = Path::new("C:\\Program Files\\Autodesk\\AutoCAD 2021\\Fonts");
    dir.is_dir().then(|| dir.to_path_buf())
}

fn load(name: &str) -> Option<Vec<u8>> {
    let Some(dir) = fonts_dir() else {
        eprintln!("SKIPPED: AutoCAD fonts dir not present");
        return None;
    };
    let path = dir.join(name);
    if !path.is_file() {
        eprintln!("SKIPPED: {} not present", path.display());
        return None;
    }
    std::fs::read(&path).ok()
}

#[test]
fn unifont_walks_consume_the_whole_file() {
    // Measured: simplex 353 glyphs, txt 287, isocp 238, each walk ending
    // exactly at EOF. A count that comes up short means the record walk
    // desynchronised.
    for (name, expected) in [("simplex.shx", 353), ("txt.shx", 287), ("isocp.shx", 238)] {
        let Some(bytes) = load(name) else { continue };
        let font = parse_container(&bytes).expect("should parse");
        assert_eq!(font.kind, ShxKind::Unifont);
        // The count excludes glyph 0, which becomes the font record.
        assert_eq!(font.glyphs.len(), expected - 1, "{name}");
    }
}

#[test]
fn unifont_font_records_carry_the_measured_em() {
    for (name, above, below, modes) in
        [("simplex.shx", 21u8, 7u8, 2u8), ("txt.shx", 6, 2, 2), ("isocp.shx", 40, 12, 0)]
    {
        let Some(bytes) = load(name) else { continue };
        let font = parse_container(&bytes).unwrap();
        assert_eq!(font.font_record[0], above, "{name} above");
        assert_eq!(font.font_record[1], below, "{name} below");
        assert_eq!(font.font_record[2], modes, "{name} modes");
    }
}

#[test]
fn bigfont_index_matches_the_measured_layout() {
    let Some(bytes) = load("gbcbig.shx") else { return };
    let font = parse_container(&bytes).expect("should parse");
    assert_eq!(font.kind, ShxKind::Bigfont);
    // 7703 declared, 684 padding, one of the rest is glyph 0.
    assert_eq!(font.glyphs.len(), 7018, "usable glyph count");
    assert_eq!(font.font_record, vec![0, 64, 2, 0]);
    // The three single-byte codes: the two shared subroutines survive.
    assert!(font.glyphs.contains_key(&0x8E), "the scale-in subroutine is missing");
    assert!(font.glyphs.contains_key(&0x8F), "the scale-out subroutine is missing");
    // Measured bytecode for the subroutines, verbatim.
    assert_eq!(
        font.glyphs[&0x8E],
        vec![0x04, 0x09, 0x03, 0x66, 0x02, 0x0E, 0x08, 0xDE, 0xB0, 0x02, 0x08, 0x00, 0xFB, 0x00]
    );
    // 一 = GBK CD... no: 0xD2BB. 17 declared bytes, 3 of which are the name.
    assert_eq!(
        font.glyphs[&0xD2BB],
        vec![0x07, 0x8E, 0x05, 0x02, 0x08, 0x05, 0x26, 0x01, 0x08, 0x38, 0x04, 0x07, 0x8F, 0x00]
    );
}

fn font(name: &str) -> Option<ShxFont> {
    let bytes = load(name)?;
    Some(ShxFont::load(&bytes).expect("real fonts must load"))
}

/// R-TXT-6.1: a Latin glyph is exactly one em tall and its advance is
/// wider than its ink. Measured: simplex 'A' bbox (0,0)-(16,21),
/// advance 22; 'X' and '0' are 14 wide with advance 20.
#[test]
fn simplex_glyph_metrics_match_the_measured_values() {
    let Some(mut f) = font("simplex.shx") else { return };
    assert_eq!(f.em(), 21.0);
    for (ch, width, advance) in [('A', 16.0, 22.0), ('X', 14.0, 20.0), ('0', 14.0, 20.0)] {
        let outline = f.glyph(ch as u16);
        let mut b = cadviewer::geom::Bounds::empty();
        for stroke in &outline.strokes {
            for p in stroke {
                b.add(*p);
            }
        }
        assert!((b.height() - 21.0).abs() < 0.01, "'{ch}' is {} tall, expected the em", b.height());
        assert!((b.width() - width).abs() < 0.01, "'{ch}' is {} wide", b.width());
        assert!(
            (outline.advance - advance).abs() < 0.01,
            "'{ch}' advance {} — the advance is the pen's final x, not the bbox",
            outline.advance
        );
    }
}

/// R-TXT-6.1 for the CJK half. `gbcbig.shx`'s font record is
/// `00 40 02 00`, so `above` reads as 0 and the fallback em applies —
/// which would size every Chinese character wrongly.
///
/// This test calibrates the real em by measurement: interpret three
/// full-width characters and assert their heights agree with each other,
/// then assert the advance of a full-width character is close to one em.
/// If it fails, read the printed numbers and set the bigfont em from them
/// in `ShxFont::load`; do not guess.
#[test]
fn gbcbig_full_width_glyphs_share_one_em() {
    let Some(mut f) = font("gbcbig.shx") else { return };
    let mut heights = Vec::new();
    let mut advances = Vec::new();
    // 图 纸 说 明 — four dense full-width characters from the title block.
    for code in [0xCDBCu16, 0xD6BD, 0xCBB5, 0xC3F7] {
        let outline = f.glyph(code);
        let mut b = cadviewer::geom::Bounds::empty();
        for stroke in &outline.strokes {
            for p in stroke {
                b.add(*p);
            }
        }
        assert!(b.valid(), "glyph {code:#06X} produced no geometry");
        heights.push(b.height());
        advances.push(outline.advance);
    }
    eprintln!("gbcbig heights {heights:?} advances {advances:?} em {}", f.em());
    let max = heights.iter().cloned().fold(f64::MIN, f64::max);
    let min = heights.iter().cloned().fold(f64::MAX, f64::min);
    assert!(max / min < 1.25, "full-width glyphs disagree about height: {heights:?}");
    let advance = advances[0];
    assert!(
        advance > 0.0 && (advance / f.em() - 1.0).abs() < 0.25,
        "a full-width advance of {advance} against an em of {} is not one em",
        f.em()
    );
}

/// R-TXT-1.4 against a real file: every glyph in the largest shipped font
/// must terminate. This is the test that would catch an unbounded loop
/// reaching production.
#[test]
fn every_gbcbig_glyph_terminates() {
    let Some(mut f) = font("gbcbig.shx") else { return };
    let start = std::time::Instant::now();
    let mut drawn = 0usize;
    for code in 0xA1A1u16..=0xA3FE {
        if f.has(code) && !f.glyph(code).strokes.is_empty() {
            drawn += 1;
        }
    }
    assert!(drawn > 100, "only {drawn} glyphs produced geometry — the interpreter is not working");
    assert!(start.elapsed().as_secs() < 20, "interpreting one GBK block took {:?}", start.elapsed());
}
