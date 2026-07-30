use crate::doc::Document;
use crate::geom::{Affine, Bounds, Point};
use crate::plot::flatten::flatten;

/// Aspect ratios that indicate a real sheet: ISO A-series plus the GB
/// extended formats used in Chinese practice.
const SHEET_RATIOS: [f64; 4] = [1.4142, 1.5, 2.0, 3.0];

/// Minimum score for a purely geometric candidate to be accepted. This is
/// the only tuning knob in the detector; keep it here, not scattered
/// through the scoring code.
pub const GEOMETRY_THRESHOLD: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signal {
    Geometry,
    LayerName,
    BlockName,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub bounds: Bounds,
    pub signal: Signal,
    pub source: String,
}

fn is_frame_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    name.contains('\u{56fe}') && (name.contains('\u{6846}') || name.contains('\u{5e45}'))
        || upper.contains("TITLEBLOCK")
        || upper.contains("TITLE_BLOCK")
        || upper.contains("FRAME")
        || upper == "TK"
}

/// Untransformed extents of a block's body.
pub fn block_extents(doc: &Document, name: &str) -> Option<Bounds> {
    let body = doc.blocks.get(name)?;
    let mut b = Bounds::empty();
    for ent in body {
        if let Some(f) = flatten(ent, Affine::identity()) {
            let fb = f.geom.bounds();
            if fb.valid() {
                b.add(Point::new(fb.min_x, fb.min_y));
                b.add(Point::new(fb.max_x, fb.max_y));
            }
        }
    }
    b.valid().then_some(b)
}

/// Score a rectangle on how much it looks like a standard sheet.
fn geometry_score(bounds: Bounds) -> f64 {
    if !bounds.valid() || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return 0.0;
    }
    let w = bounds.width();
    let h = bounds.height();
    let ratio = if w >= h { w / h } else { h / w };
    SHEET_RATIOS
        .iter()
        .map(|target| {
            let error = (ratio - target).abs() / target;
            (1.0 - error * 4.0).max(0.0)
        })
        .fold(0.0, f64::max)
}

pub fn collect_candidates(doc: &Document) -> Vec<Candidate> {
    let cp = doc.header.codepage;
    let mut out: Vec<Candidate> = Vec::new();

    // L1: INSERTs of a block whose name says "title block".
    for ent in &doc.entities {
        if ent.kind != "INSERT" {
            continue;
        }
        let Some(name) = ent.text(2, cp) else { continue };
        if !is_frame_name(&name) {
            continue;
        }
        let Some(extents) = block_extents(doc, &name) else { continue };
        let t = Affine::scale(ent.f64(41, 1.0), ent.f64(42, 1.0))
            .then(Affine::rotation(ent.f64(50, 0.0)))
            .then(Affine::translation(ent.f64(10, 0.0), ent.f64(20, 0.0)));
        let mut b = Bounds::empty();
        for corner in [
            Point::new(extents.min_x, extents.min_y),
            Point::new(extents.max_x, extents.min_y),
            Point::new(extents.max_x, extents.max_y),
            Point::new(extents.min_x, extents.max_y),
        ] {
            b.add(t.apply(corner));
        }
        out.push(Candidate { bounds: b, signal: Signal::BlockName, source: name });
    }
    if !out.is_empty() {
        return out;
    }

    // L2: closed rectangles sitting on a layer whose name says "title block".
    for ent in &doc.entities {
        if !matches!(ent.kind.as_str(), "LWPOLYLINE" | "POLYLINE") {
            continue;
        }
        let layer = ent.layer(cp);
        if !is_frame_name(&layer) {
            continue;
        }
        if let Some(b) = closed_rect_bounds(ent) {
            out.push(Candidate { bounds: b, signal: Signal::LayerName, source: layer });
        }
    }
    if !out.is_empty() {
        return out;
    }

    // L3: geometric fallback.
    for ent in &doc.entities {
        if !matches!(ent.kind.as_str(), "LWPOLYLINE" | "POLYLINE") {
            continue;
        }
        let Some(b) = closed_rect_bounds(ent) else { continue };
        if geometry_score(b) >= GEOMETRY_THRESHOLD {
            out.push(Candidate {
                bounds: b,
                signal: Signal::Geometry,
                source: ent.layer(cp),
            });
        }
    }
    out
}

fn closed_rect_bounds(ent: &crate::dxf::entities::RawEntity) -> Option<Bounds> {
    if ent.int(70, 0) & 1 == 0 {
        return None;
    }
    let pts = ent.points(10, 20);
    if pts.len() != 4 {
        return None;
    }
    let mut b = Bounds::empty();
    for p in &pts {
        b.add(*p);
    }
    b.valid().then_some(b)
}

/// Two nested rectangles this close in size are the inner and outer lines
/// of one drawn border, not a container and its contents.
const COINCIDENT_TOLERANCE: f64 = 0.02;

#[derive(Clone, Debug)]
pub struct Sheet {
    /// 1-based page number in reading order.
    pub index: usize,
    pub bounds: Bounds,
    pub source: String,
}

fn nearly_coincident(outer: Bounds, inner: Bounds) -> bool {
    let ow = outer.width().max(f64::EPSILON);
    let oh = outer.height().max(f64::EPSILON);
    (outer.width() - inner.width()).abs() / ow < COINCIDENT_TOLERANCE
        && (outer.height() - inner.height()).abs() / oh < COINCIDENT_TOLERANCE
}

/// Drop annotation and grouping rectangles.
///
/// A candidate that strictly encloses one or more *distinct* (non-coincident)
/// candidates is not a real sheet: it is either a group box wrapping several
/// frames, or a spurious annotation box wrapping one. Keeping the outermost
/// unconditionally — the intuitive rule — would silently collapse a
/// 26-sheet drawing into 3 pages (PRD 3.10.2). The one exception is a double
/// line drawn around the same frame: there the nested rectangles nearly
/// coincide (within `COINCIDENT_TOLERANCE`), and the outer line is kept as
/// the sheet while the inner duplicate is suppressed.
pub fn reject_containers(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut keep = vec![true; candidates.len()];

    for (i, outer) in candidates.iter().enumerate() {
        let mut enclosed = 0usize;
        for (j, inner) in candidates.iter().enumerate() {
            if i == j || !outer.bounds.contains(inner.bounds) {
                continue;
            }
            if nearly_coincident(outer.bounds, inner.bounds) {
                // Same border drawn twice: suppress the inner copy.
                keep[j] = false;
                continue;
            }
            enclosed += 1;
        }
        if enclosed >= 1 {
            keep[i] = false;
        }
    }

    candidates
        .into_iter()
        .zip(keep)
        .filter_map(|(c, k)| k.then_some(c))
        .collect()
}

/// Sort into reading order: rows top to bottom, each row left to right.
pub fn order_sheets(candidates: Vec<Candidate>) -> Vec<Sheet> {
    let mut remaining = candidates;
    remaining.sort_by(|a, b| {
        b.bounds
            .min_y
            .partial_cmp(&a.bounds.min_y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<Vec<Candidate>> = Vec::new();
    for c in remaining {
        // Same row when the vertical offset is under half the frame height,
        // which absorbs the jitter real drawings always have.
        let placed = rows.iter_mut().find(|row| {
            let reference = &row[0];
            let tolerance = reference.bounds.height().max(c.bounds.height()) / 2.0;
            (reference.bounds.min_y - c.bounds.min_y).abs() <= tolerance
        });
        match placed {
            Some(row) => row.push(c),
            None => rows.push(vec![c]),
        }
    }

    let mut out = Vec::new();
    for row in &mut rows {
        row.sort_by(|a, b| {
            a.bounds
                .min_x
                .partial_cmp(&b.bounds.min_x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for c in row.iter() {
            out.push(Sheet {
                index: out.len() + 1,
                bounds: c.bounds,
                source: c.source.clone(),
            });
        }
    }
    out
}

/// Full detection pipeline: collect, reject containers, order.
pub fn detect(doc: &Document) -> Vec<Sheet> {
    order_sheets(reject_containers(collect_candidates(doc)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    /// Two frame inserts of a block named with the CJK for "title block",
    /// plus a large rectangle on an unrelated layer.
    fn src() -> Vec<u8> {
        let frame = "\u{56fe}\u{6846}";
        format!(
            "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nL\n 62\n7\n370\n-3\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n\
  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\n{frame}\n\
  0\nLWPOLYLINE\n  8\nL\n 70\n1\n 10\n0.0\n 20\n0.0\n 10\n420.0\n 20\n0.0\n 10\n420.0\n 20\n297.0\n 10\n0.0\n 20\n297.0\n\
  0\nENDBLK\n  0\nENDSEC\n\
  0\nSECTION\n  2\nENTITIES\n\
  0\nINSERT\n  8\nL\n  2\n{frame}\n 10\n0.0\n 20\n0.0\n\
  0\nINSERT\n  8\nL\n  2\n{frame}\n 10\n500.0\n 20\n0.0\n\
  0\nENDSEC\n  0\nEOF\n"
        )
        .into_bytes()
    }

    #[test]
    fn finds_frames_by_block_name() {
        let doc = Document::parse(&src()).unwrap();
        let c = collect_candidates(&doc);
        assert_eq!(c.len(), 2, "got {c:?}");
        assert!(c.iter().all(|x| x.signal == Signal::BlockName));
    }

    #[test]
    fn candidate_bounds_follow_the_insert_point() {
        let doc = Document::parse(&src()).unwrap();
        let mut c = collect_candidates(&doc);
        c.sort_by(|a, b| a.bounds.min_x.partial_cmp(&b.bounds.min_x).unwrap());
        assert!((c[0].bounds.min_x - 0.0).abs() < 0.01);
        assert!((c[1].bounds.min_x - 500.0).abs() < 0.01, "got {}", c[1].bounds.min_x);
        assert!((c[0].bounds.width() - 420.0).abs() < 0.01);
    }

    #[test]
    fn block_extents_measures_the_block_body() {
        let doc = Document::parse(&src()).unwrap();
        let b = block_extents(&doc, "\u{56fe}\u{6846}").expect("block should exist");
        assert!((b.width() - 420.0).abs() < 0.01);
        assert!((b.height() - 297.0).abs() < 0.01);
    }

    #[test]
    fn strong_signals_suppress_weak_ones() {
        // A geometric candidate must not survive alongside block-name hits,
        // or annotation rectangles reappear as extra pages.
        let doc = Document::parse(&src()).unwrap();
        let c = collect_candidates(&doc);
        assert!(!c.iter().any(|x| x.signal == Signal::Geometry));
    }

    #[test]
    fn falls_back_to_geometry_when_no_block_or_layer_matches() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nLWPOLYLINE\n  8\nX\n 70\n1\n 10\n0.0\n 20\n0.0\n 10\n420.0\n 20\n0.0\n 10\n420.0\n 20\n297.0\n 10\n0.0\n 20\n297.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let c = collect_candidates(&doc);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].signal, Signal::Geometry);
    }

    fn candidate(min_x: f64, min_y: f64, w: f64, h: f64) -> Candidate {
        let mut b = Bounds::empty();
        b.add(Point::new(min_x, min_y));
        b.add(Point::new(min_x + w, min_y + h));
        Candidate { bounds: b, signal: Signal::BlockName, source: "F".to_owned() }
    }

    #[test]
    fn a_group_box_containing_many_frames_is_discarded() {
        // PRD 3.10.2: the failure that would collapse 26 pages into 3.
        let frames: Vec<Candidate> = (0..6)
            .map(|i| candidate(10.0 + i as f64 * 100.0, 10.0, 80.0, 60.0))
            .collect();
        let group = candidate(0.0, 0.0, 620.0, 80.0);
        let mut all = frames.clone();
        all.push(group);

        let kept = reject_containers(all);
        assert_eq!(kept.len(), 6, "the group box must be dropped, got {kept:?}");
        assert!(kept.iter().all(|c| c.bounds.width() < 100.0));
    }

    #[test]
    fn a_double_line_border_keeps_the_outer_rectangle() {
        let outer = candidate(0.0, 0.0, 420.0, 297.0);
        let inner = candidate(2.0, 2.0, 416.0, 293.0);
        let kept = reject_containers(vec![outer, inner]);
        assert_eq!(kept.len(), 1);
        assert!((kept[0].bounds.width() - 420.0).abs() < 0.01, "should keep the outer");
    }

    #[test]
    fn a_container_holding_only_one_frame_is_still_a_border() {
        let outer = candidate(0.0, 0.0, 500.0, 400.0);
        let inner = candidate(10.0, 10.0, 420.0, 297.0);
        let kept = reject_containers(vec![outer, inner]);
        assert_eq!(kept.len(), 1, "one nested frame is a border, not a group");
    }

    #[test]
    fn sheets_are_ordered_top_to_bottom_then_left_to_right() {
        // Two rows of two, deliberately supplied out of order.
        let all = vec![
            candidate(200.0, 0.0, 100.0, 80.0),
            candidate(0.0, 200.0, 100.0, 80.0),
            candidate(200.0, 200.0, 100.0, 80.0),
            candidate(0.0, 0.0, 100.0, 80.0),
        ];
        let sheets = order_sheets(all);
        assert_eq!(sheets.len(), 4);
        assert!((sheets[0].bounds.min_x - 0.0).abs() < 0.01, "first should be top-left");
        assert!((sheets[0].bounds.min_y - 200.0).abs() < 0.01);
        assert!((sheets[1].bounds.min_x - 200.0).abs() < 0.01, "second should be top-right");
        assert!((sheets[3].bounds.min_x - 200.0).abs() < 0.01, "last should be bottom-right");
    }

    #[test]
    fn rows_tolerate_small_vertical_jitter() {
        // Frames on the same row are rarely aligned to the micron.
        let all = vec![
            candidate(0.0, 0.0, 100.0, 80.0),
            candidate(200.0, 3.0, 100.0, 80.0),
        ];
        let sheets = order_sheets(all);
        assert!((sheets[0].bounds.min_x - 0.0).abs() < 0.01, "should read left to right");
        assert!((sheets[1].bounds.min_x - 200.0).abs() < 0.01);
    }

    #[test]
    fn sheets_are_indexed_from_one() {
        let sheets = order_sheets(vec![candidate(0.0, 0.0, 100.0, 80.0)]);
        assert_eq!(sheets[0].index, 1);
    }
}
