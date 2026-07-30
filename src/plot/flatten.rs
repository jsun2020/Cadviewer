use std::f64::consts::TAU;

use crate::dxfnew::entities::RawEntity;
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

/// Sample a circular arc counter-clockwise from `start_deg` to `end_deg`.
pub fn arc_points(
    center: Point,
    radius: f64,
    start_deg: f64,
    end_deg: f64,
    segments: usize,
) -> Vec<Point> {
    let start = start_deg.to_radians();
    let mut sweep = end_deg.to_radians() - start;
    // DXF arcs always run counter-clockwise, so an end angle numerically
    // below the start angle means the arc crosses zero.
    while sweep <= 0.0 {
        sweep += TAU;
    }
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
    if bulge.abs() < 1e-12 {
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
            Some(transformed(arc_points(c, r, 0.0, 360.0, CIRCLE_SEGMENTS), t, true))
        }
        "ARC" => {
            let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            let r = entity.f64(40, 0.0);
            if r <= 0.0 {
                return None;
            }
            let start = entity.f64(50, 0.0);
            let end = entity.f64(51, 360.0);
            Some(transformed(arc_points(c, r, start, end, CIRCLE_SEGMENTS), t, false))
        }
        "ELLIPSE" => Some(transformed(ellipse_points(entity), t, false)),
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

fn ellipse_points(entity: &RawEntity) -> Vec<Point> {
    let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
    let major = Point::new(entity.f64(11, 0.0), entity.f64(21, 0.0));
    let ratio = entity.f64(40, 1.0);
    let start = entity.f64(41, 0.0);
    let end = entity.f64(42, TAU);
    let a = (major.x * major.x + major.y * major.y).sqrt();
    let b = a * ratio;
    let rot = major.y.atan2(major.x);
    let mut sweep = end - start;
    while sweep <= 0.0 {
        sweep += TAU;
    }
    (0..=CIRCLE_SEGMENTS)
        .map(|i| {
            let param = start + sweep * (i as f64) / (CIRCLE_SEGMENTS as f64);
            let (x, y) = (a * param.cos(), b * param.sin());
            Point::new(
                c.x + x * rot.cos() - y * rot.sin(),
                c.y + x * rot.sin() + y * rot.cos(),
            )
        })
        .collect()
}

fn polyline_points(entity: &RawEntity) -> Vec<Point> {
    let verts = entity.points(10, 20);
    let bulges = entity.all_f64(42);
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
    use crate::dxfnew::lexer::Value;

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

    #[test]
    fn unsupported_entities_return_none_rather_than_empty_geometry() {
        let e = ent("3DSOLID", &[]);
        assert!(flatten(&e, Affine::identity()).is_none());
    }
}
