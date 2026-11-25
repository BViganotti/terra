//! Temperature.
//!
//! Annual mean temperature is latitude-driven insolation, modified by four
//! explicit terms:
//!
//! - elevation lapse rate (6.2 degrees Celsius per kilometer, applied to land),
//! - ocean thermal inertia (water has a small annual range and a mild offset),
//! - continentality (distance from the coast, measured in graph hops),
//! - poleward heat transport by surface currents.
//!
//! The result is an annual mean plus a seasonal amplitude. Biome classification
//! uses the mean; the Studio shows the amplitude as a measure of seasonality.

use std::collections::VecDeque;

use crate::hash::Hasher64;
use crate::noise::value_noise;
use crate::sphere::Sphere;

/// Temperature fields.
#[derive(Clone, Debug)]
pub struct Temperature {
    /// Annual mean temperature in degrees Celsius.
    pub mean_c: Vec<f64>,
    /// Half the annual range in degrees Celsius.
    pub seasonal_amplitude_c: Vec<f64>,
    /// Warmest-month mean in degrees Celsius.
    pub max_c: Vec<f64>,
    /// Coldest-month mean in degrees Celsius.
    pub min_c: Vec<f64>,
    /// Continentality in `[0, 1]`; zero on the coast and ocean.
    pub continentality: Vec<f64>,
}

impl Temperature {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.mean_c.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.mean_c.is_empty()
    }

    /// Folds the fields into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("temperature");
        h.write_f64_slice(&self.mean_c);
        h.write_f64_slice(&self.seasonal_amplitude_c);
    }
}

/// Baseline sea-level temperature for a latitude, in degrees Celsius.
///
/// Equator: about 30 C. 45 degrees: about 13 C. Poles: about -30 C.
pub fn baseline_c(lat_deg: f64) -> f64 {
    let lat = lat_deg.clamp(-90.0, 90.0).to_radians();
    let c = lat.cos();
    30.0 * c.powf(1.3) - 30.0 * (1.0 - c)
}

/// Graph distance from every cell to the nearest water cell, in hops.
pub fn distance_to_water(sphere: &Sphere, water: &[bool]) -> Vec<u32> {
    let n = sphere.len();
    let mut dist = vec![u32::MAX; n];
    let mut queue = VecDeque::new();
    for (i, &w) in water.iter().enumerate().take(n) {
        if w {
            dist[i] = 0;
            queue.push_back(i as u32);
        }
    }
    while let Some(cell) = queue.pop_front() {
        let d = dist[cell as usize];
        for &nb in sphere.neighbors(cell) {
            let ni = nb as usize;
            if ni < n && dist[ni] == u32::MAX {
                dist[ni] = d.saturating_add(1);
                queue.push_back(nb);
            }
        }
    }
    dist
}

/// Computes temperature fields.
///
/// `ocean` marks water cells; `current_poleward` is the northward component of
/// the surface current for the cell's hemisphere, in meters per second.
pub fn compute(
    sphere: &Sphere,
    elevation: &[f64],
    ocean: &[bool],
    current_poleward: &[f64],
    seed: u64,
) -> Temperature {
    let n = sphere.len();
    let to_water = distance_to_water(sphere, ocean);
    let mut mean_c = vec![0.0f64; n];
    let mut seasonal = vec![0.0f64; n];
    let mut max_c = vec![0.0f64; n];
    let mut min_c = vec![0.0f64; n];
    let mut cont = vec![0.0f64; n];

    for i in 0..n {
        let cell = i as u32;
        let pos = sphere.pos(cell);
        let lat = pos.latitude_deg();
        let is_ocean = ocean.get(i).copied().unwrap_or(false);
        let elev = elevation.get(i).copied().unwrap_or(0.0).max(0.0);
        let c = if is_ocean {
            0.0
        } else {
            let hops = if to_water[i] == u32::MAX {
                8.0
            } else {
                to_water[i] as f64
            };
            (hops / 6.0).clamp(0.0, 1.0)
        };
        cont[i] = c;

        let mut t = baseline_c(lat);
        if is_ocean {
            t += 1.5;
            // Poleward currents carry heat toward high latitudes.
            let poleward = current_poleward.get(i).copied().unwrap_or(0.0);
            let weight = (lat.abs() / 55.0).clamp(0.0, 1.0);
            t += poleward.clamp(-1.5, 1.5) * 4.5 * weight;
            // Upwelling and cold western boundary currents are not modelled
            // separately; noise keeps the field from looking banded.
            t += value_noise(seed ^ 0x00c1_0001, pos, 4.0) * 0.8;
        } else {
            t -= 6.2 * (elev / 1_000.0);
            t -= 1.0 + 2.0 * c;
            t += value_noise(seed ^ 0x00c2_0002, pos, 4.5) * 1.4;
        }
        t = t.clamp(-65.0, 42.0);
        mean_c[i] = t;

        let abs_lat = lat.abs() / 90.0;
        let amp = if is_ocean {
            1.5 + 4.0 * abs_lat
        } else {
            4.0 + 24.0 * abs_lat + 5.0 * c
        };
        let amp = amp.clamp(0.5, 32.0);
        seasonal[i] = amp;
        max_c[i] = (t + amp).clamp(-80.0, 60.0);
        min_c[i] = (t - amp).clamp(-90.0, 50.0);
    }

    Temperature {
        mean_c,
        seasonal_amplitude_c: seasonal,
        max_c,
        min_c,
        continentality: cont,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::wind;
    use crate::tectonics::{self, TectonicsConfig};
    use crate::terrain;
    use crate::terrain::TerrainConfig;

    fn sample(seed: u64) -> (Sphere, terrain::Terrain, Temperature) {
        let s = Sphere::new(3);
        let t = tectonics::generate(&s, seed, TectonicsConfig::default());
        let ter = terrain::build(&s, &t, seed, TerrainConfig::default());
        let ocean: Vec<bool> = ter.elevation.iter().map(|&e| e <= 0.0).collect();
        let w = wind::compute(&s, &ocean, seed);
        let temp = compute(&s, &ter.elevation, &ocean, &w.current_poleward, seed);
        (s, ter, temp)
    }

    #[test]
    fn equator_is_warmer_than_the_poles() {
        let (s, _, t) = sample(4);
        let mut eq = (0.0, 0u32);
        let mut pole = (0.0, 0u32);
        for i in 0..s.len() {
            let lat = s.lat(i as u32);
            if lat.abs() < 10.0 {
                eq.0 += t.mean_c[i];
                eq.1 += 1;
            } else if lat.abs() > 70.0 {
                pole.0 += t.mean_c[i];
                pole.1 += 1;
            }
        }
        assert!(eq.1 > 0 && pole.1 > 0);
        assert!(eq.0 / (eq.1 as f64) > pole.0 / (pole.1 as f64) + 25.0);
    }

    #[test]
    fn temperature_decreases_with_elevation() {
        let (s, ter, t) = sample(6);
        // Compare high and low land within the same 20-degree latitude band.
        let band = |lat: f64| lat >= -10.0 && lat < 10.0;
        let mut high = (0.0, 0u32);
        let mut low = (0.0, 0u32);
        for i in 0..s.len() {
            let lat = s.lat(i as u32);
            if !band(lat) || ter.elevation[i] <= 0.0 {
                continue;
            }
            if ter.elevation[i] > 1_500.0 {
                high.0 += t.mean_c[i];
                high.1 += 1;
            } else if ter.elevation[i] < 300.0 {
                low.0 += t.mean_c[i];
                low.1 += 1;
            }
        }
        if high.1 > 0 && low.1 > 0 {
            assert!(high.0 / (high.1 as f64) < low.0 / (low.1 as f64) - 3.0);
        }
    }

    #[test]
    fn oceans_have_smaller_seasonal_range() {
        let (s, ter, t) = sample(8);
        let mut ocean = (0.0, 0u32);
        let mut land = (0.0, 0u32);
        for i in 0..s.len() {
            if ter.elevation[i] > 100.0 {
                land.0 += t.seasonal_amplitude_c[i];
                land.1 += 1;
            } else if ter.elevation[i] < -500.0 {
                ocean.0 += t.seasonal_amplitude_c[i];
                ocean.1 += 1;
            }
        }
        assert!(land.1 > 0 && ocean.1 > 0);
        assert!(ocean.0 / (ocean.1 as f64) < land.0 / (land.1 as f64));
    }

    #[test]
    fn continentality_is_zero_on_water() {
        let (s, ter, t) = sample(10);
        for i in 0..s.len() {
            if ter.elevation[i] <= 0.0 {
                assert_eq!(t.continentality[i], 0.0);
            } else {
                assert!((0.0..=1.0).contains(&t.continentality[i]));
            }
        }
    }

    #[test]
    fn continentality_grows_inland() {
        // A polar continent surrounded by ocean: cells near the coast are
        // maritime, the interior is continental.
        let s = Sphere::new(4);
        let elev: Vec<f64> = (0..s.len())
            .map(|i| {
                let lat = s.lat(i as u32);
                if lat > 55.0 {
                    200.0
                } else {
                    -2_000.0
                }
            })
            .collect();
        let ocean: Vec<bool> = elev.iter().map(|&e| e <= 0.0).collect();
        let w = wind::compute(&s, &ocean, 2);
        let t = compute(&s, &elev, &ocean, &w.current_poleward, 2);
        let mut coastal = 0u32;
        let mut interior = 0u32;
        for i in 0..s.len() {
            if elev[i] <= 0.0 {
                continue;
            }
            let touching_water = s
                .neighbors(i as u32)
                .iter()
                .any(|&n| elev[n as usize] <= 0.0);
            if touching_water {
                coastal += 1;
                assert!(t.continentality[i] <= 1.0 / 6.0 + 1e-9);
            }
            if s.lat(i as u32) > 80.0 {
                interior += 1;
                assert!(t.continentality[i] > 0.5);
            }
        }
        assert!(coastal > 0 && interior > 0);
    }

    #[test]
    fn fields_are_bounded_and_finite() {
        let (s, _, t) = sample(12);
        for i in 0..s.len() {
            assert!(t.mean_c[i].is_finite());
            assert!(t.mean_c[i] > -70.0 && t.mean_c[i] < 45.0);
            assert!(t.min_c[i] <= t.mean_c[i] + 1e-9);
            assert!(t.max_c[i] >= t.mean_c[i] - 1e-9);
            assert!(t.seasonal_amplitude_c[i] > 0.0);
        }
    }

    #[test]
    fn baseline_profile_is_ordered() {
        let mut prev = baseline_c(0.0);
        for lat in (0..=90).step_by(5) {
            let t = baseline_c(lat as f64);
            assert!(t <= prev + 1e-9, "lat {lat}");
            prev = t;
        }
        assert!(baseline_c(0.0) > 25.0);
        assert!(baseline_c(90.0) < -20.0);
    }

    #[test]
    fn deterministic() {
        let (_, _, a) = sample(14);
        let (_, _, b) = sample(14);
        for (x, y) in a.mean_c.iter().zip(&b.mean_c) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
    }

    #[test]
    fn distance_to_water_is_zero_at_water() {
        let s = Sphere::new(3);
        let mut water = vec![false; s.len()];
        water[0] = true;
        let d = distance_to_water(&s, &water);
        assert_eq!(d[0], 0);
        for &nb in s.neighbors(0) {
            assert_eq!(d[nb as usize], 1);
        }
        assert!(d.iter().all(|&v| v != u32::MAX));
    }
}
