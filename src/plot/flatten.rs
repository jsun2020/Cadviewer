use std::f64::consts::TAU;

use crate::dxf::entities::RawEntity;
use crate::geom::{Affine, PathGeom, Point, SubPath};

/// Points used to approximate a full circle. Chosen so that the chord
/// error stays under about 0.1% of the radius, which is below plotter
/// resolution at any realistic sheet scale.
const CIRCLE_SEGMENTS: usize = 72;

pub struct FlatGeom {
    pub geom: PathGeom,
    /// True for area entities (SOLID, TRACE, 3DFACE) that AutoCAD fills
    /// rather than strokes.
    pub filled: bool,
}

/// Angles closer together than this are treated as a full turn rather than
/// a degenerate arc: `360deg.to_radians()` need not land exactly on `TAU`,
/// so a full circle can normalise to either side of zero.
const SWEEP_EPSILON: f64 = 1e-9;

/// Normalise a start/end angle pair into a strictly positive CCW sweep.
///
/// DXF arcs always run counter-clockwise, so an end angle numerically below
/// the start angle means the arc crosses zero. This is done arithmetically
/// rather than by adding turns in a loop: this code parses untrusted files,
/// and an absurd (but finite) angle such as `1e300` needs ~1.6e298
/// iterations to come into range, which is a hang with no error message.
/// Non-finite input never terminates at all — the lexer clamps those, and
/// the callers here reject them again so the property holds for any caller.
fn normalised_sweep(start: f64, end: f64) -> f64 {
    let sweep = (end - start).rem_euclid(TAU);
    if sweep <= SWEEP_EPSILON { TAU } else { sweep }
}

/// Sample a circular arc counter-clockwise from `start_deg` to `end_deg`.
///
/// Returns no points at all when any input is non-finite, so a corrupt file
/// drops one entity instead of hanging the whole export.
pub fn arc_points(
    center: Point,
    radius: f64,
    start_deg: f64,
    end_deg: f64,
    segments: usize,
) -> Vec<Point> {
    if !center.x.is_finite()
        || !center.y.is_finite()
        || !radius.is_finite()
        || !start_deg.is_finite()
        || !end_deg.is_finite()
    {
        return Vec::new();
    }
    let start = start_deg.to_radians();
    let sweep = normalised_sweep(start, end_deg.to_radians());
    let steps = segments.max(2);
    (0..=steps)
        .map(|i| {
            let a = start + sweep * (i as f64) / (steps as f64);
            Point::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

/// Expand a polyline bulge into sampled arc points.
///
/// `bulge` is tan(theta/4) where theta is the included angle; 1.0 is a
/// semicircle and 0.0 is a straight segment. Per the DXF convention, the
/// arc's sagitta (the offset of the arc's midpoint from the chord) is
/// `bulge * chord/2` along the perpendicular obtained by rotating the
/// a->b direction 90 degrees counter-clockwise, i.e. `(-dy, dx)/chord`.
/// The circle's center then sits on the *opposite* side of the chord from
/// that sagitta point, at signed distance `radius * cos(theta/2)` (the
/// apothem) along the same perpendicular -- and the arc is walked from
/// `a` by *subtracting* `theta` (not adding it), which is what actually
/// carries the sampled points through the sagitta point on the way to
/// `b`. Verified by hand against a quarter-circle (bulge = tan(22.5 deg))
/// and the semicircle test below before coding this.
pub fn bulge_arc(a: Point, b: Point, bulge: f64) -> Vec<Point> {
    if !bulge.is_finite() || bulge.abs() < 1e-12 {
        return vec![a, b];
    }
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let chord = (dx * dx + dy * dy).sqrt();
    if chord < 1e-12 {
        return vec![a, b];
    }
    let theta = 4.0 * bulge.atan();
    let radius = (chord / 2.0) / (theta / 2.0).sin();
    let apothem = radius * (theta / 2.0).cos();
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let nx = -dy / chord;
    let ny = dx / chord;
    let center = Point::new(mid.x - nx * apothem, mid.y - ny * apothem);

    let start = (a.y - center.y).atan2(a.x - center.x);
    let sweep = -theta;
    let radius_mag = radius.abs();
    let steps = ((theta.abs() / TAU) * CIRCLE_SEGMENTS as f64).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let ang = start + sweep * (i as f64) / (steps as f64);
            Point::new(center.x + radius_mag * ang.cos(), center.y + radius_mag * ang.sin())
        })
        .collect()
}

fn transformed(points: Vec<Point>, t: Affine, closed: bool) -> FlatGeom {
    FlatGeom {
        geom: PathGeom {
            subpaths: vec![SubPath {
                points: points.into_iter().map(|p| t.apply(p)).collect(),
                closed,
            }],
        },
        filled: false,
    }
}

/// Flatten one entity's geometry into transformed polylines.
///
/// Returns `None` for entity kinds this stage does not draw, so the caller
/// can count and report them rather than silently dropping them.
pub fn flatten(entity: &RawEntity, t: Affine) -> Option<FlatGeom> {
    match entity.kind.as_str() {
        "LINE" => {
            let a = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            let b = Point::new(entity.f64(11, 0.0), entity.f64(21, 0.0));
            Some(transformed(vec![a, b], t, false))
        }
        "CIRCLE" => {
            let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            let r = entity.f64(40, 0.0);
            if r <= 0.0 {
                return None;
            }
            let pts = arc_points(c, r, 0.0, 360.0, CIRCLE_SEGMENTS);
            if pts.len() < 2 {
                return None;
            }
            Some(transformed(pts, t, true))
        }
        "ARC" => {
            let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            let r = entity.f64(40, 0.0);
            if r <= 0.0 {
                return None;
            }
            let start = entity.f64(50, 0.0);
            let end = entity.f64(51, 360.0);
            let pts = arc_points(c, r, start, end, CIRCLE_SEGMENTS);
            if pts.len() < 2 {
                return None;
            }
            Some(transformed(pts, t, false))
        }
        "ELLIPSE" => {
            let (pts, closed) = ellipse_points(entity)?;
            Some(transformed(pts, t, closed))
        }
        "LEADER" => {
            // A leader is a polyline through its vertices. The arrowhead is
            // a block reference the entity does not name, so it is omitted;
            // the leader line itself is the ink that matters.
            let pts = entity.points(10, 20);
            if pts.len() < 2 {
                return None;
            }
            Some(transformed(pts, t, false))
        }
        "LWPOLYLINE" | "POLYLINE" => {
            let closed = entity.int(70, 0) & 1 != 0;
            let pts = polyline_points(entity);
            if pts.len() < 2 {
                return None;
            }
            Some(transformed(pts, t, closed))
        }
        "POINT" => {
            let p = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            Some(transformed(vec![p, p], t, false))
        }
        "SOLID" | "TRACE" | "3DFACE" => {
            // Vertex order in these entities is 1,2,4,3 -- not 1,2,3,4.
            let pts = vec![
                Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0)),
                Point::new(entity.f64(11, 0.0), entity.f64(21, 0.0)),
                Point::new(entity.f64(13, 0.0), entity.f64(23, 0.0)),
                Point::new(entity.f64(12, 0.0), entity.f64(22, 0.0)),
            ];
            let mut f = transformed(pts, t, true);
            f.filled = entity.kind != "3DFACE";
            Some(f)
        }
        "SPLINE" => {
            // Approximated by its control polygon; a proper NURBS
            // evaluation is deliberately deferred (see task scope notes).
            let pts = entity.points(10, 20);
            if pts.len() < 2 {
                return None;
            }
            let closed = entity.int(70, 0) & 1 != 0;
            Some(transformed(pts, t, closed))
        }
        _ => None,
    }
}

/// Sample an ellipse, returning its points and whether it closes.
///
/// `None` for a degenerate ellipse (no major axis) or non-finite input,
/// matching CIRCLE and ARC, which reject a non-positive radius rather than
/// emitting collapsed geometry.
fn ellipse_points(entity: &RawEntity) -> Option<(Vec<Point>, bool)> {
    let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
    let major = Point::new(entity.f64(11, 0.0), entity.f64(21, 0.0));
    let ratio = entity.f64(40, 1.0);
    let start = entity.f64(41, 0.0);
    let end = entity.f64(42, TAU);
    if ![c.x, c.y, major.x, major.y, ratio, start, end]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    let a = (major.x * major.x + major.y * major.y).sqrt();
    if a <= 0.0 {
        return None;
    }
    let b = a * ratio;
    let rot = major.y.atan2(major.x);
    let sweep = normalised_sweep(start, end);
    let closed = (sweep - TAU).abs() <= SWEEP_EPSILON;
    let pts = (0..=CIRCLE_SEGMENTS)
        .map(|i| {
            let param = start + sweep * (i as f64) / (CIRCLE_SEGMENTS as f64);
            let (x, y) = (a * param.cos(), b * param.sin());
            Point::new(
                c.x + x * rot.cos() - y * rot.sin(),
                c.y + x * rot.sin() + y * rot.cos(),
            )
        })
        .collect();
    Some((pts, closed))
}

/// Read polyline vertices and their bulges in one ordered pass.
///
/// Two independent `all_f64` filters cannot be used here: DXF writers emit
/// group 42 only for the vertices that actually carry a bulge, so indexing
/// a filtered bulge list by segment number puts the arc on the wrong
/// segment. Walking the codes in file order pairs each 42 with the vertex
/// it follows, which is what the format actually means.
fn polyline_vertices(entity: &RawEntity) -> (Vec<Point>, Vec<f64>) {
    let mut points: Vec<Point> = Vec::new();
    let mut bulges: Vec<f64> = Vec::new();
    for (code, value) in &entity.codes {
        match code {
            10 => {
                points.push(Point::new(value.as_f64().unwrap_or(0.0), 0.0));
                bulges.push(0.0);
            }
            20 => {
                if let Some(p) = points.last_mut() {
                    p.y = value.as_f64().unwrap_or(0.0);
                }
            }
            42 => {
                if let Some(b) = bulges.last_mut() {
                    *b = value.as_f64().unwrap_or(0.0);
                }
            }
            _ => {}
        }
    }
    (points, bulges)
}

fn polyline_points(entity: &RawEntity) -> Vec<Point> {
    let (verts, bulges) = polyline_vertices(entity);
    if verts.len() < 2 {
        return verts;
    }
    let closed = entity.int(70, 0) & 1 != 0;
    let mut out = Vec::with_capacity(verts.len());
    let last = if closed { verts.len() } else { verts.len() - 1 };
    for i in 0..last {
        let a = verts[i];
        let b = verts[(i + 1) % verts.len()];
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        let seg = bulge_arc(a, b, bulge);
        if out.is_empty() {
            out.extend(seg);
        } else {
            out.extend(seg.into_iter().skip(1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::lexer::Value;

    fn ent(kind: &str, codes: &[(i32, f64)]) -> RawEntity {
        RawEntity {
            kind: kind.to_owned(),
            codes: codes.iter().map(|(c, v)| (*c, Value::F64(*v))).collect(),
        }
    }

    #[test]
    fn line_becomes_a_two_point_open_subpath() {
        let e = ent("LINE", &[(10, 0.0), (20, 0.0), (11, 10.0), (21, 5.0)]);
        let f = flatten(&e, Affine::identity()).expect("LINE should flatten");
        assert_eq!(f.geom.subpaths.len(), 1);
        assert_eq!(f.geom.subpaths[0].points.len(), 2);
        assert!(!f.geom.subpaths[0].closed);
        assert_eq!(f.geom.subpaths[0].points[1], Point::new(10.0, 5.0));
        assert!(!f.filled);
    }

    #[test]
    fn circle_is_closed_and_spans_the_diameter() {
        let e = ent("CIRCLE", &[(10, 0.0), (20, 0.0), (40, 5.0)]);
        let f = flatten(&e, Affine::identity()).unwrap();
        assert!(f.geom.subpaths[0].closed);
        let b = f.geom.bounds();
        assert!((b.width() - 10.0).abs() < 0.1, "width {}", b.width());
        assert!((b.height() - 10.0).abs() < 0.1, "height {}", b.height());
    }

    #[test]
    fn arc_respects_start_and_end_angles() {
        // Quarter arc from 0 to 90 degrees, radius 10, centred at origin.
        let e = ent("ARC", &[(10, 0.0), (20, 0.0), (40, 10.0), (50, 0.0), (51, 90.0)]);
        let f = flatten(&e, Affine::identity()).unwrap();
        let pts = &f.geom.subpaths[0].points;
        assert!(!f.geom.subpaths[0].closed);
        assert!((pts[0].x - 10.0).abs() < 1e-6 && pts[0].y.abs() < 1e-6, "start {:?}", pts[0]);
        let last = pts.last().unwrap();
        assert!(last.x.abs() < 1e-6 && (last.y - 10.0).abs() < 1e-6, "end {last:?}");
    }

    #[test]
    fn arc_crossing_zero_degrees_goes_counterclockwise() {
        // 350 to 10 degrees is a 20-degree arc, not a 340-degree one.
        let e = ent("ARC", &[(10, 0.0), (20, 0.0), (40, 10.0), (50, 350.0), (51, 10.0)]);
        let f = flatten(&e, Affine::identity()).unwrap();
        let b = f.geom.bounds();
        assert!(b.height() < 4.0, "arc swept the long way; height {}", b.height());
    }

    #[test]
    fn semicircular_bulge_produces_a_curve_not_a_chord() {
        let pts = bulge_arc(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 1.0);
        assert!(pts.len() > 4, "expected a sampled arc, got {} points", pts.len());
        let peak = pts.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((peak - 5.0).abs() < 0.2, "semicircle should bulge to 5.0, got {peak}");
    }

    #[test]
    fn zero_bulge_is_a_straight_segment() {
        let pts = bulge_arc(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 0.0);
        assert_eq!(pts.len(), 2);
    }

    #[test]
    fn solid_is_marked_filled() {
        let e = ent(
            "SOLID",
            &[(10, 0.0), (20, 0.0), (11, 1.0), (21, 0.0), (12, 1.0), (22, 1.0), (13, 0.0), (23, 1.0)],
        );
        let f = flatten(&e, Affine::identity()).unwrap();
        assert!(f.filled, "SOLID must fill, not stroke");
    }

    #[test]
    fn the_transform_is_applied() {
        let e = ent("LINE", &[(10, 1.0), (20, 1.0), (11, 2.0), (21, 2.0)]);
        let t = Affine::scale(10.0, 10.0).then(Affine::translation(5.0, 5.0));
        let f = flatten(&e, t).unwrap();
        assert_eq!(f.geom.subpaths[0].points[0], Point::new(15.0, 15.0));
    }

    /// C1: a corrupt or hostile file can carry an angle the old
    /// accumulate-a-turn-at-a-time normalisation could never bring into
    /// range. `1e400` parses to `inf` (not an error), and `1e300` needs
    /// ~1.6e298 iterations — both are an unkillable hang in a converter
    /// that runs before anything is drawn. These must return promptly;
    /// the test hanging *is* the failure.
    #[test]
    fn absurd_arc_angles_terminate_instead_of_looping_forever() {
        for (start, end) in [
            (f64::INFINITY, 0.0),
            (0.0, f64::INFINITY),
            (f64::NAN, 90.0),
            (0.0, f64::NEG_INFINITY),
        ] {
            let pts = arc_points(Point::new(0.0, 0.0), 10.0, start, end, CIRCLE_SEGMENTS);
            assert!(pts.is_empty(), "non-finite angles must draw nothing at all");
        }
        // A finite but absurd angle still produces a real arc on the real
        // circle rather than spinning: 1e300 normalises arithmetically.
        let pts = arc_points(Point::new(0.0, 0.0), 10.0, 1e300, 0.0, CIRCLE_SEGMENTS);
        assert!(pts.len() > 2, "expected a sampled arc, got {}", pts.len());
        assert!(
            pts.iter()
                .all(|p| ((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() < 1e-6),
            "points left the circle"
        );
    }

    /// The same guard reached through the real entity path, spelled the way
    /// a hostile DXF would spell it.
    #[test]
    fn an_arc_with_an_overflowing_angle_is_dropped_not_hung() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nARC\n  8\n0\n 10\n0.0\n 20\n0.0\n 40\n10.0\n 50\n1e400\n 51\n90.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = crate::doc::Document::parse(src).expect("parse");
        let arc = &doc.entities[0];
        let f = flatten(arc, Affine::identity()).expect("clamped angle still draws");
        assert!(f.geom.bounds().valid());
    }

    #[test]
    fn a_full_ellipse_is_closed_and_a_degenerate_one_is_rejected() {
        let full = ent("ELLIPSE", &[(10, 0.0), (20, 0.0), (11, 10.0), (21, 0.0), (40, 0.5)]);
        let f = flatten(&full, Affine::identity()).expect("ELLIPSE should flatten");
        assert!(f.geom.subpaths[0].closed, "a full ellipse must close");

        let degenerate = ent("ELLIPSE", &[(10, 0.0), (20, 0.0), (11, 0.0), (21, 0.0)]);
        assert!(
            flatten(&degenerate, Affine::identity()).is_none(),
            "a zero-length major axis is not drawable geometry"
        );
    }

    #[test]
    fn an_ellipse_with_absurd_parameters_terminates() {
        let e = ent(
            "ELLIPSE",
            &[(10, 0.0), (20, 0.0), (11, 10.0), (21, 0.0), (40, 0.5), (41, 1e300), (42, 0.0)],
        );
        let f = flatten(&e, Affine::identity()).expect("should still produce points");
        assert!(f.geom.subpaths[0].points.iter().all(|p| p.x.is_finite()));
    }

    /// Minor 1: DXF writers emit group 42 only for vertices that carry a
    /// bulge, so pairing two independent filtered lists puts the arc on the
    /// wrong segment. Here only the last of three segments bulges.
    #[test]
    fn a_bulge_lands_on_the_vertex_that_declares_it() {
        use crate::dxf::lexer::Value;
        let e = RawEntity {
            kind: "LWPOLYLINE".to_owned(),
            codes: vec![
                (70, Value::I32(0)),
                (10, Value::F64(0.0)),
                (20, Value::F64(0.0)),
                (10, Value::F64(10.0)),
                (20, Value::F64(0.0)),
                (10, Value::F64(20.0)),
                (20, Value::F64(0.0)),
                // The bulge belongs to the vertex the segment starts at.
                (42, Value::F64(1.0)),
                (10, Value::F64(30.0)),
                (20, Value::F64(0.0)),
            ],
        };
        let f = flatten(&e, Affine::identity()).expect("LWPOLYLINE should flatten");
        let apex = f.geom.subpaths[0]
            .points
            .iter()
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .unwrap();
        // The bulge belongs to the 20->30 segment, so the arc's apex sits at
        // x = 25. Mis-paired, it would appear on the 0->10 segment (x = 5).
        assert!((apex.x - 25.0).abs() < 0.5, "arc apex landed at {apex:?}");
    }

    #[test]
    fn a_leader_is_drawn_as_a_polyline() {
        let e = ent("LEADER", &[(10, 0.0), (20, 0.0), (10, 5.0), (20, 5.0), (10, 12.0), (20, 5.0)]);
        let f = flatten(&e, Affine::identity()).expect("LEADER should flatten");
        assert_eq!(f.geom.subpaths[0].points.len(), 3);
    }

    #[test]
    fn unsupported_entities_return_none_rather_than_empty_geometry() {
        let e = ent("3DSOLID", &[]);
        assert!(flatten(&e, Affine::identity()).is_none());
    }
}
