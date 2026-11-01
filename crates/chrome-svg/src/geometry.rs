//! 2D affine maths for undoing Chrome's transforms.
//!
//! Chrome reports every box as its bounding box *after* all transforms. Takumi wants the box
//! *before* its transform plus the transform itself, so the lowering maps Chrome's boxes back
//! through the transforms of their ancestors (see `lower::Placer`).

/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f` (CSS `matrix(a, b, c, d, e, f)` order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    /// Parses Chrome's computed `transform`: `none` or `matrix(a, b, c, d, e, f)`.
    /// `matrix3d` (a 3D transform) returns `None`; the caller treats it as unsupported.
    pub fn parse_css(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || value == "none" {
            return Some(Self::IDENTITY);
        }
        let inner = value.strip_prefix("matrix(")?.strip_suffix(')')?;
        let n: Vec<f64> = inner
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect::<Option<_>>()?;
        match n[..] {
            [a, b, c, d, e, f] => Some(Self { a, b, c, d, e, f }),
            _ => None,
        }
    }

    /// `self ∘ other`: apply `other` first.
    pub fn then_inner(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    pub fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The linear part only (no translation).
    pub fn apply_linear(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    pub fn invert(&self) -> Option<Self> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Self {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    pub fn is_translation(&self) -> bool {
        self.a == 1.0 && self.b == 0.0 && self.c == 0.0 && self.d == 1.0
    }

    /// The size `(w, h)` of a rectangle whose image under this map has the bounding-box size
    /// `(bw, bh)`: solves `bw = |a|w + |c|h`, `bh = |b|w + |d|h`. `None` when the system is
    /// singular (e.g. a 45° rotation, where every rectangle of the same perimeter fits).
    pub fn unproject_size(&self, (bw, bh): (f64, f64)) -> Option<(f64, f64)> {
        if self.is_translation() {
            return Some((bw, bh));
        }
        let (a, b, c, d) = (self.a.abs(), self.b.abs(), self.c.abs(), self.d.abs());
        let det = a * d - b * c;
        if det.abs() < 1e-3 {
            return None;
        }
        Some(((d * bw - c * bh) / det, (a * bh - b * bw) / det))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_round_trips() {
        let m = Affine {
            a: 0.8,
            b: 0.6,
            c: -0.6,
            d: 0.8,
            e: 10.0,
            f: -4.0,
        };
        let p = m.invert().unwrap().apply(m.apply((3.0, 7.0)));
        assert!((p.0 - 3.0).abs() < 1e-9 && (p.1 - 7.0).abs() < 1e-9);
    }

    #[test]
    fn unproject_recovers_rotated_size() {
        let m = Affine {
            a: 0.8,
            b: 0.6,
            c: -0.6,
            d: 0.8,
            e: 0.0,
            f: 0.0,
        };
        let (w, h) = (40.0, 10.0);
        let bbox = (0.8 * w + 0.6 * h, 0.6 * w + 0.8 * h);
        let (rw, rh) = m.unproject_size(bbox).unwrap();
        assert!((rw - w).abs() < 1e-9 && (rh - h).abs() < 1e-9);
    }

    #[test]
    fn parses_chrome_matrix() {
        let m = Affine::parse_css("matrix(1, 0, 0, 1, 5, -2)").unwrap();
        assert_eq!(m, Affine::translate(5.0, -2.0));
        assert!(Affine::parse_css("matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1)").is_none());
    }
}
