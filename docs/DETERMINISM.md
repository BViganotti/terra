# Determinism and replay

Terra promises that a seed and scenario reproduce the same world and the same
chronicle. That promise is enforced, not assumed.

## Randomness

- Generator: xoshiro256**, seeded through SplitMix64.
- Streams: every subsystem has an explicit `Stream` constant (plates, terrain,
  erosion, climate, ecology, history, events, cosmetic). A child generator is
  derived with `fork(stream)`, never by consuming a shared sequence.
- Per-cell variation uses `mix3(seed, stream, cell)` hashed directly, so adding
  a new consumer cannot shift existing values.
- Timeline decisions use `Rng::new(seed, HISTORY).fork(Stream(step))`, where the
  step is derived from the simulation year. The same year always sees the same
  random choices.

There is no operating-system entropy in a simulation path. `/dev/urandom` is
used only for the Studio auth token, which is not part of the world.

## Data layout

- Grids are dense `Vec`s indexed by cell id.
- Adjacency is sorted CSR.
- Maps used in simulation are `Vec`, `BTreeMap` or sorted vectors. Iteration
  order cannot vary between runs.
- Seeds of plates are chosen by farthest-point sampling with index tie-breaks.
- Heap entries order by `f64::total_cmp`, then cell index.

## Floating point

Physics is floating point. Identical binaries on the same platform produce
identical bits. Terra does not promise cross-platform bit equality for
transcendental functions, because the standard library delegates `sin`, `cos`,
`exp` and `powf` to the platform libm. The world hash is computed over the
resulting bits, so a hash mismatch is visible rather than silently ignored.

Within one machine and one build, replay is exact. CI would have to pin the
toolchain and OS to compare hashes across platforms; Terra has no CI by design.

## Hashes

`WorldHashes` publishes one hash per stage plus a combined hash:

- plates: seed, plate assignment, motion, area, uplift;
- terrain: final elevation and lake mask;
- hydrology: elevation, filled surface, lake cells and surfaces;
- climate: temperature, wind, currents, precipitation, humidity;
- biosphere: biome ids, patch ids, NPP and carrying capacity;
- combined: configuration and all stage hashes.

`HistoryHashes` publishes:

- chronicle: every event id, year, kind, title, detail and cause;
- civs: names, populations, technologies, owned cells and final owner grid;
- combined: seed, span and both.

Hashes use FNV-1a with a final avalanche; floats are canonicalized (`-0.0` and
`NaN` normalized). Hashing is for identity, not security.

## Replay commands

```sh
# Two runs, compare world and chronicle hashes, validate causality.
cargo run --release -p terra-studio -- --verify --seed 42 --years 10000

# One run, print a machine-readable report.
cargo run --release -p terra-studio -- --report --seed 42 --years 10000

# The acceptance suite repeats ten seeds and compares every published hash.
cargo test -p terra-core --test determinism
```

The Studio shows the combined world hash and, in the World panel, whether the
chronicle is traceable.

## What can change a hash

- The seed.
- Any scenario field that reaches a simulation stage.
- A code change that alters arithmetic or iteration order.
- A different platform libm or compiler optimization that changes float results.

What must not change a hash: opening the Studio, rendering a map, exporting the
chronicle, querying events, changing the layer or year slider, or resizing the
browser. Those are read-only views.
