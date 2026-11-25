//! Biome classification.
//!
//! Biomes are a deterministic lookup over temperature, precipitation, elevation
//! and water context. The thresholds follow the familiar Whittaker layout but
//! are tuned for Terra's calibrated climate, not copied from an atlas. The
//! classification is total: every cell receives exactly one biome, and cells
//! that fail every climatic rule fall back to a cold or dry biome rather than
//! being skipped.
//!
//! Connected patches are labelled so ecosystems can reason about area and
//! fragmentation, which is what makes small islands and isolated ranges
//! ecologically fragile.

use std::collections::BTreeMap;

pub mod ecosystem;

pub use ecosystem::{Ecosystems, Tier};

use crate::climate::Climate;
use crate::hash::Hasher64;
use crate::sphere::Sphere;

/// A biome category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Biome {
    /// Open ocean.
    Ocean,
    /// Freshwater lake.
    Lake,
    /// Permanent ice.
    Ice,
    /// Cold, treeless plain.
    Tundra,
    /// High-altitude treeless zone.
    Alpine,
    /// Conifer forest of the high latitudes.
    BorealForest,
    /// Mixed or broadleaf temperate forest.
    TemperateForest,
    /// Wet, mild conifer or broadleaf forest.
    TemperateRainforest,
    /// Dry-summer shrubland.
    Mediterranean,
    /// Temperate grassland and steppe.
    Grassland,
    /// Tropical grassland with scattered trees.
    Savanna,
    /// Equatorial evergreen forest.
    TropicalRainforest,
    /// Tropical forest with a dry season.
    TropicalSeasonalForest,
    /// Hot desert.
    SubtropicalDesert,
    /// Cold desert and semi-desert.
    TemperateDesert,
    /// Waterlogged lowland.
    Wetland,
    /// Tropical coastal swamp forest.
    Mangrove,
}

impl Biome {
    /// All biomes in palette order.
    pub const ALL: [Biome; 17] = [
        Biome::Ocean,
        Biome::Lake,
        Biome::Ice,
        Biome::Tundra,
        Biome::Alpine,
        Biome::BorealForest,
        Biome::TemperateForest,
        Biome::TemperateRainforest,
        Biome::Mediterranean,
        Biome::Grassland,
        Biome::Savanna,
        Biome::TropicalRainforest,
        Biome::TropicalSeasonalForest,
        Biome::SubtropicalDesert,
        Biome::TemperateDesert,
        Biome::Wetland,
        Biome::Mangrove,
    ];

    /// Stable identifier used in JSON and map legends.
    pub fn code(self) -> &'static str {
        match self {
            Biome::Ocean => "ocean",
            Biome::Lake => "lake",
            Biome::Ice => "ice",
            Biome::Tundra => "tundra",
            Biome::Alpine => "alpine",
            Biome::BorealForest => "boreal_forest",
            Biome::TemperateForest => "temperate_forest",
            Biome::TemperateRainforest => "temperate_rainforest",
            Biome::Mediterranean => "mediterranean",
            Biome::Grassland => "grassland",
            Biome::Savanna => "savanna",
            Biome::TropicalRainforest => "tropical_rainforest",
            Biome::TropicalSeasonalForest => "tropical_seasonal_forest",
            Biome::SubtropicalDesert => "subtropical_desert",
            Biome::TemperateDesert => "temperate_desert",
            Biome::Wetland => "wetland",
            Biome::Mangrove => "mangrove",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Biome::Ocean => "Ocean",
            Biome::Lake => "Lake",
            Biome::Ice => "Ice sheet",
            Biome::Tundra => "Tundra",
            Biome::Alpine => "Alpine",
            Biome::BorealForest => "Boreal forest",
            Biome::TemperateForest => "Temperate forest",
            Biome::TemperateRainforest => "Temperate rainforest",
            Biome::Mediterranean => "Mediterranean scrub",
            Biome::Grassland => "Grassland",
            Biome::Savanna => "Savanna",
            Biome::TropicalRainforest => "Tropical rainforest",
            Biome::TropicalSeasonalForest => "Tropical seasonal forest",
            Biome::SubtropicalDesert => "Subtropical desert",
            Biome::TemperateDesert => "Cold desert",
            Biome::Wetland => "Wetland",
            Biome::Mangrove => "Mangrove",
        }
    }

    /// Palette color as packed RGB.
    pub fn rgb(self) -> u32 {
        match self {
            Biome::Ocean => 0x1b3a5c,
            Biome::Lake => 0x2f6f8f,
            Biome::Ice => 0xe8f1f5,
            Biome::Tundra => 0x9aa79b,
            Biome::Alpine => 0x8c8f93,
            Biome::BorealForest => 0x2f5d4a,
            Biome::TemperateForest => 0x3f7a4f,
            Biome::TemperateRainforest => 0x2c6b55,
            Biome::Mediterranean => 0xa1a45c,
            Biome::Grassland => 0x8ea15b,
            Biome::Savanna => 0xc0a75c,
            Biome::TropicalRainforest => 0x1f6b3a,
            Biome::TropicalSeasonalForest => 0x3c8345,
            Biome::SubtropicalDesert => 0xd8bd7a,
            Biome::TemperateDesert => 0xb9a582,
            Biome::Wetland => 0x5d7f6a,
            Biome::Mangrove => 0x476b52,
        }
    }

    /// True for water biomes.
    pub fn is_water(self) -> bool {
        matches!(self, Biome::Ocean | Biome::Lake)
    }

    /// True for terrestrial biomes.
    pub fn is_land(self) -> bool {
        !self.is_water()
    }

    /// True for forest biomes.
    pub fn is_forest(self) -> bool {
        matches!(
            self,
            Biome::BorealForest
                | Biome::TemperateForest
                | Biome::TemperateRainforest
                | Biome::TropicalRainforest
                | Biome::TropicalSeasonalForest
                | Biome::Mangrove
        )
    }

    /// Relative fertility for human agriculture and settlement, `0..=1`.
    pub fn fertility(self) -> f64 {
        match self {
            Biome::Ocean | Biome::Lake | Biome::Ice | Biome::Alpine => 0.0,
            Biome::Tundra => 0.08,
            Biome::SubtropicalDesert | Biome::TemperateDesert => 0.05,
            Biome::BorealForest => 0.28,
            Biome::Grassland => 0.72,
            Biome::Savanna => 0.5,
            Biome::Mediterranean => 0.6,
            Biome::TemperateForest | Biome::TemperateRainforest => 0.55,
            Biome::TropicalSeasonalForest => 0.6,
            Biome::TropicalRainforest => 0.42,
            Biome::Wetland | Biome::Mangrove => 0.35,
        }
    }

    /// Number of the biome in [`Biome::ALL`], for compact storage.
    pub fn index(self) -> u8 {
        match self {
            Biome::Ocean => 0,
            Biome::Lake => 1,
            Biome::Ice => 2,
            Biome::Tundra => 3,
            Biome::Alpine => 4,
            Biome::BorealForest => 5,
            Biome::TemperateForest => 6,
            Biome::TemperateRainforest => 7,
            Biome::Mediterranean => 8,
            Biome::Grassland => 9,
            Biome::Savanna => 10,
            Biome::TropicalRainforest => 11,
            Biome::TropicalSeasonalForest => 12,
            Biome::SubtropicalDesert => 13,
            Biome::TemperateDesert => 14,
            Biome::Wetland => 15,
            Biome::Mangrove => 16,
        }
    }

    /// Inverse of [`Biome::index`]. Unknown values map to ocean.
    pub fn from_index(i: u8) -> Biome {
        Biome::ALL.get(i as usize).copied().unwrap_or(Biome::Ocean)
    }
}

/// Classification inputs beyond the climate fields.
#[derive(Clone, Copy)]
pub struct BiomeContext<'a> {
    /// Elevation in meters.
    pub elevation: &'a [f64],
    /// Water mask (ocean and large lakes).
    pub ocean: &'a [bool],
    /// Lake mask, including ponds too small for the climate water mask.
    pub lake: &'a [bool],
    /// Cells carrying a significant river.
    pub river: &'a [bool],
    /// Climate fields.
    pub climate: &'a Climate,
}

/// Classified biomes and patch structure.
#[derive(Clone, Debug)]
pub struct Biomes {
    /// Biome per cell.
    pub biome: Vec<Biome>,
    /// Patch id per cell. Patches are connected runs of one biome.
    pub patch: Vec<u32>,
    /// Number of cells in each patch.
    pub patch_size: Vec<u32>,
    /// Number of patches.
    pub patch_count: u32,
    /// Count of land cells per biome.
    pub land_counts: BTreeMap<&'static str, u32>,
    /// Land cells in the largest contiguous patch of each biome, by code.
    pub largest_patch: BTreeMap<&'static str, u32>,
}

impl Biomes {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.biome.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.biome.is_empty()
    }

    /// Biome at a cell, falling back to ocean for bad indices.
    pub fn at(&self, cell: u32) -> Biome {
        self.biome
            .get(cell as usize)
            .copied()
            .unwrap_or(Biome::Ocean)
    }

    /// True when a cell belongs to the given biome.
    pub fn is(&self, cell: u32, biome: Biome) -> bool {
        self.at(cell) == biome
    }

    /// Number of distinct land biomes present.
    pub fn land_biome_count(&self) -> usize {
        self.land_counts.len()
    }

    /// Most common land biome, if any.
    pub fn dominant_land_biome(&self) -> Option<Biome> {
        let mut best: Option<(&str, u32)> = None;
        for (&k, &v) in &self.land_counts {
            if best.map_or(true, |(_, b)| v > b) {
                best = Some((k, v));
            }
        }
        best.and_then(|(code, _)| {
            Biome::ALL
                .iter()
                .copied()
                .find(|b| b.is_land() && b.code() == code)
        })
    }

    /// Share of land covered by a biome.
    pub fn land_share(&self, biome: Biome) -> f64 {
        let total: u32 = self.land_counts.values().sum();
        if total == 0 {
            return 0.0;
        }
        f64::from(self.land_counts.get(biome.code()).copied().unwrap_or(0)) / f64::from(total)
    }

    /// Folds the biome grid into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("biomes");
        for b in &self.biome {
            h.write_u32(u32::from(b.index()));
        }
        h.write_u32_slice(&self.patch);
    }
}

/// Classifies every cell.
pub fn classify(sphere: &Sphere, ctx: BiomeContext<'_>, seed: u64) -> Biomes {
    let n = sphere.len();
    let mut biome = vec![Biome::Ocean; n];
    for i in 0..n {
        let cell = i as u32;
        if ctx.ocean.get(i).copied().unwrap_or(false) {
            // Polar oceans stay open water; sea ice is not a separate biome.
            biome[i] = Biome::Ocean;
            continue;
        }
        if ctx.lake.get(i).copied().unwrap_or(false) {
            biome[i] = Biome::Lake;
            continue;
        }
        let temp = ctx
            .climate
            .temperature
            .mean_c
            .get(i)
            .copied()
            .unwrap_or(0.0);
        let tmax = ctx
            .climate
            .temperature
            .max_c
            .get(i)
            .copied()
            .unwrap_or(temp);
        let precip = ctx.climate.precip_at(cell);
        let elev = ctx.elevation.get(i).copied().unwrap_or(0.0);
        let aridity = ctx.climate.aridity_at(cell);
        let coastal = sphere
            .neighbors(cell)
            .iter()
            .any(|&nb| ctx.ocean.get(nb as usize).copied().unwrap_or(false));
        let flat_low = elev < 12.0 && elev > 0.0;
        biome[i] = classify_one(
            temp,
            tmax,
            precip,
            elev,
            aridity,
            coastal,
            flat_low,
            ctx.river.get(i).copied().unwrap_or(false),
            seed,
            cell,
        );
    }

    let (patch, patch_size, patch_count) = patchify(sphere, &biome);
    let mut land_counts: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut largest_patch: BTreeMap<&'static str, u32> = BTreeMap::new();
    for (i, &b) in biome.iter().enumerate() {
        if b.is_land() {
            *land_counts.entry(b.code()).or_insert(0) += 1;
            let p = patch[i] as usize;
            if let Some(&size) = patch_size.get(p) {
                let slot = largest_patch.entry(b.code()).or_insert(0);
                *slot = (*slot).max(size);
            }
        }
    }

    Biomes {
        biome,
        patch,
        patch_size,
        patch_count,
        land_counts,
        largest_patch,
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_one(
    temp: f64,
    tmax: f64,
    precip: f64,
    elev: f64,
    aridity: f64,
    coastal: bool,
    flat_low: bool,
    river: bool,
    seed: u64,
    cell: u32,
) -> Biome {
    // Permanent ice and high alpine zones override the moisture regime.
    if tmax < 0.0 || temp < -14.0 {
        return Biome::Ice;
    }
    if elev > 2_500.0 && temp < 3.0 {
        return Biome::Alpine;
    }
    if elev > 3_300.0 {
        return Biome::Alpine;
    }
    if temp < -2.0 {
        return if precip > 220.0 {
            Biome::BorealForest
        } else {
            Biome::Tundra
        };
    }

    // Wetlands and mangroves are azonal: water context beats the climate table.
    let jitter = crate::noise::cell_jitter(seed ^ 0x00b1_0a11, 7, cell);
    if flat_low && river && precip > 700.0 {
        return Biome::Wetland;
    }
    if coastal && temp > 20.0 && precip > 900.0 && flat_low {
        return Biome::Mangrove;
    }
    if flat_low && precip > 1_400.0 && jitter > 0.72 {
        return Biome::Wetland;
    }

    if temp < 4.0 {
        if precip > 260.0 {
            Biome::BorealForest
        } else {
            Biome::Tundra
        }
    } else if temp < 11.0 {
        if aridity < 0.35 || precip < 280.0 {
            Biome::TemperateDesert
        } else if precip > 1_200.0 {
            Biome::TemperateRainforest
        } else if precip > 520.0 {
            Biome::TemperateForest
        } else {
            Biome::Grassland
        }
    } else if temp < 19.0 {
        if precip < 250.0 || aridity < 0.25 {
            Biome::TemperateDesert
        } else if precip > 1_300.0 {
            Biome::TemperateRainforest
        } else if precip > 650.0 {
            Biome::TemperateForest
        } else if precip > 430.0 {
            // Drier summers on the warm margin read as Mediterranean scrub.
            if temp > 15.0 && aridity > 0.45 {
                Biome::Mediterranean
            } else {
                Biome::Grassland
            }
        } else {
            Biome::Grassland
        }
    } else {
        // Warm climates.
        if precip < 300.0 || aridity < 0.18 {
            Biome::SubtropicalDesert
        } else if precip > 1_900.0 {
            Biome::TropicalRainforest
        } else if precip > 1_050.0 {
            Biome::TropicalSeasonalForest
        } else if precip > 550.0 {
            Biome::Savanna
        } else if elev > 1_600.0 {
            Biome::Alpine
        } else {
            Biome::Grassland
        }
    }
}

/// Labels connected patches of equal biome.
fn patchify(sphere: &Sphere, biome: &[Biome]) -> (Vec<u32>, Vec<u32>, u32) {
    let n = sphere.len().min(biome.len());
    let mut patch = vec![u32::MAX; n];
    let mut sizes = Vec::new();
    let mut stack = Vec::new();
    for start in 0..n {
        if patch[start] != u32::MAX {
            continue;
        }
        let id = sizes.len() as u32;
        let b = biome[start];
        patch[start] = id;
        stack.push(start as u32);
        let mut size = 0u32;
        while let Some(cell) = stack.pop() {
            size += 1;
            for &nb in sphere.neighbors(cell) {
                let ni = nb as usize;
                if ni < n && patch[ni] == u32::MAX && biome[ni] == b {
                    patch[ni] = id;
                    stack.push(nb);
                }
            }
        }
        sizes.push(size);
    }
    let count = sizes.len() as u32;
    (patch, sizes, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::{Climate, ClimateConfig};
    use crate::tectonics::{self, TectonicsConfig};
    use crate::terrain::{self, TerrainConfig};

    struct Sample {
        sphere: Sphere,
        ter: terrain::Terrain,
        climate: Climate,
        biomes: Biomes,
    }

    fn sample(seed: u64, level: u32) -> Sample {
        let sphere = Sphere::new(level);
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
        let biomes = classify(
            &sphere,
            BiomeContext {
                elevation: &ter.elevation,
                ocean: &ocean,
                lake: &lake,
                river: &river,
                climate: &climate,
            },
            seed,
        );
        Sample {
            sphere,
            ter,
            climate,
            biomes,
        }
    }

    #[test]
    fn every_cell_has_exactly_one_biome() {
        let s = sample(3, 3);
        assert_eq!(s.biomes.len(), s.sphere.len());
        for i in 0..s.sphere.len() {
            let b = s.biomes.biome[i];
            assert_eq!(Biome::from_index(b.index()), b);
        }
    }

    #[test]
    fn water_cells_are_water_biomes() {
        let s = sample(5, 3);
        for i in 0..s.sphere.len() {
            if s.ter.elevation[i] <= 0.0 {
                assert!(s.biomes.biome[i].is_water());
            } else {
                assert!(s.biomes.biome[i].is_land());
            }
        }
    }

    #[test]
    fn patch_sizes_sum_to_cell_count() {
        let s = sample(7, 3);
        let total: u32 = s.biomes.patch_size.iter().sum();
        assert_eq!(total as usize, s.sphere.len());
        assert_eq!(s.biomes.patch_count as usize, s.biomes.patch_size.len());
        for (i, &p) in s.biomes.patch.iter().enumerate() {
            assert!(p < s.biomes.patch_count);
            let b = s.biomes.biome[i];
            for &nb in s.sphere.neighbors(i as u32) {
                if s.biomes.biome[nb as usize] == b {
                    assert_eq!(s.biomes.patch[nb as usize], p);
                }
            }
        }
    }

    #[test]
    fn biome_mix_is_plausible() {
        let s = sample(9, 4);
        assert!(s.biomes.land_biome_count() >= 3);
        assert!(s.biomes.dominant_land_biome().is_some());
        let shares: f64 = s
            .biomes
            .land_counts
            .keys()
            .map(|k| {
                Biome::ALL
                    .iter()
                    .find(|b| b.code() == *k)
                    .map(|b| s.biomes.land_share(*b))
                    .unwrap_or(0.0)
            })
            .sum();
        assert!((shares - 1.0).abs() < 1e-9);
    }

    #[test]
    fn classification_respects_cold_and_dry_extremes() {
        let s = sample(11, 3);
        for i in 0..s.sphere.len() {
            let b = s.biomes.biome[i];
            if b == Biome::Ice {
                assert!(
                    s.climate.temperature.max_c[i] < 0.0 || s.climate.temperature.mean_c[i] < -14.0
                );
            }
            if b == Biome::SubtropicalDesert {
                let lat = s.sphere.lat(i as u32).abs();
                assert!(s.climate.precip_at(i as u32) < 600.0 || lat < 40.0);
            }
            if b == Biome::Alpine {
                assert!(s.ter.elevation[i] > 1_500.0);
            }
        }
    }

    #[test]
    fn biome_metadata_is_consistent() {
        for b in Biome::ALL {
            assert_eq!(Biome::from_index(b.index()), b);
            assert!(!b.code().is_empty());
            assert!(!b.label().is_empty());
            assert!((0.0..=1.0).contains(&b.fertility()));
            assert_eq!(b.is_land(), !b.is_water());
        }
    }

    #[test]
    fn classification_is_deterministic() {
        let a = sample(13, 3);
        let b = sample(13, 3);
        assert_eq!(a.biomes.biome, b.biomes.biome);
        assert_eq!(a.biomes.patch, b.biomes.patch);
    }

    #[test]
    fn hash_is_stable() {
        let a = sample(15, 3);
        let b = sample(15, 3);
        let mut ha = Hasher64::new();
        let mut hb = Hasher64::new();
        a.biomes.hash_into(&mut ha);
        b.biomes.hash_into(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn bad_indices_are_safe() {
        let s = sample(17, 2);
        assert_eq!(s.biomes.at(99_999), Biome::Ocean);
        assert!(!s.biomes.is(99_999, Biome::Grassland));
        assert_eq!(Biome::from_index(200), Biome::Ocean);
    }
}
