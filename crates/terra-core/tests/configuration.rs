//! Configuration acceptance: land fraction, plate count, subdivision and
//! ocean/land invariants hold across the supported range.

mod common;

use terra_core::world::{World, WorldConfig};

#[test]
fn land_fraction_targets_are_respected() {
    for target in [0.12, 0.25, 0.4, 0.6, 0.72] {
        let world = World::generate(WorldConfig {
            seed: 31,
            subdiv: 3,
            years: 500,
            land_fraction: target,
            erosion_iterations: 4,
            ..WorldConfig::default()
        });
        // Erosion and delta deposition move the coastline slightly; 0.03 is a
        // generous bound that still catches a broken quantile.
        assert!(
            (world.stats.land_fraction - target).abs() < 0.035,
            "target {target}, realized {}",
            world.stats.land_fraction
        );
        assert!(world.stats.land_cells > 0);
        assert!(world.stats.ocean_cells > 0);
        assert!(world.stats.continents >= 1);
    }
}

#[test]
fn plate_counts_are_honored() {
    for plates in [3, 5, 9, 14, 20] {
        let world = World::generate(WorldConfig {
            seed: 4,
            subdiv: 3,
            years: 500,
            plates,
            erosion_iterations: 3,
            ..WorldConfig::default()
        });
        assert_eq!(world.stats.plates, plates);
        assert_eq!(world.tectonics.plates.len(), plates as usize);
        let cells: u32 = world.tectonics.plates.iter().map(|p| p.cell_count).sum();
        assert_eq!(cells as usize, world.len());
    }
}

#[test]
fn subdivision_levels_change_grid_size_not_correctness() {
    for (level, expected) in [(2u32, 162usize), (3, 642), (4, 2_562)] {
        let world = common::world(8, level);
        assert_eq!(world.len(), expected);
        world.sphere.validate().unwrap();
        assert!((world.tectonics.total_area() - 4.0 * std::f64::consts::PI).abs() < 1e-6);
        assert!(world.stats.river_cells > 0);
    }
}

#[test]
fn extreme_but_valid_parameters_are_safe() {
    let world = World::generate(WorldConfig {
        seed: u64::from(u32::MAX),
        subdiv: 2,
        years: 100,
        plates: 3,
        land_fraction: 0.75,
        erosion_iterations: 1,
        temperature_offset_c: -12.0,
        precip_target_mm: 200.0,
    });
    assert!(world.stats.land_fraction > 0.6);
    assert!(world.stats.mean_precip_mm > 150.0);
    assert!(world.hashes.combined.len() == 16);
    assert!(world.is_empty() == false);
    assert_eq!(world.len(), 162);
}

#[test]
fn configuration_sanitization_never_hangs_or_panics() {
    let mut state = 0x1234_5678u64;
    for _ in 0..100 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let cfg = WorldConfig {
            seed: state,
            subdiv: (state % 20) as u32,
            years: (state % 50_000) as u32,
            plates: (state % 40) as u32,
            land_fraction: ((state >> 8) % 1000) as f64 / 500.0,
            erosion_iterations: (state % 100) as u32,
            temperature_offset_c: ((state >> 16) % 400) as f64 - 200.0,
            precip_target_mm: ((state >> 24) % 5000) as f64,
        }
        .sanitized();
        assert!((2..=6).contains(&cfg.subdiv));
        assert!((100..=20_000).contains(&cfg.years));
        assert!((3..=20).contains(&cfg.plates));
        assert!((0.1..=0.75).contains(&cfg.land_fraction));
        assert!((1..=24).contains(&cfg.erosion_iterations));
        assert!((-12.0..=12.0).contains(&cfg.temperature_offset_c));
        assert!((200.0..=2500.0).contains(&cfg.precip_target_mm));
    }
}
