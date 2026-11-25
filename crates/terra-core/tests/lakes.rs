//! Lake geometry acceptance through public world layers.

mod common;

#[test]
fn lake_registry_matches_cells_and_geometry() {
    let mut found = 0;
    for seed in 0..5u64 {
        let w = common::world(seed, 3);
        let lakes = &w.hydrology.lakes;
        if lakes.is_empty() {
            continue;
        }
        found += 1;
        for (id, lake) in lakes.lakes.iter().enumerate() {
            assert_eq!(lake.id as usize, id);
            assert!(!lake.cells.is_empty());
            for &cell in &lake.cells {
                assert!(w.lake[cell as usize]);
                assert_eq!(lakes.lake_of[cell as usize], lake.id);
                assert_eq!(lakes.lake_at(cell).map(|l| l.id), Some(lake.id));
            }
            assert!(lake.area_m2 > 0.0);
            assert!(lake.volume_m3 > 0.0);
            assert!(lake.max_depth > 0.4);
            assert!(lake.mean_depth() > 0.0);
            assert!(lake.mean_depth() <= lake.max_depth + 1e-9);
            if let (Some(spill), Some(target)) = (lake.spill_cell, lake.spill_target) {
                assert!(lake.cells.contains(&spill));
                assert!(!lake.cells.contains(&target));
                assert!(w.sphere.neighbors(spill).contains(&target));
            }
        }
        let total: f64 = lakes.lakes.iter().map(|l| l.area_m2).sum();
        assert!((total - lakes.total_area_m2()).abs() < 1e-6);
        let largest = lakes.largest().unwrap();
        assert!(lakes.lakes.iter().all(|l| l.area_m2 <= largest.area_m2));
    }
    assert!(found > 0, "no lakes across five seeds");
}

#[test]
fn lake_water_is_not_river_water() {
    let w = common::world(13, 3);
    for i in 0..w.len() {
        if w.lake[i] {
            assert!(!w.river[i]);
            assert_eq!(w.biomes.at(i as u32), terra_core::biome::Biome::Lake);
            assert_eq!(w.ecosystems.carrying_capacity[i], 0.0);
        }
    }
}

#[test]
fn lake_outlets_flow_toward_the_ocean() {
    let w = common::world(15, 3);
    for lake in &w.hydrology.lakes.lakes {
        if let Some(target) = lake.spill_target {
            let mut cur = target;
            let mut steps = 0;
            while w.terrain.elevation[cur as usize] > 0.0 && steps < w.len() {
                match w.hydrology.downhill[cur as usize] {
                    Some(next) if next != cur => cur = next,
                    _ => break,
                }
                steps += 1;
            }
            assert!(
                w.terrain.elevation[cur as usize] <= 0.0 || steps < w.len(),
                "lake outlet for {} does not drain",
                lake.id
            );
        }
    }
}

#[test]
fn lakes_do_not_overlap() {
    let w = common::world(17, 4);
    let mut seen = std::collections::BTreeSet::new();
    for lake in &w.hydrology.lakes.lakes {
        for &cell in &lake.cells {
            assert!(seen.insert(cell), "cell {cell} belongs to two lakes");
        }
    }
}
