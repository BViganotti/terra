//! Terra Studio: the user-facing product.
//!
//! The crate embeds an original HTML/CSS/JS client, serves it from a loopback
//! HTTP server with a random bearer token, runs world generation on a worker
//! thread, streams progress over Server-Sent Events and renders map layers with
//! its own PNG encoder. It has no dependencies outside the standard library and
//! the other Terra crates.

#![forbid(unsafe_code)]

pub mod assets;
pub mod json;
pub mod png;
pub mod server;

/// Semantic version of the Studio.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the launcher. Implemented by the server module.
pub fn launch() -> std::io::Result<()> {
    server::launch()
}
