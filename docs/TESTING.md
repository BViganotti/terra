# Testing

Terra has no CI by design. The suite is small enough to run locally in seconds
and is expected to stay green at every commit.

```sh
cargo test                 # debug
cargo test --release       # optimized; catches timing-sensitive mistakes
cargo run --release -p terra-fuzz -- 1000
cargo run --release -p terra-bench
python3 tests/studio/test_studio.py   # Playwright, if installed
```

## Unit tests

Every module carries tests next to the code:

- `terra-core`: hashing, RNG, vectors, icosphere, noise, tectonics, terrain,
  hydrology (fill, flow, lakes, erosion), climate (wind, temperature, moisture),
  biomes, ecosystems, scenario parsing, world assembly.
- `terra-history`: chronicle invariants, queries, tracing, exports, civilization
  records, full simulation.
- `terra-studio`: JSON parser/writer, PNG encoder and inflater, renderer, HTTP
  parsing, auth, job runner, SSE, every endpoint.

Unit tests focus on local behavior and edge cases: empty inputs, non-finite
floats, out-of-range indices, duplicate keys, oversized documents, missing
files.

## Acceptance tests

| Requirement | Test |
|---|---|
| 10 seeds replay identically | `crates/terra-core/tests/determinism.rs` |
| Plate area within 0.1% | `crates/terra-core/tests/tectonics.rs` |
| Mountains at convergent margins | `crates/terra-core/tests/tectonics.rs` |
| Rivers downhill; lakes are basins | `crates/terra-core/tests/hydrology.rs` |
| Sediment budget closes | `crates/terra-core/tests/hydrology.rs` |
| Latitudinal climate and wind belts | `crates/terra-core/tests/climate.rs` |
| Biome/ecosystem consistency | `crates/terra-core/tests/biosphere.rs` |
| Whole-world invariants | `crates/terra-core/tests/world.rs` |
| 1,000 malformed scenarios fail cleanly | `crates/terra-core/tests/malformed.rs` |
| Every chronicle event traceable | `crates/terra-history/tests/traceability.rs` |
| History replay hashes stable | `crates/terra-history/tests/replay.rs` |
| 10,000-year run completes | `crates/terra-history/tests/long_run.rs` |
| Settlement, trade, war, collapse fire | `crates/terra-history/tests/kinds.rs` |
| Ownership matches civilization records | `crates/terra-history/tests/owners.rs` |
| Studio generates, maps and scrubs | `tests/studio/test_studio.py` |

The Playwright test drives a real browser against a real release binary. It
forges a world, switches layers, scrubs the political year, inspects a cell,
filters and pages the chronicle, follows a cause link and downloads a map.

## Fuzzing

`fuzz/` is a deterministic, standard-library-only campaign, not a `cargo-fuzz`
target (Terra has no external crates). It generates guaranteed-malformed
scenario files and JSON documents, plus a mutation sweep of valid inputs, and
asserts that every malformed case is rejected without a panic.

```sh
cargo run --release -p terra-fuzz -- 1000
cargo run --release -p terra-fuzz -- 100000
```

Output is one JSON line with counts. A panic aborts the process; a rejected-case
mismatch exits non-zero.

## Benchmarks

`bench/` times world generation at every subdivision, 10,000-year histories,
cause chains, exports, all ten render layers, PNG encoding and parser
throughput. Medians over repeated runs are printed with minimum and maximum.
Results are recorded in `docs/PERFORMANCE.md`.

## Property-style tests

Without external proptest crates, Terra uses deterministic loops:

- every cell is checked for invariants in small worlds;
- ten seeds are replayed twice;
- ten malformed families cover each parser rule;
- mutation sweeps apply 5,000 structural edits;
- every event in a run is traced to state facts, not sampled.

## Determinism caveat

Tests compare hashes within the same build and platform. Cross-platform bit
equality is not promised because `sin`, `cos`, `exp` and `powf` delegate to the
platform libm. This is documented in `docs/DETERMINISM.md` and
`LIMITATIONS.md`.

## Writing new tests

- Put integration tests in `crates/<crate>/tests/`.
- Reuse `tests/common/mod.rs` for world construction.
- Prefer invariants and conservation laws over golden numbers.
- Keep debug runtimes short: use subdivision 2 or 3 and short spans unless the
  test is specifically about scale.
- If a test needs a Studio job, use a small configuration
  (`subdiv: 2, years: 200`) and poll the job endpoint.
