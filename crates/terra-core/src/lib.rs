//! Terra core: a seeded planet.
//!
//! The crate is organized as a deterministic pipeline. Every stage consumes the
//! grid produced by earlier stages and produces plain vectors, which keeps the
//! world cheap to clone, hash and render.
//!
//! ```text
//! sphere -> tectonics -> hydrology -> climate -> biome -> world
//! ```

#![forbid(unsafe_code)]

pub mod hash;
pub mod noise;
pub mod rng;
pub mod sphere;
pub mod tectonics;
pub mod vec3;

/// Semantic version of the simulation model.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Number of grid cells for a given icosphere subdivision level.
///
/// The formula is `10 * 4^level + 2`, clamped so callers cannot overflow.
pub fn cells_for_subdiv(subdiv: u32) -> usize {
    if subdiv > 10 {
        return cells_for_subdiv(10);
    }
    10usize * (1usize << (2 * subdiv)) + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_counts_match_icosphere() {
        assert_eq!(cells_for_subdiv(0), 12);
        assert_eq!(cells_for_subdiv(1), 42);
        assert_eq!(cells_for_subdiv(2), 162);
        assert_eq!(cells_for_subdiv(5), 10_242);
    }

    #[test]
    fn cell_counts_are_bounded() {
        assert_eq!(cells_for_subdiv(200), cells_for_subdiv(10));
    }
}
