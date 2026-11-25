//! World assembly.
//!
//! [`World::generate`] runs the physical pipeline in dependency order and keeps
//! every intermediate layer. The result is a plain, cloneable value: no file
//! handles, no threads, no hidden caches. History is applied on top of it by
//! `terra-history`, so the same world can be replayed with different
//! civilizations without re-running geology.

use std::collections::BTreeMap;

use crate::biome::{self, Biome, BiomeContext, Biomes, Ecosystems};
use crate::climate::{Climate, ClimateConfig, MoistureConfig};
use crate::hash::{hex, Hasher64};
use crate::hydrology::{Hydrology, HydrologyConfig};
use crate::rng::{Rng, Stream};
use crate::sphere::Sphere;
use crate::tectonics::{self, Stress, Tectonics, TectonicsConfig};
use crate::terrain::{self, Terrain, TerrainConfig};

/// Everything needed to reproduce a world.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldConfig {
    /// World seed.
    pub seed: u64,
    /// Icosphere subdivision level, clamped to `2..=6`.
    pub subdiv: u32,
    /// Historical span in years, clamped to `100..=20000`.
    pub years: u32,
    /// Number of plates, clamped to `3..=20`.
    pub plates: u32,
    /// Target land fraction, clamped to `0.1..=0.75`.
    pub land_fraction: f64,
    /// Erosion passes, clamped to `1..=24`.
    pub erosion_iterations: u32,
    /// Global temperature offset in degrees Celsius.
    pub temperature_offset_c: f64,
    /// Target mean annual precipitation over land in millimeters.
    pub precip_target_mm: f64,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            subdiv: 5,
            years: 10_000,
            plates: 9,
            land_fraction: 0.31,
            erosion_iterations: 14,
            temperature_offset_c: 0.0,
            precip_target_mm: 860.0,
        }
    }
}

impl WorldConfig {
    /// Clamps every field into a supported, bounded range.
    pub fn sanitized(mut self) -> Self {
        self.subdiv = self.subdiv.clamp(2, 6);
        self.years = self.years.clamp(100, 20_000);
        self.plates = self.plates.clamp(3, 20);
        self.land_fraction = if self.land_fraction.is_finite() {
            self.land_fraction.clamp(0.1, 0.75)
        } else {
            0.31
        };
        self.erosion_iterations = self.erosion_iterations.clamp(1, 24);
        self.temperature_offset_c = if self.temperature_offset_c.is_finite() {
            self.temperature_offset_c.clamp(-12.0, 12.0)
        } else {
            0.0
        };
        self.precip_target_mm = if self.precip_target_mm.is_finite() {
            self.precip_target_mm.clamp(200.0, 2_500.0)
        } else {
            860.0
        };
        self
    }

    /// Short stable configuration string for reports.
    pub fn describe(&self) -> String {
        format!(
            "seed={} subdiv={} plates={} years={} land={:.2}",
            self.seed, self.subdiv, self.plates, self.years, self.land_fraction
        )
    }
}

/// Aggregate statistics used by the Studio header and reports.
#[derive(Clone, Debug, Default)]
pub struct WorldStats {
    /// Total cells.
    pub cells: u32,
    /// Land cells.
    pub land_cells: u32,
    /// Ocean cells.
    pub ocean_cells: u32,
    /// Lake cells.
    pub lake_cells: u32,
    /// River cells.
    pub river_cells: u32,
    /// Delta cells.
    pub delta_cells: u32,
    /// Realized land fraction.
    pub land_fraction: f64,
    /// Minimum elevation in meters.
    pub min_elevation: f64,
    /// Maximum elevation in meters.
    pub max_elevation: f64,
    /// Mean elevation over land in meters.
    pub mean_land_elevation: f64,
    /// Mean annual temperature in degrees Celsius.
    pub mean_temperature_c: f64,
    /// Mean annual precipitation over land in millimeters.
    pub mean_precip_mm: f64,
    /// Number of lakes.
    pub lakes: u32,
    /// Surface area of the largest lake in square kilometers.
    pub largest_lake_km2: f64,
    /// Number of plates.
    pub plates: u32,
    /// Convergent boundary count.
    pub convergent: u32,
    /// Divergent boundary count.
    pub divergent: u32,
    /// Transform boundary count.
    pub transform: u32,
    /// Number of connected landmasses.
    pub continents: u32,
    /// Area of the largest landmass in square kilometers.
    pub largest_continent_km2: f64,
    /// Land cell count per biome code.
    pub biome_counts: BTreeMap<&'static str, u32>,
    /// Mean species richness over land.
    pub mean_species: f64,
    /// Total human carrying capacity over land in people.
    pub carrying_capacity: f64,
    /// Total net primary productivity in tonnes of carbon per year.
    pub npp_t_c_yr: f64,
    /// Number of distinct land biomes.
    pub land_biomes: u32,
}

/// A single cell's full physical description, used by the map inspector.
#[derive(Clone, Debug)]
pub struct CellInfo {
    /// Cell index.
    pub cell: u32,
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
    /// Elevation in meters.
    pub elevation: f64,
    /// Biome code.
    pub biome: &'static str,
    /// Biome label.
    pub biome_label: &'static str,
    /// Mean annual temperature in degrees Celsius.
    pub temperature_c: f64,
    /// Seasonal amplitude in degrees Celsius.
    pub seasonality_c: f64,
    /// Annual precipitation in millimeters.
    pub precip_mm: f64,
    /// Aridity index.
    pub aridity: f64,
    /// Relative humidity.
    pub humidity: f64,
    /// Plate id.
    pub plate: u16,
    /// Plate kind code.
    pub plate_kind: &'static str,
    /// Dominant tectonic stress code.
    pub stress: &'static str,
    /// Uplift in meters.
    pub uplift_m: f64,
    /// Discharge in cubic meters per second.
    pub discharge_m3s: f64,
    /// Whether the cell carries a river.
    pub river: bool,
    /// Whether the cell is lake water.
    pub lake: bool,
    /// Whether the cell is a delta.
    pub delta: bool,
    /// Net primary productivity in grams of carbon per square meter per year.
    pub npp_g_m2_yr: f64,
    /// Estimated species richness.
    pub species: u16,
    /// Human carrying capacity in people.
    pub carrying_capacity: f64,
    /// Landmass label, or `u32::MAX` for ocean.
    pub landmass: u32,
    /// Crustal age proxy in million years.
    pub crust_age_myr: f64,
}

/// Hashes identifying every physical layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldHashes {
    /// Hash of the tectonic state.
    pub plates: String,
    /// Hash of the eroded terrain.
    pub terrain: String,
    /// Hash of the drainage network and lakes.
    pub hydrology: String,
    /// Hash of climate fields.
    pub climate: String,
    /// Hash of biomes and ecosystems.
    pub biosphere: String,
    /// Combined hash over all layers and the configuration.
    pub combined: String,
}

/// A fully generated planet.
#[derive(Clone, Debug)]
pub struct World {
    /// Reproduction parameters after sanitization.
    pub config: WorldConfig,
    /// Generated name, deterministic from the seed.
    pub name: String,
    /// Sphere grid.
    pub sphere: Sphere,
    /// Tectonic state.
    pub tectonics: Tectonics,
    /// Terrain after erosion and coastline reclassification.
    pub terrain: Terrain,
    /// Hydrology.
    pub hydrology: Hydrology,
    /// Climate.
    pub climate: Climate,
    /// Biomes.
    pub biomes: Biomes,
    /// Ecosystems.
    pub ecosystems: Ecosystems,
    /// Cells carrying a river.
    pub river: Vec<bool>,
    /// Cells covered by lake water.
    pub lake: Vec<bool>,
    /// Aggregate statistics.
    pub stats: WorldStats,
    /// Layer hashes.
    pub hashes: WorldHashes,
}

impl World {
    /// Generates a world. This is a pure function of the sanitized config.
    pub fn generate(config: WorldConfig) -> Self {
        Self::generate_with(config, |_, _| {})
    }

    /// Generates a world while reporting coarse progress.
    ///
    /// The callback receives a stage name and a fraction in `[0, 1]`. Stages are
    /// reported after they complete, so a progress bar never claims work that
    /// has not happened.
    pub fn generate_with<F: FnMut(&str, f64)>(config: WorldConfig, mut report: F) -> Self {
        let config = config.sanitized();
        let seed = config.seed;
        let sphere = Sphere::new(config.subdiv);
        let name = planet_name(seed);
        report("grid", 0.03);

        let tectonics = tectonics::generate(
            &sphere,
            seed,
            TectonicsConfig {
                plates: config.plates,
                ..TectonicsConfig::default()
            },
        );
        report("plates", 0.12);
        let mut terrain = terrain::build(
            &sphere,
            &tectonics,
            seed,
            TerrainConfig {
                land_fraction: config.land_fraction,
                ..TerrainConfig::default()
            },
        );
        report("terrain", 0.24);
        let hydrology = Hydrology::build(
            &sphere,
            &terrain.elevation,
            terrain.radius_km,
            HydrologyConfig {
                erosion: crate::hydrology::ErosionConfig {
                    iterations: config.erosion_iterations,
                    ..crate::hydrology::ErosionConfig::default()
                },
                ..HydrologyConfig::default()
            },
        );
        terrain.reclassify(&sphere, &hydrology.elevation);
        report("hydrology", 0.52);

        let ocean: Vec<bool> = terrain.elevation.iter().map(|&e| e <= 0.0).collect();
        let lake: Vec<bool> = hydrology
            .lakes
            .lake_of
            .iter()
            .map(|&id| id != u32::MAX)
            .collect();
        let climate = Climate::build(
            &sphere,
            &terrain.elevation,
            &ocean,
            seed,
            ClimateConfig {
                moisture: MoistureConfig {
                    target_land_mm: config.precip_target_mm,
                    ..MoistureConfig::default()
                },
                temperature_offset_c: config.temperature_offset_c,
            },
        );

        // Discharge now uses the precipitation field, not uniform runoff.
        let mut hydrology = hydrology;
        hydrology.apply_precipitation(&sphere, &climate.moisture.precip_mm);
        report("climate", 0.72);

        let area_m2: Vec<f64> = (0..sphere.len())
            .map(|i| sphere.cell_area(i as u32) * 6_371_000.0 * 6_371_000.0)
            .collect();
        let mut river = vec![false; sphere.len()];
        for i in 0..sphere.len() {
            if !lake[i]
                && terrain.elevation[i] > 0.0
                && hydrology.drainage_area_m2[i] >= area_m2[i] * 2.2
            {
                river[i] = true;
            }
        }

        let ctx = BiomeContext {
            elevation: &terrain.elevation,
            ocean: &ocean,
            lake: &lake,
            river: &river,
            climate: &climate,
        };
        let biomes = biome::classify(&sphere, ctx, seed);
        let ecosystems = biome::ecosystem::build(&sphere, ctx, &biomes, seed);
        report("biosphere", 0.9);

        let stats = compute_stats(
            &sphere,
            &tectonics,
            &terrain,
            &hydrology,
            &climate,
            &biomes,
            &ecosystems,
            &ocean,
            &lake,
            &river,
        );
        let hashes = compute_hashes(
            &config,
            &sphere,
            &tectonics,
            &hydrology,
            &climate,
            &biomes,
            &ecosystems,
            &lake,
            &river,
        );

        World {
            config,
            name,
            sphere,
            tectonics,
            terrain,
            hydrology,
            climate,
            biomes,
            ecosystems,
            river,
            lake,
            stats,
            hashes,
        }
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.sphere.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.sphere.is_empty()
    }

    /// Full description of one cell, or `None` for an invalid index.
    pub fn cell_info(&self, cell: u32) -> Option<CellInfo> {
        let i = cell as usize;
        if i >= self.len() {
            return None;
        }
        let biome = self.biomes.biome[i];
        Some(CellInfo {
            cell,
            lat: self.sphere.lat(cell),
            lon: self.sphere.lon(cell),
            elevation: self.terrain.elevation[i],
            biome: biome.code(),
            biome_label: biome.label(),
            temperature_c: self.climate.temperature.mean_c[i],
            seasonality_c: self.climate.temperature.seasonal_amplitude_c[i],
            precip_mm: self.climate.moisture.precip_mm[i],
            aridity: self.climate.moisture.aridity[i],
            humidity: self.climate.moisture.humidity[i],
            plate: self.tectonics.plate_of[i],
            plate_kind: self
                .tectonics
                .plates
                .get(self.tectonics.plate_of[i] as usize)
                .map(|p| p.kind.code())
                .unwrap_or("unknown"),
            stress: self.tectonics.stress[i].code(),
            uplift_m: self.tectonics.uplift[i],
            discharge_m3s: self.hydrology.discharge_m3s[i],
            river: self.river[i],
            lake: self.lake[i],
            delta: self.hydrology.delta[i],
            npp_g_m2_yr: self.ecosystems.npp_g_m2_yr[i],
            species: self.ecosystems.species_richness[i],
            carrying_capacity: self.ecosystems.carrying_capacity[i],
            landmass: self.terrain.landmass[i],
            crust_age_myr: self.terrain.crust_age_myr.get(i).copied().unwrap_or(0.0),
        })
    }

    /// Number of land cells.
    pub fn land_cells(&self) -> usize {
        self.terrain.elevation.iter().filter(|&&e| e > 0.0).count()
    }

    /// Number of distinct biomes over land.
    pub fn biome_share(&self, biome: Biome) -> f64 {
        self.biomes.land_share(biome)
    }

    /// Human-readable one-line summary.
    pub fn summary(&self) -> String {
        format!(
            "{}: {} cells, {:.1}% land, {} plates, {} lakes, {} km2 largest continent",
            self.name,
            self.stats.cells,
            self.stats.land_fraction * 100.0,
            self.stats.plates,
            self.stats.lakes,
            self.stats.largest_continent_km2.round()
        )
    }

    /// Tectonic stress at a cell.
    pub fn stress_at(&self, cell: u32) -> Stress {
        self.tectonics
            .stress
            .get(cell as usize)
            .copied()
            .unwrap_or(Stress::Quiet)
    }
}

fn compute_stats(
    sphere: &Sphere,
    tectonics: &Tectonics,
    terrain: &Terrain,
    hydrology: &Hydrology,
    climate: &Climate,
    biomes: &Biomes,
    ecosystems: &Ecosystems,
    ocean: &[bool],
    lake: &[bool],
    river: &[bool],
) -> WorldStats {
    let n = sphere.len();
    let mut stats = WorldStats {
        cells: n as u32,
        ..WorldStats::default()
    };
    let radius_m = terrain.radius_km * 1_000.0;
    let mut min_e = f64::MAX;
    let mut max_e = f64::MIN;
    let mut land_e_sum = 0.0;
    let mut temp_sum = 0.0;
    let mut precip_sum = 0.0;
    let mut precip_n = 0u64;
    let mut species_sum = 0.0;
    for i in 0..n {
        let e = terrain.elevation[i];
        min_e = min_e.min(e);
        max_e = max_e.max(e);
        if ocean[i] {
            stats.ocean_cells += 1;
        } else {
            stats.land_cells += 1;
            land_e_sum += e;
            precip_sum += climate.moisture.precip_mm[i];
            precip_n += 1;
            species_sum += f64::from(ecosystems.species_richness[i]);
        }
        if lake[i] {
            stats.lake_cells += 1;
        }
        if river[i] {
            stats.river_cells += 1;
        }
        if hydrology.delta[i] {
            stats.delta_cells += 1;
        }
        temp_sum += climate.temperature.mean_c[i];
    }
    stats.min_elevation = if min_e.is_finite() { min_e } else { 0.0 };
    stats.max_elevation = if max_e.is_finite() { max_e } else { 0.0 };
    stats.mean_land_elevation = if stats.land_cells > 0 {
        land_e_sum / f64::from(stats.land_cells)
    } else {
        0.0
    };
    stats.land_fraction = f64::from(stats.land_cells) / n.max(1) as f64;
    stats.mean_temperature_c = if n > 0 { temp_sum / n as f64 } else { 0.0 };
    stats.mean_precip_mm = if precip_n > 0 {
        precip_sum / precip_n as f64
    } else {
        0.0
    };
    stats.mean_species = if stats.land_cells > 0 {
        species_sum / f64::from(stats.land_cells)
    } else {
        0.0
    };
    stats.lakes = hydrology.lakes.len() as u32;
    stats.largest_lake_km2 = hydrology
        .lakes
        .largest()
        .map(|l| l.area_m2 / 1_000_000.0)
        .unwrap_or(0.0);
    stats.plates = tectonics.plates.len() as u32;
    let (c, d, t) = tectonics.boundary_counts();
    stats.convergent = c as u32;
    stats.divergent = d as u32;
    stats.transform = t as u32;
    stats.continents = terrain.landmass_count;
    stats.largest_continent_km2 = terrain
        .largest_landmass(sphere)
        .map(|(_, a)| a * radius_m * radius_m / 1_000_000.0)
        .unwrap_or(0.0);
    stats.biome_counts = biomes.land_counts.clone();
    stats.land_biomes = stats.biome_counts.len() as u32;
    stats.carrying_capacity = ecosystems.total_carrying_capacity(biomes);
    stats.npp_t_c_yr = ecosystems.total_npp_t_c_yr;
    stats
}

fn compute_hashes(
    config: &WorldConfig,
    sphere: &Sphere,
    tectonics: &Tectonics,
    hydrology: &Hydrology,
    climate: &Climate,
    biomes: &Biomes,
    ecosystems: &Ecosystems,
    lake: &[bool],
    river: &[bool],
) -> WorldHashes {
    let mut hp = Hasher64::with_domain("plates");
    hp.write_u64(config.seed);
    hp.write_u32_slice(
        &tectonics
            .plate_of
            .iter()
            .map(|&p| u32::from(p))
            .collect::<Vec<_>>(),
    );
    for p in &tectonics.plates {
        hp.write_f64(p.motion.x);
        hp.write_f64(p.motion.y);
        hp.write_f64(p.motion.z);
        hp.write_f64(p.area);
    }
    hp.write_f64_slice(&tectonics.uplift);

    let mut ht = Hasher64::with_domain("terrain");
    ht.write_f64_slice(&hydrology.elevation);
    for &l in lake {
        ht.write_bool(l);
    }

    let mut hh = Hasher64::with_domain("hydrology");
    hydrology.hash_into(&mut hh);
    for &r in river {
        hh.write_bool(r);
    }

    let mut hc = Hasher64::with_domain("climate");
    climate.hash_into(&mut hc);

    let mut hb = Hasher64::with_domain("biosphere");
    biomes.hash_into(&mut hb);
    ecosystems.hash_into(&mut hb);

    let mut hall = Hasher64::with_domain("world");
    hall.write_u64(config.seed);
    hall.write_u32(config.subdiv);
    hall.write_u32(config.plates);
    hall.write_u32(config.years);
    hall.write_f64(config.land_fraction);
    hall.write_u32(sphere.len() as u32);
    hall.write_u64(hp.finish());
    hall.write_u64(ht.finish());
    hall.write_u64(hh.finish());
    hall.write_u64(hc.finish());
    hall.write_u64(hb.finish());

    WorldHashes {
        plates: hex(hp.finish()),
        terrain: hex(ht.finish()),
        hydrology: hex(hh.finish()),
        climate: hex(hc.finish()),
        biosphere: hex(hb.finish()),
        combined: hex(hall.finish()),
    }
}

const NAME_ONSETS: [&str; 16] = [
    "ka", "th", "ve", "or", "sol", "mar", "ny", "ael", "tor", "lys", "hal", "dra", "in", "ur",
    "ser", "mon",
];
const NAME_VOWELS: [&str; 8] = ["a", "e", "i", "o", "a", "e", "io", "ae"];
const NAME_CODAS: [&str; 12] = [
    "n", "th", "r", "l", "s", "m", "nd", "rk", "s", "n", "ra", "li",
];

/// Generates a deterministic planet name from a seed.
pub fn planet_name(seed: u64) -> String {
    let mut rng = Rng::new(seed, Stream::COSMETIC);
    let parts = 2 + (rng.below(2) as usize);
    let mut name = String::new();
    for _ in 0..parts {
        name.push_str(NAME_ONSETS[rng.below(NAME_ONSETS.len() as u64) as usize]);
        name.push_str(NAME_VOWELS[rng.below(NAME_VOWELS.len() as u64) as usize]);
        if rng.chance(0.65) {
            name.push_str(NAME_CODAS[rng.below(NAME_CODAS.len() as u64) as usize]);
        }
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out
        }
        None => "Terra".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small(seed: u64) -> World {
        World::generate(WorldConfig {
            seed,
            subdiv: 2,
            years: 500,
            erosion_iterations: 4,
            ..WorldConfig::default()
        })
    }

    #[test]
    fn generation_completes_and_is_consistent() {
        let w = small(3);
        assert!(!w.is_empty());
        assert_eq!(w.stats.cells as usize, w.len());
        assert_eq!(w.stats.land_cells + w.stats.ocean_cells, w.stats.cells);
        assert!(w.stats.lakes <= w.stats.lake_cells as u32 || w.stats.lake_cells == 0);
        assert!(w.stats.river_cells > 0);
        assert!(w.stats.land_fraction > 0.05 && w.stats.land_fraction < 0.95);
        assert!(w.stats.max_elevation > 100.0);
        assert!(w.stats.min_elevation < 0.0);
        assert_eq!(
            w.stats.biome_counts.values().sum::<u32>(),
            w.stats.land_cells
        );
        assert!(!w.hashes.combined.is_empty());
    }

    #[test]
    fn layer_lengths_agree() {
        let w = small(5);
        let n = w.len();
        assert_eq!(w.terrain.elevation.len(), n);
        assert_eq!(w.hydrology.elevation.len(), n);
        assert_eq!(w.climate.temperature.mean_c.len(), n);
        assert_eq!(w.climate.moisture.precip_mm.len(), n);
        assert_eq!(w.biomes.biome.len(), n);
        assert_eq!(w.ecosystems.npp_g_m2_yr.len(), n);
        assert_eq!(w.river.len(), n);
        assert_eq!(w.lake.len(), n);
        assert_eq!(w.tectonics.plate_of.len(), n);
    }

    #[test]
    fn different_seeds_differ() {
        let a = small(7);
        let b = small(8);
        assert_ne!(a.hashes.combined, b.hashes.combined);
        assert_ne!(a.name, b.name);
    }

    #[test]
    fn same_seed_same_world() {
        let a = small(9);
        let b = small(9);
        assert_eq!(a.hashes, b.hashes);
        assert_eq!(a.name, b.name);
        assert_eq!(a.stats.biome_counts, b.stats.biome_counts);
    }

    #[test]
    fn rivers_and_lakes_are_on_land() {
        let w = small(11);
        for i in 0..w.len() {
            if w.river[i] {
                assert!(w.terrain.elevation[i] > 0.0);
                assert!(!w.lake[i]);
            }
            if w.lake[i] {
                assert!(w.terrain.elevation[i] > 0.0);
            }
        }
    }

    #[test]
    fn cell_info_matches_layers() {
        let w = small(13);
        let info = w.cell_info(0).unwrap();
        assert_eq!(info.cell, 0);
        assert_eq!(info.elevation.to_bits(), w.terrain.elevation[0].to_bits());
        assert_eq!(info.biome, w.biomes.biome[0].code());
        assert!(w.cell_info(9_999_999).is_none());
    }

    #[test]
    fn config_is_sanitized() {
        let c = WorldConfig {
            seed: 1,
            subdiv: 99,
            years: 1,
            plates: 500,
            land_fraction: f64::NAN,
            erosion_iterations: 0,
            temperature_offset_c: 99.0,
            precip_target_mm: -5.0,
        }
        .sanitized();
        assert_eq!(c.subdiv, 6);
        assert_eq!(c.years, 100);
        assert_eq!(c.plates, 20);
        assert_eq!(c.land_fraction, 0.31);
        assert_eq!(c.erosion_iterations, 1);
        assert!(c.temperature_offset_c <= 12.0);
        assert!(c.precip_target_mm >= 200.0);
    }

    #[test]
    fn planet_names_are_stable() {
        assert_eq!(planet_name(42), planet_name(42));
        assert!(!planet_name(42).is_empty());
        assert!(planet_name(42).chars().next().unwrap().is_uppercase());
        let names: std::collections::BTreeSet<String> = (0..16).map(|s| planet_name(s)).collect();
        assert!(names.len() > 10);
    }

    #[test]
    fn summary_is_readable() {
        let w = small(17);
        let s = w.summary();
        assert!(s.contains(&w.name));
        assert!(s.contains("cells"));
        assert_eq!(w.stress_at(0), w.tectonics.stress[0]);
        assert_eq!(w.stress_at(9_999), Stress::Quiet);
    }
}
