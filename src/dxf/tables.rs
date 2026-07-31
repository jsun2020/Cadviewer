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
    /// Layer switched off. DXF marks this by negating group 62 rather than
    /// with a flag of its own.
    pub off: bool,
    /// Group 70 bit 1.
    pub frozen: bool,
    /// Group 290, absent meaning plottable.
    pub plottable: bool,
}

impl LayerRecord {
    /// Whether AutoCAD would put ink on paper for entities on this layer.
    pub fn plotted(&self) -> bool {
        !self.off && !self.frozen && self.plottable
    }
}

#[derive(Clone, Debug)]
pub struct LtypeRecord {
    pub name: String,
    /// Dash pattern in drawing units. Positive is ink, negative is gap.
    pub pattern: Vec<f64>,
}

/// One TEXTSTYLE record.
///
/// Font names are kept exactly as written. The reference drawing spells
/// them five different ways — `isocp.shx`, `SIMPLEX`, `txt`, `simhei.ttf`
/// and empty — and normalising here would lose the distinction between
/// "no font named" and "a font whose name is missing an extension".
/// Normalisation belongs to the file search (`text::search`).
#[derive(Clone, Debug)]
pub struct StyleRecord {
    pub name: String,
    /// Group 3.
    pub primary: String,
    /// Group 4. Empty for Latin-only styles.
    pub bigfont: String,
    /// Group 40. Non-zero overrides the entity's own height (group 40 on
    /// TEXT); measured values in the reference drawing include 2.5, 3.0,
    /// 3.5, 200.0 and 250.0.
    pub fixed_height: f64,
    /// Group 41. Measured: 0.667, 0.7, 0.707, 0.75, 0.8, 0.9, 1.0.
    pub width_factor: f64,
    /// Group 50, degrees.
    pub oblique: f64,
    /// Group 71: bit 2 backward, bit 4 upside down.
    pub generation: i32,
}

impl Default for StyleRecord {
    fn default() -> Self {
        Self {
            name: String::new(),
            primary: String::new(),
            bigfont: String::new(),
            fixed_height: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            generation: 0,
        }
    }
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

fn record_f64(rec: &[&Pair], code: i32) -> Option<f64> {
    rec.iter().find(|p| p.code == code).and_then(|p| p.value.as_f64())
}

pub fn read_layers(pairs: &[Pair], cp: Codepage) -> HashMap<String, LayerRecord> {
    let mut out = HashMap::new();
    for rec in table_records(pairs, "LAYER") {
        let Some(name) = record_string(&rec, 2, cp) else { continue };
        let aci = record_i32(&rec, 62).unwrap_or(7);
        let flags = record_i32(&rec, 70).unwrap_or(0);
        let record = LayerRecord {
            aci: aci as i16,
            true_color: record_i32(&rec, 420).filter(|v| *v >= 0).map(|v| v as u32),
            lineweight: record_i32(&rec, 370).unwrap_or(-3) as i16,
            linetype: record_string(&rec, 6, cp).unwrap_or_else(|| "CONTINUOUS".to_owned()),
            off: aci < 0,
            frozen: flags & 1 != 0,
            plottable: record_i32(&rec, 290).unwrap_or(1) != 0,
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
        // Keyed upper-case: linetype names are case-insensitive in AutoCAD,
        // and an entity naming `hidden` must find the `HIDDEN` record
        // rather than falling through to a solid line.
        out.insert(name.to_ascii_uppercase(), LtypeRecord { name, pattern });
    }
    out
}

/// Read the STYLE table, keyed upper-case.
///
/// Style names are case-insensitive in AutoCAD, and every ATTRIB in the
/// reference drawing omits group 7 entirely, so it must find `Standard`
/// however the table spells it.
pub fn read_styles(pairs: &[Pair], cp: Codepage) -> HashMap<String, StyleRecord> {
    let mut out = HashMap::new();
    for rec in table_records(pairs, "STYLE") {
        let Some(name) = record_string(&rec, 2, cp) else { continue };
        let width = record_f64(&rec, 41).unwrap_or(1.0);
        let record = StyleRecord {
            primary: record_string(&rec, 3, cp).unwrap_or_default(),
            bigfont: record_string(&rec, 4, cp).unwrap_or_default(),
            fixed_height: record_f64(&rec, 40).unwrap_or(0.0),
            // A zero or negative width factor would collapse every glyph
            // to a vertical line; AutoCAD treats it as 1.
            width_factor: if width > 0.0 { width } else { 1.0 },
            oblique: record_f64(&rec, 50).unwrap_or(0.0),
            generation: record_i32(&rec, 71).unwrap_or(0),
            name: name.clone(),
        };
        out.insert(name.to_ascii_uppercase(), record);
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

    /// The five shapes a font reference takes in the reference drawing:
    /// a full filename, an extension-less upper-case name, an empty
    /// bigfont, a `.ttf`, and no font at all.
    const STYLE_SRC: &[u8] = b"  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nSTYLE\n  0\nSTYLE\n  2\nStandard\n  3\nisocp.shx\n  4\nhztxt.shx\n 40\n0.0\n 41\n0.707\n 50\n0.0\n 71\n0\n  0\nSTYLE\n  2\nDIM_FONT\n  3\nSIMPLEX\n  4\nGBCBIG\n 40\n3.5\n 41\n0.7\n 50\n15.0\n 71\n2\n  0\nSTYLE\n  2\nTKHT\n  3\nsimhei.ttf\n 40\n0.0\n 41\n0.75\n  0\nSTYLE\n  2\nBARE\n 41\n1.0\n  0\nENDTAB\n  0\nENDSEC\n  0\nEOF\n";

    #[test]
    fn reads_style_font_references_in_every_shape_they_take() {
        let styles = read_styles(&lex(STYLE_SRC).unwrap(), Codepage::Gbk);
        assert_eq!(styles.len(), 4);
        let standard = &styles["STANDARD"];
        assert_eq!(standard.primary, "isocp.shx");
        assert_eq!(standard.bigfont, "hztxt.shx");
        assert!((standard.width_factor - 0.707).abs() < 1e-9);
        let dim = &styles["DIM_FONT"];
        assert_eq!(dim.primary, "SIMPLEX", "the name must not be normalised here");
        assert_eq!(dim.fixed_height, 3.5);
        assert_eq!(dim.oblique, 15.0);
        assert_eq!(dim.generation, 2);
        assert_eq!(styles["TKHT"].primary, "simhei.ttf");
        assert_eq!(styles["TKHT"].bigfont, "");
        assert_eq!(styles["BARE"].primary, "");
    }

    /// A style with no group 41 must not scale text to nothing. AutoCAD's
    /// default width factor is 1.
    #[test]
    fn a_missing_width_factor_defaults_to_one() {
        let src = b"  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nSTYLE\n  0\nSTYLE\n  2\nS\n  3\ntxt\n  0\nENDTAB\n  0\nENDSEC\n  0\nEOF\n";
        let styles = read_styles(&lex(src).unwrap(), Codepage::Gbk);
        assert_eq!(styles["S"].width_factor, 1.0);
        assert_eq!(styles["S"].fixed_height, 0.0);
    }

    /// Style names are case-insensitive in AutoCAD, and 504 ATTRIBs in the
    /// reference drawing carry no group 7 at all — they resolve through
    /// the name `Standard`, which must be found however it is spelled.
    #[test]
    fn styles_are_keyed_case_insensitively() {
        let styles = read_styles(&lex(STYLE_SRC).unwrap(), Codepage::Gbk);
        assert!(styles.contains_key("STANDARD"));
        assert!(!styles.contains_key("Standard"), "keys must be upper-cased");
    }
}
