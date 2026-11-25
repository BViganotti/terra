//! Hydrology: depression filling, drainage networks, lakes, erosion and
//! sediment transport.
//!
//! The pipeline is iterative because erosion changes the surface that defines
//! drainage. Each pass fills depressions, computes flow, erodes channels,
//! transports sediment and deposits it in low-gradient reaches and at river
//! mouths. All bookkeeping is in cubic meters so sediment mass can be checked.
//!
//! [`Hydrology::build`] runs the whole system. [`Hydrology::apply_precipitation`]
//! later replaces the uniform runoff assumption with the climate field, which is
//! what the Studio reports as discharge.

pub mod erosion;
pub mod fill;
pub mod flow;
pub mod lakes;

pub use erosion::{erode, Erosion, ErosionConfig};
pub use fill::{priority_flood, Filled};
pub use flow::Drainage;
pub use lakes::{Lake, Lakes};

use crate::hash::Hasher64;
use crate::sphere::Sphere;

/// Seconds in a Julian year, used to convert runoff depth to discharge.
pub const SECONDS_PER_YEAR: f64 = 31_557_600.0;

/// Full hydrology configuration.
#[derive(Clone, Copy, Debug)]
pub struct HydrologyConfig {
    /// Erosion parameters.
    pub erosion: ErosionConfig,
    /// Uniform runoff used before climate exists, in meters per year.
    pub runoff_m_per_year: f64,
}

impl Default for HydrologyConfig {
    fn default() -> Self {
        Self {
            erosion: ErosionConfig::default(),
            runoff_m_per_year: 0.35,
        }
    }
}

impl HydrologyConfig {
    /// Clamps runoff into a plausible range.
    pub fn sanitized(mut self) -> Self {
        self.runoff_m_per_year = if self.runoff_m_per_year.is_finite() {
            self.runoff_m_per_year.clamp(0.0, 5.0)
        } else {
            0.35
        };
        self.erosion = self.erosion.sanitized();
        self
    }
}

/// The hydrological state of a world.
#[derive(Clone, Debug)]
pub struct Hydrology {
    /// Elevation after erosion in meters.
    pub elevation: Vec<f64>,
    /// Depression-filled water surface in meters.
    pub filled: Vec<f64>,
    /// Downstream cell per cell.
    pub downhill: Vec<Option<u32>>,
    /// Upstream drainage area in square meters.
    pub drainage_area_m2: Vec<f64>,
    /// Discharge in cubic meters per second.
    pub discharge_m3s: Vec<f64>,
    /// Strahler stream order.
    pub strahler: Vec<u8>,
    /// True when flow reaches the ocean.
    pub ocean_reach: Vec<bool>,
    /// River mouths emptying into the sea.
    pub outlets: Vec<u32>,
    /// Lakes.
    pub lakes: Lakes,
    /// Delta cells at river mouths.
    pub delta: Vec<bool>,
    /// Cumulative sediment volume that passed each cell, in cubic meters.
    pub sediment_flux: Vec<f64>,
    /// Gross eroded volume in cubic meters.
    pub eroded_m3: f64,
    /// Gross deposited volume in cubic meters.
    pub deposited_m3: f64,
    /// Volume exported to the deep ocean in cubic meters.
    pub exported_m3: f64,
    /// Mass-balance residual; zero within floating point error.
    pub residual_m3: f64,
    /// Uniform runoff used before climate, in meters per year.
    pub runoff_m_per_year: f64,
    /// Planet radius in kilometers.
    pub radius_km: f64,
    /// Number of erosion passes.
    pub iterations: u32,
}

impl Hydrology {
    /// Runs erosion and produces the final drainage network.
    pub fn build(
        sphere: &Sphere,
        elevation: &[f64],
        radius_km: f64,
        config: HydrologyConfig,
    ) -> Self {
        let config = config.sanitized();
        let radius_km = if radius_km.is_finite() && radius_km > 0.0 {
            radius_km
        } else {
            6_371.0
        };
        let radius_m = radius_km * 1_000.0;
        let erosion = erode(sphere, elevation, radius_km, config.erosion);
        let n = sphere.len().min(erosion.elevation.len());
        let areas: Vec<f64> = (0..n)
            .map(|i| (sphere.cell_area(i as u32) * radius_m * radius_m).max(1.0))
            .collect();
        let filled = priority_flood(sphere, &erosion.elevation, fill::DEFAULT_EPSILON);
        let drainage = flow::build(
            sphere,
            &erosion.elevation,
            &filled.level,
            &filled.order,
            &areas,
        );
        let lake_map = lakes::extract(sphere, &erosion.elevation, &filled.level, radius_km);
        let discharge = runoff_to_discharge(&drainage.accumulation, config.runoff_m_per_year);
        let residual_m3 = erosion.residual_m3();
        Self {
            elevation: erosion.elevation,
            filled: filled.level,
            downhill: drainage.downhill,
            drainage_area_m2: drainage.accumulation,
            discharge_m3s: discharge,
            strahler: drainage.strahler,
            ocean_reach: drainage.ocean_reach,
            outlets: drainage.outlets,
            lakes: lake_map,
            delta: erosion.delta,
            sediment_flux: erosion.sediment_flux,
            eroded_m3: erosion.eroded_m3,
            deposited_m3: erosion.deposited_m3,
            exported_m3: erosion.exported_m3,
            residual_m3,
            runoff_m_per_year: config.runoff_m_per_year,
            radius_km,
            iterations: erosion.iterations,
        }
    }

    /// Replaces uniform runoff with a precipitation field in millimeters per
    /// year. Discharge becomes `drainage area * precipitation`, which is the
    /// standard water-balance approximation.
    pub fn apply_precipitation(&mut self, sphere: &Sphere, precip_mm: &[f64]) {
        let n = self.elevation.len().min(precip_mm.len());
        let radius_m = self.radius_km * 1_000.0;
        let mut weights = vec![0.0f64; n];
        for i in 0..n {
            let p = precip_mm[i];
            if p.is_finite() && p > 0.0 {
                let area = (sphere.cell_area(i as u32) * radius_m * radius_m).max(1.0);
                weights[i] = area * (p / 1_000.0);
            }
        }
        let filled = priority_flood(sphere, &self.elevation, fill::DEFAULT_EPSILON);
        let drainage = flow::build(
            sphere,
            &self.elevation,
            &self.filled,
            &filled.order,
            &weights,
        );
        self.drainage_area_m2 = drainage
            .accumulation
            .iter()
            .zip(&weights)
            .map(|(&a, &w)| if w > 0.0 { a } else { 0.0 })
            .collect();
        self.discharge_m3s = self
            .drainage_area_m2
            .iter()
            .map(|&a| a / SECONDS_PER_YEAR)
            .collect();
        self.strahler = drainage.strahler;
        self.ocean_reach = drainage.ocean_reach;
        self.outlets = drainage.outlets;
        self.downhill = drainage.downhill;
        self.runoff_m_per_year = precipitation_mean(precip_mm);
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.elevation.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.elevation.is_empty()
    }

    /// True when a cell holds lake water.
    pub fn is_lake(&self, cell: u32) -> bool {
        self.lakes.is_lake(cell)
    }

    /// True when a cell is a channel above the given drainage area threshold in
    /// square kilometers.
    pub fn is_channel_km2(&self, cell: u32, min_km2: f64) -> bool {
        self.drainage_area_m2
            .get(cell as usize)
            .is_some_and(|&a| a >= min_km2 * 1_000_000.0)
    }

    /// Largest discharge in cubic meters per second.
    pub fn max_discharge(&self) -> f64 {
        self.discharge_m3s.iter().copied().fold(0.0, f64::max)
    }

    /// Follows flow from a cell.
    pub fn path(&self, start: u32, max_steps: usize) -> Vec<u32> {
        let mut out = Vec::with_capacity(max_steps.min(64));
        let mut cur = start;
        for _ in 0..max_steps {
            out.push(cur);
            match self.downhill.get(cur as usize).copied().flatten() {
                Some(next) if next != cur => cur = next,
                _ => break,
            }
        }
        out
    }

    /// Total land area in square kilometers.
    pub fn land_area_km2(&self, sphere: &Sphere) -> f64 {
        let radius_m = self.radius_km * 1_000.0;
        let mut sum = 0.0;
        for i in 0..self.len() {
            if self.elevation[i] > 0.0 {
                sum += sphere.cell_area(i as u32) * radius_m * radius_m;
            }
        }
        sum / 1_000_000.0
    }

    /// Water balance check: discharge leaving all river mouths should equal
    /// runoff over the land area that drains to the ocean, within rounding.
    pub fn outlet_discharge(&self) -> f64 {
        self.outlets
            .iter()
            .map(|&c| self.discharge_m3s.get(c as usize).copied().unwrap_or(0.0))
            .sum()
    }

    /// Folds the hydrology state into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("hydrology");
        h.write_f64_slice(&self.elevation);
        h.write_f64_slice(&self.filled);
        h.write_u64(self.lakes.len() as u64);
        for lake in &self.lakes.lakes {
            h.write_u32_slice(&lake.cells);
            h.write_f64(lake.surface);
        }
    }
}

fn runoff_to_discharge(accumulation: &[f64], runoff_m_per_year: f64) -> Vec<f64> {
    let runoff = if runoff_m_per_year.is_finite() {
        runoff_m_per_year.max(0.0)
    } else {
        0.0
    };
    accumulation
        .iter()
        .map(|&a| {
            let q = a * runoff / SECONDS_PER_YEAR;
            if q.is_finite() && q > 0.0 {
                q
            } else {
                0.0
            }
        })
        .collect()
}

fn precipitation_mean(precip: &[f64]) -> f64 {
    let mut sum = 0.0;
    let mut n = 0u64;
    for &p in precip {
        if p.is_finite() && p > 0.0 {
            sum += p;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64 / 1_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tectonics::{self, TectonicsConfig};
    use crate::terrain::{self, TerrainConfig};

    fn hydrology(seed: u64, level: u32) -> (Sphere, Hydrology) {
        let s = Sphere::new(level);
        let t = tectonics::generate(&s, seed, TectonicsConfig::default());
        let ter = terrain::build(&s, &t, seed, TerrainConfig::default());
        let h = Hydrology::build(&s, &ter.elevation, 6_371.0, HydrologyConfig::default());
        (s, h)
    }

    #[test]
    fn every_land_cell_has_a_downstream_or_is_a_lake() {
        let (s, h) = hydrology(3, 4);
        for i in 0..s.len() {
            if h.elevation[i] > 0.0 {
                assert!(
                    h.downhill[i].is_some() || h.lakes.is_lake(i as u32),
                    "cell {i} has no route"
                );
            }
        }
    }

    #[test]
    fn rivers_never_flow_uphill() {
        let (_, h) = hydrology(5, 4);
        for i in 0..h.len() {
            if let Some(ds) = h.downhill[i] {
                let dsi = ds as usize;
                assert!(h.filled[dsi] <= h.filled[i] + 1e-9);
                if !h.lakes.is_lake(i as u32) && !h.lakes.is_lake(ds) && h.elevation[i] > 0.0 {
                    assert!(h.elevation[dsi] < h.elevation[i] + 1e-9);
                }
            }
        }
    }

    #[test]
    fn lakes_are_basins_of_the_final_surface() {
        let (s, h) = hydrology(7, 4);
        for lake in &h.lakes.lakes {
            assert!(!lake.cells.is_empty());
            for &c in &lake.cells {
                assert!(h.lakes.is_lake(c));
                assert!(h.elevation[c as usize] <= lake.surface + 0.01);
            }
            if let (Some(sc), Some(st)) = (lake.spill_cell, lake.spill_target) {
                assert!(s.neighbors(sc).contains(&st));
                assert!(h.lakes.lake_at(st).is_none());
            }
        }
    }

    #[test]
    fn outlets_drain_the_land() {
        let (s, h) = hydrology(9, 4);
        assert!(!h.outlets.is_empty());
        let radius_m = h.radius_km * 1_000.0;
        let land_area: f64 = (0..s.len())
            .filter(|&i| h.elevation[i] > 0.0)
            .map(|i| s.cell_area(i as u32) * radius_m * radius_m)
            .sum();
        let outlet_area: f64 = h
            .outlets
            .iter()
            .map(|&c| h.drainage_area_m2[c as usize])
            .sum();
        // Every non-lake land cell's area is delivered to one river mouth.
        assert!(
            (outlet_area - land_area).abs() / land_area < 0.02,
            "outlet {outlet_area} land {land_area}"
        );
    }

    #[test]
    fn discharge_is_zero_without_runoff() {
        let s = Sphere::new(3);
        assert!(s.len() > 0);
        let t = tectonics::generate(&s, 11, TectonicsConfig::default());
        let ter = terrain::build(&s, &t, 11, TerrainConfig::default());
        let h = Hydrology::build(
            &s,
            &ter.elevation,
            6_371.0,
            HydrologyConfig {
                runoff_m_per_year: 0.0,
                ..HydrologyConfig::default()
            },
        );
        assert!(h.discharge_m3s.iter().all(|&q| q == 0.0));

        let mut dry = h.clone();
        dry.apply_precipitation(&s, &vec![0.0; s.len()]);
        assert_eq!(dry.runoff_m_per_year, 0.0);
        assert!(dry.discharge_m3s.iter().all(|&q| q == 0.0));
    }

    #[test]
    fn precipitation_rebuilds_discharge() {
        let (s, h) = hydrology(13, 3);
        let mut wet = h.clone();
        let precip = vec![1_000.0; s.len()];
        wet.apply_precipitation(&s, &precip);
        assert!((wet.runoff_m_per_year - 1.0).abs() < 1e-9);
        // Downstream cells carry at least as much discharge as upstream.
        for (i, ds) in wet.downhill.iter().enumerate() {
            if let Some(ds) = ds {
                assert!(wet.discharge_m3s[*ds as usize] >= wet.discharge_m3s[i]);
            }
        }
        assert!(wet.max_discharge() > 0.0);
    }

    #[test]
    fn hydrology_is_deterministic() {
        let (_, a) = hydrology(17, 3);
        let (_, b) = hydrology(17, 3);
        for (x, y) in a.elevation.iter().zip(&b.elevation) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
        assert_eq!(a.outlets, b.outlets);
        assert_eq!(a.lakes.len(), b.lakes.len());
    }

    #[test]
    fn hydrology_hash_is_stable() {
        let (_, a) = hydrology(19, 3);
        let (_, b) = hydrology(19, 3);
        let mut ha = Hasher64::new();
        let mut hb = Hasher64::new();
        a.hash_into(&mut ha);
        b.hash_into(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn area_is_positive_and_delta_exists() {
        let (s, h) = hydrology(21, 4);
        assert!(h.land_area_km2(&s) > 0.0);
        assert!(h.delta.iter().any(|&d| d));
        assert!(h.eroded_m3 > 0.0);
        assert!(h.exported_m3 > 0.0);
    }
}
