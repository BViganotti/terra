//! Minimal 3-vector math for spherical geometry.
//!
//! Terra stores every cell direction as a unit [`Vec3`]. Working directly in
//! three dimensions avoids the pole and wrap-around special cases that plague
//! latitude/longitude grids, and it makes nearest-neighbor and boundary
//! calculations branch-free.

use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// A three-component double precision vector.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    /// The unit X axis.
    pub const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    /// The unit Y axis.
    pub const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    /// The unit Z axis (the north pole in Terra's convention).
    pub const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    /// Constructs a vector.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product.
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Squared Euclidean length.
    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }

    /// Euclidean length.
    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }

    /// Returns the unit vector, or `None` for a zero or non-finite vector.
    pub fn try_normalize(self) -> Option<Self> {
        let len = self.length();
        if len.is_finite() && len > 1e-12 {
            Some(self / len)
        } else {
            None
        }
    }

    /// Returns the unit vector, falling back to `+Z` for degenerate input.
    pub fn normalized(self) -> Self {
        self.try_normalize().unwrap_or(Self::Z)
    }

    /// Euclidean distance.
    pub fn distance(self, other: Self) -> f64 {
        (self - other).length()
    }

    /// Great-circle angle in radians, clamped to `[0, pi]`.
    pub fn angle(self, other: Self) -> f64 {
        let a = self.normalized();
        let b = other.normalized();
        a.dot(b).clamp(-1.0, 1.0).acos()
    }

    /// Great-circle distance on a unit sphere, in radians.
    pub fn arc_distance(self, other: Self) -> f64 {
        self.angle(other)
    }

    /// Linear interpolation.
    pub fn lerp(self, other: Self, t: f64) -> Self {
        self * (1.0 - t) + other * t
    }

    /// Component-wise minimum.
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    /// Component-wise maximum.
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }

    /// Projects onto the plane tangent at `normal` and returns a unit vector.
    /// Falls back to `None` when the projection is degenerate.
    pub fn project_tangent(self, normal: Self) -> Option<Self> {
        let n = normal.normalized();
        (self - n * self.dot(n)).try_normalize()
    }

    /// Rotates `self` around the unit `axis` by `angle` radians (Rodrigues).
    pub fn rotate_around(self, axis: Self, angle: f64) -> Self {
        let k = axis.normalized();
        let (s, c) = angle.sin_cos();
        self * c + k.cross(self) * s + k * (k.dot(self) * (1.0 - c))
    }

    /// An orthonormal tangent basis at this (assumed unit) direction. The
    /// returned pair `(east, north)` is right-handed with `self` as up.
    pub fn tangent_basis(self) -> (Self, Self) {
        let up = self.normalized();
        // Pick the world axis least aligned with `up` to avoid cancellation.
        let helper = if up.z.abs() < 0.9 { Self::Z } else { Self::X };
        let east = helper.cross(up).normalized();
        let north = up.cross(east).normalized();
        (east, north)
    }

    /// Latitude in degrees, `[-90, 90]`, positive north.
    pub fn latitude_deg(self) -> f64 {
        let n = self.normalized();
        n.z.clamp(-1.0, 1.0).asin().to_degrees()
    }

    /// Longitude in degrees, `[-180, 180]`, positive east.
    pub fn longitude_deg(self) -> f64 {
        let n = self.normalized();
        n.y.atan2(n.x).to_degrees()
    }

    /// Builds a unit vector from latitude and longitude in degrees.
    pub fn from_lat_lon(lat_deg: f64, lon_deg: f64) -> Self {
        let lat = lat_deg.to_radians();
        let lon = lon_deg.to_radians();
        let (sl, cl) = lat.sin_cos();
        Self::new(cl * lon.cos(), cl * lon.sin(), sl)
    }

    /// True when every component is finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Folds the vector into a stable hash.
    pub fn hash_into(self, h: &mut crate::hash::Hasher64) {
        h.write_f64(self.x);
        h.write_f64(self.y);
        h.write_f64(self.z);
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, rhs: f64) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, rhs: Vec3) -> Self::Output {
        rhs * self
    }
}

impl Div<f64> for Vec3 {
    type Output = Vec3;
    fn div(self, rhs: f64) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl fmt::Debug for Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Vec3({:.5}, {:.5}, {:.5})", self.x, self.y, self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn basic_arithmetic() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(-1.0, 0.5, 2.0);
        assert_eq!(a + b, Vec3::new(0.0, 2.5, 5.0));
        assert_eq!(a - b, Vec3::new(2.0, 1.5, 1.0));
        assert_eq!(a * 2.0, Vec3::new(2.0, 4.0, 6.0));
        assert_eq!(2.0 * a, a * 2.0);
        assert_eq!(a / 2.0, Vec3::new(0.5, 1.0, 1.5));
        assert_eq!(-a, Vec3::new(-1.0, -2.0, -3.0));
    }

    #[test]
    fn cross_and_dot() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);
        assert_eq!(Vec3::X.dot(Vec3::Y), 0.0);
        assert_eq!(Vec3::X.dot(Vec3::X), 1.0);
        let v = Vec3::new(3.0, 4.0, 12.0);
        assert_eq!(v.length(), 13.0);
    }

    #[test]
    fn normalize_handles_degenerate() {
        assert_eq!(Vec3::ZERO.normalized(), Vec3::Z);
        assert!(Vec3::ZERO.try_normalize().is_none());
        assert!(Vec3::new(f64::NAN, 0.0, 0.0).try_normalize().is_none());
        assert!(close(Vec3::new(5.0, 0.0, 0.0).normalized().length(), 1.0));
    }

    #[test]
    fn angle_is_clamped() {
        let a = Vec3::new(0.7, 0.0, 0.7).normalized();
        assert!(close(a.angle(a), 0.0));
        assert!(close(Vec3::X.angle(-Vec3::X), std::f64::consts::PI));
        // Slightly non-unit inputs must not produce NaN.
        let b = Vec3::new(1.0000001, 0.0, 0.0);
        assert!(b.angle(Vec3::X).is_finite());
    }

    #[test]
    fn lat_lon_round_trip() {
        for &(lat, lon) in &[
            (0.0, 0.0),
            (45.0, 90.0),
            (-30.0, -120.0),
            (89.0, 179.0),
            (-89.0, -179.0),
        ] {
            let v = Vec3::from_lat_lon(lat, lon);
            assert!(close(v.length(), 1.0));
            assert!(close(v.latitude_deg(), lat));
            assert!((v.longitude_deg() - lon).abs() < 1e-6);
        }
    }

    #[test]
    fn poles_have_stable_basis() {
        for p in [Vec3::Z, -Vec3::Z] {
            let (e, n) = p.tangent_basis();
            assert!(close(e.length(), 1.0));
            assert!(close(n.length(), 1.0));
            assert!(close(e.dot(n), 0.0));
            assert!(close(e.dot(p), 0.0));
            assert!(close(n.dot(p), 0.0));
        }
    }

    #[test]
    fn projection_removes_normal_component() {
        let normal = Vec3::new(1.0, 1.0, 0.0).normalized();
        let v = Vec3::new(0.0, 0.0, 1.0);
        let t = v.project_tangent(normal).unwrap();
        assert!(close(t.dot(normal), 0.0));
        assert!(close(t.length(), 1.0));
        assert!(normal.project_tangent(normal).is_none());
    }

    #[test]
    fn rotation_preserves_length_and_angle() {
        let axis = Vec3::new(0.3, 0.4, 0.5).normalized();
        // Pick a vector in the plane perpendicular to the axis.
        let v = axis.cross(Vec3::X).normalized();
        let r = v.rotate_around(axis, 1.1);
        assert!(close(r.length(), 1.0));
        assert!(close(v.angle(r), 1.1));
        let back = r.rotate_around(axis, -1.1);
        assert!(close(back.distance(v), 0.0));
    }

    #[test]
    fn lerp_endpoints() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(2.0, 4.0, 6.0);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, 0.5), Vec3::new(1.0, 2.0, 3.0));
    }
}
