# Development

Terra is a Rust workspace with no external crates. This guide is for changing it
without breaking determinism or causality.

## Prerequisites

- Rust stable (the workspace declares `rust-version = "1.80"`).
- Optional: Playwright for the browser end-to-end tests, `cargo-llvm-cov` for
  coverage. Neither is needed to build or run Terra.

## Commands

```sh
make check       # build all targets
make test        # debug tests
make test-rel    # release tests
make fuzz        # malformed-input campaign
make bench       # measured timings
make studio      # launch
make e2e         # Playwright scripts
make coverage    # llvm-cov summary
```

There is no CI. Every slice is expected to be green before it is committed.

## Code map

```
crates/terra-core/src/
  sphere.rs        icosphere, adjacency, areas, nearest index
  vec3.rs          vector math
  rng.rs           xoshiro256**, streams
  hash.rs          FNV-1a, canonical float bits
  noise.rs         value noise, fBm, ridged, warp
  tectonics.rs     plates, motion, boundaries, uplift
  terrain.rs       elevation, sea level, landmasses, crust
  hydrology/       fill, flow, lakes, erosion
  climate/         wind, temperature, moisture
  biome/           classification, ecosystems
  scenario.rs      bounded parser and presets
  world.rs         assembly, stats, hashes, cell info

crates/terra-history/src/
  civ/mod.rs       records, names, resources
  civ/sim.rs       year loop, events, snapshots
  chronicle/mod.rs event store and validation
  chronicle/query.rs chains, paths, filters
  chronicle/export.rs markdown, JSON, CSV

crates/terra-studio/src/
  json.rs          bounded parser/writer
  png.rs           raster and PNG encoder
  render.rs        layers, political overlay, labels
  server.rs        HTTP, auth, jobs, SSE, endpoints
  launcher.rs      flags, browser, report, verify
  examples.rs      bundled scenario registry
```

## Determinism rules

1. Never add OS entropy to a simulation path. Use `Rng::new(seed, stream)` or a
   hashed per-cell value.
2. Add a new stream constant for a new subsystem. Do not consume another
   subsystem's sequence.
3. Never iterate a `HashMap` in simulation code. Use `Vec`, `BTreeMap`, or sort.
4. Tie-break comparisons by index, not pointer or insertion order.
5. Hash floats by bits, not formatted text.
6. If you change a physics formula, expect world and chronicle hashes to change.
   Update the version communication and the reports; do not "fix" hashes.
7. Rendering, queries and exports must be read-only. They may not mutate a
   world or chronicle.

## Causality rules

- Every event needs at least one cause.
- Causes must already exist.
- State facts use stable keys and units.
- If a new rule fires, cite the rule's measured trigger and the actor's previous
  event. Do not fabricate a chain.
- Run `Chronicle::validate()` (or `History::verify()`) in the test for the new
  rule.

## Adding a stage

1. Create a module with a `Config` that has a `sanitized()` method and a result
   struct with public layers.
2. Use only deterministic iteration and explicit RNG streams.
3. Add the stage to `World::generate_with` in dependency order and report
   progress after it finishes.
4. Add its layers to `compute_hashes` and `compute_stats` if they are part of the
   world's identity.
5. Add a renderer branch only if it has a map meaning.
6. Add unit tests plus one integration invariant.
7. Update `docs/SIMULATION.md`.

## Adding an endpoint

1. Add a route match in `server.rs`.
2. Validate every query parameter and clamp sizes.
3. Take an `Arc<Artifact>` snapshot; never mutate a history in a request.
4. Build JSON with `Json` (no string concatenation of untrusted values).
5. Add a unit test and, for important routes, an integration test in
   `crates/terra-studio/tests/`.
6. Update `docs/API.md`.

## Style

- `cargo fmt --all` is mandatory.
- `cargo build --workspace --all-targets` must be warning-free.
- Prefer explicit names, small public structs and plain vectors.
- No `unsafe` (forbidden at crate level).
- No new dependency, ever, for the product. If something seems to need a crate,
  implement the smallest sound version or document the gap.
- Comments explain why, not what. Do not restate code.

## Testing

- Unit tests belong in the module.
- Integration tests belong in `crates/<crate>/tests/`.
- Test invariants, conservation laws and bounds, not golden prose.
- Use subdivision 2–3 for most tests; subdivision 5 is for benchmarks and the
  Studio.
- Malformed input tests must assert an error and never a panic.
- For browser changes, run both Playwright scripts.

## Commits

- One vertical slice per commit: a feature with its tests, or a focused doc.
- Build and test before committing.
- Do not add CI workflows.
- Keep `CHANGELOG.md` current for user-visible changes.

## Debugging

- `--report` prints a one-line world/history summary and hashes.
- `--verify` runs twice and compares hashes, catching accidental nondeterminism.
- `--validate-scenario` shows the canonical scenario and the first error.
- The Studio job record exposes stage, progress and errors; SSE frames are
  human-readable.
- The cause panel is the quickest way to see why a rule fired; the same data is
  available at `/api/event`.

## Performance

Measure, do not guess: `cargo run --release -p terra-bench`. Rendering is the
hot path; do not re-render on every keystroke. Simulation is cheap by comparison.
