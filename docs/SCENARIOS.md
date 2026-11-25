# Scenarios

A scenario is a small text document that configures a world. It is deliberately
not JSON and not a scripting language: every key maps to one bounded field.

## Example

```ini
# Drowned world with many plates.
name = "Thousand Sounds"
note = "Trade and isolation both matter."
seed = 123
subdiv = 4
years = 5000
plates = 14
land = 0.18
erosion = 8
temperature = 2.0
precip = 1100
```

## Keys

| Key | Type | Range | Meaning |
|---|---|---|---|
| `name` | quoted string | 80 chars | Display name |
| `note` | quoted string | 80 chars | Optional note |
| `seed` | unsigned integer | 32-bit | World seed |
| `subdiv` | integer | 2–6 | Icosphere subdivision |
| `years` | integer | 100–20,000 | Historical span |
| `plates` | integer | 3–20 | Plate count |
| `land` | decimal | 0.10–0.75 | Target land fraction |
| `erosion` | integer | 1–24 | Erosion passes |
| `temperature` | decimal | −12–12 | Global temperature offset, °C |
| `precip` | decimal | 200–2,500 | Target mean land precipitation, mm/yr |
| `preset` | identifier | see below | Base configuration |

`preset` must appear at most once. Explicit numeric keys override the preset
regardless of line order.

Presets: `temperate`, `arid`, `icehouse`, `archipelago`, `pangaea`.

## Syntax

- One `key = value` per line.
- `#` and `//` start a comment.
- Whitespace around keys and values is ignored.
- Strings may be quoted with `"`; a quote inside a string is not supported.
- Unknown keys, duplicate keys, malformed numbers, non-finite numbers and
  out-of-range values are errors with a line number.

## Limits

| Limit | Value |
|---|---|
| Input size | 8 KiB |
| Lines | 96 |
| Line length | 512 bytes |
| Entries | 32 |
| Name/note | 80 characters |

These limits are checked before allocation. The fuzz harness verifies thousands
of malformed documents.

## Precedence

1. Defaults (seed 42, subdivision 5, 10,000 years, 9 plates, 31% land, 14
   erosion passes, 860 mm/yr, 0 °C offset).
2. Preset, if any.
3. Explicit keys, in any order.
4. The launcher or Studio may override `seed`, `years`, `subdiv`, `civs`,
   `aggression` and `expansion`. In the Studio, a custom scenario keeps its own
   subdivision; the header controls override seed, span and peoples.
5. `WorldConfig::sanitized` clamps everything into the supported range.

Scenario parsing itself rejects out-of-range values instead of clamping, so a
typo is visible. Clamping exists for programmatic callers and for fields the
Studio exposes as selectors.

## Validating

```sh
cargo run --release -p terra-studio -- --validate-scenario examples/scenarios/arid.terra
```

The command prints the canonical document and exits non-zero on the first error.

## Reproducing

```sh
cargo run --release -p terra-studio -- --scenario examples/scenarios/arid.terra --report
cargo run --release -p terra-studio -- --verify --scenario examples/scenarios/arid.terra
```

The report contains the seed, hashes and headline statistics. The verifier runs
the same scenario twice and compares world and chronicle hashes.
