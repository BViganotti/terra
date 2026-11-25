//! Launcher binary for Terra Studio.
//!
//! It starts the loopback server and opens the default browser. Headless flags
//! exist only for tests, benchmarks and deterministic seed runs; the Studio is
//! the product.

fn main() {
    if let Err(err) = terra_studio::launch() {
        eprintln!("terra: {err}");
        std::process::exit(1);
    }
}
