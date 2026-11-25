//! Deterministic value noise on the sphere.
//!
//! The world is sampled in 3D so there is no seam at the date line and no
//! pinching at the poles. Noise is a pure function of a seed, a direction and a
//! frequency: it never consumes a sequential stream, which means adding a new
//! noise consumer cannot shift unrelated results.

use crate::rng::{mix3, mix64, unit_from_hash};
use crate::vec3::Vec3;

#[inline]
fn lattice_value(seed: u64, x: i64, y: i64, z: i64) -> f64 {
    let h = mix3(
        seed,
        x as u64,
        mix64(y as u64) ^ (z as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15),
    );
    unit_from_hash(h)
}

#[inline]
fn quintic(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Value noise in `[-1, 1]` at a single frequency.
///
/// The lattice is hashed with the world seed, so different seeds produce
/// unrelated fields. `freq` is the number of lattice cells per unit length; for
/// a unit sphere, values around 2 to 12 give continent-scale detail.
pub fn value_noise(seed: u64, p: Vec3, freq: f64) -> f64 {
    if !(freq.is_finite() && freq > 0.0) {
        return 0.0;
    }
    let q = p * freq;
    let x0 = q.x.floor();
    let y0 = q.y.floor();
    let z0 = q.z.floor();
    let fx = quintic(q.x - x0);
    let fy = quintic(q.y - y0);
    let fz = quintic(q.z - z0);
    let (xi, yi, zi) = (x0 as i64, y0 as i64, z0 as i64);

    let c000 = lattice_value(seed, xi, yi, zi);
    let c100 = lattice_value(seed, xi + 1, yi, zi);
    let c010 = lattice_value(seed, xi, yi + 1, zi);
    let c110 = lattice_value(seed, xi + 1, yi + 1, zi);
    let c001 = lattice_value(seed, xi, yi, zi + 1);
    let c101 = lattice_value(seed, xi + 1, yi, zi + 1);
    let c011 = lattice_value(seed, xi, yi + 1, zi + 1);
    let c111 = lattice_value(seed, xi + 1, yi + 1, zi + 1);

    let x00 = lerp(c000, c100, fx);
    let x10 = lerp(c010, c110, fx);
    let x01 = lerp(c001, c101, fx);
    let x11 = lerp(c011, c111, fx);
    let y0v = lerp(x00, x10, fy);
    let y1v = lerp(x01, x11, fy);
    lerp(y0v, y1v, fz) * 2.0 - 1.0
}

/// Fractal Brownian motion: a sum of value-noise octaves.
///
/// The result is normalized to approximately `[-1, 1]`. Octaves are capped at
/// 12 so a malformed scenario cannot request unbounded work.
pub fn fbm(seed: u64, p: Vec3, freq: f64, octaves: u32, lacunarity: f64, gain: f64) -> f64 {
    let octaves = octaves.min(12);
    let lacunarity = if lacunarity.is_finite() && lacunarity > 1.0 {
        lacunarity.min(8.0)
    } else {
        2.0
    };
    let gain = if gain.is_finite() {
        gain.clamp(0.05, 0.95)
    } else {
        0.5
    };
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut f = freq;
    for o in 0..octaves {
        // Offset each octave's seed so octaves are not correlated.
        let octave_seed = seed ^ (u64::from(o).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        sum += value_noise(octave_seed, p, f) * amp;
        norm += amp;
        amp *= gain;
        f *= lacunarity;
    }
    if norm > 0.0 {
        (sum / norm).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Ridged multifractal noise in `[0, 1]`, useful for mountain belts and
/// drainage divides. Ridges are sharp where the underlying noise crosses zero.
pub fn ridged(seed: u64, p: Vec3, freq: f64, octaves: u32) -> f64 {
    let octaves = octaves.min(12);
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut f = freq;
    for o in 0..octaves {
        let octave_seed = seed ^ (u64::from(o).wrapping_mul(0x94d0_49bb_1331_11eb));
        let n = 1.0 - value_noise(octave_seed, p, f).abs();
        sum += n * n * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    if norm > 0.0 {
        (sum / norm).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Domain-warped fbm: offsets the sample point by another noise field. This
/// breaks the axis-aligned look of plain value noise and produces more natural
/// coastlines.
pub fn warped_fbm(seed: u64, p: Vec3, freq: f64, octaves: u32, warp: f64) -> f64 {
    let warp = warp.clamp(0.0, 0.5);
    if warp <= 0.0 {
        return fbm(seed, p, freq, octaves, 2.0, 0.5);
    }
    let (east, north) = p.tangent_basis();
    let dx = fbm(seed ^ 0x1111, p, freq * 0.5, 3, 2.0, 0.5);
    let dy = fbm(seed ^ 0x2222, p, freq * 0.5, 3, 2.0, 0.5);
    let q = (p + east * (dx * warp) + north * (dy * warp)).normalized();
    fbm(seed, q, freq, octaves, 2.0, 0.5)
}

/// A smooth, deterministic per-cell constant in `[0, 1)`.
pub fn cell_jitter(seed: u64, stream: u64, cell: u32) -> f64 {
    unit_from_hash(mix3(seed, stream, u64::from(cell)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic() {
        let p = Vec3::from_lat_lon(12.0, 34.0);
        let a = fbm(7, p, 3.0, 5, 2.0, 0.5);
        let b = fbm(7, p, 3.0, 5, 2.0, 0.5);
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn noise_is_bounded() {
        for seed in 0..20u64 {
            for i in 0..200 {
                let p = Vec3::from_lat_lon((i as f64 * 1.7) % 180.0 - 90.0, i as f64 * 3.1);
                let v = fbm(seed, p, 5.0, 6, 2.0, 0.5);
                assert!(v.is_finite() && (-1.0..=1.0).contains(&v), "{v}");
                let r = ridged(seed, p, 5.0, 4);
                assert!((0.0..=1.0).contains(&r), "{r}");
            }
        }
    }

    #[test]
    fn different_seeds_differ() {
        let p = Vec3::from_lat_lon(3.0, 4.0);
        assert_ne!(fbm(1, p, 4.0, 4, 2.0, 0.5), fbm(2, p, 4.0, 4, 2.0, 0.5));
    }

    #[test]
    fn nearby_points_are_correlated() {
        let p = Vec3::from_lat_lon(20.0, 30.0);
        let q = Vec3::from_lat_lon(20.05, 30.05);
        let a = value_noise(5, p, 2.0);
        let b = value_noise(5, q, 2.0);
        assert!((a - b).abs() < 0.2);
    }

    #[test]
    fn no_seam_at_the_date_line() {
        let a = fbm(11, Vec3::from_lat_lon(10.0, 179.999), 4.0, 5, 2.0, 0.5);
        let b = fbm(11, Vec3::from_lat_lon(10.0, -179.999), 4.0, 5, 2.0, 0.5);
        assert!((a - b).abs() < 0.01);
    }

    #[test]
    fn bad_parameters_are_safe() {
        let p = Vec3::X;
        assert_eq!(value_noise(1, p, 0.0), 0.0);
        assert_eq!(value_noise(1, p, f64::NAN), 0.0);
        assert!(fbm(1, p, 2.0, 0, 2.0, 0.5).is_finite());
        assert!(fbm(1, p, 2.0, 100, 2.0, 0.5).is_finite());
        assert!(fbm(1, p, 2.0, 4, f64::NAN, f64::NAN).is_finite());
        assert!(ridged(1, p, 2.0, 100).is_finite());
        assert!((-0.2..=1.2).contains(&warped_fbm(1, p, 2.0, 4, 99.0)));
    }

    #[test]
    fn ridged_has_ridges() {
        // Ridged noise should frequently reach high values, unlike fbm which is
        // concentrated near zero.
        let mut high = 0;
        let mut total = 0;
        for i in 0..500 {
            let p = Vec3::from_lat_lon((i as f64 * 0.7) % 180.0 - 90.0, i as f64 * 2.3);
            if ridged(3, p, 6.0, 5) > 0.6 {
                high += 1;
            }
            total += 1;
        }
        assert!(high > total / 20, "high={high} total={total}");
    }

    #[test]
    fn cell_jitter_is_stable_and_spread() {
        let a = cell_jitter(4, 9, 100);
        let b = cell_jitter(4, 9, 100);
        assert_eq!(a, b);
        assert!((0.0..1.0).contains(&a));
        assert_ne!(cell_jitter(4, 9, 100), cell_jitter(4, 9, 101));
    }
}
