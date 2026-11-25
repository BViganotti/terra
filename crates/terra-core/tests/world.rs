//! Whole-world acceptance tests.

mod common;

use terra_core::tectonics::Stress;
use terra_core::world::World;

#[test]
fn every_layer_has_the_same_length() {
    let w = common::world(3, 3);
    let n = w.len();
    assert_eq!(w.sphere.len(), n);
    assert_eq!(w.tectonics.len(), n);
    assert_eq!(w.terrain.elevation.len(), n);
    assert_eq!(w.hydrology.elevation.len(), n);
    assert_eq!(w.climate.temperature.mean_c.len(), n);
    assert_eq!(w.climate.moisture.precip_mm.len(), n);
    assert_eq!(w.biomes.biome.len(), n);
    assert_eq!(w.ecosystems.npp_g_m2_yr.len(), n);
    assert_eq!(w.river.len(), n);
    assert_eq!(w.lake.len(), n);
}

#[test]
fn statistics_describe_the_layers() {
    let w = common::world(5, 4);
    let s = &w.stats;
    assert_eq!(s.cells as usize, w.len());
    assert_eq!(s.land_cells + s.ocean_cells, s.cells);
    assert!(s.lake_cells <= s.land_cells);
    let biome_cells: u32 = s.biome_counts.values().sum();
    assert_eq!(biome_cells + s.lake_cells, s.land_cells);
    assert!((0.0..=1.0).contains(&s.land_fraction));
    assert!(s.max_elevation > s.min_elevation);
    assert!(s.convergent + s.divergent + s.transform > 0);
    assert!(s.plates >= 3);
    assert!(s.continents >= 1);
    assert!(s.carrying_capacity >= 0.0);
    assert!(s.npp_t_c_yr >= 0.0);
    assert!(s.mean_species >= 0.0);
    assert!(s.mean_temperature_c.is_finite());
    assert!(s.mean_precip_mm.is_finite());
}

#[test]
fn cell_info_agrees_with_every_layer() {
    let w = common::world(7, 3);
    for cell in [0u32, 1, 100, (w.len() / 2) as u32, (w.len() - 1) as u32] {
        let info = w.cell_info(cell).unwrap_or_else(|| panic!("cell {cell}"));
        assert_eq!(info.cell, cell);
        assert_eq!(info.elevation, w.terrain.elevation[cell as usize]);
        assert_eq!(info.biome, w.biomes.biome[cell as usize].code());
        assert_eq!(
            info.temperature_c,
            w.climate.temperature.mean_c[cell as usize]
        );
        assert_eq!(info.precip_mm, w.climate.moisture.precip_mm[cell as usize]);
        assert_eq!(info.plate, w.tectonics.plate_of[cell as usize]);
        assert_eq!(info.species, w.ecosystems.species_richness[cell as usize]);
        assert_eq!(info.river, w.river[cell as usize]);
        assert_eq!(info.lake, w.lake[cell as usize]);
        assert_eq!(info.delta, w.hydrology.delta[cell as usize]);
        assert_eq!(info.landmass, w.terrain.landmass[cell as usize]);
    }
    assert!(w.cell_info(u32::MAX).is_none());
}

#[test]
fn world_is_in_a_physically_plausible_range() {
    let w = common::world(9, 4);
    assert!(w.stats.land_fraction > 0.1 && w.stats.land_fraction < 0.8);
    assert!(w.stats.max_elevation > 500.0 && w.stats.max_elevation < 12_000.0);
    assert!(w.stats.min_elevation < -1_000.0 && w.stats.min_elevation > -12_000.0);
    assert!(w.stats.mean_temperature_c > -30.0 && w.stats.mean_temperature_c < 35.0);
    assert!(w.stats.mean_precip_mm > 300.0 && w.stats.mean_precip_mm < 2_000.0);
    assert!(w.stats.river_cells > 0);
    assert!(w.stats.largest_continent_km2 > 0.0);
    assert!(w.stats.plates >= 3 && w.stats.plates <= 20);
}

#[test]
fn stress_accessor_and_summary_are_safe() {
    let w = common::world(11, 2);
    for cell in 0..w.len() as u32 {
        assert_eq!(w.stress_at(cell), w.tectonics.stress[cell as usize]);
    }
    assert_eq!(w.stress_at(u32::MAX), Stress::Quiet);
    let summary = w.summary();
    assert!(summary.contains(&w.name));
    assert!(summary.contains("cells"));
    assert!(summary.contains("plates"));
    assert!(!w.is_empty());
    assert_eq!(w.land_cells(), w.stats.land_cells as usize);
}

#[test]
fn biome_shares_sum_to_one_over_land() {
    let w = common::world(13, 3);
    let mut share = 0.0;
    for biome in terra_core::biome::Biome::ALL {
        if biome.is_land() {
            share += w.biome_share(biome);
        }
    }
    assert!((share - 1.0).abs() < 1e-9, "share {share}");
}

#[test]
fn world_generation_reports_progress_monotonically() {
    let mut stages = Vec::new();
    let mut last = -1.0;
    let world = World::generate_with(common::config(15, 2), |stage, progress| {
        assert!(progress >= last, "progress went backwards at {stage}");
        assert!((0.0..=1.0).contains(&progress));
        last = progress;
        stages.push(stage.to_string());
    });
    assert_eq!(world.len(), 162);
    assert!(stages.len() >= 6, "{stages:?}");
    assert_eq!(stages.last().map(String::as_str), Some("complete"));
}
