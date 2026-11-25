# Architecture

Terra is a pipeline of deterministic stages. Each stage reads the previous
stage's grid layers and writes new ones; nothing depends on wall-clock time,
hash-map iteration order or floating point ambient state.

```
seed + scenario
      │
      ▼
  icosphere  ──►  plates  ──►  tectonics  ──►  elevation
                                                │
              ┌─────────────────────────────────┘
              ▼
        hydrology (fill, erode, rivers, lakes, deltas)
              │
              ▼
        climate (temperature, wind, moisture, currents)
              │
              ▼
        biomes + ecosystems
              │
              ▼
        civilizations + chronicle (year loop)
              │
              ▼
        snapshots, PNG maps, Markdown/JSON exports
```

## Determinism

- All randomness comes from `terra_core::rng::Rng`, a xoshiro256** generator
  seeded through SplitMix64. Independent streams are derived with explicit
  stream identifiers, never from iteration order.
- Grids are dense `Vec`s indexed by cell id. There is no randomized hashing in
  any simulation path; maps used for lookups are `BTreeMap` or sorted vectors.
- Floating point is used for physics. Runs are deterministic on a single
  platform and for a fixed build; the hash is computed over layer bits.
- The chronicle is append-only. Event ids are sequential and every cause points
  backwards in time, which makes causality acyclic by construction.

## Geometry

The world is a subdivided icosahedron. `Sphere::new(subdiv)` normalizes the 12
base vertices and divides each triangle into four, deduplicating midpoints by
index pair. At subdivision 5 this is 10,242 cells and 20,480 triangles, which is
the default Studio resolution. Cells store a position on the unit sphere,
neighbors in counter-clockwise order and a spherical area derived from the
incident triangles.

## Coordinate conventions

- Latitude is degrees in `[-90, 90]`, longitude degrees in `[-180, 180]`.
- Elevation is meters relative to sea level, positive up.
- Temperature is degrees Celsius (annual mean).
- Precipitation is millimeters per year.
- Discharge is cubic meters per second (scaled approximation).
- Years are integers from `0` (planet formation) forward.

## Layers

`terra_core::world::World` owns the layer arrays. Layers are plain vectors so a
snapshot is cheap to clone and hash. The Studio renders layers independently:
elevation with hypsometric tint and hillshade, plates with categorical colors,
climate with ramps, biomes with a fixed palette, rivers as accumulation-weighted
polylines, and political maps from periodic ownership snapshots.

## Chronicle

`terra_history::chronicle::Chronicle` stores events with structured causes:

- `Cause::State(StateFact)` — a measured physical or social fact, e.g.
  `precip` at a cell, or `cohesion` of a civilization.
- `Cause::Event(EventId)` — an earlier chronicle event.

Queries never mutate history. `explain` walks causes into a deterministic
narrative, and `validate` rejects orphan events or forward references. The
Studio's event panel uses the same query API as the exports.

## Server

`terra-studio` runs a small HTTP/1.1 server on loopback with a random bearer
token. It serves embedded assets from `include_str!`, accepts bounded JSON
requests, runs generation on a worker thread, and streams progress over SSE.
Rendered maps are encoded to PNG by the built-in encoder, which uses a
fixed-Huffman DEFLATE stream with an Adler-32 trailer.

No stage writes to stdout unless the launcher is in headless reporting mode.
