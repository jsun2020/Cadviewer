const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\x00";

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(Vec<u8>),
    F64(f64),
    I16(i16),
    I32(i32),
    I64(i64),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(v) => Some(*v),
            Value::I16(v) => Some(*v as f64),
            Value::I32(v) => Some(*v as f64),
            Value::I64(v) => Some(*v as f64),
            Value::Str(b) => core::str::from_utf8(b).ok()?.trim().parse().ok(),
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::I16(v) => Some(*v as i32),
            Value::I32(v) => Some(*v),
            Value::I64(v) => Some(*v as i32),
            Value::F64(v) => Some(*v as i32),
            Value::Str(b) => core::str::from_utf8(b).ok()?.trim().parse().ok(),
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(b) => Some(b.as_slice()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub code: i32,
    pub value: Value,
}

/// Value width implied by a DXF group code, per the DXF reference.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Str,
    F64,
    I16,
    I32,
    I64,
}

fn kind_of(code: i32) -> Kind {
    match code {
        0..=9 => Kind::Str,
        10..=59 => Kind::F64,
        60..=79 => Kind::I16,
        90..=99 => Kind::I32,
        100..=109 => Kind::Str,
        110..=149 => Kind::F64,
        160..=169 => Kind::I64,
        170..=179 => Kind::I16,
        210..=239 => Kind::F64,
        270..=289 => Kind::I16,
        290..=299 => Kind::I16,
        300..=369 => Kind::Str,
        370..=389 => Kind::I16,
        390..=399 => Kind::Str,
        400..=409 => Kind::I16,
        410..=419 => Kind::Str,
        420..=429 => Kind::I32,
        430..=439 => Kind::Str,
        440..=449 => Kind::I32,
        450..=459 => Kind::I32,
        460..=469 => Kind::F64,
        470..=479 => Kind::Str,
        999 => Kind::Str,
        1000..=1009 => Kind::Str,
        1010..=1059 => Kind::F64,
        1060..=1070 => Kind::I16,
        1071 => Kind::I32,
        _ => Kind::Str,
    }
}

pub fn lex(bytes: &[u8]) -> Result<Vec<Pair>, String> {
    if bytes.starts_with(BINARY_SENTINEL) {
        lex_binary(&bytes[BINARY_SENTINEL.len()..])
    } else {
        lex_ascii(bytes)
    }
}

fn lex_binary(mut b: &[u8]) -> Result<Vec<Pair>, String> {
    let mut out = Vec::new();
    while b.len() >= 2 {
        let code = u16::from_le_bytes([b[0], b[1]]) as i32;
        b = &b[2..];
        let value = match kind_of(code) {
            Kind::Str => {
                let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
                let s = b[..end].to_vec();
                b = &b[(end + 1).min(b.len())..];
                Value::Str(s)
            }
            Kind::F64 => {
                if b.len() < 8 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = f64::from_le_bytes(b[..8].try_into().unwrap());
                b = &b[8..];
                Value::F64(v)
            }
            Kind::I16 => {
                if b.len() < 2 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = i16::from_le_bytes([b[0], b[1]]);
                b = &b[2..];
                Value::I16(v)
            }
            Kind::I32 => {
                if b.len() < 4 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = i32::from_le_bytes(b[..4].try_into().unwrap());
                b = &b[4..];
                Value::I32(v)
            }
            Kind::I64 => {
                if b.len() < 8 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = i64::from_le_bytes(b[..8].try_into().unwrap());
                b = &b[8..];
                Value::I64(v)
            }
        };
        out.push(Pair { code, value });
    }
    Ok(out)
}

fn lex_ascii(bytes: &[u8]) -> Result<Vec<Pair>, String> {
    let mut out = Vec::new();
    let mut lines = bytes.split(|c| *c == b'\n');
    loop {
        let Some(code_line) = lines.next() else { break };
        let code_text = core::str::from_utf8(strip_cr(code_line))
            .map_err(|_| "non-ASCII DXF group code".to_owned())?;
        let trimmed = code_text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let code: i32 = trimmed
            .parse()
            .map_err(|_| format!("bad DXF group code {trimmed:?}"))?;
        let Some(value_line) = lines.next() else { break };
        let raw = strip_cr(value_line);
        let value = match kind_of(code) {
            Kind::Str => Value::Str(raw.to_vec()),
            Kind::F64 => Value::F64(parse_ascii(raw).unwrap_or(0.0)),
            Kind::I16 => Value::I16(parse_ascii::<f64>(raw).unwrap_or(0.0) as i16),
            Kind::I32 => Value::I32(parse_ascii::<f64>(raw).unwrap_or(0.0) as i32),
            Kind::I64 => Value::I64(parse_ascii::<f64>(raw).unwrap_or(0.0) as i64),
        };
        out.push(Pair { code, value });
    }
    Ok(out)
}

fn strip_cr(line: &[u8]) -> &[u8] {
    match line.strip_suffix(b"\r") {
        Some(rest) => rest,
        None => line,
    }
}

fn parse_ascii<T: core::str::FromStr>(raw: &[u8]) -> Option<T> {
    core::str::from_utf8(raw).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary_fixture() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"AutoCAD Binary DXF\r\n\x1a\x00");
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(b"SECTION\0");
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(b"ENTITIES\0");
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(b"LINE\0");
        v.extend_from_slice(&10u16.to_le_bytes());
        v.extend_from_slice(&1.5f64.to_le_bytes());
        v.extend_from_slice(&70u16.to_le_bytes());
        v.extend_from_slice(&(-3i16).to_le_bytes());
        v.extend_from_slice(&90u16.to_le_bytes());
        v.extend_from_slice(&7i32.to_le_bytes());
        v
    }

    #[test]
    fn lexes_binary_dxf() {
        let pairs = lex(&binary_fixture()).expect("lex should succeed");
        assert_eq!(pairs[0].code, 0);
        assert_eq!(pairs[0].value.as_bytes(), Some(&b"SECTION"[..]));
        assert_eq!(pairs[1].code, 2);
        assert_eq!(pairs[1].value.as_bytes(), Some(&b"ENTITIES"[..]));
        assert_eq!(pairs[2].value.as_bytes(), Some(&b"LINE"[..]));
        assert_eq!(pairs[3].code, 10);
        assert_eq!(pairs[3].value.as_f64(), Some(1.5));
        assert_eq!(pairs[4].code, 70);
        assert_eq!(pairs[4].value.as_i32(), Some(-3));
        assert_eq!(pairs[5].code, 90);
        assert_eq!(pairs[5].value.as_i32(), Some(7));
    }

    #[test]
    fn lexes_ascii_dxf() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n 10\n1.5\n 70\n-3\n";
        let pairs = lex(src).expect("lex should succeed");
        assert_eq!(pairs[0].code, 0);
        assert_eq!(pairs[0].value.as_bytes(), Some(&b"SECTION"[..]));
        assert_eq!(pairs[3].code, 10);
        assert_eq!(pairs[3].value.as_f64(), Some(1.5));
        assert_eq!(pairs[4].value.as_i32(), Some(-3));
    }

    #[test]
    fn keeps_string_bytes_undecoded() {
        // The lexer must not decode: only the caller knows the codepage,
        // and the file mixes encodings (PRD 3.1).
        let mut v = Vec::new();
        v.extend_from_slice(b"AutoCAD Binary DXF\r\n\x1a\x00");
        v.extend_from_slice(&8u16.to_le_bytes());
        v.extend_from_slice(&[0xB2, 0xBC, 0xBE, 0xD6, 0x31, 0x00]);
        let pairs = lex(&v).expect("lex should succeed");
        assert_eq!(pairs[0].value.as_bytes(), Some(&[0xB2u8, 0xBC, 0xBE, 0xD6, 0x31][..]));
    }
}
