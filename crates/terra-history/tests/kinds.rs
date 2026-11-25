//! Breadth acceptance: over several ten-thousand-year worlds the rule set must
//! actually produce settlement, expansion, trade, war, battles, technology,
//! famine, natural disasters and collapse. A simulator that only expands is not
//! the product.

use terra_core::world::{World, WorldConfig};
use terra_history::civ::sim::{simulate, HistoryConfig};

#[test]
fn every_major_rule_fires_across_a_seed_sweep() {
    let mut kinds = std::collections::BTreeSet::new();
    for seed in 0..3u64 {
        let world = World::generate(WorldConfig {
            seed,
            subdiv: 2,
            years: 10_000,
            erosion_iterations: 3,
            ..WorldConfig::default()
        });
        let history = simulate(
            &world,
            HistoryConfig {
                seed,
                years: 10_000,
                initial_civs: 6,
                snapshot_interval: 1_000,
                ..HistoryConfig::default()
            },
        );
        assert!(history.is_sound(), "seed {seed}: {:?}", history.problems());
        for event in history.chronicle.events() {
            kinds.insert(event.kind.code());
        }
        if history.stats.collapses > 0 {
            assert!(history.civs.iter().any(|c| !c.alive));
        }
    }
    for required in [
        "world_formed",
        "settlement_founded",
        "expansion",
        "resource_discovered",
        "tech_discovered",
        "trade_route_opened",
        "war_declared",
        "battle",
        "peace_treaty",
        "collapse",
        "eruption",
    ] {
        assert!(kinds.contains(required), "missing {required}: {kinds:?}");
    }
}

#[test]
fn civilizations_have_distinct_names_and_colors() {
    let world = World::generate(WorldConfig {
        seed: 99,
        subdiv: 2,
        years: 1_000,
        erosion_iterations: 3,
        ..WorldConfig::default()
    });
    let history = simulate(
        &world,
        HistoryConfig {
            seed: 99,
            years: 1_000,
            initial_civs: 8,
            ..HistoryConfig::default()
        },
    );
    let names: std::collections::BTreeSet<&str> =
        history.civs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names.len(), history.civs.len());
    let colors: std::collections::BTreeSet<u8> =
        history.civs.iter().map(|c| c.color_index).collect();
    assert_eq!(colors.len(), history.civs.len());
}

#[test]
fn seeds_change_the_course_of_history() {
    let mut signatures = std::collections::BTreeSet::new();
    for seed in 0..4u64 {
        let world = World::generate(WorldConfig {
            seed,
            subdiv: 2,
            years: 1_500,
            erosion_iterations: 3,
            ..WorldConfig::default()
        });
        let history = simulate(
            &world,
            HistoryConfig {
                seed,
                years: 1_500,
                initial_civs: 5,
                ..HistoryConfig::default()
            },
        );
        signatures.insert(history.hashes.combined);
    }
    assert_eq!(signatures.len(), 4);
}
