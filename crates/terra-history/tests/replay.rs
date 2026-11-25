//! Replay acceptance for civilizations and the chronicle.

use terra_core::world::{World, WorldConfig};
use terra_history::civ::sim::{simulate, HistoryConfig};

fn run(seed: u64) -> terra_history::civ::History {
    let world = World::generate(WorldConfig {
        seed,
        subdiv: 3,
        years: 2_500,
        erosion_iterations: 5,
        ..WorldConfig::default()
    });
    simulate(
        &world,
        HistoryConfig {
            seed,
            years: 2_500,
            initial_civs: 6,
            snapshot_interval: 100,
            ..HistoryConfig::default()
        },
    )
}

#[test]
fn same_seed_replays_identical_history() {
    for seed in [3u64, 7, 11] {
        let a = run(seed);
        let b = run(seed);
        assert_eq!(a.hashes, b.hashes, "seed {seed} hashes");
        assert_eq!(a.chronicle.len(), b.chronicle.len(), "seed {seed} events");
        assert_eq!(a.civs.len(), b.civs.len());
        for (x, y) in a.civs.iter().zip(&b.civs) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.cells, y.cells);
            assert_eq!(x.population.to_bits(), y.population.to_bits());
            assert_eq!(x.tech, y.tech);
            assert_eq!(x.alive, y.alive);
        }
        assert_eq!(a.stats.events, b.stats.events);
        assert_eq!(a.stats.wars, b.stats.wars);
        assert_eq!(a.snapshots.len(), b.snapshots.len());
    }
}

#[test]
fn different_seeds_produce_different_histories() {
    let a = run(13);
    let b = run(14);
    assert_ne!(a.hashes.combined, b.hashes.combined);
    assert_ne!(a.hashes.chronicle, b.hashes.chronicle);
}

#[test]
fn chronicle_hash_changes_with_events() {
    use terra_history::chronicle::{Cause, EventKind, StateFact};
    let a = run(17);
    let mut b = run(17);
    assert!(b.verify());
    b.chronicle
        .record_simple(
            b.config.years,
            EventKind::GoldenAge,
            "An appended test event",
            "This event exists only to prove the hash covers the event stream.",
            Cause::State(StateFact::global("test", 1.0, "", "test fact")),
            0.5,
        )
        .unwrap();
    let recomputed = b.recompute_hashes();
    assert_ne!(a.hashes.chronicle, recomputed.chronicle);
    assert_ne!(a.hashes.combined, recomputed.combined);
    assert!(b.is_sound());
    // Stored hashes now describe the old content, so verification fails until
    // they are refreshed.
    assert!(!b.verify());
    b.hashes = recomputed;
    assert!(b.verify());
}
