use std::collections::HashMap;

use crate::dxf::lexer::Pair;
use crate::encoding::{Codepage, codepage_from_dxf, decode};
use crate::geom::Point;

#[derive(Clone, Debug)]
pub struct HeaderVars {
    pub codepage: Codepage,
    pub ltscale: f64,
    pub psltscale: i32,
    pub celweight: i16,
    pub extmin: Point,
    pub extmax: Point,
}

impl Default for HeaderVars {
    fn default() -> Self {
        Self {
            codepage: Codepage::Latin1,
            ltscale: 1.0,
            psltscale: 1,
            celweight: -3,
            extmin: Point::new(0.0, 0.0),
            extmax: Point::new(0.0, 0.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayerRecord {
    pub name: String,
    pub aci: i16,
    pub true_color: Option<u32>,
    pub lineweight: i16,
    pub linetype: String,
}

#[derive(Clone, Debug)]
pub struct LtypeRecord {
    pub name: String,
    /// Dash pattern in drawing units. Positive is ink, negative is gap.
    pub pattern: Vec<f64>,
}

/// Read `$`-prefixed header variables. Layout is `9 <name>` followed by one
/// or more value pairs belonging to that name.
pub fn read_header(pairs: &[Pair]) -> HeaderVars {
    let mut h = HeaderVars::default();
    let mut i = 0usize;
    while i < pairs.len() {
        if pairs[i].code != 9 {
            i += 1;
            continue;
        }
        let name = pairs[i]
            .value
            .as_bytes()
            .map(|b| String::from_utf8_lossy(b).trim().to_owned())
            .unwrap_or_default();
        let mut j = i + 1;
        let mut values: Vec<&Pair> = Vec::new();
        while j < pairs.len() && pairs[j].code != 9 && pairs[j].code != 0 {
            values.push(&pairs[j]);
            j += 1;
        }
        match name.as_str() {
            "$DWGCODEPAGE" => {
                if let Some(b) = values.first().and_then(|p| p.value.as_bytes()) {
                    h.codepage = codepage_from_dxf(&String::from_utf8_lossy(b));
                }
            }
            "$LTSCALE" => h.ltscale = first_f64(&values).unwrap_or(1.0),
            "$PSLTSCALE" => h.psltscale = first_i32(&values).unwrap_or(1),
            "$CELWEIGHT" => h.celweight = first_i32(&values).unwrap_or(-3) as i16,
            "$EXTMIN" => h.extmin = header_point(&values),
            "$EXTMAX" => h.extmax = header_point(&values),
            _ => {}
        }
        i = j;
    }
    h
}

fn first_f64(values: &[&Pair]) -> Option<f64> {
    values.iter().find_map(|p| p.value.as_f64())
}

fn first_i32(values: &[&Pair]) -> Option<i32> {
    values.iter().find_map(|p| p.value.as_i32())
}

fn header_point(values: &[&Pair]) -> Point {
    let x = values.iter().find(|p| p.code == 10).and_then(|p| p.value.as_f64());
    let y = values.iter().find(|p| p.code == 20).and_then(|p| p.value.as_f64());
    Point::new(x.unwrap_or(0.0), y.unwrap_or(0.0))
}

/// Split the TABLE of the given name into per-record pair slices.
fn table_records<'a>(pairs: &'a [Pair], table: &str) -> Vec<Vec<&'a Pair>> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut i = 0usize;
    while i < pairs.len() {
        if pairs[i].code == 0 {
            let kind = pairs[i].value.as_bytes().unwrap_or_default();
            if kind == b"TABLE" {
                inside = pairs
                    .get(i + 1)
                    .and_then(|n| n.value.as_bytes())
                    .is_some_and(|n| n == table.as_bytes());
            } else if kind == b"ENDTAB" {
                inside = false;
            } else if inside && kind == table.as_bytes() {
                let mut rec = Vec::new();
                let mut j = i + 1;
                while j < pairs.len() && pairs[j].code != 0 {
                    rec.push(&pairs[j]);
                    j += 1;
                }
                out.push(rec);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn record_string(rec: &[&Pair], code: i32, cp: Codepage) -> Option<String> {
    rec.iter()
        .find(|p| p.code == code)
        .and_then(|p| p.value.as_bytes())
        .map(|b| decode(b, cp))
}

fn record_i32(rec: &[&Pair], code: i32) -> Option<i32> {
    rec.iter().find(|p| p.code == code).and_then(|p| p.value.as_i32())
}

pub fn read_layers(pairs: &[Pair], cp: Codepage) -> HashMap<String, LayerRecord> {
    let mut out = HashMap::new();
    for rec in table_records(pairs, "LAYER") {
        let Some(name) = record_string(&rec, 2, cp) else { continue };
        let record = LayerRecord {
            aci: record_i32(&rec, 62).unwrap_or(7) as i16,
            true_color: record_i32(&rec, 420).filter(|v| *v > 0).map(|v| v as u32),
            lineweight: record_i32(&rec, 370).unwrap_or(-3) as i16,
            linetype: record_string(&rec, 6, cp).unwrap_or_else(|| "CONTINUOUS".to_owned()),
            name: name.clone(),
        };
        out.insert(name, record);
    }
    out
}

pub fn read_ltypes(pairs: &[Pair], cp: Codepage) -> HashMap<String, LtypeRecord> {
    let mut out = HashMap::new();
    for rec in table_records(pairs, "LTYPE") {
        let Some(name) = record_string(&rec, 2, cp) else { continue };
        let pattern = rec
            .iter()
            .filter(|p| p.code == 49)
            .filter_map(|p| p.value.as_f64())
            .collect();
        out.insert(name.clone(), LtypeRecord { name, pattern });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::lexer::lex;
    use crate::encoding::Codepage;

    const SRC: &[u8] = b"  0\nSECTION\n  2\nHEADER\n  9\n$DWGCODEPAGE\n  3\nANSI_936\n  9\n$LTSCALE\n 40\n10.0\n  9\n$CELWEIGHT\n370\n25\n  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nWALL\n 62\n4\n370\n35\n  6\nHIDDEN\n  0\nLAYER\n  2\nTHIN\n 62\n8\n370\n-3\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nTABLE\n  2\nLTYPE\n  0\nLTYPE\n  2\nHIDDEN\n 73\n2\n 49\n6.35\n 49\n-3.175\n  0\nENDTAB\n  0\nENDSEC\n";

    #[test]
    fn reads_header_variables() {
        let h = read_header(&lex(SRC).unwrap());
        assert_eq!(h.codepage, Codepage::Gbk);
        assert_eq!(h.ltscale, 10.0);
        assert_eq!(h.celweight, 25);
    }

    #[test]
    fn reads_layers_with_lineweight_and_linetype() {
        let layers = read_layers(&lex(SRC).unwrap(), Codepage::Gbk);
        let wall = &layers["WALL"];
        assert_eq!(wall.aci, 4);
        assert_eq!(wall.lineweight, 35);
        assert_eq!(wall.linetype, "HIDDEN");
        assert_eq!(layers["THIN"].lineweight, -3, "ByLayer-default sentinel must survive");
    }

    #[test]
    fn reads_linetype_dash_patterns() {
        let ltypes = read_ltypes(&lex(SRC).unwrap(), Codepage::Gbk);
        assert_eq!(ltypes["HIDDEN"].pattern, vec![6.35, -3.175]);
    }
}
