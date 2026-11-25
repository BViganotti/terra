# Terra — Implementation Plan

Status: ready for one-shot implementation.

## Mission

Terra is a seeded planet with a history: plate tectonics, erosion, climate,
biomes, ecosystems, and rule-based civilizations whose rise and fall is written
into a traceable chronicle. It is a browser-based world simulator, not a game
and not a database.

## Product shape

- `terra-core`: sphere grid, tectonics, erosion, climate, biomes, ecosystems.
- `terra-history`: civilizations, events, chronicle, causes and traceability.
- `terra-studio`: local loopback HTTP/SSE Studio with embedded assets.
- `examples/`: seeds and scenario scripts.
- `tests/`, `fuzz/`, `bench/`.

The user-facing product is the Studio and the embeddable library. A CLI may exist
only for tests, benchmarks and deterministic seed runs.

## Scope

### In v1
- Seedable icosphere world with plates, boundaries, uplift and subduction.
- Erosion, sediment transport, rivers and lakes.
- Temperature, precipitation, wind and ocean-current approximations.
- Biomes and simple ecosystems.
- Rule-based civilizations: settlement, expansion, trade, war, collapse.
- Chronicle log with explicit causal links; every event traceable to simulation
  state or prior event.
- Map pages: plates, elevation, biomes, rivers, civilizations, chronicle timeline.
- Deterministic replay from seed and scenario.
- Export maps as PNG and chronicle as Markdown/JSON.

### Out of v1
- Real Earth data import, GIS, exact climatology, sentient agents, economy
  simulation, multiplayer.
- Databases, CRDTs, crash analysis, observability.

## Architecture

```
terra/
  Cargo.toml
  crates/
    terra-core/
      src/sphere/       # icosphere, neighbors, interpolation
      src/tectonics/    # plates, boundaries, uplift, subduction
      src/hydrology/    # erosion, rivers, lakes, sediment
      src/climate/      # temperature, precipitation, wind, currents
      src/biome/        # biomes, ecosystems
    terra-history/
      src/civ/          # settlements, expansion, diplomacy, war
      src/chronicle/    # events, causes, queries
    terra-studio/
      src/server.rs
      src/json.rs
      src/assets.rs
  assets/
  examples/
  tests/ fuzz/ bench/
```

Rust std only. Own JSON, HTTP/SSE, PNG encoder, map renderer and UI assets.

## LOC budget (target 14–16k)

| Module | LOC |
|---|---:|
| Sphere grid and neighbors | 1,200 |
| Tectonics/erosion/hydrology | 2,800 |
| Climate/biomes/ecosystems | 2,200 |
| Civilizations/history | 2,500 |
| Chronicle causes/queries | 1,200 |
| Map renderer/PNG | 1,500 |
| Studio server/API | 1,200 |
| UI assets | 1,400 |
| Tests/fuzz/bench | 1,500 |

## Milestones and commit ranges

### P0 Scaffold (commits 1–8)
Cargo workspace, test harness, README, LICENSE, Makefile, no CI, no crates.

### P1 Sphere and tectonics (commits 9–24)
Icosphere → neighbors → plates → boundary types → uplift/subduction → seeded
determinism → plate-area invariants → visual plates.

### P2 Erosion and hydrology (commits 25–40)
Height field → erosion → sediment → rivers/lakes → drainage → delta → invariants
→ map layers.

### P3 Climate and biomes (commits 41–54)
Temperature → precipitation → wind/currents → biome classification → ecosystem
rules → latitude tests → map layers.

### P4 Civilizations and chronicle (commits 55–72)
Settlement → expansion → resources → trade/war → collapse → event log → causal
links → chronicle queries → timeline.

### P5 Studio (commits 73–86)
HTTP/JSON/SSE → embedded assets → world map pages → timeline → seed input →
chronicle browser → event cause panel.

### P6 Determinism and hardening (commits 87–102)
Seed replay → 10,000-year runs → malformed scenario fuzz → performance → docs →
FINAL_REPORT.

## Testing

- 10 seeds replay to identical world and chronicle hashes.
- Plate area conservation within 0.1%.
- Mountains correlate with convergent boundaries.
- Rivers never flow uphill; lakes are basins.
- Every chronicle event has a cause chain ending in simulation state or a prior
  event.
- 1,000 malformed scenarios fail cleanly.
- Studio Playwright: generate seed, view maps, scrub chronicle.

## Acceptance criteria

- Deterministic seed runs with identical hashes.
- Plate area within 0.1%.
- Mountains at convergent edges; rivers downhill.
- Chronicle events traceable; no orphan events.
- 10,000-year run completes on 4 cores in reasonable time.
- Browser Studio shows plates, biomes, civilizations and timeline.
- No external crates; no CI; no CLI-only product.

## Risks and fallbacks

- Realistic climate is a research project: document approximations and test
  invariants, not Earth parity.
- Civilizations can become arbitrary: keep rules explicit and events causal.
- Map rendering: software rasterizer to PNG; no map tiles or external data.
