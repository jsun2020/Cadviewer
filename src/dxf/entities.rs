use std::collections::HashMap;

use crate::dxf::lexer::{Pair, Value};
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

    /// True for an entity that belongs to a paper-space layout rather than
    /// to model space (group 67). Layouts are a separate feature; plotting
    /// their entities into the model scene draws a second, misplaced copy
    /// of whatever the layout shows.
    pub fn paper_space(&self) -> bool {
        self.int(67, 0) == 1
    }
}

/// Collect `0`-delimited records inside the named section.
///
/// One record shape is not `0`-delimited in the way the rest are: a *heavy*
/// `POLYLINE` carries no coordinates of its own. Its vertices follow it as
/// separate top-level `VERTEX` records terminated by `SEQEND`, so splitting
/// naively leaves a POLYLINE with nothing to draw and a run of orphan
/// VERTEX records. They are folded back into the POLYLINE here — the same
/// state machine `read_blocks` uses for `BLOCK`/`ENDBLK` — so every
/// consumer downstream sees one entity carrying its own geometry.
pub fn read_section(pairs: &[Pair], section: &str) -> Vec<RawEntity> {
    let mut out: Vec<RawEntity> = Vec::new();
    let Some(start) = find_section(pairs, section) else {
        return out;
    };
    // Index of the POLYLINE currently collecting VERTEX records, if any.
    let mut collecting: Option<usize> = None;
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
            i = j;

            if kind == "VERTEX"
                && let Some(index) = collecting
            {
                let vertex = RawEntity { kind, codes };
                append_vertex(&mut out[index], &vertex);
            } else if kind == "SEQEND" {
                // SEQEND also terminates ATTRIB sequences, which nothing is
                // collecting; either way it carries no geometry.
                collecting = None;
            } else if kind == "POLYLINE" {
                // The POLYLINE's own 10/20/30 is a "vertices follow"
                // placeholder at the origin, not a vertex. Keeping it would
                // prepend a spurious point at (0, 0).
                codes.retain(|(code, _)| !matches!(code, 10 | 20 | 30));
                out.push(RawEntity { kind, codes });
                collecting = Some(out.len() - 1);
            } else {
                collecting = None;
                out.push(RawEntity { kind, codes });
            }
            continue;
        }
        i += 1;
    }
    out
}

/// Fold one `VERTEX` record's coordinates into its owning `POLYLINE`.
///
/// A bulge is written for every vertex, including the zero ones the file
/// omits, so the bulge list stays index-aligned with the vertex list.
fn append_vertex(polyline: &mut RawEntity, vertex: &RawEntity) {
    const FACE_RECORD: i32 = 128;
    const MESH_VERTEX: i32 = 64;
    let flags = vertex.int(70, 0);
    if flags & FACE_RECORD != 0 && flags & MESH_VERTEX == 0 {
        // A polyface mesh's face records index other vertices; they carry
        // no position of their own.
        return;
    }
    let has_position = vertex.codes.iter().any(|(code, _)| *code == 10);
    if !has_position {
        return;
    }
    polyline.codes.push((10, Value::F64(vertex.f64(10, 0.0))));
    polyline.codes.push((20, Value::F64(vertex.f64(20, 0.0))));
    polyline.codes.push((42, Value::F64(vertex.f64(42, 0.0))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::lexer::lex;

    /// I10: before the fold, a heavy POLYLINE carried no coordinates at
    /// all — its vertices arrived as separate top-level records, so it
    /// produced zero drawn items and three "skipped" entries. The vertices
    /// must end up on the POLYLINE, and the placeholder point the POLYLINE
    /// record itself carries must not become a vertex at the origin.
    #[test]
    fn heavy_polyline_vertices_are_folded_into_the_polyline() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nPOLYLINE\n  8\nL\n 66\n1\n 70\n1\n 10\n0.0\n 20\n0.0\n 30\n0.0\n  0\nVERTEX\n  8\nL\n 10\n10.0\n 20\n20.0\n  0\nVERTEX\n  8\nL\n 10\n30.0\n 20\n40.0\n 42\n0.5\n  0\nSEQEND\n  8\nL\n  0\nENDSEC\n  0\nEOF\n";
        let entities = read_section(&lex(src).unwrap(), "ENTITIES");
        assert_eq!(entities.len(), 1, "got {:?}", entities.iter().map(|e| &e.kind).collect::<Vec<_>>());
        let poly = &entities[0];
        assert_eq!(poly.kind, "POLYLINE");
        assert_eq!(poly.points(10, 20), vec![Point::new(10.0, 20.0), Point::new(30.0, 40.0)]);
        assert_eq!(poly.all_f64(42), vec![0.0, 0.5], "every vertex needs a bulge slot");
    }

    /// A SEQEND that closes an ATTRIB sequence must not be mistaken for the
    /// end of a polyline, and ATTRIBs must not be swallowed as vertices.
    #[test]
    fn attrib_sequences_are_left_alone() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  2\nB\n 10\n0.0\n 20\n0.0\n  0\nATTRIB\n  1\nX\n  0\nSEQEND\n  0\nLINE\n 10\n1.0\n 20\n1.0\n 11\n2.0\n 21\n2.0\n  0\nENDSEC\n  0\nEOF\n";
        let kinds: Vec<String> = read_section(&lex(src).unwrap(), "ENTITIES")
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, vec!["INSERT", "ATTRIB", "LINE"]);
    }

    /// I5: the BLOCK record's group 10/20 is the point of the block's own
    /// coordinate system that lands on the insertion point. Dropping it
    /// offsets every entity of the block.
    #[test]
    fn block_base_points_are_read() {
        let src = b"  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nOFFSET\n 10\n100.0\n 20\n50.0\n  0\nLINE\n 10\n100.0\n 20\n50.0\n 11\n110.0\n 21\n50.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nEOF\n";
        let blocks = read_blocks(&lex(src).unwrap(), Codepage::Latin1);
        let record = &blocks["OFFSET"];
        assert_eq!(record.base, Point::new(100.0, 50.0));
        assert_eq!(record.entities.len(), 1);
    }
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

/// One block definition: its body plus the base point that body is drawn
/// around.
///
/// The base point (group 10/20 on the `BLOCK` record) is the point of the
/// block's own coordinate system that lands on the INSERT's insertion
/// point. Ignoring it displaces every entity of a block that was not
/// authored at its own origin by exactly that vector.
#[derive(Clone, Debug, Default)]
pub struct BlockRecord {
    pub base: Point,
    pub entities: Vec<RawEntity>,
}

/// Group BLOCKS-section records by owning block name.
pub fn read_blocks(pairs: &[Pair], cp: Codepage) -> HashMap<String, BlockRecord> {
    let mut out: HashMap<String, BlockRecord> = HashMap::new();
    let mut current: Option<String> = None;
    for ent in read_section(pairs, "BLOCKS") {
        match ent.kind.as_str() {
            "BLOCK" => {
                let name = ent.text(2, cp).unwrap_or_default();
                let record = out.entry(name.clone()).or_default();
                record.base = Point::new(ent.f64(10, 0.0), ent.f64(20, 0.0));
                current = Some(name);
            }
            "ENDBLK" => current = None,
            _ => {
                if let Some(name) = &current {
                    out.entry(name.clone()).or_default().entities.push(ent);
                }
            }
        }
    }
    out
}
