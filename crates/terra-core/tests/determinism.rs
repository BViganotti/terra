//! Determinism acceptance tests.
//!
//! The first acceptance criterion is that the same seed and configuration
//! produce bit-identical worlds. These tests generate each seed twice and
//! compare every published hash, not just the combined digest.

mod common;

use terra_core::world::World;

#[test]
fn ten_seeds_replay_identically() {
    for seed in 0..10u64 {
        let a = common::world(seed, 3);
        let b = common::world(seed, 3);
        assert_eq!(a.name, b.name, "seed {seed} name");
        assert_eq!(a.hashes, b.hashes, "seed {seed} hashes");
        assert_eq!(a.stats.biome_counts, b.stats.biome_counts);
        assert_eq!(a.stats.land_cells, b.stats.land_cells);
        assert_eq!(a.stats.lakes, b.stats.lakes);
        for (x, y) in a.terrain.elevation.iter().zip(&b.terrain.elevation) {
            assert_eq!(x.to_bits(), y.to_bits(), "seed {seed} elevation bits");
        }
        for (x, y) in a
            .climate
            .moisture
            .precip_mm
            .iter()
            .zip(&b.climate.moisture.precip_mm)
        {
            assert_eq!(x.to_bits(), y.to_bits(), "seed {seed} precipitation bits");
        }
    }
}

#[test]
fn different_seeds_have_different_histories() {
    let mut hashes = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for seed in 0..8u64 {
        let w = common::world(seed, 3);
        hashes.insert(w.hashes.combined.clone());
        names.insert(w.name.clone());
    }
    assert_eq!(hashes.len(), 8, "seeds collided on a world hash");
    assert!(names.len() >= 7, "planet names repeat too often: {names:?}");
}

#[test]
fn configuration_changes_the_hash() {
    let base = World::generate(common::config(5, 3));
    let mut different_land = common::config(5, 3);
    different_land.land_fraction = 0.55;
    let other = World::generate(different_land);
    assert_ne!(base.hashes.combined, other.hashes.combined);
    assert_ne!(base.hashes.terrain, other.hashes.terrain);

    let mut different_plates = common::config(5, 3);
    different_plates.plates = 14;
    let third = World::generate(different_plates);
    assert_ne!(base.hashes.plates, third.hashes.plates);
    assert_ne!(base.hashes.combined, third.hashes.combined);
}

#[test]
fn hashes_are_sixteen_hex_digits() {
    let w = common::world(11, 2);
    for field in [
        &w.hashes.plates,
        &w.hashes.terrain,
        &w.hashes.hydrology,
        &w.hashes.climate,
        &w.hashes.biosphere,
        &w.hashes.combined,
    ] {
        assert_eq!(field.len(), 16);
        assert!(field.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
