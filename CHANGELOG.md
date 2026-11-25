# Changelog

All notable changes to Terra are documented here. The project uses a simple
`Added / Changed / Fixed` format and stays on `0.x` while the simulation model
evolves. Hashes are not promised across model versions.

## [0.1.0] - 2026-11-25

The first complete Terra: a deterministic seeded planet, a traceable history,
and a local browser Studio. Standard library only; no external crates.

### Added — foundations

- Cargo workspace with `terra-core`, `terra-history`, `terra-studio`, `fuzz`
  and `bench`.
- MIT license, README, architecture notes, Makefile and build targets.
- FNV-1a hashing with canonical float bits.
- xoshiro256** RNG seeded through SplitMix64, explicit streams for every
  subsystem.
- 3-vector math and icosphere generation, subdivision, sorted CSR adjacency,
  spherical cell areas, nearest-cell index.
- Seeded value noise, fBm, ridged and warped fields.

### Added — planet

- Farthest-point plate seeding, contiguous graph Voronoi partition, angular
  plate motion, boundary classification, subduction, trenches, volcanic arcs and
  orogenic uplift.
- Elevation from a continental mask, tectonic relief, sea-level quantile,
  landmasses, shelves, coastlines and crustal age.
- Priority-Flood depression filling.
- Steepest-descent drainage, accumulation, Strahler order and ocean capture.
- Stream-power erosion, mass-conserving diffusion, sediment routing, deltas.
- Lakes with surface, volume, spill point and endorheic cases.
- Zonal winds, Ekman-like currents, temperature with lapse rate and
  continentality, synchronous moisture transport, orographic lift, rain shadows,
  ITCZ and subtropical dryness.
- Whittaker-like biome classification with azonal wetlands and mangroves.
- Connected biome patches, Miami-model productivity, species richness,
  fragility and carrying capacity.
- World assembly with statistics, per-stage hashes, progress callbacks and cell
  inspection.

### Added — history

- Append-only chronicle with dense ids, state facts and event causes.
- Structural validation, bounded cause chains, explicit paths to state facts,
  descendants, explanations and filtered queries.
- Markdown, JSON and CSV exports.
- Civilizations with traits, settlements, cities, resources, technology,
  growth, food, cohesion, treasury and military.
- Frontier expansion, trade routes, border friction, wars, battles, city
  capture, peace, famine, epidemics, golden ages, migration and collapse.
- Political snapshots, war records, replay hashes and verification.

### Added — Studio

- Loopback HTTP/1.1 server with random bearer token, constant-time comparison,
  CSP, bounded headers, bodies and URLs.
- Worker-thread job runner with two concurrent generations and a ten-world
  cache.
- Server-Sent Events progress with polling fallback.
- Bounded JSON parser and stable writer.
- RGB rasterizer, hillshade, hypsometric and categorical layers, river
  polylines, political territory, borders, cities and bitmap labels.
- Deterministic PNG encoder with CRC-32, Adler-32 and fixed-Huffman DEFLATE.
- Original client: ten layers, zoom/pan, cell inspector, political year scrub,
  civilization list, chronicle filters, cause tracing and exports.
- Launcher with browser auto-open, `--report`, `--verify`, `--validate-scenario`,
  `--scenario`, `--preset`, `--port` and `--no-open`.

### Added — examples, tests and tools

- Six example scenarios covering temperate, arid, icehouse, archipelago,
  pangaea and a fast micro world.
- Acceptance tests for ten-seed replay, plate area, convergence/relief,
  downhill rivers, lake basins, sediment budget, climate belts, biomes,
  ecosystem bounds, world statistics, 1,000 malformed scenarios, traceability,
  replay hashes, 10,000-year runs, rule breadth and ownership consistency.
- Playwright end-to-end Studio test: forge, layers, year scrub, inspection,
  filters, paging, cause links, map download.
- Deterministic malformed scenario and JSON campaign.
- Measured benchmark harness covering generation, history, queries, exports,
  rendering, PNG and parsing.

### Changed

- Political maps tint whole cell areas instead of single pixels and draw
  continuous borders.
- Civilization rows show fallen peoples as fallen instead of stale population.
- History refuses publication when the chronicle is not sound.

### Fixed

- Nearest-cell index coverage at coarse subdivisions.
- Duplicate cause citations from converging rules.
- Ocean cells excluded from lake extraction.
- Deterministic tie-breaking in BFS plate assignment and flow direction.

### Known gaps

See [LIMITATIONS.md](LIMITATIONS.md). Highlights: fixed plate partition,
qualitative climate and orogeny, scalar ecosystems, rule-based civilizations,
same-platform determinism only, no coverage instrumentation, and no CI by
requirement.
