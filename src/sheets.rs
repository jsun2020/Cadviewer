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
}
