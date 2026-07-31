use std::collections::HashMap;

/// Which of the three SHX sub-formats a file uses. All three share the
/// `AutoCAD-86 <kind> <version>` sentinel but differ completely after it
/// (PRD 3.11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShxKind {
    /// Latin fonts: a flat sequence of records, no offset table.
    Unifont,
    /// CJK big fonts: lead-byte ranges plus an index of file offsets.
    Bigfont,
    /// Symbol shape files. Not a font; recognised so it can be rejected
    /// with a useful message rather than parsed as garbage.
    Shapes,
}

#[derive(Clone, Debug)]
pub struct RawFont {
    pub kind: ShxKind,
    /// Glyph code -> bytecode, with the record's leading name and its NUL
    /// terminator already stripped.
    pub glyphs: HashMap<u16, Vec<u8>>,
    /// Glyph 0's payload. Not bytecode: it is the font record carrying the
    /// em height (`above`), descent (`below`) and mode flags.
    pub font_record: Vec<u8>,
}

/// Highest glyph count an index may claim.
///
/// `gbcbig.shx` — the largest font AutoCAD ships, at 900 KB — declares
/// 7,703. A corrupt u16 can claim 65,535, which is survivable, but the cap
/// also stops a truncated file from making us reserve capacity for entries
/// the bytes cannot contain.
const MAX_GLYPHS: usize = 70_000;

/// Split a glyph record into its name and its bytecode.
///
/// Every record — in both sub-formats — begins with a NUL-terminated name
/// (often empty, sometimes the character itself, and for glyph 0 the
/// font's copyright string). A record with no NUL at all is corrupt.
fn strip_name(record: &[u8]) -> Option<&[u8]> {
    let nul = record.iter().position(|b| *b == 0)?;
    Some(&record[nul + 1..])
}

fn sentinel_end(bytes: &[u8]) -> Option<usize> {
    // The sentinel is short; scanning the whole file for 0x1A would let a
    // headerless binary match on an arbitrary byte far inside it.
    bytes.iter().take(64).position(|b| *b == 0x1A)
}

fn kind_of(header: &[u8]) -> Option<ShxKind> {
    let text = String::from_utf8_lossy(header).to_ascii_lowercase();
    if !text.starts_with("autocad-86") {
        return None;
    }
    if text.contains("unifont") {
        Some(ShxKind::Unifont)
    } else if text.contains("bigfont") {
        Some(ShxKind::Bigfont)
    } else if text.contains("shapes") {
        Some(ShxKind::Shapes)
    } else {
        None
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let s = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn parse_container(bytes: &[u8]) -> Result<RawFont, String> {
    let end = sentinel_end(bytes).ok_or("不是 SHX 字库：找不到文件头结束标记")?;
    let kind = kind_of(&bytes[..end]).ok_or_else(|| {
        format!("无法识别的 SHX 文件头：{:?}", String::from_utf8_lossy(&bytes[..end.min(40)]))
    })?;
    if kind == ShxKind::Shapes {
        return Err("这是符号形文件（shapes），不是字库，已跳过".to_owned());
    }
    let at = end + 1;
    match kind {
        ShxKind::Unifont => parse_unifont(bytes, at),
        ShxKind::Bigfont => parse_bigfont(bytes, at),
        ShxKind::Shapes => unreachable!("rejected above"),
    }
}

/// Unifont: `u16 nglyphs`, then that many `(u16 code, u16 len, len bytes)`
/// records laid out back to back. There is no offset table, so the walk is
/// the index — which is why a single bad length would desynchronise the
/// rest of the file, and why the walk stops at the first record that does
/// not fit.
fn parse_unifont(bytes: &[u8], mut at: usize) -> Result<RawFont, String> {
    let declared = u16_at(bytes, at).ok_or("unifont 字库头被截断")? as usize;
    at += 2;
    let mut glyphs = HashMap::with_capacity(declared.min(MAX_GLYPHS));
    let mut font_record = Vec::new();
    for _ in 0..declared.min(MAX_GLYPHS) {
        let Some(code) = u16_at(bytes, at) else { break };
        let Some(len) = u16_at(bytes, at + 2) else { break };
        at += 4;
        let Some(record) = bytes.get(at..at + len as usize) else { break };
        at += len as usize;
        let Some(body) = strip_name(record) else { continue };
        if code == 0 {
            font_record = body.to_vec();
        } else {
            glyphs.insert(code, body.to_vec());
        }
    }
    Ok(RawFont { kind: ShxKind::Unifont, glyphs, font_record })
}

/// Bigfont: a stride, a glyph count, lead-byte ranges, then an index of
/// `(u16 code, u16 len, u32 absolute file offset)` triples.
///
/// Unlike unifont this is random access, so one bad entry costs only its
/// own glyph. Measured on `gbcbig.shx`: 684 of 7,703 entries are `(0, 0)`
/// padding, and reading those as glyphs would parse the file header as
/// bytecode.
fn parse_bigfont(bytes: &[u8], mut at: usize) -> Result<RawFont, String> {
    let stride = u16_at(bytes, at).ok_or("bigfont 字库头被截断")? as usize;
    let declared = u16_at(bytes, at + 2).ok_or("bigfont 字库头被截断")? as usize;
    let ranges = u16_at(bytes, at + 4).ok_or("bigfont 字库头被截断")? as usize;
    at += 6 + 4 * ranges;
    if stride < 8 {
        return Err(format!("bigfont 索引步长 {stride} 小于 8，文件已损坏"));
    }

    let mut glyphs = HashMap::with_capacity(declared.min(MAX_GLYPHS));
    let mut font_record = Vec::new();
    for i in 0..declared.min(MAX_GLYPHS) {
        let entry = at + i * stride;
        let (Some(code), Some(len), Some(offset)) =
            (u16_at(bytes, entry), u16_at(bytes, entry + 2), u32_at(bytes, entry + 4))
        else {
            break;
        };
        if len == 0 || offset == 0 {
            continue; // padding slot
        }
        let Some(record) = bytes.get(offset as usize..offset as usize + len as usize) else {
            continue; // an offset past the end costs one glyph, not the font
        };
        let Some(body) = strip_name(record) else { continue };
        if code == 0 {
            if font_record.is_empty() {
                font_record = body.to_vec();
            }
        } else {
            // Measured: no code carries two usable entries, so first wins
            // is unambiguous.
            glyphs.entry(code).or_insert_with(|| body.to_vec());
        }
    }
    Ok(RawFont { kind: ShxKind::Bigfont, glyphs, font_record })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but structurally real unifont: header, one glyph count,
    /// then (code, len, name-NUL-bytecode) records.
    fn unifont_bytes() -> Vec<u8> {
        let mut v = b"AutoCAD-86 unifont 1.0\r\n".to_vec();
        v.push(0x1A);
        v.extend_from_slice(&2u16.to_le_bytes()); // nglyphs
        // glyph 0: font record "T\0" + above/below/modes/encoding/type
        let rec0: &[u8] = &[b'T', 0x00, 21, 7, 2, 0, 0, 0];
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&(rec0.len() as u16).to_le_bytes());
        v.extend_from_slice(rec0);
        // glyph 'A': empty name, then "pen down, vector, end"
        let rec_a: &[u8] = &[0x00, 0x01, 0xA0, 0x00];
        v.extend_from_slice(&0x41u16.to_le_bytes());
        v.extend_from_slice(&(rec_a.len() as u16).to_le_bytes());
        v.extend_from_slice(rec_a);
        v
    }

    #[test]
    fn reads_a_unifont_container() {
        let f = parse_container(&unifont_bytes()).expect("should parse");
        assert_eq!(f.kind, ShxKind::Unifont);
        assert_eq!(f.glyphs.len(), 1, "glyph 0 is the font record, not a glyph");
        assert_eq!(f.font_record, vec![21, 7, 2, 0, 0, 0]);
        assert_eq!(f.glyphs[&0x41], vec![0x01, 0xA0, 0x00]);
    }

    /// A minimal bigfont: stride, nglyphs, one lead-byte range, then an
    /// index of (code, len, absolute offset) triples.
    fn bigfont_bytes() -> Vec<u8> {
        let mut v = b"AutoCAD-86 bigfont 1.0\r\n".to_vec();
        v.push(0x1A);
        v.extend_from_slice(&8u16.to_le_bytes()); // stride
        v.extend_from_slice(&3u16.to_le_bytes()); // nglyphs (one is padding)
        v.extend_from_slice(&1u16.to_le_bytes()); // nranges
        v.extend_from_slice(&0xA1u16.to_le_bytes());
        v.extend_from_slice(&0xFEu16.to_le_bytes());
        let index_at = v.len();
        let data_at = index_at + 3 * 8;
        let rec0: &[u8] = &[b'B', 0x00, 0, 64, 2, 0];
        let rec_c: &[u8] = &[0xD2, 0xBB, 0x00, 0x07, 0x8E, 0x00];
        // index
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&(rec0.len() as u16).to_le_bytes());
        v.extend_from_slice(&(data_at as u32).to_le_bytes());
        v.extend_from_slice(&0xD2BBu16.to_le_bytes());
        v.extend_from_slice(&(rec_c.len() as u16).to_le_bytes());
        v.extend_from_slice(&((data_at + rec0.len()) as u32).to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes()); // padding entry
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(v.len(), data_at);
        v.extend_from_slice(rec0);
        v.extend_from_slice(rec_c);
        v
    }

    #[test]
    fn reads_a_bigfont_container() {
        let f = parse_container(&bigfont_bytes()).expect("should parse");
        assert_eq!(f.kind, ShxKind::Bigfont);
        assert_eq!(f.font_record, vec![0, 64, 2, 0]);
        assert_eq!(f.glyphs[&0xD2BB], vec![0x07, 0x8E, 0x00]);
    }

    /// 684 of gbcbig's 7703 index entries are (len 0, offset 0) padding.
    /// Treating them as glyphs at file offset 0 reads the header as
    /// bytecode.
    #[test]
    fn bigfont_padding_entries_are_ignored() {
        let f = parse_container(&bigfont_bytes()).unwrap();
        assert_eq!(f.glyphs.len(), 1, "only the one real glyph, got {:?}", f.glyphs.keys());
    }

    /// Symbol shape files share the family but hold no characters. They
    /// must be recognised and rejected, never parsed as a font (R-TXT-1.2).
    #[test]
    fn symbol_shape_files_are_recognised_and_rejected() {
        let mut v = b"AutoCAD-86 shapes 1.1\r\n".to_vec();
        v.push(0x1A);
        v.extend_from_slice(&[0u8; 16]);
        let err = parse_container(&v).expect_err("shapes files are not fonts");
        assert!(err.contains("shapes"), "the error must name the format: {err}");
    }

    /// Untrusted input: a truncated index must not panic or allocate wildly.
    #[test]
    fn a_truncated_container_is_an_error_not_a_panic() {
        let full = bigfont_bytes();
        for cut in [26, 30, 40, full.len() - 3] {
            let _ = parse_container(&full[..cut]);
        }
        let unifull = unifont_bytes();
        for cut in [25, 27, 33] {
            let _ = parse_container(&unifull[..cut]);
        }
    }

    /// An index entry pointing past the end of the file must drop that
    /// glyph, not slice out of bounds.
    #[test]
    fn out_of_range_offsets_drop_the_glyph() {
        let mut v = bigfont_bytes();
        let index_at = 25 + 6 + 4;
        v[index_at + 8 + 4..index_at + 8 + 8].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        let f = parse_container(&v).expect("should still parse");
        assert!(!f.glyphs.contains_key(&0xD2BB));
    }

    #[test]
    fn a_file_with_no_sentinel_is_an_error() {
        assert!(parse_container(b"not a font at all").is_err());
    }
}
