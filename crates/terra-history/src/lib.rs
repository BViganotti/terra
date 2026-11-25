//! Terra history: civilizations and the chronicle.
//!
//! This crate depends on the physical world produced by `terra-core` but never
//! writes to it. History is a pure function of the world, the scenario and the
//! seed; the chronicle it produces is append-only and every event cites its
//! causes.

#![forbid(unsafe_code)]

pub mod chronicle;
pub mod civ;

/// Semantic version of the history model.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_present() {
        assert!(!super::VERSION.is_empty());
    }
}
