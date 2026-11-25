//! Shared helpers for core acceptance tests.

#![allow(dead_code)]

use terra_core::world::{World, WorldConfig};

/// A small, fast world configuration for invariant tests.
pub fn config(seed: u64, subdiv: u32) -> WorldConfig {
    WorldConfig {
        seed,
        subdiv,
        years: 1_000,
        erosion_iterations: 6,
        ..WorldConfig::default()
    }
}

/// Generates a world at the given seed and subdivision.
pub fn world(seed: u64, subdiv: u32) -> World {
    World::generate(config(seed, subdiv))
}

/// Relative error helper.
pub fn rel_err(a: f64, b: f64) -> f64 {
    if b.abs() < 1e-12 {
        a.abs()
    } else {
        (a - b).abs() / b.abs()
    }
}

/// Mean of a slice.
pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}
