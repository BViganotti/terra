# Terra examples

The files in `scenarios/` are ordinary Terra scenario documents. They are
validated by the parser and by `--validate-scenario`, and each has a stable seed
so you can reproduce a run exactly.

## Run a scenario in the Studio

```sh
cargo run --release -p terra-studio
```

Then paste the scenario text into the Studio? No: the Studio currently exposes
seeds and presets, not arbitrary scenario text. Use the headless launcher for a
scenario file:

```sh
cargo run --release -p terra-studio -- --scenario examples/scenarios/micro.terra --report
```

The launcher starts the Studio when `--report` is absent, so to inspect a
scenario in the browser use a temporary seed equivalent:

```sh
cargo run --release -p terra-studio -- --seed 123 --years 5000 --no-open
```

Open the printed `http://127.0.0.1:.../?t=...` URL. The seed, span and planet
are deterministic; no account, database or network service is involved.

## Validate without running

```sh
cargo run --release -p terra-studio -- --validate-scenario examples/scenarios/temperate.terra
```

This prints the canonical key order and exits non-zero on the first problem,
with a line number.

## Replay check

```sh
cargo run --release -p terra-studio -- --verify --seed 42 --years 1000 --subdiv 3
```

The verifier generates the world and history twice, compares the physical and
chronicle hashes, validates every causal link, and prints a one-line JSON
verdict.

## Scenario catalogue

| File | Seed | Span | Plates | Land | Character |
|---|---:|---:|---:|---:|---|
| `temperate.terra` | 42 | 6,000 | 9 | 31% | Balanced default |
| `arid.terra` | 77 | 8,000 | 8 | 44% | Hot, dry, river-bound |
| `icehouse.terra` | 5 | 7,000 | 10 | 36% | Glacial, short seasons |
| `archipelago.terra` | 123 | 5,000 | 14 | 18% | Fragmented islands |
| `pangaea.terra` | 9 | 9,000 | 5 | 48% | One vast continent |
| `micro.terra` | 3 | 1,000 | 6 | 30% | Fast smoke test |

## Notes

- Scenario text is limited to 8 KiB, 96 lines and 32 entries. Unknown keys,
  duplicate keys and out-of-range values are errors, not warnings.
- `seed` is limited to 32 bits for JavaScript safety in the Studio. The launcher
  accepts larger integers but clamps them the same way.
- The chronicle export from the Studio includes Markdown, JSON and CSV.
- No example downloads anything. The map renderer and UI are compiled into the
  binary.
