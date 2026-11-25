//! Demographic and social bounds across long runs.

use terra_core::world::{World, WorldConfig};
use terra_history::civ::sim::{simulate, HistoryConfig};
use terra_history::civ::{Resource, MAX_CITIES, MAX_TECH};

fn history(seed: u64) -> terra_history::civ::History {
    let world = World::generate(WorldConfig {
        seed,
        subdiv: 2,
        years: 4_000,
        erosion_iterations: 3,
        ..WorldConfig::default()
    });
    simulate(
        &world,
        HistoryConfig {
            seed,
            years: 4_000,
            initial_civs: 5,
            snapshot_interval: 200,
            ..HistoryConfig::default()
        },
    )
}

#[test]
fn civilizations_stay_within_social_bounds() {
    for seed in 0..3u64 {
        let h = history(seed);
        for civ in &h.civs {
            assert!(civ.population.is_finite() && civ.population >= 0.0);
            assert!(civ.cohesion >= 0.0 && civ.cohesion <= 1.0);
            assert!(civ.military >= 0.0 && civ.military.is_finite());
            assert!(civ.treasury >= 0.0 && civ.treasury.is_finite());
            assert!(civ.food >= 0.0 && civ.food <= 24.0);
            assert!(civ.tech <= MAX_TECH);
            assert!(civ.cities.len() <= MAX_CITIES);
            assert!(civ.cells.len() <= h.snapshots[0].owner.len());
            assert!(civ.founded_year <= h.config.years);
            if let Some(year) = civ.collapsed_year {
                assert!(year <= h.config.years);
                assert!(!civ.alive);
            }
            for resource in civ.resources.keys() {
                assert!(Resource::ALL.contains(resource));
            }
            let mut sorted = civ.cells.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, civ.cells);
        }
    }
}

#[test]
fn wars_are_symmetric_and_reference_known_civs() {
    for seed in 0..3u64 {
        let h = history(seed);
        for civ in &h.civs {
            for &enemy in &civ.at_war_with {
                assert!((enemy as usize) < h.civs.len());
                assert!(h.civs[enemy as usize].at_war_with.contains(&civ.id));
            }
        }
        for war in &h.wars {
            assert!((war.a as usize) < h.civs.len());
            assert!((war.b as usize) < h.civs.len());
            assert_ne!(war.a, war.b);
            assert!(war.casualties >= 0.0);
            if let Some(end) = war.end {
                assert!(end >= war.start);
            }
        }
    }
}

#[test]
fn stats_agree_with_records() {
    let h = history(7);
    assert_eq!(
        h.stats.surviving_civs as usize,
        h.civs.iter().filter(|c| c.alive).count()
    );
    assert_eq!(
        h.stats.cities as usize,
        h.civs.iter().map(|c| c.cities.len()).sum::<usize>()
    );
    assert_eq!(h.stats.events as usize, h.chronicle.len());
    assert_eq!(
        h.stats.max_tech,
        h.civs.iter().map(|c| c.tech).max().unwrap_or(0)
    );
    let alive = h.civs.iter().filter(|c| c.alive).count();
    if alive > 0 {
        let mean = h
            .civs
            .iter()
            .filter(|c| c.alive)
            .map(|c| f64::from(c.tech))
            .sum::<f64>()
            / alive as f64;
        assert!((h.stats.mean_tech - mean).abs() < 1e-9);
    }
}

#[test]
fn cities_are_owned_and_named() {
    let h = history(9);
    for civ in &h.civs {
        let mut names = std::collections::BTreeSet::new();
        for city in &civ.cities {
            assert!(!city.name.is_empty());
            assert!(city.population >= 0.0);
            assert!(names.insert(city.name.clone()), "duplicate city name");
            if civ.owns(city.cell) {
                assert!(city.cell < h.snapshots[0].owner.len() as u32);
            }
        }
    }
}
