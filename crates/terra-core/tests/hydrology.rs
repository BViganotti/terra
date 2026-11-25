//! Hydrology acceptance tests: downhill flow, basin lakes, drainage capture and
//! sediment mass balance.

mod common;

use terra_core::hydrology::SECONDS_PER_YEAR;

#[test]
fn rivers_never_run_uphill() {
    for seed in 0..4u64 {
        let w = common::world(seed, 3);
        let h = &w.hydrology;
        for i in 0..w.len() {
            if let Some(ds) = h.downhill[i] {
                let d = ds as usize;
                assert!(
                    h.filled[d] <= h.filled[i] + 1e-9,
                    "seed {seed}: filled rise {i}->{d}"
                );
                if h.elevation[i] > 0.0 && !w.lake[i] && !w.lake[d] {
                    assert!(
                        h.elevation[d] < h.elevation[i] + 1e-9,
                        "seed {seed}: original rise {i}->{d}"
                    );
                }
            }
        }
    }
}

#[test]
fn every_land_cell_reaches_the_sea_or_a_lake() {
    let w = common::world(7, 3);
    for i in 0..w.len() {
        if w.terrain.elevation[i] > 0.0 {
            assert!(
                w.hydrology.downhill[i].is_some() || w.lake[i],
                "cell {i} has no drainage"
            );
        }
    }
    // Following any path must terminate; the graph is a forest rooted at ocean.
    for start in (0..w.len() as u32).step_by(11) {
        let path = w.hydrology.path(start, w.len());
        assert!(path.len() <= w.len(), "cycle from {start}");
    }
}

#[test]
fn lakes_are_closed_basins_with_spill_points() {
    let w = common::world(9, 4);
    for lake in &w.hydrology.lakes.lakes {
        assert!(!lake.cells.is_empty());
        assert!(lake.area_m2 > 0.0);
        assert!(lake.volume_m3 > 0.0);
        assert!(lake.mean_depth() > 0.0);
        for &cell in &lake.cells {
            assert!(w.lake[cell as usize]);
            assert!(w.terrain.elevation[cell as usize] <= lake.surface + 0.02);
        }
        if let (Some(spill), Some(target)) = (lake.spill_cell, lake.spill_target) {
            assert!(lake.cells.contains(&spill));
            assert!(!lake.cells.contains(&target));
            assert!(w.sphere.neighbors(spill).contains(&target));
            assert!((lake.surface - lake.spill_level).abs() < 0.02);
        }
    }
}

#[test]
fn outlet_discharge_matches_land_precipitation() {
    let w = common::world(11, 3);
    let radius_m = w.terrain.radius_km * 1_000.0;
    let expected: f64 = (0..w.len())
        .filter(|&i| w.terrain.elevation[i] > 0.0)
        .map(|i| {
            let area = w.sphere.cell_area(i as u32) * radius_m * radius_m;
            area * w.climate.moisture.precip_mm[i] / 1_000.0 / SECONDS_PER_YEAR
        })
        .sum();
    let delivered: f64 = w
        .hydrology
        .outlets
        .iter()
        .map(|&c| w.hydrology.discharge_m3s[c as usize])
        .sum();
    assert!(
        common::rel_err(delivered, expected) < 0.03,
        "delivered {delivered} vs expected {expected}"
    );
    assert!(w.hydrology.discharge_m3s.iter().all(|&q| q >= 0.0));
    assert!(w.hydrology.max_discharge() > 0.0);
}

#[test]
fn sediment_budget_closes() {
    for seed in 0..3u64 {
        let w = common::world(seed, 3);
        let h = &w.hydrology;
        assert!(h.eroded_m3 > 0.0);
        assert!(h.exported_m3 > 0.0);
        let net = (h.eroded_m3 - h.exported_m3).abs().max(1.0);
        assert!(
            h.residual_m3.abs() / net < 0.01,
            "seed {seed}: residual {} net {net}",
            h.residual_m3
        );
        assert!(h.delta.iter().any(|&d| d), "seed {seed}: no deltas");
    }
}

#[test]
fn river_cells_carry_accumulated_area() {
    let w = common::world(13, 3);
    let radius_m = w.terrain.radius_km * 1_000.0;
    for i in 0..w.len() {
        if w.river[i] {
            let area = w.sphere.cell_area(i as u32) * radius_m * radius_m;
            assert!(w.terrain.elevation[i] > 0.0);
            assert!(!w.lake[i]);
            assert!(w.hydrology.drainage_area_m2[i] >= area * 2.2);
            assert!(w.hydrology.strahler[i] >= 1);
        }
    }
}

#[test]
fn discharge_increases_downstream() {
    let w = common::world(15, 3);
    for (i, ds) in w.hydrology.downhill.iter().enumerate() {
        if let Some(ds) = ds {
            assert!(
                w.hydrology.discharge_m3s[*ds as usize] + 1e-9 >= w.hydrology.discharge_m3s[i],
                "discharge falls from {i} to {ds}"
            );
        }
    }
}
