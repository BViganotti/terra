//! Long-run acceptance: a ten-thousand-year history must complete, stay sound
//! and keep working memory bounded (event and snapshot caps are enforced by the
//! simulation, not by luck).

use std::time::Instant;

use terra_core::world::{World, WorldConfig};
use terra_history::chronicle::MAX_EVENTS;
use terra_history::civ::sim::{simulate, HistoryConfig};

#[test]
fn ten_thousand_years_completes_and_stays_sound() {
    let world = World::generate(WorldConfig {
        seed: 42,
        subdiv: 3,
        years: 10_000,
        erosion_iterations: 6,
        ..WorldConfig::default()
    });
    let started = Instant::now();
    let history = simulate(
        &world,
        HistoryConfig {
            seed: 42,
            years: 10_000,
            initial_civs: 6,
            snapshot_interval: 100,
            ..HistoryConfig::default()
        },
    );
    let elapsed = started.elapsed();
    assert!(history.is_sound(), "{:?}", history.problems());
    assert!(history.chronicle.len() > 50);
    assert!((history.chronicle.len()) <= MAX_EVENTS);
    assert_eq!(history.config.years, 10_000);
    assert_eq!(history.snapshots.last().map(|s| s.year), Some(10_000));
    assert!(history.snapshots.len() <= 130);
    assert!(history.civs.len() >= 2);
    assert!(history.stats.events > 0);
    assert!(history.verify());
    // The run should finish quickly even in a debug build. The benchmark
    // measures release performance; this bound only catches accidental
    // super-linear behavior.
    assert!(
        elapsed.as_secs() < 60,
        "10k-year debug run took {elapsed:?}"
    );
}

#[test]
fn long_run_has_no_runaway_populations() {
    let world = World::generate(WorldConfig {
        seed: 7,
        subdiv: 2,
        years: 10_000,
        erosion_iterations: 3,
        ..WorldConfig::default()
    });
    let history = simulate(
        &world,
        HistoryConfig {
            seed: 7,
            years: 10_000,
            initial_civs: 4,
            snapshot_interval: 500,
            ..HistoryConfig::default()
        },
    );
    for civ in &history.civs {
        assert!(civ.population.is_finite() && civ.population >= 0.0);
        assert!(civ.cohesion >= 0.0 && civ.cohesion <= 1.0);
        assert!(civ.tech <= terra_history::civ::MAX_TECH);
        assert!(civ.cells.len() <= world.len());
        assert!(civ.treasury.is_finite());
    }
    assert!(history.stats.total_population.is_finite());
    assert!(history.stats.total_population >= 0.0);
}
