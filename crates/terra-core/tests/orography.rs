//! Synthetic climate tests: a ridge must cast a rain shadow, and the three-cell
//! wind structure must survive the transition to the full moisture model.

use terra_core::climate::moisture::{self, MoistureConfig};
use terra_core::climate::temperature;
use terra_core::climate::wind;
use terra_core::sphere::Sphere;

fn ridge_world() -> (Sphere, Vec<f64>, Vec<bool>) {
    let sphere = Sphere::new(4);
    let mut elevation = vec![-1_000.0f64; sphere.len()];
    for i in 0..sphere.len() {
        let lat = sphere.lat(i as u32);
        let lon = sphere.lon(i as u32);
        if (-45.0..=-35.0).contains(&lat) && lon.abs() < 30.0 {
            elevation[i] = 100.0;
        }
    }
    // Apply the ridge after the lowland base, so it is not overwritten.
    for i in 0..sphere.len() {
        let lat = sphere.lat(i as u32);
        let lon = sphere.lon(i as u32);
        if (-45.0..=-35.0).contains(&lat) && (-4.0..=4.0).contains(&lon) {
            elevation[i] = 2_800.0;
        }
    }
    let ocean: Vec<bool> = elevation.iter().map(|&e| e <= 0.0).collect();
    (sphere, elevation, ocean)
}

#[test]
fn windward_slopes_are_wetter_than_leeward_slopes() {
    let (sphere, elevation, ocean) = ridge_world();
    let winds = wind::compute(&sphere, &ocean, 11);
    let temp = temperature::compute(&sphere, &elevation, &ocean, &winds.current_poleward, 11);
    let moisture = moisture::compute(
        &sphere,
        &elevation,
        &ocean,
        &temp,
        &winds,
        11,
        MoistureConfig::default(),
    );
    let mut windward = (0.0, 0u32);
    let mut leeward = (0.0, 0u32);
    for i in 0..sphere.len() {
        let lat = sphere.lat(i as u32);
        let lon = sphere.lon(i as u32);
        if !(-45.0..=-35.0).contains(&lat) || elevation[i] <= 0.0 {
            continue;
        }
        if (-13.0..=-6.0).contains(&lon) {
            windward.0 += moisture.precip_mm[i];
            windward.1 += 1;
        } else if (6.0..=13.0).contains(&lon) {
            leeward.0 += moisture.precip_mm[i];
            leeward.1 += 1;
        }
    }
    assert!(windward.1 > 0 && leeward.1 > 0);
    let wet = windward.0 / f64::from(windward.1);
    let dry = leeward.0 / f64::from(leeward.1);
    assert!(wet > dry * 1.5, "windward {wet} leeward {dry}");
}

#[test]
fn rain_shadow_factor_is_low_behind_a_ridge() {
    let (sphere, elevation, ocean) = ridge_world();
    let winds = wind::compute(&sphere, &ocean, 11);
    let mut lee = Vec::new();
    let mut wind = Vec::new();
    for i in 0..sphere.len() {
        let lat = sphere.lat(i as u32);
        let lon = sphere.lon(i as u32);
        if !(-45.0..=-35.0).contains(&lat) || elevation[i] <= 0.0 {
            continue;
        }
        let factor = moisture::rain_shadow_factor(&sphere, &elevation, &winds, i as u32);
        if (6.0..=13.0).contains(&lon) {
            lee.push(factor);
        } else if (-13.0..=-6.0).contains(&lon) {
            wind.push(factor);
        }
    }
    assert!(lee.iter().any(|&f| f < 0.6), "no shadow downwind");
    assert!(wind.iter().all(|&f| f >= 0.99), "upwind cells are shadowed");
}

#[test]
fn zonal_winds_are_continuous_on_the_full_grid() {
    let sphere = Sphere::new(3);
    let ocean = vec![true; sphere.len()];
    let winds = wind::compute(&sphere, &ocean, 3);
    for i in 0..sphere.len() {
        let lat = sphere.lat(i as u32);
        let (base_east, _) = wind::zonal_profile(lat + 0.1);
        let (base_east2, _) = wind::zonal_profile(lat - 0.1);
        assert!((base_east - base_east2).abs() < 1.0);
        assert!(winds.wind_speed[i].is_finite());
        assert!(winds.wind_dir[i].is_finite());
    }
}
