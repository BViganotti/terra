# Benchmark harness

`terra-bench` measures wall-clock time with `std::time::Instant`. No external
benchmarking crate is used, and no number is estimated.

## Run

```sh
cargo run --release -p terra-bench
```

The harness prints a table of median, minimum and maximum times and a final JSON
line with headline numbers.

## Workloads

- World generation at subdivision 2–5, including plates, orogeny, erosion,
  climate, biomes, ecosystems, statistics and hashing.
- 10,000-year histories at subdivision 4 and 5 with eight civilizations.
- Cause chains for every seventh event.
- Filtered chronicle pages.
- Markdown and JSON chronicle exports.
- All ten 1024×512 render layers.
- PNG encoding of the atlas.
- Parsing 1,000 valid JSON documents, 1,000 malformed ones, and 1,000 scenario
  documents.

## Methodology

- Each workload is run several times; medians are reported with min/max.
- The machine is not warmed or pinned; absolute numbers vary with load.
- Results are recorded in `docs/PERFORMANCE.md` and summarized in
  `FINAL_REPORT.md`.
- The final JSON line exists so a report can cite exact measured values instead
  of rounded guesses.

## Interpretation

World generation is dominated by nearest-cell lookups in rendering and by
per-cell climate transport. History is cheap because it operates on
civilizations and frontiers, not every cell every year. PNG encoding is a
meaningful fraction of a layer change, which is why the Studio caches rendered
images in the browser canvas rather than re-encoding on every pan.
