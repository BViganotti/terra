//! Atmospheric moisture and precipitation.
//!
//! Moisture is a per-cell scalar that evaporates from water, advects along the
//! wind, rains out over land and is wrung out when air is forced over mountains.
//! The transport is semi-Lagrangian and synchronous: every cell reads the same
//! moisture field, so the result does not depend on iteration order.
//!
//! Three controls shape the pattern:
//!
//! - the intertropical convergence zone adds convective rain near the equator,
//! - subtropical subsidence suppresses rain near 25 degrees latitude, producing
//!   the world's dry belts,
//! - orographic lift increases rain on windward slopes and leaves a rain shadow
//!   downwind.
//!
//! Absolute totals are calibrated to a scenario target mean over land. The
//! relative pattern comes from the physics; the scale is a documented choice.

use crate::hash::Hasher64;
use crate::noise::value_noise;
use crate::sphere::Sphere;
use crate::vec3::Vec3;

use super::temperature::Temperature;
use super::wind::Winds;

/// Precipitation fields.
#[derive(Clone, Debug)]
pub struct Moisture {
    /// Annual precipitation in millimeters.
    pub precip_mm: Vec<f64>,
    /// Relative humidity in `[0, 1]`.
    pub humidity: Vec<f64>,
    /// Aridity index: precipitation divided by potential evapotranspiration.
    pub aridity: Vec<f64>,
    /// Estimated cloud cover in `[0, 1]`.
    pub cloud: Vec<f64>,
    /// Potential evapotranspiration in millimeters per year.
    pub pet_mm: Vec<f64>,
}

impl Moisture {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.precip_mm.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.precip_mm.is_empty()
    }

    /// Folds the fields into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("moisture");
        h.write_f64_slice(&self.precip_mm);
        h.write_f64_slice(&self.humidity);
    }
}

/// Moisture transport parameters.
#[derive(Clone, Copy, Debug)]
pub struct MoistureConfig {
    /// Advection passes, clamped to `4..=120`.
    pub passes: u32,
    /// Fraction of moisture handed to the wind each pass.
    pub advection: f64,
    /// Target mean annual precipitation over land in millimeters.
    pub target_land_mm: f64,
}

impl Default for MoistureConfig {
    fn default() -> Self {
        Self {
            passes: 32,
            advection: 0.55,
            target_land_mm: 860.0,
        }
    }
}

impl MoistureConfig {
    /// Clamps parameters into supported ranges.
    pub fn sanitized(mut self) -> Self {
        self.passes = self.passes.clamp(4, 120);
        self.advection = if self.advection.is_finite() {
            self.advection.clamp(0.05, 0.9)
        } else {
            0.55
        };
        self.target_land_mm = if self.target_land_mm.is_finite() {
            self.target_land_mm.clamp(50.0, 5_000.0)
        } else {
            860.0
        };
        self
    }
}

/// Saturation capacity in millimeters of precipitable water, from temperature.
pub fn capacity_mm(temp_c: f64) -> f64 {
    // Clausius-Clapeyron-like exponential, scaled to a column water proxy.
    let t = temp_c.clamp(-60.0, 50.0);
    (2.0 + 0.55 * (17.27 * t / (237.3 + t)).exp()).clamp(2.0, 26.0)
}

/// Potential evapotranspiration in millimeters per year.
pub fn potential_et_mm(temp_c: f64) -> f64 {
    let t = temp_c.clamp(-30.0, 50.0);
    (180.0 + 38.0 * t.max(0.0) + 12.0 * t.max(0.0).powi(2) / 10.0).clamp(120.0, 2_400.0)
}

/// Computes precipitation and humidity.
pub fn compute(
    sphere: &Sphere,
    elevation: &[f64],
    ocean: &[bool],
    temp: &Temperature,
    winds: &Winds,
    seed: u64,
    config: MoistureConfig,
) -> Moisture {
    let config = config.sanitized();
    let n = sphere.len();
    let capacity: Vec<f64> = (0..n)
        .map(|i| capacity_mm(temp.mean_c.get(i).copied().unwrap_or(0.0)))
        .collect();

    let mut moisture: Vec<f64> = (0..n)
        .map(|i| {
            if ocean.get(i).copied().unwrap_or(false) {
                capacity[i] * 0.9
            } else {
                capacity[i] * 0.15
            }
        })
        .collect();
    let mut precip = vec![0.0f64; n];

    // Precompute the upwind neighbor slope for orographic lift.
    let mut orographic_lift = vec![0.0f64; n];
    for i in 0..n {
        let cell = i as u32;
        let dir = winds.wind_dir[i];
        let mut best_dot = f64::INFINITY;
        let mut upwind = None;
        for &nb in sphere.neighbors(cell) {
            let toward = (sphere.pos(nb) - sphere.pos(cell))
                .try_normalize()
                .unwrap_or(Vec3::ZERO);
            let dot = toward.dot(dir);
            if dot < best_dot {
                best_dot = dot;
                upwind = Some(nb);
            }
        }
        if let Some(up) = upwind {
            let up_elev = elevation.get(up as usize).copied().unwrap_or(0.0).max(0.0);
            let here = elevation.get(i).copied().unwrap_or(0.0).max(0.0);
            orographic_lift[i] = (here - up_elev).max(0.0) / 1_000.0;
        }
    }

    for _ in 0..config.passes {
        let mut next = vec![0.0f64; n];
        let mut outflow = vec![0.0f64; n];
        for i in 0..n {
            let cell = i as u32;
            let lat = sphere.lat(cell).to_radians();
            let abs_lat = lat.abs().to_degrees();
            let is_ocean = ocean.get(i).copied().unwrap_or(false);

            let itcz = (-(lat / 0.16).powi(2)).exp();
            let subtropics = (-((abs_lat - 25.0) / 12.0).powi(2)).exp();
            let mut rain_fraction = if is_ocean { 0.025 } else { 0.045 };
            rain_fraction *= 1.0 + 0.7 * itcz;
            rain_fraction *= 1.0 - 0.82 * subtropics;
            rain_fraction += 0.20 * orographic_lift[i].min(3.0);
            rain_fraction *= 1.0 + 0.05 * value_noise(seed ^ 0x00d1_0001, sphere.pos(cell), 7.0);
            rain_fraction = rain_fraction.clamp(0.002, 0.45);

            let rain = (moisture[i] * rain_fraction).max(0.0);
            precip[i] += rain;
            let base = (moisture[i] - rain).max(0.0);
            let out = base * config.advection;
            outflow[i] = out;
            next[i] = base - out;
        }

        // Distribute outflow to downwind neighbors.
        for i in 0..n {
            let out = outflow[i];
            if out <= 0.0 {
                next[i] += out;
                continue;
            }
            let cell = i as u32;
            let dir = winds.wind_dir[i];
            let mut weights: Vec<(usize, f64)> = Vec::with_capacity(6);
            let mut total = 0.0;
            for &nb in sphere.neighbors(cell) {
                let toward = (sphere.pos(nb) - sphere.pos(cell))
                    .try_normalize()
                    .unwrap_or(Vec3::ZERO);
                let align = toward.dot(dir);
                if align > 0.05 {
                    let w = align * align;
                    weights.push((nb as usize, w));
                    total += w;
                }
            }
            if total > 0.0 {
                for (nb, w) in weights {
                    next[nb] += out * w / total;
                }
            } else {
                // Convergent wind: moisture stays and rains here next pass.
                next[i] += out;
            }
        }

        // Evaporation from water bodies, plus a small lake/river source.
        for i in 0..n {
            let is_ocean = ocean.get(i).copied().unwrap_or(false);
            if is_ocean {
                let deficit = (capacity[i] * 1.05 - next[i]).max(0.0);
                next[i] += deficit * 0.35;
            } else {
                let upstream = evaporation_from_land(elevation, i);
                next[i] = (next[i] + upstream).min(capacity[i] * 1.1);
            }
            if !next[i].is_finite() {
                next[i] = 0.0;
            }
        }
        moisture = next;
    }

    // Explicit foehn drying: a cell downwind of a ridge that is much higher
    // than the cell itself receives much less rain. The advection model
    // captures windward lift, but without this term leeward recycling can hide
    // the rain shadow.
    for i in 0..n {
        if ocean.get(i).copied().unwrap_or(false) {
            continue;
        }
        let factor = rain_shadow_factor(sphere, elevation, winds, i as u32);
        precip[i] = (precip[i] * factor).clamp(0.0, 12_000.0);
    }

    // Normalize the land mean to the scenario target. The relative pattern is
    // physical; the absolute scale is a modelling choice.
    let mut land_sum = 0.0;
    let mut land_n = 0u64;
    for i in 0..n {
        if !ocean.get(i).copied().unwrap_or(false) {
            land_sum += precip[i];
            land_n += 1;
        }
    }
    let scale = if land_n > 0 && land_sum > 0.0 {
        (config.target_land_mm * land_n as f64 / land_sum).clamp(0.01, 5_000.0)
    } else {
        0.0
    };
    for p in &mut precip {
        *p = (*p * scale).clamp(0.0, 12_000.0);
        if !p.is_finite() {
            *p = 0.0;
        }
    }

    let mut humidity = vec![0.0f64; n];
    let mut aridity = vec![0.0f64; n];
    let mut cloud = vec![0.0f64; n];
    let mut pet = vec![0.0f64; n];
    for i in 0..n {
        let cap = capacity[i].max(1e-6);
        humidity[i] = (moisture[i] / cap).clamp(0.0, 1.0);
        let p_et = potential_et_mm(temp.mean_c[i]);
        pet[i] = p_et;
        aridity[i] = (precip[i] / p_et).clamp(0.01, 6.0);
        cloud[i] = (1.0 - (-precip[i] / 900.0).exp()) * 0.75 + humidity[i] * 0.25;
        cloud[i] = cloud[i].clamp(0.0, 1.0);
    }

    Moisture {
        precip_mm: precip,
        humidity,
        aridity,
        cloud,
        pet_mm: pet,
    }
}

/// Small moisture source from lakes and large rivers on land. Without this,
/// interiors downwind of dry regions would be unrealistically sterile.
fn evaporation_from_land(elevation: &[f64], i: usize) -> f64 {
    let e = elevation.get(i).copied().unwrap_or(0.0);
    if e > 0.0 && e < 5.0 {
        // Coastal lowland and wetlands recycle a little water.
        return 0.03;
    }
    0.0
}

/// Multiplicative rain-shadow factor for a cell, based on the highest terrain
/// along the upwind path. Returns 1.0 when there is no barrier.
pub fn rain_shadow_factor(sphere: &Sphere, elevation: &[f64], winds: &Winds, start: u32) -> f64 {
    let mut cell = start;
    let mut peak = elevation.get(start as usize).copied().unwrap_or(0.0);
    for _ in 0..5 {
        let dir = match winds.wind_dir.get(cell as usize) {
            Some(d) => *d,
            None => return 1.0,
        };
        let mut best = None;
        let mut best_dot = f64::INFINITY;
        for &nb in sphere.neighbors(cell) {
            let toward = (sphere.pos(nb) - sphere.pos(cell))
                .try_normalize()
                .unwrap_or(Vec3::ZERO);
            let dot = toward.dot(dir);
            if dot < best_dot {
                best_dot = dot;
                best = Some(nb);
            }
        }
        match best {
            Some(up) if up != cell => {
                cell = up;
                peak = peak.max(elevation.get(up as usize).copied().unwrap_or(0.0));
            }
            _ => break,
        }
    }
    let here = elevation.get(start as usize).copied().unwrap_or(0.0);
    let barrier = peak - here;
    if barrier > 700.0 {
        (0.28 - (barrier - 700.0) / 4_000.0).clamp(0.12, 0.45)
    } else if barrier > 300.0 {
        0.6
    } else {
        1.0
    }
}

/// Mean precipitation over land cells, in millimeters per year.
pub fn mean_land_precip(precip_mm: &[f64], ocean: &[bool]) -> f64 {
    let mut sum = 0.0;
    let mut n = 0u64;
    for (i, &p) in precip_mm.iter().enumerate() {
        if !ocean.get(i).copied().unwrap_or(false) && p.is_finite() && p > 0.0 {
            sum += p;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::{temperature, wind};
    use crate::tectonics::{self, TectonicsConfig};
    use crate::terrain::{self, TerrainConfig};

    fn climate(seed: u64, level: u32) -> (Sphere, Vec<f64>, Vec<bool>, Moisture) {
        let s = Sphere::new(level);
        let t = tectonics::generate(&s, seed, TectonicsConfig::default());
        let ter = terrain::build(&s, &t, seed, TerrainConfig::default());
        let ocean: Vec<bool> = ter.elevation.iter().map(|&e| e <= 0.0).collect();
        let w = wind::compute(&s, &ocean, seed);
        let temp = temperature::compute(&s, &ter.elevation, &ocean, &w.current_poleward, seed);
        let m = compute(
            &s,
            &ter.elevation,
            &ocean,
            &temp,
            &w,
            seed,
            MoistureConfig::default(),
        );
        (s, ter.elevation, ocean, m)
    }

    #[test]
    fn precipitation_is_non_negative_and_bounded() {
        let (s, _, _, m) = climate(3, 3);
        for i in 0..s.len() {
            assert!(m.precip_mm[i].is_finite());
            assert!((0.0..=12_000.0).contains(&m.precip_mm[i]));
            assert!((0.0..=1.0).contains(&m.humidity[i]));
            assert!((0.0..=1.0).contains(&m.cloud[i]));
            assert!(m.aridity[i] > 0.0);
        }
    }

    #[test]
    fn land_mean_matches_target() {
        let (_, _, ocean, m) = climate(5, 4);
        let mean = mean_land_precip(&m.precip_mm, &ocean);
        assert!((mean - 860.0).abs() < 5.0, "mean {mean}");
    }

    #[test]
    fn tropics_are_wetter_than_subtropics() {
        let (s, _, ocean, m) = climate(7, 4);
        let mut wet = (0.0, 0u32);
        let mut dry = (0.0, 0u32);
        for i in 0..s.len() {
            if ocean[i] {
                continue;
            }
            let lat = s.lat(i as u32).abs();
            if lat < 10.0 {
                wet.0 += m.precip_mm[i];
                wet.1 += 1;
            } else if (18.0..32.0).contains(&lat) {
                dry.0 += m.precip_mm[i];
                dry.1 += 1;
            }
        }
        assert!(wet.1 > 0 && dry.1 > 0);
        assert!(wet.0 / (wet.1 as f64) > dry.0 / (dry.1 as f64) * 1.4);
    }

    #[test]
    fn deserts_and_rainforests_exist() {
        let (_, _, ocean, m) = climate(9, 4);
        let min_land = (0..m.len())
            .filter(|&i| !ocean[i])
            .map(|i| m.precip_mm[i])
            .fold(f64::MAX, f64::min);
        let max_land = (0..m.len())
            .filter(|&i| !ocean[i])
            .map(|i| m.precip_mm[i])
            .fold(f64::MIN, f64::max);
        assert!(min_land < 400.0, "min {min_land}");
        assert!(max_land > 1_500.0, "max {max_land}");
    }

    #[test]
    fn mountains_create_a_rain_shadow() {
        // A synthetic north-south ridge in the southern westerlies.
        let s = Sphere::new(4);
        let mut elev = vec![-1_000.0f64; s.len()];
        for i in 0..s.len() {
            let lat = s.lat(i as u32);
            let lon = s.lon(i as u32);
            if (-45.0..=-35.0).contains(&lat) && lon.abs() < 30.0 {
                elev[i] = 100.0;
            }
            if (-45.0..=-35.0).contains(&lat) && (-4.0..=4.0).contains(&lon) {
                elev[i] = 2_800.0;
            }
        }
        let ocean: Vec<bool> = elev.iter().map(|&e| e <= 0.0).collect();
        let w = wind::compute(&s, &ocean, 11);
        let temp = temperature::compute(&s, &elev, &ocean, &w.current_poleward, 11);
        let m = compute(&s, &elev, &ocean, &temp, &w, 11, MoistureConfig::default());
        let mut windward = (0.0, 0u32);
        let mut leeward = (0.0, 0u32);
        for i in 0..s.len() {
            let lat = s.lat(i as u32);
            let lon = s.lon(i as u32);
            if !(-45.0..=-35.0).contains(&lat) || elev[i] <= 0.0 {
                continue;
            }
            // Sample the slopes immediately upwind and downwind of the ridge.
            if (-13.0..=-6.0).contains(&lon) {
                windward.0 += m.precip_mm[i];
                windward.1 += 1;
            } else if (6.0..=13.0).contains(&lon) {
                leeward.0 += m.precip_mm[i];
                leeward.1 += 1;
            }
        }
        assert!(windward.1 > 0 && leeward.1 > 0);
        assert!(
            windward.0 / (windward.1 as f64) > leeward.0 / (leeward.1 as f64) * 1.5,
            "windward {:?} leeward {:?}",
            windward.0 / (windward.1 as f64),
            leeward.0 / (leeward.1 as f64)
        );
    }

    #[test]
    fn capacity_increases_with_temperature() {
        assert!(capacity_mm(30.0) > capacity_mm(10.0));
        assert!(capacity_mm(10.0) > capacity_mm(-10.0));
        assert!(capacity_mm(-200.0).is_finite());
    }

    #[test]
    fn pet_increases_with_temperature() {
        assert!(potential_et_mm(30.0) > potential_et_mm(0.0));
        assert!(potential_et_mm(f64::NAN).is_finite());
    }

    #[test]
    fn deterministic() {
        let (_, _, _, a) = climate(13, 3);
        let (_, _, _, b) = climate(13, 3);
        assert_eq!(a.precip_mm, b.precip_mm);
        assert_eq!(a.humidity, b.humidity);
    }

    #[test]
    fn config_is_sanitized() {
        let c = MoistureConfig {
            passes: 0,
            advection: 9.0,
            target_land_mm: f64::NAN,
        }
        .sanitized();
        assert_eq!(c.passes, 4);
        assert!(c.advection <= 0.9);
        assert_eq!(c.target_land_mm, 860.0);
    }
}
