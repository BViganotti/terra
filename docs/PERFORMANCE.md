# Performance

Measured on the development machine with `cargo run --release -p terra-bench`.
Times are medians of repeated runs; minimum and maximum are printed by the
harness. They are wall-clock numbers, not estimates. The machine is an Apple
Silicon laptop (arm64, macOS); absolute values will differ elsewhere.

## World generation

| Workload | Median |
|---|---:|
| subdiv 2 (162 cells) | 1.65 ms |
| subdiv 3 (642 cells) | 6.13 ms |
| subdiv 4 (2,562 cells) | 23.96 ms |
| subdiv 5 (10,242 cells) | 94.65 ms |

World generation includes plates, orogeny, erosion passes, climate transport,
biomes, ecosystems, stats and hashing.

## History

| Workload | Median |
|---|---:|
| 10,000 years, 8 civilizations, subdiv 4 | 1.74 ms |
| 10,000 years, 8 civilizations, subdiv 5 | 3.95 ms |

History is cheap relative to geology because it operates on civilizations and
frontiers, not every cell every year. A 10,000-year run is far below the
"reasonable time" acceptance bar.

## Queries and exports

| Workload | Median |
|---|---:|
| Cause chains for every 7th event | 0.10 ms |
| Filtered chronicle page | < 0.01 ms |
| Markdown export | 0.47 ms |
| JSON export | 0.48 ms |

## Rendering

| Workload | Median |
|---|---:|
| Any 1024×512 layer | 123–133 ms |
| PNG encode 1024×512 | 33.6 ms |

Rendering dominates wall-clock when a layer is changed. The nearest-cell lookup
is the hot path; the index keeps it responsive without a spatial data structure
dependency.

## Parsing

| Workload | Median |
|---|---:|
| 1,000 valid JSON documents | 0.40 ms |
| 1,000 malformed JSON documents | 0.17 ms |
| 1,000 scenario documents | 0.57 ms |

## Memory

Per-cell layers are `f64` vectors: at subdivision 5, each layer is about 82 KiB.
The world keeps roughly 20 layers, plus biome/plate ids and the sphere (about
10,242 unit vectors). A generated world is on the order of 5–8 MiB. Political
snapshots store a `u16` owner per cell every 100 years; a 10,000-year run keeps
101 snapshots, about 2 MiB. The Studio caches at most ten finished worlds.

## Reproduce

```sh
cargo run --release -p terra-bench
```

The final JSON line contains the headline numbers copied above. If a number
changes, update this document and `FINAL_REPORT.md`; do not round it into a
claim that was not measured.
