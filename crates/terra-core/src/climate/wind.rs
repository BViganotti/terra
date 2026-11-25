//! Wind and surface currents.
//!
//! Winds are the classic three-cell zonal pattern: tropical easterlies,
//! mid-latitude westerlies and polar easterlies, blended smoothly across
//! latitudes. A deterministic per-cell perturbation breaks the perfect zonal
//! symmetry without changing the band structure.
//!
//! Ocean currents are an Ekman-like approximation: surface flow is the wind
//! rotated to the right in the northern hemisphere and to the left in the
//! southern hemisphere, scaled down and smoothed across ocean cells. This is
//! enough to move heat poleward and to shape moisture transport; it is not a
//! solution of the shallow-water equations, which is documented in
//! `LIMITATIONS.md`.

use crate::hash::Hasher64;
use crate::noise::value_noise;
use crate::sphere::Sphere;
use crate::vec3::Vec3;

/// Wind and current vectors per cell.
#[derive(Clone, Debug)]
pub struct Winds {
    /// Eastward wind component in meters per second.
    pub wind_east: Vec<f64>,
    /// Northward wind component in meters per second.
    pub wind_north: Vec<f64>,
    /// Wind speed in meters per second.
    pub wind_speed: Vec<f64>,
    /// Unit wind direction on the sphere.
    pub wind_dir: Vec<Vec3>,
    /// Eastward surface current in meters per second (ocean only).
    pub current_east: Vec<f64>,
    /// Northward surface current in meters per second (ocean only).
    pub current_north: Vec<f64>,
    /// Current speed in meters per second.
    pub current_speed: Vec<f64>,
    /// Poleward component of the current, positive toward the nearest pole.
    pub current_poleward: Vec<f64>,
}

impl Winds {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.wind_speed.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.wind_speed.is_empty()
    }

    /// Wind direction as a compass bearing in degrees, meteorological
    /// convention (the direction the wind blows *from*).
    pub fn bearing_from(&self, cell: u32) -> f64 {
        let Some(dir) = self.wind_dir.get(cell as usize) else {
            return 0.0;
        };
        // Bearing toward which the wind blows, then add 180 degrees.
        let toward = dir.longitude_deg();
        ((toward + 180.0) % 360.0 + 360.0) % 360.0
    }

    /// Folds the fields into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("winds");
        h.write_f64_slice(&self.wind_east);
        h.write_f64_slice(&self.wind_north);
        h.write_f64_slice(&self.current_east);
        h.write_f64_slice(&self.current_north);
    }
}

/// Zonal wind components at a latitude, in meters per second.
///
/// Returns `(eastward, northward)`. The profile is continuous: trades peak at
/// the equator, westerlies at 45 degrees, polar easterlies near 75 degrees.
pub fn zonal_profile(lat_deg: f64) -> (f64, f64) {
    let lat = lat_deg.clamp(-90.0, 90.0);
    let a = lat.abs();
    let sign = lat.signum();

    let trades_u = if a < 30.0 {
        -6.0 * (a * std::f64::consts::PI / 60.0).cos()
    } else {
        0.0
    };
    let trades_v = if a < 30.0 {
        // Continuous through the equator: both hemispheres flow toward it.
        -2.0 * (lat * std::f64::consts::PI / 60.0).sin() * (a * std::f64::consts::PI / 60.0).cos()
    } else {
        0.0
    };
    let west_u = if (30.0..60.0).contains(&a) {
        8.0 * ((a - 30.0) * std::f64::consts::PI / 30.0).sin()
    } else {
        0.0
    };
    let west_v = if (30.0..60.0).contains(&a) {
        1.2 * sign * ((a - 30.0) * std::f64::consts::PI / 30.0).sin()
    } else {
        0.0
    };
    let polar_u = if a > 60.0 {
        -4.0 * ((a - 60.0) * std::f64::consts::PI / 30.0).sin()
    } else {
        0.0
    };
    let polar_v = if a > 60.0 {
        -0.8 * sign * ((a - 60.0) * std::f64::consts::PI / 30.0).sin()
    } else {
        0.0
    };
    (trades_u + west_u + polar_u, trades_v + west_v + polar_v)
}

/// Computes wind and surface currents.
///
/// `ocean` marks water cells; currents are zero on land. `seed` drives small
/// perturbations in wind speed and direction.
pub fn compute(sphere: &Sphere, ocean: &[bool], seed: u64) -> Winds {
    let n = sphere.len();
    let mut wind_east = vec![0.0f64; n];
    let mut wind_north = vec![0.0f64; n];
    let mut wind_speed = vec![0.0f64; n];
    let mut wind_dir = vec![Vec3::Z; n];

    for i in 0..n {
        let cell = i as u32;
        let pos = sphere.pos(cell);
        let (east, north) = pos.tangent_basis();
        let lat = pos.latitude_deg();
        let (u0, v0) = zonal_profile(lat);
        // Perturb the speed by up to +-25% and rotate by up to +-18 degrees.
        let speed_noise = value_noise(seed ^ 0x00a1_7e00, pos, 3.0);
        let angle_noise = value_noise(seed ^ 0x00b2_7e00, pos, 2.2) * 0.31;
        let scale = 1.0 + speed_noise * 0.25;
        let (s, c) = angle_noise.sin_cos();
        let u = (u0 * c - v0 * s) * scale;
        let v = (u0 * s + v0 * c) * scale;
        let speed = (u * u + v * v).sqrt();
        wind_east[i] = u;
        wind_north[i] = v;
        wind_speed[i] = speed;
        wind_dir[i] = if speed > 1e-9 {
            (east * u + north * v).normalized()
        } else {
            east
        };
    }

    // Ekman-like rotation, scaled to a few percent of wind speed.
    let mut current_east = vec![0.0f64; n];
    let mut current_north = vec![0.0f64; n];
    for i in 0..n {
        if !ocean.get(i).copied().unwrap_or(false) {
            continue;
        }
        let lat = sphere.lat(i as u32);
        let theta: f64 = if lat >= 0.0 { 0.6 } else { -0.6 };
        let (s, c) = theta.sin_cos();
        let u = wind_east[i] * 0.028;
        let v = wind_north[i] * 0.028;
        // Right rotation in the north, left in the south.
        current_east[i] = u * c + v * s;
        current_north[i] = -u * s + v * c;
    }

    // Smooth currents across water; land stays at zero.
    for _ in 0..3 {
        let mut ne = current_east.clone();
        let mut nn = current_north.clone();
        for i in 0..n {
            if !ocean.get(i).copied().unwrap_or(false) {
                ne[i] = 0.0;
                nn[i] = 0.0;
                continue;
            }
            let mut se = current_east[i] * 2.0;
            let mut sn = current_north[i] * 2.0;
            let mut w = 2.0;
            for &nb in sphere.neighbors(i as u32) {
                let ni = nb as usize;
                if ocean.get(ni).copied().unwrap_or(false) {
                    se += current_east[ni];
                    sn += current_north[ni];
                    w += 1.0;
                }
            }
            ne[i] = se / w;
            nn[i] = sn / w;
        }
        current_east = ne;
        current_north = nn;
    }

    let mut current_speed = vec![0.0f64; n];
    let mut current_poleward = vec![0.0f64; n];
    for i in 0..n {
        let speed = (current_east[i] * current_east[i] + current_north[i] * current_north[i])
            .sqrt()
            .min(3.0);
        current_speed[i] = speed;
        let lat = sphere.lat(i as u32);
        let poleward = if lat >= 0.0 { 1.0 } else { -1.0 };
        current_poleward[i] = current_north[i] * poleward;
    }

    Winds {
        wind_east,
        wind_north,
        wind_speed,
        wind_dir,
        current_east,
        current_north,
        current_speed,
        current_poleward,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trades_blow_west() {
        for lat in [-20.0, -15.0, -5.0, 0.0, 5.0, 15.0, 20.0] {
            let (u, _) = zonal_profile(lat);
            assert!(u < -2.0, "lat {lat} u {u}");
        }
    }

    #[test]
    fn westerlies_blow_east() {
        for lat in [-52.0, -45.0, -38.0, 38.0, 45.0, 52.0] {
            let (u, _) = zonal_profile(lat);
            assert!(u > 2.0, "lat {lat} u {u}");
        }
    }

    #[test]
    fn polar_easterlies_blow_west() {
        for lat in [-80.0, -72.0, 72.0, 80.0] {
            let (u, _) = zonal_profile(lat);
            assert!(u < -1.0, "lat {lat} u {u}");
        }
    }

    #[test]
    fn profile_is_continuous() {
        let mut prev = zonal_profile(-90.0);
        for i in -899..=899 {
            let lat = i as f64 / 10.0;
            let cur = zonal_profile(lat);
            assert!((cur.0 - prev.0).abs() < 1.0, "jump at {lat}");
            assert!((cur.1 - prev.1).abs() < 1.0, "jump at {lat}");
            prev = cur;
        }
    }

    #[test]
    fn winds_are_bounded_and_finite() {
        let s = Sphere::new(3);
        let ocean = vec![true; s.len()];
        let w = compute(&s, &ocean, 4);
        for i in 0..s.len() {
            assert!(w.wind_speed[i].is_finite());
            assert!(w.wind_speed[i] < 30.0);
            assert!(w.wind_dir[i].is_finite());
            assert!((w.wind_dir[i].length() - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn currents_exist_only_on_water() {
        let s = Sphere::new(3);
        let mut ocean = vec![true; s.len()];
        for i in (0..s.len()).step_by(3) {
            ocean[i] = false;
        }
        let w = compute(&s, &ocean, 6);
        for i in 0..s.len() {
            if ocean[i] {
                assert!(w.current_speed[i] >= 0.0);
                assert!(w.current_speed[i] <= 3.0);
            } else {
                assert_eq!(w.current_east[i], 0.0);
                assert_eq!(w.current_north[i], 0.0);
                assert_eq!(w.current_speed[i], 0.0);
            }
        }
        assert!(w.current_speed.iter().any(|&s| s > 0.0));
    }

    #[test]
    fn ekman_rotation_has_the_right_sign() {
        let s = Sphere::new(2);
        let ocean = vec![true; s.len()];
        let w = compute(&s, &ocean, 8);
        let mut sum_cross = 0.0;
        let mut checked = 0;
        for i in 0..s.len() {
            let lat = s.lat(i as u32);
            if lat > 20.0 && w.wind_speed[i] > 3.0 {
                // Northern hemisphere currents veer right of the wind.
                let cross =
                    w.wind_east[i] * w.current_north[i] - w.wind_north[i] * w.current_east[i];
                sum_cross += cross;
                checked += 1;
            }
        }
        assert!(checked > 0);
        assert!(sum_cross < 0.0, "aggregate cross {sum_cross}");
    }

    #[test]
    fn currents_are_slow() {
        let s = Sphere::new(3);
        let ocean = vec![true; s.len()];
        let w = compute(&s, &ocean, 10);
        let max = w.current_speed.iter().copied().fold(0.0f64, f64::max);
        assert!(max > 0.0 && max < 1.0);
    }

    #[test]
    fn deterministic() {
        let s = Sphere::new(3);
        let ocean = vec![true; s.len()];
        let a = compute(&s, &ocean, 12);
        let b = compute(&s, &ocean, 12);
        assert_eq!(a.wind_east, b.wind_east);
        assert_eq!(a.current_north, b.current_north);
    }

    #[test]
    fn bearing_is_meteorological() {
        let s = Sphere::new(2);
        let ocean = vec![true; s.len()];
        let w = compute(&s, &ocean, 14);
        for cell in 0..s.len() as u32 {
            let b = w.bearing_from(cell);
            assert!((0.0..360.0).contains(&b));
        }
        assert_eq!(w.bearing_from(99_999), 0.0);
    }
}
