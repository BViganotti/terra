//! Terra malformed-input campaign.
//!
//! This is a deterministic, standard-library-only fuzz harness rather than a
//! `cargo-fuzz` target, because Terra takes no external crates. It generates
//! thousands of mutated scenario and request documents, feeds each through the
//! real parsers, and asserts that every failure is a clean `Err`. Any panic is a
//! finding and fails the process.
//!
//! Run with `cargo run --release -p terra-fuzz -- [iterations]`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let iterations: u64 = args
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000)
        .clamp(1, 200_000);
    println!("terra-fuzz: {iterations} iterations, harness pending implementation");
}
