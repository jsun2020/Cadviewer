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
}
