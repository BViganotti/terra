//! Climate acceptance tests.

mod common;

#[test]
fn equator_is_much_warmer_than_the_poles() {
    for seed in 0..4u64 {
        let w = common::world(seed, 3);
        let mut equator = (0.0, 0u32);
        let mut poles = (0.0, 0u32);
        for i in 0..w.len() {
            let lat = w.sphere.lat(i as u32);
            if lat.abs() < 10.0 {
                equator.0 += w.climate.temperature.mean_c[i];
                equator.1 += 1;
            } else if lat.abs() > 70.0 {
                poles.0 += w.climate.temperature.mean_c[i];
                poles.1 += 1;
            }
        }
        assert!(equator.1 > 0 && poles.1 > 0);
        let delta = equator.0 / f64::from(equator.1) - poles.0 / f64::from(poles.1);
        assert!(delta > 20.0, "seed {seed}: equator-pole delta {delta}");
    }
}

#[test]
fn temperature_lapse_rate_is_applied() {
    let w = common::world(6, 4);
    let mut high = (0.0, 0u32);
    let mut low = (0.0, 0u32);
    for i in 0..w.len() {
        let lat = w.sphere.lat(i as u32);
        if !(-15.0..=15.0).contains(&lat) || w.terrain.elevation[i] <= 0.0 {
            continue;
        }
        if w.terrain.elevation[i] > 2_000.0 {
            high.0 += w.climate.temperature.mean_c[i];
            high.1 += 1;
        } else if w.terrain.elevation[i] < 400.0 {
            low.0 += w.climate.temperature.mean_c[i];
            low.1 += 1;
        }
    }
    if high.1 > 0 && low.1 > 0 {
        let drop = low.0 / f64::from(low.1) - high.0 / f64::from(high.1);
        assert!(drop > 5.0, "lapse drop only {drop} C");
    }
}

#[test]
fn oceans_have_milder_seasons_than_land() {
    let w = common::world(8, 3);
    let mut ocean = (0.0, 0u32);
    let mut land = (0.0, 0u32);
    for i in 0..w.len() {
        if w.terrain.elevation[i] <= -1_000.0 {
            ocean.0 += w.climate.temperature.seasonal_amplitude_c[i];
            ocean.1 += 1;
        } else if w.terrain.elevation[i] > 200.0 {
            land.0 += w.climate.temperature.seasonal_amplitude_c[i];
            land.1 += 1;
        }
    }
    assert!(ocean.1 > 0 && land.1 > 0);
    assert!(ocean.0 / f64::from(ocean.1) < land.0 / f64::from(land.1));
}

#[test]
fn precipitation_mean_matches_the_scenario() {
    for seed in 0..3u64 {
        let w = common::world(seed, 3);
        let mut sum = 0.0;
        let mut n = 0u32;
        for i in 0..w.len() {
            if w.terrain.elevation[i] > 0.0 {
                sum += w.climate.moisture.precip_mm[i];
                n += 1;
            }
        }
        let mean = sum / f64::from(n);
        assert!((mean - 860.0).abs() < 5.0, "seed {seed}: mean {mean}");
    }
}

#[test]
fn wet_and_dry_belts_exist() {
    let w = common::world(10, 4);
    let mut tropics = (0.0, 0u32);
    let mut subtropics = (0.0, 0u32);
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    for i in 0..w.len() {
        if w.terrain.elevation[i] <= 0.0 {
            continue;
        }
        let lat = w.sphere.lat(i as u32).abs();
        let p = w.climate.moisture.precip_mm[i];
        min = min.min(p);
        max = max.max(p);
        if lat < 10.0 {
            tropics.0 += p;
            tropics.1 += 1;
        } else if (18.0..32.0).contains(&lat) {
            subtropics.0 += p;
            subtropics.1 += 1;
        }
    }
    assert!(tropics.1 > 0 && subtropics.1 > 0);
    assert!(tropics.0 / f64::from(tropics.1) > subtropics.0 / f64::from(subtropics.1) * 1.4);
    assert!(min < 500.0, "no deserts: min {min}");
    assert!(max > 1_500.0, "no rainforests: max {max}");
}

#[test]
fn wind_bands_follow_the_three_cell_model() {
    let w = common::world(12, 3);
    let mut trades = (0.0, 0u32);
    let mut westerlies = (0.0, 0u32);
    let mut polar = (0.0, 0u32);
    for i in 0..w.len() {
        let lat = w.sphere.lat(i as u32);
        let u = w.climate.winds.wind_east[i];
        if lat.abs() < 20.0 {
            trades.0 += u;
            trades.1 += 1;
        } else if (38.0..55.0).contains(&lat.abs()) {
            westerlies.0 += u;
            westerlies.1 += 1;
        } else if lat.abs() > 70.0 {
            polar.0 += u;
            polar.1 += 1;
        }
    }
    assert!(trades.0 / f64::from(trades.1) < 0.0);
    assert!(westerlies.0 / f64::from(westerlies.1) > 0.0);
    assert!(polar.0 / f64::from(polar.1) < 0.0);
}

#[test]
fn currents_stay_on_the_ocean() {
    let w = common::world(14, 3);
    for i in 0..w.len() {
        if w.climate.ocean[i] {
            assert!(w.climate.winds.current_speed[i] <= 3.0);
            assert!(w.climate.winds.current_speed[i].is_finite());
        } else {
            assert_eq!(w.climate.winds.current_speed[i], 0.0);
        }
    }
    assert!(w.climate.winds.current_speed.iter().any(|&s| s > 0.0));
    assert!(w
        .climate
        .winds
        .current_poleward
        .iter()
        .any(|&s| s.abs() > 0.0));
}

#[test]
fn humidity_aridity_and_pet_are_bounded() {
    let w = common::world(16, 3);
    for i in 0..w.len() {
        assert!((0.0..=1.0).contains(&w.climate.moisture.humidity[i]));
        assert!((0.0..=1.0).contains(&w.climate.moisture.cloud[i]));
        assert!(w.climate.moisture.aridity[i] > 0.0);
        assert!(w.climate.moisture.pet_mm[i] > 0.0);
        assert!(w.climate.moisture.precip_mm[i].is_finite());
        assert!(w.climate.temperature.mean_c[i].is_finite());
    }
}
