#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codepage {
    Utf8,
    Gbk,
    Big5,
    Latin1,
}

/// Map a DXF `$DWGCODEPAGE` value onto a decoder.
///
/// Unknown values fall back to Latin-1 because it is total: every byte
/// sequence decodes, so an unrecognised codepage degrades to mojibake
/// rather than to data loss.
pub fn codepage_from_dxf(name: &str) -> Codepage {
    let upper = name.trim().to_ascii_uppercase();
    match upper.as_str() {
        "ANSI_936" | "GB2312" | "GBK" | "CP936" => Codepage::Gbk,
        "ANSI_950" | "BIG5" | "CP950" => Codepage::Big5,
        "UTF8" | "UTF-8" => Codepage::Utf8,
        _ => Codepage::Latin1,
    }
}

/// Decode one DXF string.
///
/// UTF-8 is always attempted first: files written by newer tools mix
/// genuine UTF-8 strings into a CP936 drawing (PRD 3.1), so a single
/// whole-file decoding strategy is guaranteed to corrupt one group or
/// the other.
pub fn decode(bytes: &[u8], cp: Codepage) -> String {
    if let Ok(s) = core::str::from_utf8(bytes) {
        return s.to_owned();
    }
    let encoding = match cp {
        Codepage::Utf8 | Codepage::Latin1 => encoding_rs::WINDOWS_1252,
        Codepage::Gbk => encoding_rs::GBK,
        Codepage::Big5 => encoding_rs::BIG5,
    };
    let (decoded, _, _) = encoding.decode(bytes);
    decoded.into_owned()
}

/// The multi-byte code a big font indexes this character by.
///
/// `Document` hands out decoded Rust strings, but `gbcbig.shx` is keyed by
/// the character's codepage bytes: 图 is glyph `0xCDBC` because `CD BC` is
/// its GBK encoding. Returns `None` for anything that encodes to a single
/// byte (which belongs to the primary font) or that the codepage cannot
/// represent at all.
pub fn bigfont_code(ch: char, cp: Codepage) -> Option<u16> {
    let encoding = match cp {
        Codepage::Gbk => encoding_rs::GBK,
        Codepage::Big5 => encoding_rs::BIG5,
        // A UTF-8 or Latin-1 drawing has no double-byte codes to look up.
        Codepage::Utf8 | Codepage::Latin1 => return None,
    };
    let mut buffer = [0u8; 4];
    let text = ch.encode_utf8(&mut buffer);
    let (bytes, _, had_errors) = encoding.encode(text);
    if had_errors || bytes.len() != 2 {
        return None;
    }
    Some(u16::from(bytes[0]) << 8 | u16::from(bytes[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_dwgcodepage_header_value() {
        assert_eq!(codepage_from_dxf("ANSI_936"), Codepage::Gbk);
        assert_eq!(codepage_from_dxf("ansi_950"), Codepage::Big5);
        assert_eq!(codepage_from_dxf("UTF8"), Codepage::Utf8);
        assert_eq!(codepage_from_dxf("ANSI_1252"), Codepage::Latin1);
        assert_eq!(codepage_from_dxf("something unknown"), Codepage::Latin1);
    }

    #[test]
    fn decodes_gbk_layer_names() {
        // "布局1" as stored by dwg2dxf for a CP936 drawing.
        let bytes = [0xB2u8, 0xBC, 0xBE, 0xD6, 0x31];
        assert_eq!(decode(&bytes, Codepage::Gbk), "布局1");
    }

    #[test]
    fn prefers_utf8_even_when_the_header_says_gbk() {
        // The same file also contains genuine UTF-8 strings (PRD 3.1).
        let bytes = "图框文字说明".as_bytes();
        assert_eq!(decode(bytes, Codepage::Gbk), "图框文字说明");
    }

    #[test]
    fn never_produces_replacement_characters_for_gbk_input() {
        let bytes = [0xB2u8, 0xBC, 0xBE, 0xD6, 0x31];
        assert!(!decode(&bytes, Codepage::Gbk).contains('\u{FFFD}'));
    }

    #[test]
    fn passes_pre_damaged_strings_through_unchanged() {
        // R-ENC-4: this layer name is already corrupt in the source DWG and
        // AutoCAD's own PDF shows it corrupt too. We must not "fix" it.
        let bytes = b"230626-\xC3\x94\xC2\xADJC$0$DOTE";
        let out = decode(bytes, Codepage::Gbk);
        assert!(out.starts_with("230626-"), "got {out:?}");
        assert!(out.ends_with("JC$0$DOTE"), "got {out:?}");
    }

    /// A bigfont indexes glyphs by the character's codepage bytes, not by
    /// its Unicode scalar: 图 is glyph 0xCDBC in gbcbig.shx because CD BC
    /// is its GBK encoding. Looking it up as U+56FE finds nothing and the
    /// character silently vanishes from the page.
    #[test]
    fn bigfont_codes_come_from_the_drawings_codepage() {
        assert_eq!(bigfont_code('图', Codepage::Gbk), Some(0xCDBC));
        assert_eq!(bigfont_code('纸', Codepage::Gbk), Some(0xD6BD));
        assert_eq!(bigfont_code('一', Codepage::Gbk), Some(0xD2BB));
    }

    #[test]
    fn single_byte_characters_have_no_bigfont_code() {
        assert_eq!(bigfont_code('A', Codepage::Gbk), None);
        assert_eq!(bigfont_code('7', Codepage::Gbk), None);
    }

    /// A character the codepage cannot represent must report that rather
    /// than encode to a replacement byte that indexes the wrong glyph.
    #[test]
    fn characters_outside_the_codepage_have_no_code() {
        assert_eq!(bigfont_code('\u{1F600}', Codepage::Gbk), None);
    }
}
