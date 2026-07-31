use std::path::{Path, PathBuf};

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
