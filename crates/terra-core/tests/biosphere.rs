//! Biome and ecosystem acceptance tests.

mod common;

use terra_core::biome::Biome;

#[test]
fn water_and_land_are_classified_consistently() {
    for seed in 0..3u64 {
        let w = common::world(seed, 3);
        for i in 0..w.len() {
            let b = w.biomes.biome[i];
            if w.terrain.elevation[i] <= 0.0 {
                assert_eq!(b, Biome::Ocean, "seed {seed} cell {i}");
                assert_eq!(w.ecosystems.npp_g_m2_yr[i], 0.0);
                assert_eq!(w.ecosystems.carrying_capacity[i], 0.0);
            } else if w.lake[i] {
                assert_eq!(b, Biome::Lake);
                assert!(w.ecosystems.carrying_capacity[i] <= 1.0);
            } else {
                assert!(b.is_land(), "seed {seed} land cell {i} is {b:?}");
            }
        }
        let counted: u32 = w.stats.biome_counts.values().sum();
        // Land biome counts exclude lake water, which is still "land" by
        // elevation; the two categories together cover every positive cell.
        assert_eq!(counted + w.stats.lake_cells, w.stats.land_cells);
    }
}

#[test]
fn biome_metadata_is_unique_and_complete() {
    let mut codes = std::collections::BTreeSet::new();
    let mut labels = std::collections::BTreeSet::new();
    let mut colors = std::collections::BTreeSet::new();
    for b in Biome::ALL {
        assert!(codes.insert(b.code()));
        assert!(labels.insert(b.label()));
        assert!(colors.insert(b.rgb()));
        assert_eq!(Biome::from_index(b.index()), b);
        assert!((0.0..=1.0).contains(&b.fertility()));
    }
    assert_eq!(codes.len(), 17);
}

#[test]
fn productivity_and_richness_are_bounded() {
    let w = common::world(5, 3);
    for i in 0..w.len() {
        assert!((0.0..=3_000.0).contains(&w.ecosystems.npp_g_m2_yr[i]));
        assert!(w.ecosystems.species_richness[i] <= 400);
        if w.terrain.elevation[i] > 0.0 && !w.lake[i] {
            assert!(w.ecosystems.species_richness[i] >= 5);
        } else {
            assert_eq!(w.ecosystems.species_richness[i], 0);
        }
        assert!((0.0..=1.0).contains(&w.ecosystems.fragility[i]));
        assert!(w.ecosystems.carrying_capacity[i].is_finite());
        assert!(w.ecosystems.carrying_capacity[i] >= 0.0);
    }
    assert!(w.ecosystems.total_npp_t_c_yr > 0.0);
    assert!(w.ecosystems.mean_land_npp(&w.biomes) > 0.0);
    assert!(w.ecosystems.total_carrying_capacity(&w.biomes) > 0.0);
}

#[test]
fn patches_are_connected_and_partition_the_grid() {
    let w = common::world(7, 3);
    let total: u32 = w.biomes.patch_size.iter().sum();
    assert_eq!(total as usize, w.len());
    for i in 0..w.len() {
        let patch = w.biomes.patch[i] as usize;
        assert_eq!(w.biomes.patch_size[patch] >= 1, true);
        for &nb in w.sphere.neighbors(i as u32) {
            if w.biomes.biome[nb as usize] == w.biomes.biome[i] {
                assert_eq!(w.biomes.patch[nb as usize], patch as u32);
            }
        }
    }
}

#[test]
fn forests_are_richer_than_deserts_when_both_exist() {
    let mut compared = false;
    for seed in 0..5u64 {
        let w = common::world(seed, 4);
        let mut forest = (0.0, 0u32);
        let mut desert = (0.0, 0u32);
        for i in 0..w.len() {
            match w.biomes.biome[i] {
                Biome::TropicalRainforest | Biome::TemperateRainforest => {
                    forest.0 += f64::from(w.ecosystems.species_richness[i]);
                    forest.1 += 1;
                }
                Biome::SubtropicalDesert | Biome::TemperateDesert => {
                    desert.0 += f64::from(w.ecosystems.species_richness[i]);
                    desert.1 += 1;
                }
                _ => {}
            }
        }
        if forest.1 > 0 && desert.1 > 0 {
            compared = true;
            assert!(
                forest.0 / f64::from(forest.1) > desert.0 / f64::from(desert.1),
                "seed {seed}: forests are not richer than deserts"
            );
        }
    }
    assert!(compared, "no seed produced both forest and desert");
}

#[test]
fn lakes_are_lake_biome_and_have_no_capacity() {
    let w = common::world(9, 4);
    if w.stats.lake_cells == 0 {
        return;
    }
    for i in 0..w.len() {
        if w.lake[i] {
            assert_eq!(w.biomes.biome[i], Biome::Lake);
            assert_eq!(w.ecosystems.carrying_capacity[i], 0.0);
            assert_eq!(w.biomes.at(i as u32).is_water(), true);
        }
    }
}
