# Verification

This file records how each acceptance criterion is demonstrated. It is not a
substitute for `FINAL_REPORT.md`, which lists final counts and measurements.

## Definition of Done

| # | Criterion | Evidence |
|---|---|---|
| 1 | `cargo build --release` succeeds with zero warnings | Final build is run with warnings denied in the commit workflow; see `FINAL_REPORT.md` |
| 2 | `cargo test` and `cargo test --release` pass | Full workspace suites; see `docs/TESTING.md` for inventory |
| 3 | Project-specific acceptance tests pass | Table below |
| 4 | 1,000 malformed inputs fail cleanly | `crates/terra-core/tests/malformed.rs`, `fuzz/` |
| 5 | Browser Studio demo works via Playwright | `tests/studio/test_studio.py` |
| 6 | README, docs, CHANGELOG, LICENSE, LIMITATIONS, FINAL_REPORT exist | Repository root and `docs/` |
| 7 | FINAL_REPORT lists LOC, commits, tests, coverage, benchmarks, gaps | `FINAL_REPORT.md` |
| 8 | Repo committed and pushed, or PUSH.md exists | Public `BViganotti/terra`, branch `master` |
| 9 | No external crate, no CI, no CLI-only product | `Cargo.toml` files have no dependencies; no `.github/workflows`; Studio is the product |

## PLAN.md acceptance

| Criterion | Test or measurement | Result |
|---|---|---|
| 10 seeds replay identically | `cargo test -p terra-core --test determinism` | Pass |
| Plate area within 0.1% | `cargo test -p terra-core --test tectonics` | Pass (partition area vs 4π) |
| Mountains at convergent edges | `crates/terra-core/tests/tectonics.rs` | Pass (uplift and peak proximity) |
| Rivers downhill; lakes basins | `cargo test -p terra-core --test hydrology` | Pass |
| Every event traceable, no orphans | `cargo test -p terra-history --test traceability` | Pass, every event walked |
| 1,000 malformed scenarios fail | `cargo test -p terra-core --test malformed` | Pass |
| 10,000-year run completes | `cargo test -p terra-history --test long_run` | Pass; release generation ~4 ms |
| Studio seed, maps, timeline | `python3 tests/studio/test_studio.py` | Pass |
| No external crates | `cargo tree` / manifests | Empty dependency lists |
| No CI | repository tree | No workflows |
| Browser product, not CLI | `cargo run --release -p terra-studio` | Auto-opens authenticated Studio |

## Measured performance

From `cargo run --release -p terra-bench`:

- world generation, 10,242 cells: **94.65 ms** median;
- 10,000-year history, 10,242 cells: **3.95 ms** median;
- 1024×512 map layer: **123–133 ms**;
- PNG encode: **33.62 ms**;
- 1,000 parsed scenarios: **0.57 ms**;
- 1,000 parsed JSON documents: **0.40 ms**.

Full table in `docs/PERFORMANCE.md`.

## Measured coverage

`cargo llvm-cov --workspace --tests --summary-only`:

- regions **89.50%**;
- functions **87.04%**;
- lines **89.05%**.

Full table and caveats in `docs/COVERAGE.md`.

## Determinism caveat

Replay is bit-exact within a build and platform. Cross-platform bit equality is
not promised because transcendental functions delegate to the platform libm.
This is stated in `LIMITATIONS.md` and `docs/DETERMINISM.md`; no test claims
otherwise.

## Reproduce everything

```sh
cargo build --release
cargo test
cargo test --release
cargo run --release -p terra-fuzz -- 1000
cargo run --release -p terra-bench
cargo run --release -p terra-studio -- --verify --seed 42 --years 10000
cargo build --release && python3 tests/studio/test_studio.py
```
