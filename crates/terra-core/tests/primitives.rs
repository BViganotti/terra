//! Public primitive contracts: RNG, hashing, sphere, noise and safe accessors.
//!
//! These tests are deliberately broad and shallow. They exist so a refactor
//! cannot quietly change an invariant that every higher layer relies on.

use terra_core::hash::{canonical_bits, hash_str, hex, Hasher64};
use terra_core::noise::{fbm, ridged, value_noise};
use terra_core::rng::{mix3, Rng, Stream};
use terra_core::sphere::Sphere;
use terra_core::vec3::Vec3;

#[test]
fn rng_streams_are_reproducible_and_independent() {
    let mut a = Rng::new(7, Stream::PLATES);
    let mut b = Rng::new(7, Stream::PLATES);
    let mut c = Rng::new(7, Stream::CLIMATE);
    let mut equal = 0;
    for _ in 0..1_000 {
        let x = a.next_u64();
        assert_eq!(x, b.next_u64());
        if x == c.next_u64() {
            equal += 1;
        }
    }
    assert!(equal < 3);
    // Forks are independent of the parent stream.
    let mut parent = Rng::new(11, Stream::TERRAIN);
    let mut child = parent.fork(Stream::EROSION);
    assert_ne!(parent.next_u64(), child.next_u64());
}

#[test]
fn rng_bounds_hold_across_seeds() {
    for seed in 0..50u64 {
        let mut r = Rng::new(seed, Stream::HISTORY);
        for _ in 0..500 {
            let f = r.next_f64();
            assert!((0.0..1.0).contains(&f));
            let v = r.below(17);
            assert!(v < 17);
            let g = r.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&g));
        }
        let mut values: Vec<u32> = (0..32).collect();
        r.shuffle(&mut values);
        values.sort_unstable();
        assert_eq!(values, (0..32).collect::<Vec<_>>());
    }
}

#[test]
fn hashing_is_stable_and_framed() {
    assert_eq!(hash_str("terra"), hash_str("terra"));
    let mut a = Hasher64::new();
    a.write_str_framed("ab");
    a.write_str_framed("c");
    let mut b = Hasher64::new();
    b.write_str_framed("a");
    b.write_str_framed("bc");
    assert_ne!(a.finish(), b.finish());
    assert_eq!(canonical_bits(0.0), canonical_bits(-0.0));
    assert_eq!(canonical_bits(f64::NAN), canonical_bits(-f64::NAN));
    assert_eq!(hex(0).len(), 16);
}

#[test]
fn every_supported_sphere_validates() {
    for level in 0..=4u32 {
        let sphere = Sphere::new(level);
        sphere.validate().unwrap();
        let total = sphere.total_area();
        assert!((total - 4.0 * std::f64::consts::PI).abs() < 1e-9);
        for cell in (0..sphere.len() as u32).step_by(17) {
            assert_eq!(sphere.nearest(sphere.pos(cell)), cell);
            assert!(sphere.degree(cell) >= 5);
            assert!(sphere.lat(cell) >= -90.0 && sphere.lat(cell) <= 90.0);
            assert!(sphere.lon(cell) >= -180.0 && sphere.lon(cell) <= 180.0);
        }
    }
}

#[test]
fn nearest_lookup_matches_brute_force_on_samples() {
    let sphere = Sphere::new(4);
    for lat in (-80..=80).step_by(23) {
        for lon in (-180..=180).step_by(31) {
            let dir = Vec3::from_lat_lon(f64::from(lat), f64::from(lon));
            let best = sphere.nearest(dir);
            let best_dot = sphere.pos(best).dot(dir);
            for cell in 0..sphere.len() as u32 {
                assert!(sphere.pos(cell).dot(dir) <= best_dot + 1e-12);
            }
        }
    }
}

#[test]
fn noise_is_bounded_and_deterministic_across_the_sphere() {
    let mut state = 0xabcdefu64;
    for _ in 0..1_000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let lat = ((state % 1800) as f64) / 10.0 - 90.0;
        let lon = ((state >> 11) % 3600) as f64 / 10.0 - 180.0;
        let p = Vec3::from_lat_lon(lat, lon);
        let v = fbm(3, p, 4.0, 5, 2.0, 0.5);
        assert!((-1.0..=1.0).contains(&v));
        assert_eq!(v.to_bits(), fbm(3, p, 4.0, 5, 2.0, 0.5).to_bits());
        let r = ridged(3, p, 6.0, 4);
        assert!((0.0..=1.0).contains(&r));
        let n = value_noise(3, p, 2.0);
        assert!((-1.0..=1.0).contains(&n));
    }
    assert_eq!(mix3(1, 2, 3), mix3(1, 2, 3));
    assert_ne!(mix3(1, 2, 3), mix3(1, 3, 2));
}

#[test]
fn degenerate_inputs_are_safe() {
    let s = Sphere::new(1);
    assert_eq!(s.nearest(Vec3::ZERO), s.nearest(Vec3::ZERO));
    assert!(Vec3::ZERO.try_normalize().is_none());
    assert_eq!(Vec3::ZERO.normalized(), Vec3::Z);
    assert!(fbm(1, Vec3::Z, 0.0, 4, 2.0, 0.5).is_finite());
    assert!(value_noise(1, Vec3::Z, f64::NAN) == 0.0);
    assert_eq!(
        canonical_bits(f64::NEG_INFINITY),
        f64::NEG_INFINITY.to_bits()
    );
}
