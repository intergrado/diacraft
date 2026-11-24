//! A two-dimensional affine map, in the order MS-EMF writes an `XForm`: a point (x, y) goes to
//! (`a`·x + `c`·y + `e`, `b`·x + `d`·y + `f`).

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Xf {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Xf {
    pub const IDENTITY: Xf = Xf { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    /// From the six numbers of an `XForm`: M11, M12, M21, M22, Dx, Dy.
    pub fn from_array(m: [f64; 6]) -> Xf {
        Xf { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] }
    }

    pub fn translate(dx: f64, dy: f64) -> Xf {
        Xf { e: dx, f: dy, ..Xf::IDENTITY }
    }

    pub fn scale(sx: f64, sy: f64) -> Xf {
        Xf { a: sx, d: sy, ..Xf::IDENTITY }
    }

    /// A turn by `degrees`, the way GDI+ writes it: x' = x·cos − y·sin, y' = x·sin + y·cos.
    pub fn rotate_degrees(degrees: f64) -> Xf {
        let (s, c) = degrees.to_radians().sin_cos();
        Xf { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// The map that applies `self` first and then `then`.
    pub fn then(self, then: Xf) -> Xf {
        Xf {
            a: self.a * then.a + self.b * then.c,
            b: self.a * then.b + self.b * then.d,
            c: self.c * then.a + self.d * then.c,
            d: self.c * then.b + self.d * then.d,
            e: self.e * then.a + self.f * then.c + then.e,
            f: self.e * then.b + self.f * then.d + then.f,
        }
    }

    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        [self.a * p[0] + self.c * p[1] + self.e, self.b * p[0] + self.d * p[1] + self.f]
    }

    /// The linear part applied to a vector (no translation).
    pub fn apply_vector(&self, v: [f64; 2]) -> [f64; 2] {
        [self.a * v[0] + self.c * v[1], self.b * v[0] + self.d * v[1]]
    }

    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// Whether every number is finite and the map can be inverted (otherwise it is not taken).
    pub fn is_usable(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f].iter().all(|v| v.is_finite()) && self.determinant().is_finite() && self.determinant() != 0.0
    }

    /// The inverse, or `None` for a map that cannot be inverted.
    pub fn inverse(&self) -> Option<Xf> {
        let det = self.determinant();
        if !det.is_finite() || det == 0.0 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        let inv = Xf { a, b, c, d, e: -(self.e * a + self.f * c), f: -(self.e * b + self.f * d) };
        inv.is_usable().then_some(inv)
    }

    /// The length one unit of x or y becomes, taken together: the square root of the area scale. A pen width goes through it.
    pub fn mean_scale(&self) -> f64 {
        self.determinant().abs().sqrt()
    }
}
