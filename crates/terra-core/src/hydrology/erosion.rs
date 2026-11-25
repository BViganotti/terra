//! Erosion and sediment transport.
//!
//! Two processes act on the terrain:
//!
//! - **Stream power**: channel incision proportional to drainage area to the
//!   power `m` and slope to the power `n`. Eroded rock becomes sediment.
//! - **Hillslope diffusion**: mass-conserving pairwise smoothing that removes
//!   unrealistically sharp spikes and represents creep and mass wasting.
//!
//! Sediment travels downstream with the flow network. It is deposited where
//! gradients are gentle and at river mouths, where repeated deposition builds
//! deltas. Every cubic meter is accounted for: the total eroded volume equals
//! the volume deposited on land and in the sea plus the volume exported beyond
//! the shelf. The mass-balance test enforces this.

use crate::hydrology::fill::{priority_flood, DEFAULT_EPSILON};
use crate::hydrology::flow::{self, Drainage};
use crate::hydrology::lakes::{self, Lakes};
use crate::sphere::Sphere;

/// Erosion parameters.
#[derive(Clone, Copy, Debug)]
pub struct ErosionConfig {
    /// Number of coupled fill/erode/transport passes, clamped to `1..=64`.
    pub iterations: u32,
    /// Stream-power coefficient.
    pub stream_power: f64,
    /// Drainage-area exponent.
    pub area_exponent: f64,
    /// Slope exponent.
    pub slope_exponent: f64,
    /// Maximum fraction of the local drop removed in one pass.
    pub max_drop_fraction: f64,
    /// Hillslope diffusion coefficient per pass.
    pub diffusion: f64,
    /// Fraction of arriving sediment deposited on gentle reaches.
    pub deposition: f64,
    /// Maximum delta growth per pass in meters.
    pub delta_growth_m: f64,
}

impl Default for ErosionConfig {
    fn default() -> Self {
        Self {
            iterations: 14,
            stream_power: 9e-5,
            area_exponent: 0.5,
            slope_exponent: 1.0,
            max_drop_fraction: 0.3,
            diffusion: 0.012,
            deposition: 0.32,
            delta_growth_m: 4.0,
        }
    }
}

impl ErosionConfig {
    /// Clamps parameters into physically sane, bounded ranges.
    pub fn sanitized(mut self) -> Self {
        self.iterations = self.iterations.clamp(1, 64);
        let finite = |v: f64, lo: f64, hi: f64, fallback: f64| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                fallback
            }
        };
        self.stream_power = finite(self.stream_power, 0.0, 0.01, 9e-5);
        self.area_exponent = finite(self.area_exponent, 0.0, 1.5, 0.5);
        self.slope_exponent = finite(self.slope_exponent, 0.0, 3.0, 1.0);
        self.max_drop_fraction = finite(self.max_drop_fraction, 0.0, 0.9, 0.3);
        self.diffusion = finite(self.diffusion, 0.0, 0.2, 0.012);
        self.deposition = finite(self.deposition, 0.0, 1.0, 0.32);
        self.delta_growth_m = finite(self.delta_growth_m, 0.0, 30.0, 4.0);
        self
    }
}

/// Result of an erosion run.
#[derive(Clone, Debug)]
pub struct Erosion {
    /// Eroded and deposited elevation in meters.
    pub elevation: Vec<f64>,
    /// Cumulative sediment volume that passed through each cell, in cubic
    /// meters. Useful for map shading and as a check against flow.
    pub sediment_flux: Vec<f64>,
    /// Cumulative deposited volume per cell, in cubic meters.
    pub deposition: Vec<f64>,
    /// Cells where deposition built a delta at a river mouth.
    pub delta: Vec<bool>,
    /// Gross eroded volume in cubic meters, including reworked sediment.
    pub eroded_m3: f64,
    /// Gross deposited volume in cubic meters, including re-eroded deposits.
    pub deposited_m3: f64,
    /// Total volume carried past the shelf and lost to the deep ocean.
    pub exported_m3: f64,
    /// Rock volume of the initial surface in cubic meters.
    pub initial_m3: f64,
    /// Rock volume of the final surface in cubic meters.
    pub final_m3: f64,
    /// Passes actually performed.
    pub iterations: u32,
}

impl Erosion {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.elevation.len()
    }

    /// True when no cells exist.
    pub fn is_empty(&self) -> bool {
        self.elevation.is_empty()
    }

    /// Fraction of eroded material that was deposited rather than exported.
    /// Returns zero when nothing was eroded.
    pub fn retention(&self) -> f64 {
        if self.eroded_m3 > 0.0 {
            self.deposited_m3 / self.eroded_m3
        } else {
            0.0
        }
    }

    /// Mass-balance residual in cubic meters. Rock can leave the system only
    /// through the deep-ocean export, so the net volume lost must equal it.
    pub fn residual_m3(&self) -> f64 {
        (self.initial_m3 - self.final_m3) - self.exported_m3
    }
}

/// Runs the erosion and transport model.
pub fn erode(
    sphere: &Sphere,
    initial_elevation: &[f64],
    radius_km: f64,
    config: ErosionConfig,
) -> Erosion {
    let config = config.sanitized();
    let n = sphere.len().min(initial_elevation.len());
    let radius_m = if radius_km.is_finite() && radius_km > 0.0 {
        radius_km * 1_000.0
    } else {
        6_371_000.0
    };
    let areas: Vec<f64> = (0..n)
        .map(|i| (sphere.cell_area(i as u32) * radius_m * radius_m).max(1.0))
        .collect();

    let mut elevation: Vec<f64> = initial_elevation[..n].to_vec();
    let initial_m3: f64 = elevation
        .iter()
        .zip(&areas)
        .filter(|(e, _)| e.is_finite())
        .map(|(e, a)| e * a)
        .sum();
    let mut sediment_flux = vec![0.0f64; n];
    let mut deposition = vec![0.0f64; n];
    let mut delta = vec![false; n];
    let mut eroded_m3 = 0.0;
    let mut deposited_m3 = 0.0;
    let mut exported_m3 = 0.0;

    for _ in 0..config.iterations {
        let filled = priority_flood(sphere, &elevation, DEFAULT_EPSILON);
        let drainage = flow::build(sphere, &elevation, &filled.level, &filled.order, &areas);
        let lake_map = lakes::extract(sphere, &elevation, &filled.level, radius_km);
        let mut flux = vec![0.0f64; n];
        let mut dropped = 0.0;
        let mut added = 0.0;
        let mut lost = 0.0;

        // Incision. Process cells independently: sediment routing happens in a
        // second pass so incision does not depend on visit order.
        for i in 0..n {
            let cell = i as u32;
            if elevation[i] <= 0.0 || lake_map.is_lake(cell) {
                continue;
            }
            let Some(ds) = drainage.downhill[i] else {
                continue;
            };
            let dsi = ds as usize;
            let drop = filled.level[i] - filled.level[dsi];
            if drop <= 0.0 {
                continue;
            }
            let distance = (sphere.arc(cell, ds) as f64 * radius_m).max(1.0);
            let slope = (drop / distance).max(0.0);
            let area = drainage.accumulation[i].max(areas[i]);
            let rate = config.stream_power
                * area.powf(config.area_exponent)
                * slope.powf(config.slope_exponent);
            let local_drop = (elevation[i] - elevation[dsi]).max(0.0);
            let limit = config.max_drop_fraction * local_drop;
            let remove = rate.min(limit).max(0.0);
            if !remove.is_finite() || remove <= 0.0 {
                continue;
            }
            elevation[i] -= remove;
            let volume = remove * areas[i];
            flux[i] += volume;
            dropped += volume;
        }

        // Sediment routing, upstream first (descending filled surface).
        for &cell in filled.order.iter().rev() {
            let i = cell as usize;
            if i >= n {
                continue;
            }
            let load = flux[i];
            if load <= 0.0 {
                continue;
            }
            sediment_flux[i] += load;
            let Some(ds) = drainage.downhill[i] else {
                lost += load;
                flux[i] = 0.0;
                continue;
            };
            let dsi = ds as usize;
            let distance = (sphere.arc(cell, ds) as f64 * radius_m).max(1.0);
            let slope = ((filled.level[i] - filled.level[dsi]) / distance).max(0.0);
            let target_ocean = elevation[dsi] <= 0.0;
            let mut keep = if target_ocean {
                load * (1.0 - config.deposition * 0.5)
            } else {
                let gentle = (1.0 - (slope / 0.04).min(1.0)).max(0.0);
                load * config.deposition * gentle
            };
            if lake_map.is_lake(ds) {
                // Lakes trap sediment.
                keep = load * 0.85;
            }
            keep = keep.clamp(0.0, load);
            if keep > 0.0 {
                let rise = keep / areas[dsi];
                let cap = if target_ocean {
                    // Deltas may emerge, but only a few meters at a time, and
                    // never in deep water.
                    if elevation[dsi] > -120.0 {
                        config.delta_growth_m
                    } else {
                        0.0
                    }
                } else {
                    6.0
                };
                let applied_rise = rise.min(cap);
                let applied = applied_rise * areas[dsi];
                if applied > 0.0 {
                    elevation[dsi] += applied_rise;
                    deposition[dsi] += applied;
                    added += applied;
                    if target_ocean && applied_rise > 0.0 {
                        delta[dsi] = true;
                    }
                }
                let remainder = load - applied;
                if remainder > 0.0 {
                    flux[dsi] += remainder;
                }
                flux[i] = 0.0;
            } else {
                flux[dsi] += load;
                flux[i] = 0.0;
            }
        }

        // Anything still in transit this pass is exported; the loop above
        // empties every load into a downstream cell or the sea.
        for &l in &flux {
            lost += l;
        }

        diffuse(sphere, &mut elevation, &areas, config.diffusion);
        eroded_m3 += dropped;
        deposited_m3 += added;
        exported_m3 += lost;
    }
    // Delta status is a property of the final surface: depositional features
    // that remain at the coast. Deep-water fans are not deltas.
    for i in 0..n {
        let cell = i as u32;
        let near_land = elevation[i] > 0.0
            || sphere
                .neighbors(cell)
                .iter()
                .any(|&nb| elevation.get(nb as usize).is_some_and(|&e| e > 0.0));
        delta[i] = deposition[i] > 0.0 && elevation[i] > -150.0 && near_land;
    }
    let final_m3: f64 = elevation.iter().zip(&areas).map(|(e, a)| e * a).sum();

    Erosion {
        elevation,
        sediment_flux,
        deposition,
        delta,
        eroded_m3,
        deposited_m3,
        exported_m3,
        initial_m3,
        final_m3,
        iterations: config.iterations,
    }
}

/// Mass-conserving pairwise hillslope diffusion.
///
/// Each undirected edge exchanges a volume proportional to the elevation
/// difference and the mean area. Volumes, not elevations, are moved, so a cell
/// with a large area changes elevation less than a small one. The exchange is
/// capped so the lower cell cannot be pushed above the higher one in one pass.
pub fn diffuse(sphere: &Sphere, elevation: &mut [f64], areas: &[f64], coefficient: f64) {
    let n = sphere.len().min(elevation.len()).min(areas.len());
    if !(coefficient.is_finite() && coefficient > 0.0) || n < 2 {
        return;
    }
    let k = coefficient.clamp(0.0, 0.2);
    let mut change = vec![0.0f64; n];
    for a in 0..n as u32 {
        for &b in sphere.neighbors(a) {
            if b <= a || b as usize >= n {
                continue;
            }
            let ai = a as usize;
            let bi = b as usize;
            let diff = elevation[ai] - elevation[bi];
            if diff.abs() < 1e-9 {
                continue;
            }
            let mean_area = (areas[ai] + areas[bi]) * 0.5;
            let capacity = diff.abs() * mean_area * 0.25;
            let volume = (k * diff * mean_area).clamp(-capacity, capacity);
            change[ai] -= volume;
            change[bi] += volume;
        }
    }
    for i in 0..n {
        if elevation[i].is_finite() {
            elevation[i] += change[i] / areas[i].max(1.0);
        }
    }
}

/// Recomputes the final drainage network for an eroded surface.
pub fn final_network(
    sphere: &Sphere,
    elevation: &[f64],
    weights: &[f64],
) -> (Vec<f64>, Drainage, Lakes) {
    let filled = priority_flood(sphere, elevation, DEFAULT_EPSILON);
    let drainage = flow::build(sphere, elevation, &filled.level, &filled.order, weights);
    let lake_map = lakes::extract(sphere, elevation, &filled.level, 6_371.0);
    (filled.level, drainage, lake_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tectonics::{self, TectonicsConfig};
    use crate::terrain::{self, TerrainConfig};

    fn world(seed: u64) -> (Sphere, Vec<f64>) {
        let s = Sphere::new(4);
        let t = tectonics::generate(&s, seed, TectonicsConfig::default());
        let ter = terrain::build(&s, &t, seed, TerrainConfig::default());
        (s, ter.elevation)
    }

    #[test]
    fn mass_is_conserved() {
        let (s, e) = world(5);
        let out = erode(&s, &e, 6_371.0, ErosionConfig::default());
        assert!(out.eroded_m3 > 0.0);
        assert!(out.deposited_m3 > 0.0);
        let residual = out.residual_m3().abs();
        let net = (out.initial_m3 - out.final_m3).abs().max(1.0);
        assert!(residual / net < 0.01, "residual {residual} of net {net}");
        assert!(out.exported_m3 > 0.0);
        assert!(out.retention() >= 0.0 && out.retention() <= 1.0);
    }

    #[test]
    fn erosion_lowers_peaks() {
        let (s, e) = world(7);
        let out = erode(&s, &e, 6_371.0, ErosionConfig::default());
        let before_max = e.iter().copied().fold(f64::MIN, f64::max);
        let after_max = out.elevation.iter().copied().fold(f64::MIN, f64::max);
        assert!(after_max < before_max);
        assert!(out.elevation.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn stream_power_never_inverts_a_slope() {
        let (s, e) = world(9);
        let out = erode(&s, &e, 6_371.0, ErosionConfig::default());
        for i in 0..s.len() {
            assert!(out.elevation[i] >= -12_000.0 && out.elevation[i] <= 12_000.0);
        }
    }

    #[test]
    fn deltas_form() {
        let (s, e) = world(11);
        let out = erode(&s, &e, 6_371.0, ErosionConfig::default());
        assert!(out.delta.iter().any(|&d| d));
        for (i, &d) in out.delta.iter().enumerate() {
            if d {
                assert!(out.elevation[i] > -150.0);
                assert!(
                    out.elevation[i] > 0.0
                        || s.neighbors(i as u32)
                            .iter()
                            .any(|&nb| out.elevation[nb as usize] > 0.0)
                );
            }
        }
    }

    #[test]
    fn diffusion_conserves_volume() {
        let s = Sphere::new(3);
        let mut e: Vec<f64> = (0..s.len())
            .map(|i| ((i as f64 * 0.37).sin() * 900.0) + 200.0)
            .collect();
        let areas: Vec<f64> = (0..s.len())
            .map(|i| s.cell_area(i as u32) * 6_371_000.0 * 6_371_000.0)
            .collect();
        let before: f64 = e.iter().zip(&areas).map(|(x, a)| x * a).sum();
        diffuse(&s, &mut e, &areas, 0.05);
        let after: f64 = e.iter().zip(&areas).map(|(x, a)| x * a).sum();
        assert!((after - before).abs() / before.abs() < 1e-12);
    }

    #[test]
    fn diffusion_smooths() {
        let s = Sphere::new(3);
        let mut e: Vec<f64> = (0..s.len())
            .map(|i| if i % 2 == 0 { 1_000.0 } else { 0.0 })
            .collect();
        let areas = vec![1.0; s.len()];
        let roughness = |v: &[f64]| -> f64 {
            let mut sum = 0.0;
            for (i, &x) in v.iter().enumerate() {
                for &nb in s.neighbors(i as u32) {
                    sum += (x - v[nb as usize]).abs();
                }
            }
            sum
        };
        let before = roughness(&e);
        diffuse(&s, &mut e, &areas, 0.08);
        let after = roughness(&e);
        assert!(after < before);
    }

    #[test]
    fn diffusion_ignores_bad_coefficients() {
        let s = Sphere::new(2);
        let mut e = vec![10.0; s.len()];
        let areas = vec![1.0; s.len()];
        diffuse(&s, &mut e, &areas, f64::NAN);
        diffuse(&s, &mut e, &areas, -1.0);
        diffuse(&s, &mut e, &areas, 0.0);
        assert!(e.iter().all(|&v| v == 10.0));
    }

    #[test]
    fn deterministic() {
        let (s, e) = world(13);
        let a = erode(&s, &e, 6_371.0, ErosionConfig::default());
        let b = erode(&s, &e, 6_371.0, ErosionConfig::default());
        for (x, y) in a.elevation.iter().zip(&b.elevation) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
        assert_eq!(a.eroded_m3.to_bits(), b.eroded_m3.to_bits());
        assert_eq!(a.delta, b.delta);
    }

    #[test]
    fn config_is_sanitized() {
        let c = ErosionConfig {
            iterations: 1000,
            stream_power: f64::NAN,
            area_exponent: -5.0,
            slope_exponent: 99.0,
            max_drop_fraction: 5.0,
            diffusion: -1.0,
            deposition: 3.0,
            delta_growth_m: f64::INFINITY,
        }
        .sanitized();
        assert!(c.iterations <= 64);
        assert!(c.stream_power.is_finite());
        assert!((0.0..=1.5).contains(&c.area_exponent));
        assert!((0.0..=1.0).contains(&c.deposition));
    }

    #[test]
    fn final_network_is_downhill() {
        let (s, e) = world(15);
        let out = erode(&s, &e, 6_371.0, ErosionConfig::default());
        let weights = vec![1.0; s.len()];
        let (level, d, _) = final_network(&s, &out.elevation, &weights);
        for (i, ds) in d.downhill.iter().enumerate() {
            if let Some(ds) = ds {
                assert!(level[*ds as usize] < level[i]);
            }
        }
    }
}
