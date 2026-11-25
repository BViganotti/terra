//! Elevation and crust.
//!
//! Elevation is assembled from three explicit contributions:
//!
//! - a broad continental mask that decides whether a cell is buoyant crust or
//!   dense ocean floor,
//! - accumulated tectonic uplift and subsidence from [`crate::tectonics`],
//! - ridged noise that roughens mountain belts without creating isolated spikes.
//!
//! Sea level is then chosen to hit a scenario land fraction. This means the
//! coastline is a consequence of the crust, not a per-cell random decision, and
//! the land/ocean split is stable across seeds.

use crate::hash::Hasher64;
use crate::noise::{cell_jitter, ridged};
use crate::sphere::Sphere;
use crate::tectonics::Tectonics;

/// Elevation construction parameters.
#[derive(Clone, Copy, Debug)]
pub struct TerrainConfig {
    /// Desired fraction of cells above sea level, clamped to `0.05..=0.85`.
    pub land_fraction: f64,
    /// Vertical exaggeration of tectonic relief, clamped to `0.2..=2.5`.
    pub relief: f64,
    /// Amplitude of fine relief noise in meters, clamped to `0..=1200`.
    pub roughness: f64,
    /// Radius of the planet in kilometers, used by hydrology and climate.
    pub radius_km: f64,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            land_fraction: 0.31,
            relief: 1.0,
            roughness: 420.0,
            radius_km: 6_371.0,
        }
    }
}

impl TerrainConfig {
    /// Clamps every field into a supported range.
    pub fn sanitized(mut self) -> Self {
        self.land_fraction = if self.land_fraction.is_finite() {
            self.land_fraction.clamp(0.05, 0.85)
        } else {
            0.31
        };
        self.relief = if self.relief.is_finite() {
            self.relief.clamp(0.2, 2.5)
        } else {
            1.0
        };
        self.roughness = if self.roughness.is_finite() {
            self.roughness.clamp(0.0, 1_200.0)
        } else {
            420.0
        };
        self.radius_km = if self.radius_km.is_finite() {
            self.radius_km.clamp(1_000.0, 100_000.0)
        } else {
            6_371.0
        };
        self
    }
}

/// Elevation, crust and coastline description.
#[derive(Clone, Debug)]
pub struct Terrain {
    /// Elevation in meters relative to sea level. Positive is land.
    pub elevation: Vec<f64>,
    /// Elevation before sea level was subtracted, retained for tests and maps.
    pub raw_elevation: Vec<f64>,
    /// Sea level as an offset in the raw elevation frame, in meters.
    pub sea_level: f64,
    /// Realized land fraction.
    pub land_fraction: f64,
    /// Connected landmass label per cell; `u32::MAX` for ocean.
    pub landmass: Vec<u32>,
    /// Number of landmasses.
    pub landmass_count: u32,
    /// True for shallow water next to a coast.
    pub shelf: Vec<bool>,
    /// True for land cells adjacent to water.
    pub coastal: Vec<bool>,
    /// Crustal thickness proxy in kilometers.
    pub crust_km: Vec<f64>,
    /// Crustal age proxy in million years.
    pub crust_age_myr: Vec<f64>,
    /// Planet radius in kilometers.
    pub radius_km: f64,
}

impl Terrain {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.elevation.len()
    }

    /// True when the terrain is empty.
    pub fn is_empty(&self) -> bool {
        self.elevation.is_empty()
    }

    /// True when a cell is above sea level.
    pub fn is_land(&self, cell: u32) -> bool {
        self.elevation
            .get(cell as usize)
            .map(|&e| e > 0.0)
            .unwrap_or(false)
    }

    /// True when a cell is below sea level.
    pub fn is_ocean(&self, cell: u32) -> bool {
        !self.is_land(cell)
    }

    /// Elevation in meters, zero for out-of-range cells.
    pub fn elevation_at(&self, cell: u32) -> f64 {
        self.elevation.get(cell as usize).copied().unwrap_or(0.0)
    }

    /// Minimum and maximum elevation.
    pub fn range(&self) -> (f64, f64) {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for &e in &self.elevation {
            lo = lo.min(e);
            hi = hi.max(e);
        }
        if lo > hi {
            (0.0, 0.0)
        } else {
            (lo, hi)
        }
    }

    /// Mean elevation over land only.
    pub fn mean_land_elevation(&self) -> f64 {
        let mut sum = 0.0;
        let mut n = 0u64;
        for &e in &self.elevation {
            if e > 0.0 {
                sum += e;
                n += 1;
            }
        }
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    }

    /// Area of a landmass in steradians.
    pub fn landmass_area(&self, sphere: &Sphere, id: u32) -> f64 {
        let mut a = 0.0;
        for (i, &m) in self.landmass.iter().enumerate() {
            if m == id {
                a += sphere.cell_area(i as u32);
            }
        }
        a
    }

    /// Largest landmass id and its area, if any land exists.
    pub fn largest_landmass(&self, sphere: &Sphere) -> Option<(u32, f64)> {
        let mut best: Option<(u32, f64)> = None;
        for id in 0..self.landmass_count {
            let a = self.landmass_area(sphere, id);
            if best.map_or(true, |(_, b)| a > b) {
                best = Some((id, a));
            }
        }
        best
    }

    /// Folds the terrain into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("terrain");
        h.write_f64_slice(&self.elevation);
        h.write_f64(self.sea_level);
        h.write_u32_slice(&self.landmass);
    }
}

/// Builds the elevation field for a world.
pub fn build(sphere: &Sphere, tectonics: &Tectonics, seed: u64, config: TerrainConfig) -> Terrain {
    let config = config.sanitized();
    let n = sphere.len();
    let mut raw = vec![0.0f64; n];

    // Ridged relief is stronger where the crust is thick and where tectonic
    // uplift already exists, so mountain chains follow margins rather than
    // scattering across ocean basins.
    let max_uplift = tectonics
        .uplift
        .iter()
        .copied()
        .fold(0.0f64, f64::max)
        .max(1.0);
    for i in 0..n {
        let cell = i as u32;
        let pos = sphere.pos(cell);
        let c = tectonics.continental[i].clamp(-1.0, 1.0);
        let base = if c >= 0.0 {
            280.0 + 4_600.0 * c.powf(1.35)
        } else {
            -250.0 + 6_000.0 * c
        };
        let uplift = tectonics.uplift[i] * config.relief;
        let subsidence = tectonics.subsidence[i] * config.relief;
        let uplift_norm = (tectonics.uplift[i] / max_uplift).clamp(0.0, 1.0);
        let belt = ridged(seed ^ 0x00e1_1e00, pos, 5.5, 4);
        let roughness = (belt - 0.5) * config.roughness * (0.35 + 0.65 * uplift_norm);
        let detail = (cell_jitter(seed ^ 0x00d1_7a11, 3, cell) - 0.5) * 90.0;
        raw[i] = base + uplift - subsidence + roughness + detail;
        if !raw[i].is_finite() {
            raw[i] = 0.0;
        }
    }

    // Choose sea level as the quantile matching the requested land fraction.
    let sea_level = quantile(&raw, 1.0 - config.land_fraction);
    let mut elevation: Vec<f64> = raw.iter().map(|&e| e - sea_level).collect();
    // The quantile threshold guarantees at least one exact zero; nudge those
    // cells into the ocean so land is strictly positive.
    for e in &mut elevation {
        if e.abs() < 1e-9 {
            *e = -1e-6;
        }
    }

    let land_fraction = elevation.iter().filter(|&&e| e > 0.0).count() as f64 / n.max(1) as f64;
    let landmass = sphere.components(|c| elevation[c as usize] > 0.0);
    let mut landmass_count = 0u32;
    for &m in &landmass {
        if m != u32::MAX {
            landmass_count = landmass_count.max(m + 1);
        }
    }

    let mut shelf = vec![false; n];
    let mut coastal = vec![false; n];
    for i in 0..n {
        let cell = i as u32;
        let land = elevation[i] > 0.0;
        let touching_water = sphere
            .neighbors(cell)
            .iter()
            .any(|&nb| elevation[nb as usize] <= 0.0);
        let touching_land = sphere
            .neighbors(cell)
            .iter()
            .any(|&nb| elevation[nb as usize] > 0.0);
        if land {
            coastal[i] = touching_water;
        } else {
            shelf[i] = touching_land || elevation[i] > -220.0;
        }
    }

    // Crustal thickness is a smooth function of buoyancy plus orogenic
    // thickening; age grows away from spreading centers within each plate.
    let mut crust_km = vec![0.0f64; n];
    for i in 0..n {
        let c = tectonics.continental[i];
        let thick = if c >= 0.0 {
            26.0 + 18.0 * c
        } else {
            7.0 + 3.0 * (1.0 + c)
        };
        let orogen = tectonics.uplift[i] / 9_000.0 * 14.0;
        crust_km[i] = (thick + orogen).clamp(4.0, 72.0);
    }
    let age = crust_ages(sphere, tectonics, seed);

    Terrain {
        elevation,
        raw_elevation: raw,
        sea_level,
        land_fraction,
        landmass,
        landmass_count,
        shelf,
        coastal,
        crust_km,
        crust_age_myr: age,
        radius_km: config.radius_km,
    }
}

/// Crust age proxy: distance from the nearest divergent margin, scaled to
/// `0..=180` Myr with a per-plate offset. Cells far from any spreading center
/// are old; rift-adjacent cells are young.
fn crust_ages(sphere: &Sphere, tectonics: &Tectonics, seed: u64) -> Vec<f64> {
    let n = sphere.len();
    let mut rifts: Vec<u32> = Vec::new();
    for b in &tectonics.boundaries {
        if b.kind == crate::tectonics::BoundaryKind::Divergent {
            rifts.push(b.a);
            rifts.push(b.b);
        }
    }
    let dist = if rifts.is_empty() {
        vec![u32::MAX; n]
    } else {
        sphere.hop_distances(&rifts)
    };
    let mut offsets = vec![0.0f64; tectonics.plates.len()];
    for (i, o) in offsets.iter_mut().enumerate() {
        *o = cell_jitter(seed ^ 0x00a9_e000, 1, i as u32) * 35.0;
    }
    (0..n)
        .map(|i| {
            let d = dist[i];
            let base = if d == u32::MAX {
                140.0
            } else {
                (f64::from(d) * 7.5).min(165.0)
            };
            let plate = tectonics.plate_of[i] as usize;
            let off = offsets.get(plate).copied().unwrap_or(0.0);
            (base + off).clamp(0.0, 190.0)
        })
        .collect()
}

fn quantile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = q.clamp(0.0, 1.0);
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tectonics::{self, TectonicsConfig};

    fn world(seed: u64, level: u32) -> (Sphere, Tectonics, Terrain) {
        let s = Sphere::new(level);
        let t = tectonics::generate(&s, seed, TectonicsConfig::default());
        let ter = build(&s, &t, seed, TerrainConfig::default());
        (s, t, ter)
    }

    #[test]
    fn land_fraction_hits_target() {
        for seed in 0..4u64 {
            let (_, _, ter) = world(seed, 4);
            assert!(
                (ter.land_fraction - 0.31).abs() < 0.01,
                "seed {seed}: {}",
                ter.land_fraction
            );
        }
    }

    #[test]
    fn custom_land_fraction_is_respected() {
        let s = Sphere::new(3);
        let t = tectonics::generate(&s, 5, TectonicsConfig::default());
        for target in [0.1, 0.5, 0.75] {
            let ter = build(
                &s,
                &t,
                5,
                TerrainConfig {
                    land_fraction: target,
                    ..TerrainConfig::default()
                },
            );
            assert!((ter.land_fraction - target).abs() < 0.02, "{target}");
        }
    }

    #[test]
    fn elevation_is_bounded_and_finite() {
        let (_, _, ter) = world(3, 4);
        let (lo, hi) = ter.range();
        assert!(lo > -12_000.0 && lo < 0.0);
        assert!(hi > 1_000.0 && hi < 12_000.0);
        assert!(ter.elevation.iter().all(|e| e.is_finite()));
    }

    #[test]
    fn mountains_follow_uplift() {
        let (s, t, ter) = world(17, 4);
        // Uplift is a primary control on elevation; the Pearson correlation
        // over every cell must be clearly positive.
        let n = s.len() as f64;
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sxx = 0.0;
        let mut syy = 0.0;
        let mut sxy = 0.0;
        for c in 0..s.len() {
            let x = t.uplift[c];
            let y = ter.elevation[c];
            sx += x;
            sy += y;
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
        }
        let cov = sxy / n - (sx / n) * (sy / n);
        let vx = sxx / n - (sx / n).powi(2);
        let vy = syy / n - (sy / n).powi(2);
        let r = cov / (vx.sqrt() * vy.sqrt()).max(1e-9);
        assert!(r > 0.35, "uplift/elevation correlation {r}");

        // And the highest uplift cells are, on average, far above the lowest.
        let mut pairs: Vec<(f64, f64)> = (0..s.len())
            .map(|c| (t.uplift[c], ter.elevation[c]))
            .collect();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let third = pairs.len() / 3;
        let low: f64 = pairs[..third].iter().map(|p| p.1).sum::<f64>() / third as f64;
        let high: f64 =
            pairs[2 * third..].iter().map(|p| p.1).sum::<f64>() / (pairs.len() - 2 * third) as f64;
        assert!(high > low + 300.0, "low {low} high {high}");
    }

    #[test]
    fn landmasses_are_contiguous() {
        let (s, ter) = {
            let (s, _, ter) = world(6, 3);
            (s, ter)
        };
        assert!(ter.landmass_count > 0);
        for id in 0..ter.landmass_count {
            let cells: Vec<u32> = (0..s.len() as u32)
                .filter(|&c| ter.landmass[c as usize] == id)
                .collect();
            assert!(!cells.is_empty());
            for &c in &cells[1..] {
                assert!(s.neighbors(c).iter().any(|n| cells.contains(n)));
            }
        }
        assert_eq!(
            ter.landmass.iter().filter(|&&m| m != u32::MAX).count(),
            ter.elevation.iter().filter(|&&e| e > 0.0).count()
        );
    }

    #[test]
    fn coasts_have_shelves() {
        let (_, _, ter) = world(8, 3);
        let mut coast_cells = 0;
        for i in 0..ter.len() {
            if ter.coastal[i] {
                coast_cells += 1;
                assert!(ter.elevation[i] > 0.0);
            }
            if ter.shelf[i] {
                assert!(ter.elevation[i] <= 0.0 || ter.elevation[i] < 220.0);
            }
        }
        assert!(coast_cells > 0);
        assert!(ter.shelf.iter().any(|&s| s));
    }

    #[test]
    fn crust_is_thicker_under_continents() {
        let (_, _, ter) = world(10, 3);
        let mut land = (0.0, 0u64);
        let mut sea = (0.0, 0u64);
        for i in 0..ter.len() {
            if ter.elevation[i] > 0.0 {
                land.0 += ter.crust_km[i];
                land.1 += 1;
            } else {
                sea.0 += ter.crust_km[i];
                sea.1 += 1;
            }
        }
        assert!(land.0 / land.1 as f64 > sea.0 / sea.1 as f64 + 10.0);
    }

    #[test]
    fn terrain_is_deterministic() {
        let (_, _, a) = world(33, 3);
        let (_, _, b) = world(33, 3);
        for (x, y) in a.elevation.iter().zip(&b.elevation) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
        assert_eq!(a.landmass, b.landmass);
    }

    #[test]
    fn hash_changes_with_seed() {
        use crate::hash::Hasher64;
        let (_, _, a) = world(1, 2);
        let (_, _, b) = world(2, 2);
        let mut ha = Hasher64::new();
        let mut hb = Hasher64::new();
        a.hash_into(&mut ha);
        b.hash_into(&mut hb);
        assert_ne!(ha.finish(), hb.finish());
    }

    #[test]
    fn quantile_bounds() {
        assert_eq!(quantile(&[], 0.5), 0.0);
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(quantile(&v, 0.0), 1.0);
        assert_eq!(quantile(&v, 1.0), 5.0);
        assert_eq!(quantile(&v, 0.5), 3.0);
    }

    #[test]
    fn config_sanitizes() {
        let c = TerrainConfig {
            land_fraction: 9.0,
            relief: f64::NAN,
            roughness: -5.0,
            radius_km: 0.0,
        }
        .sanitized();
        assert!(c.land_fraction <= 0.85 && c.land_fraction >= 0.05);
        assert_eq!(c.relief, 1.0);
        assert_eq!(c.roughness, 0.0);
        assert!(c.radius_km >= 1_000.0);
    }
}
