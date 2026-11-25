//! Terrain acceptance: sea level, landmasses, shelves, coastlines and crust.

mod common;

#[test]
fn sea_level_relates_raw_and_relative_elevation() {
    let w = common::world(3, 3);
    let t = &w.terrain;
    assert!(t.sea_level.is_finite());
    assert_eq!(t.raw_elevation.len(), t.elevation.len());
    for (raw, elev) in t.raw_elevation.iter().zip(&t.elevation) {
        assert!((raw - (elev + t.sea_level)).abs() < 1e-6);
    }
    let above: usize = t.elevation.iter().filter(|&&e| e > 0.0).count();
    assert_eq!(above, w.stats.land_cells as usize);
}

#[test]
fn landmasses_partition_the_land() {
    let w = common::world(5, 3);
    let t = &w.terrain;
    let land: usize = t.elevation.iter().filter(|&&e| e > 0.0).count();
    let labelled: usize = t.landmass.iter().filter(|&&m| m != u32::MAX).count();
    assert_eq!(land, labelled);
    assert_eq!(t.landmass_count, w.stats.continents);
    let mut area = 0.0;
    for id in 0..t.landmass_count {
        let a = t.landmass_area(&w.sphere, id);
        assert!(a > 0.0);
        area += a;
    }
    let expected: f64 = (0..w.len())
        .filter(|&i| t.elevation[i] > 0.0)
        .map(|i| w.sphere.cell_area(i as u32))
        .sum();
    assert!((area - expected).abs() < 1e-12);
    let (largest, largest_area) = t.largest_landmass(&w.sphere).unwrap();
    assert!(largest < t.landmass_count);
    assert!(largest_area <= area + 1e-12);
}

#[test]
fn coasts_have_shelves_and_land_edges() {
    let w = common::world(7, 3);
    let t = &w.terrain;
    let mut coasts = 0;
    for i in 0..w.len() {
        if t.coastal[i] {
            coasts += 1;
            assert!(t.elevation[i] > 0.0);
            assert!(w
                .sphere
                .neighbors(i as u32)
                .iter()
                .any(|&nb| t.elevation[nb as usize] <= 0.0));
        }
        if t.shelf[i] {
            assert!(t.elevation[i] <= 0.0);
        }
    }
    assert!(coasts > 0);
    assert!(t.shelf.iter().any(|&s| s));
}

#[test]
fn crust_is_thicker_under_continents_and_bounded() {
    let w = common::world(9, 3);
    let t = &w.terrain;
    for (i, &crust) in t.crust_km.iter().enumerate() {
        assert!((4.0..=72.0).contains(&crust), "cell {i}: {crust}");
        assert!(t.crust_age_myr[i] >= 0.0 && t.crust_age_myr[i] <= 190.0);
    }
    let mut land = (0.0, 0u32);
    let mut ocean = (0.0, 0u32);
    for i in 0..w.len() {
        if t.elevation[i] > 0.0 {
            land.0 += t.crust_km[i];
            land.1 += 1;
        } else {
            ocean.0 += t.crust_km[i];
            ocean.1 += 1;
        }
    }
    assert!(land.0 / f64::from(land.1) > ocean.0 / f64::from(ocean.1) + 10.0);
}

#[test]
fn land_fraction_is_realized_and_stable() {
    let w = common::world(11, 3);
    let counted = w.terrain.elevation.iter().filter(|&&e| e > 0.0).count() as f64 / w.len() as f64;
    assert!((counted - w.terrain.land_fraction).abs() < 1e-12);
    assert!((counted - w.config.land_fraction).abs() < 0.04);
}
