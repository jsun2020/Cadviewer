#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Row-major 2x3 affine transform: [a c e; b d f].
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub fn identity() -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }

    pub fn translation(x: f64, y: f64) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y }
    }

    pub fn scale(x: f64, y: f64) -> Self {
        Self { a: x, b: 0.0, c: 0.0, d: y, e: 0.0, f: 0.0 }
    }

    pub fn rotation(degrees: f64) -> Self {
        let r = degrees.to_radians();
        let (s, c) = r.sin_cos();
        Self { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// `self` applied first, then `rhs`.
    pub fn then(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.b * rhs.c,
            b: self.a * rhs.b + self.b * rhs.d,
            c: self.c * rhs.a + self.d * rhs.c,
            d: self.c * rhs.b + self.d * rhs.d,
            e: self.e * rhs.a + self.f * rhs.c + rhs.e,
            f: self.e * rhs.b + self.f * rhs.d + rhs.f,
        }
    }

    pub fn apply(self, p: Point) -> Point {
        Point::new(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }

    /// Geometric mean of the two axis scales. Used to convert drawing-unit
    /// radii and text heights through a transform.
    pub fn average_scale(self) -> f64 {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        let sy = (self.c * self.c + self.d * self.d).sqrt();
        ((sx * sy).abs()).sqrt()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds {
    pub fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    pub fn add(&mut self, p: Point) {
        if p.x < self.min_x { self.min_x = p.x; }
        if p.y < self.min_y { self.min_y = p.y; }
        if p.x > self.max_x { self.max_x = p.x; }
        if p.y > self.max_y { self.max_y = p.y; }
    }

    pub fn valid(self) -> bool {
        self.min_x <= self.max_x && self.min_y <= self.max_y
    }

    pub fn width(self) -> f64 {
        (self.max_x - self.min_x).max(0.0)
    }

    pub fn height(self) -> f64 {
        (self.max_y - self.min_y).max(0.0)
    }

    /// True when `other` lies entirely inside `self`.
    pub fn contains(self, other: Bounds) -> bool {
        self.valid()
            && other.valid()
            && self.min_x <= other.min_x
            && self.min_y <= other.min_y
            && self.max_x >= other.max_x
            && self.max_y >= other.max_y
    }
}

/// A run of connected points. Curves are flattened before they get here.
#[derive(Clone, Debug, Default)]
pub struct SubPath {
    pub points: Vec<Point>,
    pub closed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PathGeom {
    pub subpaths: Vec<SubPath>,
}

impl PathGeom {
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::empty();
        for sp in &self.subpaths {
            for p in &sp.points {
                b.add(*p);
            }
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_composes_in_application_order() {
        let t = Affine::scale(2.0, 2.0).then(Affine::translation(10.0, 5.0));
        let p = t.apply(Point::new(1.0, 1.0));
        assert!((p.x - 12.0).abs() < 1e-9, "x was {}", p.x);
        assert!((p.y - 7.0).abs() < 1e-9, "y was {}", p.y);
    }

    #[test]
    fn bounds_contains_is_strict_about_the_outer_box() {
        let mut outer = Bounds::empty();
        outer.add(Point::new(0.0, 0.0));
        outer.add(Point::new(100.0, 100.0));
        let mut inner = Bounds::empty();
        inner.add(Point::new(10.0, 10.0));
        inner.add(Point::new(20.0, 20.0));
        assert!(outer.contains(inner));
        assert!(!inner.contains(outer));
    }
}
