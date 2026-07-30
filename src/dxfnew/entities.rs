use std::collections::HashMap;

use crate::dxfnew::lexer::{Pair, Value};
use crate::encoding::{Codepage, decode};
use crate::geom::Point;

#[derive(Clone, Debug)]
pub struct RawEntity {
    pub kind: String,
    pub codes: Vec<(i32, Value)>,
}

impl RawEntity {
    pub fn f64(&self, code: i32, default: f64) -> f64 {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.as_f64())
            .unwrap_or(default)
    }

    pub fn int(&self, code: i32, default: i32) -> i32 {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.as_i32())
            .unwrap_or(default)
    }

    pub fn text(&self, code: i32, cp: Codepage) -> Option<String> {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.as_bytes())
            .map(|b| decode(b, cp))
    }

    pub fn all_f64(&self, code: i32) -> Vec<f64> {
        self.codes
            .iter()
            .filter(|(c, _)| *c == code)
            .filter_map(|(_, v)| v.as_f64())
            .collect()
    }

    /// Pair up two coordinate group codes into points, in file order.
    pub fn points(&self, x_code: i32, y_code: i32) -> Vec<Point> {
        self.all_f64(x_code)
            .into_iter()
            .zip(self.all_f64(y_code))
            .map(|(x, y)| Point::new(x, y))
            .collect()
    }

    pub fn layer(&self, cp: Codepage) -> String {
        self.text(8, cp).unwrap_or_else(|| "0".to_owned())
    }
}

/// Collect `0`-delimited records inside the named section.
pub fn read_section(pairs: &[Pair], section: &str) -> Vec<RawEntity> {
    let mut out = Vec::new();
    let Some(start) = find_section(pairs, section) else {
        return out;
    };
    let mut i = start;
    while i < pairs.len() {
        if pairs[i].code == 0 {
            let kind = pairs[i].value.as_bytes().unwrap_or_default();
            if kind == b"ENDSEC" {
                break;
            }
            let kind = String::from_utf8_lossy(kind).into_owned();
            let mut codes = Vec::new();
            let mut j = i + 1;
            while j < pairs.len() && pairs[j].code != 0 {
                codes.push((pairs[j].code, pairs[j].value.clone()));
                j += 1;
            }
            out.push(RawEntity { kind, codes });
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn find_section(pairs: &[Pair], section: &str) -> Option<usize> {
    let mut i = 0usize;
    while i + 1 < pairs.len() {
        if pairs[i].code == 0
            && pairs[i].value.as_bytes() == Some(b"SECTION")
            && pairs[i + 1].code == 2
            && pairs[i + 1].value.as_bytes() == Some(section.as_bytes())
        {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

/// Group BLOCKS-section records by owning block name.
pub fn read_blocks(pairs: &[Pair], cp: Codepage) -> HashMap<String, Vec<RawEntity>> {
    let mut out: HashMap<String, Vec<RawEntity>> = HashMap::new();
    let mut current: Option<String> = None;
    for ent in read_section(pairs, "BLOCKS") {
        match ent.kind.as_str() {
            "BLOCK" => {
                let name = ent.text(2, cp).unwrap_or_default();
                out.entry(name.clone()).or_default();
                current = Some(name);
            }
            "ENDBLK" => current = None,
            _ => {
                if let Some(name) = &current {
                    out.entry(name.clone()).or_default().push(ent);
                }
            }
        }
    }
    out
}
