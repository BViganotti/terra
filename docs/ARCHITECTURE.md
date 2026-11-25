# Architecture

Terra is a deterministic pipeline followed by a read-only presentation layer. A
world is a value; a history is a value; the Studio renders both. No stage depends
on wall-clock time, hash-map iteration order or ambient random state.

## Workspace

```
terra/
  Cargo.toml                 virtual workspace, no external dependencies
  crates/terra-core/         planet physics
  crates/terra-history/      civilizations and chronicle
  crates/terra-studio/       server, renderer, PNG, JSON, launcher, UI
  assets/                    HTML, CSS, JS, SVG embedded at compile time
  examples/scenarios/        six reproducible scenario documents
  fuzz/                      std-only malformed-input campaign
  bench/                     std-only timing harness
  tests/studio/              Playwright browser tests
  docs/                      simulation, chronicle, API, security, testing, etc.
```

The binary is produced by `terra-studio` and named `terra`.

## Pipeline

```
seed + scenario
      │
      ▼
  icosphere  ──►  plates  ──►  boundary stress  ──►  elevation
                                                      │
                    ┌─────────────────────────────────┘
                    ▼
        hydrology: fill → erode → route sediment → lakes/deltas
                    │
                    ▼
        climate: wind → currents → temperature → moisture
                    │
                    ▼
        biomes → ecosystems
                    │
                    ▼
        civilizations → chronicle → snapshots
                    │
                    ▼
        PNG maps, JSON/CSV/Markdown exports, Studio
```

Each arrow is a pure function of the prior layers plus the configuration and
explicit random streams. `World::generate_with` reports progress after a stage
completes; it never claims partial work.

## Core data

- **Sphere** — 12·4^level + 2 unit cells, 20·4^level triangles, sorted CSR
  neighbours, spherical areas summing to 4π.
- **Tectonics** — `plate_of: Vec<u16>`, plates, boundaries, stress, uplift,
  subsidence, volcanism, continental mask.
- **Terrain** — relative and raw elevation, sea level, landmasses, shelves,
  coasts, crust thickness and age.
- **Hydrology** — eroded elevation, filled surface, downhill targets, drainage
  area, discharge, Strahler order, lakes, deltas, sediment accounting.
- **Climate** — temperature mean/min/max/seasonality/continentality, wind
  vectors, currents, precipitation, humidity, aridity, cloud, PET.
- **Biomes/Ecosystems** — biome id, patch id/size, NPP, richness, capacity,
  tier, fragility.
- **World** — config, name, all layers, stats, per-stage hashes, cell inspector.

Vectors are dense and indexed by cell. Layers are cheap to clone and hash.

## History data

- **Civilization** — id, name, colour, capital, cells (sorted), cities,
  population, food, cohesion, military, treasury, technology, traits, resources,
  founded/collapsed years, last event, wars.
- **Chronicle** — append-only `Vec<Event>` with dense ids, kinds, text, optional
  cell/civ, structured causes, severity and tags.
- **Snapshots** — owner grid and civ stats at a year, used for political maps and
  timeline series.
- **Wars** — active and historical records.

History never writes back into a world. A political map is a view of a snapshot,
not a mutation of terrain.

## Determinism boundaries

- All simulation randomness comes from `Rng` streams or hashed per-cell values.
- No randomized hashing in simulation paths.
- Floats are hashed by canonical bits.
- Stage hashes are published; the combined hash couples configuration and
  physics; the history hash couples events and territory.
- Same build and platform replay is exact. Cross-platform transcendentals are
  not promised.

See `docs/DETERMINISM.md`.

## Causality

Events are validated at record time: non-empty causes, existing past causes,
monotone time, text and severity limits. Validation at the end re-checks the
whole store. Chain traversal is breadth-first and bounded; path enumeration is
explicit and bounded. The Studio refuses to serve an unsound chronicle.

See `docs/CHRONICLE.md`.

## Studio

```
client (assets/app.js)
   │  fetch + EventSource, token in query
   ▼
server.rs
   ├─ HTTP parse (bounded), token auth, static assets
   ├─ job runner (max 2 active, 10 cached)
   ├─ render endpoints → render.rs → png.rs
   ├─ JSON endpoints → json.rs
   └─ exports → chronicle/export.rs
```

Generation runs on worker threads. The client subscribes to SSE progress and
falls back to polling. Maps are encoded to PNG and cached in the browser canvas;
the server does not keep rendered images.

The server binds loopback only, requires a random token, sets a restrictive CSP,
and enforces header/body/URL limits. See `docs/SECURITY.md` and `docs/API.md`.

## Coordinate conventions

- Latitude degrees `[-90, 90]`, longitude `[-180, 180]`.
- Elevation meters relative to sea level, positive up.
- Temperature degrees Celsius, annual mean.
- Precipitation millimetres per year.
- Discharge cubic metres per second.
- Years integer from 0.
- Areas steradians on the unit sphere; distances radians unless stated.
- Civilizations numbered densely from 0; cells are `u32`; owners use
  `u16::MAX` for unowned.

## Failure policy

- Malformed input returns an error with a location, never a panic.
- Simulation configuration is clamped by `sanitized()`; the scenario parser
  rejects out-of-range values explicitly.
- A job that panics or produces an unsound chronicle is marked failed and is not
  served.
- Rendering and queries never mutate state, so a bad request cannot corrupt a
  world.
- Bounded caches and caps keep memory predictable: two generations, ten worlds,
  200,000 events, 101 snapshots, 48-deep chains.

## Tooling

- `fuzz/` generates malformed scenarios and JSON plus a mutation sweep.
- `bench/` measures world, history, queries, exports, rendering, PNG and parsing.
- `tests/studio/` drives the real browser; `crates/*/tests/` cover public APIs.
- No CI exists by requirement. Every commit is built and tested locally.

## Where to look next

- Physics: `docs/SIMULATION.md`.
- History: `docs/CHRONICLE.md` and `docs/EVENTS.md`.
- Product: `docs/STUDIO.md`, `docs/API.md`, `docs/RENDERING.md`.
- Quality: `docs/TESTING.md`, `docs/COVERAGE.md`, `docs/PERFORMANCE.md`.
- Limits: `LIMITATIONS.md`.
