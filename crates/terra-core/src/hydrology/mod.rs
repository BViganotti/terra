//! Hydrology: depression filling, drainage networks, lakes, erosion and
//! sediment transport.
//!
//! The pipeline is iterative because erosion changes the surface that defines
//! drainage. Each pass fills depressions, computes flow, erodes channels,
//! transports sediment and deposits it in low-gradient reaches and at river
//! mouths. All bookkeeping is in cubic meters so sediment mass can be checked.

pub mod fill;
pub mod flow;
pub mod lakes;

pub use fill::{priority_flood, Filled};
pub use flow::Drainage;
