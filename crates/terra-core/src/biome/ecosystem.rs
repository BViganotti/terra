//! Ecosystems.
//!
//! Net primary productivity follows the Miami model (Lieth, 1975): the minimum
//! of a temperature-limited and a precipitation-limited curve. Biome factors
//! adjust the result for waterlogging, aridity and altitude.
//!
//! Biodiversity is a coarse richness estimate driven by biome, patch area,
//! productivity and freshwater. It is not a species list; it is a scalar that
//! lets history ask whether a settlement sits in a rich or a marginal
//! ecosystem. Fragility rises when a rich biome survives only in small
//! fragments, which is what makes islands and isolated ranges vulnerable.

use crate::hash::Hasher64;
use crate::sphere::Sphere;

use super::{Biome, BiomeContext, Biomes};

/// Coarse trophic description of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// No meaningful production (water, ice, rock).
    Barren,
    /// Sparse scrub or tundra.
    Sparse,
    /// Grass and shrub production.
    Grassland,
    /// Productive forest.
    Forest,
    /// Highly productive wet forest.
    Rainforest,
}

impl Tier {
    /// Stable identifier.
    pub fn code(self) -> &'static str {
        match self {
            Tier::Barren => "barren",
            Tier::Sparse => "sparse",
            Tier::Grassland => "grassland",
            Tier::Forest => "forest",
            Tier::Rainforest => "rainforest",
        }
    }
}

/// Ecosystem fields.
#[derive(Clone, Debug)]
pub struct Ecosystems {
    /// Net primary productivity in grams of carbon per square meter per year.
    pub npp_g_m2_yr: Vec<f64>,
    /// Estimated species richness, roughly the number of vascular plant species
    /// supported by the cell's patch. Bounded to `0..=400`.
    pub species_richness: Vec<u16>,
    /// Human carrying capacity in people per cell under simple agriculture.
    pub carrying_capacity: Vec<f64>,
    /// Trophic tier per cell.
    pub tier: Vec<Tier>,
    /// Fragility in `[0, 1]`; high for rich but small patches.
    pub fragility: Vec<f64>,
    /// Total land productivity in tonnes of carbon per year.
    pub total_npp_t_c_yr: f64,
}

impl Ecosystems {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.npp_g_m2_yr.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.npp_g_m2_yr.is_empty()
    }

    /// Mean productivity over land.
    pub fn mean_land_npp(&self, biomes: &Biomes) -> f64 {
        let mut sum = 0.0;
        let mut n = 0u64;
        for (i, &v) in self.npp_g_m2_yr.iter().enumerate() {
            if biomes.biome[i].is_land() {
                sum += v;
                n += 1;
            }
        }
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    }

    /// Sum of carrying capacity over land in people.
    pub fn total_carrying_capacity(&self, biomes: &Biomes) -> f64 {
        self.carrying_capacity
            .iter()
            .enumerate()
            .filter(|(i, _)| biomes.biome[*i].is_land())
            .map(|(_, &c)| c)
            .sum()
    }

    /// Folds the ecosystem fields into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("ecosystems");
        h.write_f64_slice(&self.npp_g_m2_yr);
        h.write_f64_slice(&self.carrying_capacity);
        for s in &self.species_richness {
            h.write_u32(u32::from(*s));
        }
    }
}

/// Miami-model temperature limitation.
pub fn npp_temperature(temp_c: f64) -> f64 {
    let t = temp_c.clamp(-20.0, 45.0);
    let denom = 1.0 + (1.315 - 0.119 * t).exp();
    if denom.is_finite() && denom > 0.0 {
        (3_000.0 / denom).clamp(0.0, 3_000.0)
    } else {
        0.0
    }
}

/// Miami-model precipitation limitation.
pub fn npp_precipitation(precip_mm: f64) -> f64 {
    let p = precip_mm.clamp(0.0, 20_000.0);
    (3_000.0 * (1.0 - (-0.000_664 * p).exp())).clamp(0.0, 3_000.0)
}

fn biome_npp_factor(biome: Biome) -> f64 {
    match biome {
        Biome::Ocean | Biome::Lake | Biome::Ice => 0.0,
        Biome::Alpine => 0.18,
        Biome::Tundra => 0.28,
        Biome::SubtropicalDesert => 0.22,
        Biome::TemperateDesert => 0.3,
        Biome::Grassland => 0.82,
        Biome::Savanna => 0.85,
        Biome::Mediterranean => 0.75,
        Biome::BorealForest => 0.62,
        Biome::TemperateForest => 0.95,
        Biome::TemperateRainforest => 1.05,
        Biome::TropicalSeasonalForest => 1.0,
        Biome::TropicalRainforest => 1.1,
        Biome::Wetland => 1.15,
        Biome::Mangrove => 1.2,
    }
}

/// Computes ecosystem fields.
pub fn build(sphere: &Sphere, ctx: BiomeContext<'_>, biomes: &Biomes, seed: u64) -> Ecosystems {
    let n = sphere.len().min(biomes.len());
    let mut npp = vec![0.0f64; n];
    let mut species = vec![0u16; n];
    let mut capacity = vec![0.0f64; n];
    let mut tier = vec![Tier::Barren; n];
    let mut fragility = vec![0.0f64; n];
    let total_cells = n.max(1) as f64;

    for i in 0..n {
        let cell = i as u32;
        let b = biomes.biome[i];
        if !b.is_land() {
            tier[i] = Tier::Barren;
            continue;
        }
        let temp = ctx
            .climate
            .temperature
            .mean_c
            .get(i)
            .copied()
            .unwrap_or(0.0);
        let precip = ctx.climate.precip_at(cell);
        let factor = biome_npp_factor(b);
        let value =
            (npp_temperature(temp).min(npp_precipitation(precip)) * factor).clamp(0.0, 3_000.0);
        npp[i] = value;
        tier[i] = match value {
            v if v < 40.0 => Tier::Barren,
            v if v < 260.0 => Tier::Sparse,
            v if v < 750.0 => Tier::Grassland,
            v if v < 1_450.0 => Tier::Forest,
            _ => Tier::Rainforest,
        };

        let patch = biomes.patch[i] as usize;
        let patch_cells = biomes.patch_size.get(patch).copied().unwrap_or(1).max(1);
        let patch_frac = f64::from(patch_cells) / total_cells;
        let area_term = (patch_frac * 40.0).ln_1p() / 3.0;
        let npp_term = (value / 1_800.0).clamp(0.0, 1.0);
        let base = match b {
            Biome::TropicalRainforest => 0.92,
            Biome::TropicalSeasonalForest => 0.74,
            Biome::TemperateRainforest => 0.72,
            Biome::Mangrove => 0.7,
            Biome::TemperateForest => 0.62,
            Biome::BorealForest => 0.42,
            Biome::Mediterranean => 0.5,
            Biome::Wetland => 0.66,
            Biome::Savanna => 0.55,
            Biome::Grassland => 0.45,
            Biome::Tundra => 0.16,
            Biome::Alpine => 0.2,
            Biome::SubtropicalDesert => 0.18,
            Biome::TemperateDesert => 0.2,
            Biome::Ice => 0.02,
            Biome::Ocean | Biome::Lake => 0.0,
        };
        let river_bonus = if ctx.river.get(i).copied().unwrap_or(false) {
            0.08
        } else {
            0.0
        };
        let richness =
            (base * 0.55 + area_term.clamp(0.0, 1.0) * 0.25 + npp_term * 0.2 + river_bonus)
                .clamp(0.0, 1.0);
        let jitter = crate::noise::cell_jitter(seed ^ 0x00e0_0c01, 11, cell) * 0.04 - 0.02;
        let richness = (richness + jitter).clamp(0.0, 1.0);
        species[i] = (5.0 + richness * 395.0).round().clamp(5.0, 400.0) as u16;

        // Carrying capacity: productivity, fertility, water and coastal access.
        let coastal = sphere
            .neighbors(cell)
            .iter()
            .any(|&nb| ctx.ocean.get(nb as usize).copied().unwrap_or(false));
        let water = if ctx.river.get(i).copied().unwrap_or(false) {
            1.3
        } else if ctx.lake.get(i).copied().unwrap_or(false) {
            1.15
        } else {
            1.0
        };
        let coast = if coastal { 1.2 } else { 1.0 };
        let kc = (value / 2_000.0) * (0.3 + b.fertility()) * 400_000.0 * water * coast;
        capacity[i] = kc.clamp(0.0, 2_000_000.0);

        // Fragility: rich but small patches are vulnerable.
        let small = (1.0 - (f64::from(patch_cells) / 24.0)).clamp(0.0, 1.0);
        fragility[i] = (richness * small).clamp(0.0, 1.0);
    }

    // Total productivity in tonnes of carbon per year.
    let radius_m = 6_371_000.0;
    let mut total = 0.0;
    for i in 0..n {
        let area_m2 = sphere.cell_area(i as u32) * radius_m * radius_m;
        total += npp[i] * area_m2 / 1_000_000.0 / 1_000.0;
    }

    Ecosystems {
        npp_g_m2_yr: npp,
        species_richness: species,
        carrying_capacity: capacity,
        tier,
        fragility,
        total_npp_t_c_yr: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::{Climate, ClimateConfig};
    use crate::tectonics::{self, TectonicsConfig};
    use crate::terrain::{self, TerrainConfig};

    fn world(seed: u64) -> (Sphere, Biomes, Ecosystems) {
        let sphere = Sphere::new(3);
        let t = tectonics::generate(&sphere, seed, TectonicsConfig::default());
        let ter = terrain::build(&sphere, &t, seed, TerrainConfig::default());
        let ocean: Vec<bool> = ter.elevation.iter().map(|&e| e <= 0.0).collect();
        let climate = Climate::build(
            &sphere,
            &ter.elevation,
            &ocean,
            seed,
            ClimateConfig::default(),
        );
        let lake = vec![false; sphere.len()];
        let river = vec![false; sphere.len()];
        let ctx = BiomeContext {
            elevation: &ter.elevation,
            ocean: &ocean,
            lake: &lake,
            river: &river,
            climate: &climate,
        };
        let biomes = crate::biome::classify(&sphere, ctx, seed);
        let eco = build(&sphere, ctx, &biomes, seed);
        (sphere, biomes, eco)
    }

    #[test]
    fn water_has_no_terrestrial_productivity() {
        let (sphere, biomes, eco) = world(3);
        for i in 0..sphere.len() {
            if !biomes.biome[i].is_land() {
                assert_eq!(eco.npp_g_m2_yr[i], 0.0);
                assert_eq!(eco.carrying_capacity[i], 0.0);
                assert_eq!(eco.tier[i], Tier::Barren);
            }
        }
    }

    #[test]
    fn productivity_is_bounded() {
        let (sphere, _, eco) = world(5);
        for i in 0..sphere.len() {
            assert!(eco.npp_g_m2_yr[i].is_finite());
            assert!((0.0..=3_000.0).contains(&eco.npp_g_m2_yr[i]));
            assert!(eco.species_richness[i] <= 400);
            assert!(eco.fragility[i] >= 0.0 && eco.fragility[i] <= 1.0);
            assert!(eco.carrying_capacity[i] >= 0.0);
        }
    }

    #[test]
    fn miami_model_has_expected_shape() {
        assert!(npp_temperature(-10.0) < npp_temperature(20.0));
        assert!(npp_temperature(0.0) < 1_000.0);
        assert!(npp_precipitation(100.0) < npp_precipitation(1_000.0));
        assert!(npp_precipitation(10_000.0) > 2_900.0);
        assert_eq!(npp_temperature(f64::NAN), 0.0);
    }

    #[test]
    fn rainforests_are_richer_than_deserts() {
        let (sphere, biomes, eco) = world(7);
        let mut forest = (0.0, 0u32);
        let mut desert = (0.0, 0u32);
        for i in 0..sphere.len() {
            match biomes.biome[i] {
                Biome::TropicalRainforest => {
                    forest.0 += f64::from(eco.species_richness[i]);
                    forest.1 += 1;
                }
                Biome::SubtropicalDesert => {
                    desert.0 += f64::from(eco.species_richness[i]);
                    desert.1 += 1;
                }
                _ => {}
            }
        }
        if forest.1 > 0 && desert.1 > 0 {
            assert!(forest.0 / f64::from(forest.1) > desert.0 / f64::from(desert.1));
        }
    }

    #[test]
    fn carrying_capacity_follows_fertility() {
        let (sphere, biomes, eco) = world(9);
        for i in 0..sphere.len() {
            if biomes.biome[i] == Biome::Grassland {
                assert!(eco.carrying_capacity[i] > 0.0);
            }
            if biomes.biome[i] == Biome::Ice {
                assert!(eco.carrying_capacity[i] < 1_000.0);
            }
        }
    }

    #[test]
    fn ecosystems_are_deterministic() {
        let (_, _, a) = world(11);
        let (_, _, b) = world(11);
        assert_eq!(a.npp_g_m2_yr, b.npp_g_m2_yr);
        assert_eq!(a.species_richness, b.species_richness);
        assert_eq!(a.total_npp_t_c_yr.to_bits(), b.total_npp_t_c_yr.to_bits());
    }

    #[test]
    fn tiers_are_ordered_by_productivity() {
        let (sphere, _, eco) = world(13);
        let rank = |t: Tier| match t {
            Tier::Barren => 0,
            Tier::Sparse => 1,
            Tier::Grassland => 2,
            Tier::Forest => 3,
            Tier::Rainforest => 4,
        };
        for i in 0..sphere.len() {
            for &nb in sphere.neighbors(i as u32) {
                if rank(eco.tier[i]) > rank(eco.tier[nb as usize]) {
                    assert!(eco.npp_g_m2_yr[i] >= eco.npp_g_m2_yr[nb as usize] - 1.0);
                }
            }
        }
    }
}
