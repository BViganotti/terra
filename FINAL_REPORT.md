# Terra — Final Report

Date: 2026-11-25

Status: **complete**. The Definition of Done in `REQUIREMENTS.md` is satisfied.
This report records the measured state of the repository, the evidence for each
acceptance claim, and the remaining honest gaps.

## 1. What was built

Terra is a seeded planet with a history, delivered as a local browser Studio and
a headless library. It contains:

- a 10,242-cell icosphere and deterministic noise;
- plate tectonics with graph-Voronoi partitions, motion, boundary stress,
  subduction, trenches, volcanic arcs and orogenic uplift;
- erosion, sediment routing, drainage networks, lakes with spill points and
  deltas;
- wind, currents, temperature, moisture transport, orographic rain shadows and
  dry belts;
- Whittaker-like biomes, Miami-model productivity, species richness, fragility
  and carrying capacity;
- rule-based civilizations with settlements, expansion, resources, technology,
  trade, wars, battles, city capture, famine, epidemics, migration and collapse;
- an append-only chronicle whose events cite measured state or earlier events,
  with bounded causal chains and explicit paths to roots;
- local HTTP/SSE Studio with ten layers, political year scrubbing, cell
  inspection, timeline search, cause tracing, and PNG/Markdown/JSON/CSV exports;
- headless replay, verification, scenario validation, fuzz and benchmark tools.

## 2. Delivery numbers

### Lines of code

| Category | LOC |
|---|---:|
| Production Rust (`crates/*/src`, `fuzz`, `bench`) | 15,739 |
| Rust tests (unit + integration) | 6,923 |
| Rust total | 22,662 |
| Embedded UI (HTML/CSS/JS) | 1,840 |
| Playwright test (Python) | 163 |

Production Rust is inside the 14–16k target. Total code is above the target
because the test suite is large and the benchmark/fuzz harnesses are real
programs, not stubs. No code was added to pad the count; the budget explanation
is in section 9.

### Commits

- 90 commits before this report; 91 including it.
- Every commit was built and tested locally before committing.
- No CI is configured, by requirement.
- Repository: <https://github.com/BViganotti/terra>, branch `master`.

### Tests

376 tests, split as:

| Crate | Tests |
|---|---:|
| `terra-core` | 243 |
| `terra-history` | 73 |
| `terra-studio` | 60 |

17 integration test files plus a shared helper, and unit modules across 56 Rust source files.

Measured wall time on the development machine:

- `cargo test --workspace`: 9.11 s
- `cargo test --workspace --release`: 1.78 s

Both suites pass in full.

### Coverage

Measured with `cargo llvm-cov --workspace --tests --summary-only`:

| Scope | Region | Function | Line |
|---|---:|---:|---:|
| Workspace total | 89.50% | 87.04% | 89.05% |

Coverage includes untested fuzz/bench binaries, so library coverage is higher.
Details and caveats: `docs/COVERAGE.md`.

### Build

- `cargo build --release`: success, **zero warnings**.
- `cargo fmt --all -- --check`: clean.
- `unsafe` is forbidden at crate level.
- `cargo tree` shows only workspace crates; no external dependencies.
- No `.github/workflows` exists.

## 3. Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| 10 seeds replay identically | `terra-core/tests/determinism.rs` replays every seed twice and compares every stage hash and elevation bits | Pass |
| Plate area within 0.1% | `terra-core/tests/tectonics.rs` compares partition area to 4π and shares to 1 | Pass (`< 1e-3` relative; typically `< 1e-6`) |
| Mountains at convergent edges | same file: convergent uplift vs interior, and top 5% peaks within two hops of convergence | Pass |
| Rivers never uphill | `terra-core/tests/hydrology.rs` checks every downhill edge on the filled and original surfaces | Pass |
| Lakes are basins | same file: surface, spill cell, spill target, volume and depth | Pass |
| Sediment mass balance | same file: net rock loss equals deep-ocean export within 1% | Pass |
| Climate latitude tests | `terra-core/tests/climate.rs`: equator–pole delta, lapse rate, seasonality, wind bands, wet/dry belts | Pass |
| Biomes and ecosystems | `terra-core/tests/biosphere.rs`: partition, patches, richness, carrying capacity | Pass |
| 1,000 malformed scenarios fail cleanly | `terra-core/tests/malformed.rs` constructs 1,000 guaranteed-invalid documents | Pass |
| Every event traceable | `terra-history/tests/traceability.rs` walks every event, every chain and every path | Pass |
| No orphan events | same file: every event has a cause; every chain reaches state facts | Pass |
| History replay | `terra-history/tests/replay.rs` compares hashes, names, cells, populations and stats | Pass |
| 10,000-year run | `terra-history/tests/long_run.rs`; release simulation is milliseconds | Pass |
| Rise and fall coverage | `terra-history/tests/kinds.rs` over a seed sweep | Pass |
| Ownership consistency | `terra-history/tests/owners.rs` checks every snapshot | Pass |
| Studio demo in Playwright | `tests/studio/test_studio.py` | Pass |
| HTTP API | `terra-studio/tests/http_api.rs` | Pass |
| PNG validity | `terra-studio/tests/render_layers.rs` verifies chunks, CRCs, deflate and pixels | Pass |

### Fuzz campaign

```json
{"iterations":1000,"scenarioRejected":1000,"jsonRejected":1000,
 "mutationsParsed":359,"mutationsRejected":641,
 "validRoundTrips":1000,"status":"pass"}
```

No panic. Every constructed malformed input was rejected. A 10,000-iteration
run was also executed successfully during development:

```json
{"iterations":10000,"scenarioRejected":10000,"jsonRejected":10000,
 "mutationsParsed":3475,"mutationsRejected":6525,
 "validRoundTrips":10000,"status":"pass"}
```

### Replay verification

```json
{"worldHash":"e4e612d7fa645774","chronicleHash":"abebe8067b1f1aa4",
 "worldMatch":true,"historyMatch":true,"sound":true,"verified":true,
 "status":"pass"}
```

Command: `cargo run --release -p terra-studio -- --verify --seed 42 --years
10000 --subdiv 5`.

### Playwright output

```
status: Seramonionya forged · 10,242 cells · 229 events
canvas: {'lit': 5406, 'colors': 1124}
inspector: Tropical rainforest
cause panel: ok
filters and paging: ok
download: Seramonionya-political.png
STUDIO E2E PASSED
```

## 4. Benchmarks (measured)

`cargo run --release -p terra-bench`, medians:

| Workload | Median |
|---|---:|
| World subdiv 2 (162 cells) | 1.65 ms |
| World subdiv 3 (642 cells) | 6.13 ms |
| World subdiv 4 (2,562 cells) | 23.96 ms |
| World subdiv 5 (10,242 cells) | 94.65 ms |
| 10,000-year history, subdiv 4 | 1.74 ms |
| 10,000-year history, subdiv 5 | 3.95 ms |
| Cause chains, every 7th event | 0.10 ms |
| Filtered chronicle page | < 0.01 ms |
| Markdown export | 0.47 ms |
| JSON export | 0.48 ms |
| Each 1024×512 layer | 123–133 ms |
| PNG encode 1024×512 | 33.62 ms |
| 1,000 JSON docs parsed | 0.40 ms |
| 1,000 malformed JSON docs rejected | 0.17 ms |
| 1,000 scenarios parsed | 0.57 ms |

Headline: a 10,242-cell world is ready in about 95 ms, a 10,000-year history
adds about 4 ms, and a map layer renders in about 125 ms. A full Studio forge is
dominated by rendering, not simulation.

## 5. Determinism

- Same seed and scenario reproduce world and chronicle hashes exactly within the
  build and platform.
- Ten seeds are replayed by the acceptance suite, not one.
- Randomness is explicit: xoshiro256** with SplitMix64 seeding and named streams
  per subsystem; per-cell noise is hashed, not drawn from shared state.
- There is no hash-map iteration in a simulation path.
- Cross-platform bit equality is **not** claimed, because transcendental
  functions use the platform libm. This is documented, not hidden.

## 6. Chronicle causality

- Every event has at least one cause.
- Event causes must already exist; forward references are rejected.
- Duplicate event causes are collapsed at record time.
- Each run is validated before the Studio serves it; an unsound chronicle marks
  the job failed.
- Acceptance walks every event in a run, not a sample, and verifies that every
  chain reaches measured state.
- Chains and paths are bounded, so a hostile history cannot hang the UI.

## 7. Security and bounds

- Server binds `127.0.0.1`, requires a 128-bit token on every API route, compares
  in constant time, and emits no CORS headers.
- CSP blocks remote and inline scripts; all assets are embedded.
- Headers ≤ 16 KiB, bodies ≤ 64 KiB, URLs ≤ 512 bytes, JSON depth ≤ 32, scenario
  ≤ 8 KiB, chain depth ≤ 48.
- Two concurrent generations, ten cached worlds, 200,000 events per run.
- No `unsafe`, no external crates, no runtime downloads.

Details: `docs/SECURITY.md`.

## 8. Definition of Done checklist

1. **Release build with zero warnings** — verified.
2. **Debug and release test suites pass** — 376 tests each, verified.
3. **Project acceptance tests pass** — table in section 3.
4. **1,000 malformed inputs fail cleanly** — verified by test and fuzz campaign.
5. **Browser Studio works via Playwright** — verified, including forge, layers,
   year scrub, inspection, chronicle, causes and map download.
6. **README, docs, CHANGELOG, LICENSE, LIMITATIONS, FINAL_REPORT** — present.
7. **This report lists LOC, commits, tests, coverage, benchmarks and gaps** —
   sections 2–9.
8. **Repository committed and pushed** — public `BViganotti/terra`.
9. **No external crate, no CI, no CLI-only product** — verified; the product is
   the Studio, and headless flags are only for tests/benchmarks.

## 9. Gaps and honest limitations

- **Total LOC above target.** Production Rust is 15,739 lines, inside the
  14–16k target. Including 6,923 lines of Rust tests and 1,840 lines of UI, the
  repository is larger. The test suite is the main reason. Cutting it to hit a
  total line count would reduce evidence, not improve the product.
- **Fixed plate partition.** Plates have motion and evolving stress, but cells do
  not migrate between plates. "Plate area within 0.1%" is therefore verified as a
  partition invariant, not a moving-area ledger.
- **Qualitative geology and climate.** Orogeny, currents, moisture and
  precipitation are plausible approximations with tested invariants, not
  geodynamic or general-circulation models. Precipitation is calibrated to a
  scenario mean after the relative pattern is computed.
- **Scalar ecology.** Species richness is an estimate, not a species list.
- **Rule-based societies.** No individual agents, markets, alliances or trade
  networks. Diplomacy is a relation score, the economy is one treasury and nine
  resources.
- **Natural events do not reshape terrain.** Eruptions, droughts and sea-level
  changes affect societies and the record; they do not re-run hydrology.
- **Same-platform determinism.** Cross-platform hashes may differ through libm.
- **No coverage instrumentation for the launcher and harnesses.** Coverage is
  measured for libraries and endpoints; launcher browser branches and fuzz/bench
  binaries are not covered.
- **Playwright is development-only.** It is not a Rust dependency. A pure-Rust
  HTTP end-to-end test covers the same API when Playwright is unavailable.
- **No CI by requirement.** Every commit was built and tested locally; the final
  state was re-verified with the commands in section 11.
- **Nearest-cell rendering.** At high zoom, cell facets are visible. PNG output
  uses fixed-Huffman DEFLATE and is larger than a general encoder would produce.

The full list is in `LIMITATIONS.md`.

## 10. Repository map

```
Cargo.toml                 workspace
crates/terra-core/         sphere, tectonics, terrain, hydrology, climate, biomes, scenarios, world
crates/terra-history/      civilizations, war, chronicle, queries, exports
crates/terra-studio/       server, JSON, PNG, renderer, launcher, embedded client
assets/                    HTML, CSS, JS, favicon
examples/scenarios/        six reproducible worlds
fuzz/                      deterministic malformed-input campaign
bench/                     measured timing harness
tests/studio/              Playwright end-to-end test
docs/                      simulation, chronicle, Studio, API, determinism, testing,
                           performance, security, coverage, verification, glossary
```

## 11. Reproduce the final state

```sh
cargo build --release
cargo test
cargo test --release
cargo fmt --all -- --check

cargo run --release -p terra-studio -- --verify --seed 42 --years 10000 --subdiv 5
cargo run --release -p terra-fuzz -- 1000
cargo run --release -p terra-bench

cargo run --release -p terra-studio          # opens the Studio
python3 tests/studio/test_studio.py          # development-only browser test
```
