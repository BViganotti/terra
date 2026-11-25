# Fuzz harness

`terra-fuzz` is a deterministic malformed-input campaign written with the
standard library. It is not a `cargo-fuzz` target because Terra takes no
external crates.

## Run

```sh
cargo run --release -p terra-fuzz -- 1000
cargo run --release -p terra-fuzz -- 100000
```

The optional first argument is the number of cases per family (1–200,000,
default 1,000). The second is the mutation seed.

## Families

1. **Scenario rejection** — 1,000+ documents guaranteed malformed by
   construction: unknown keys, duplicates, unterminated quotes, out-of-range
   values, non-numeric values, overlong names, invalid keys and oversized input.
2. **JSON rejection** — truncated objects, duplicate keys, trailing data, leading
   zeros, invalid escapes, lone surrogates, depth bombs, oversized arrays,
   non-finite numbers and trailing commas.
3. **Valid JSON round trip** — nested objects, quoted Unicode, booleans, nulls
   and negative floats must parse back to an equal value.
4. **Mutation sweep** — valid scenario and request documents receive
   deterministic structural edits. Outcomes may be valid or rejected; accepted
   scenarios must pass validation.

## Output

One JSON line:

```json
{"iterations":1000,"scenarioRejected":1000,"jsonRejected":1000,"mutationsParsed":359,"mutationsRejected":641,"validRoundTrips":1000,"status":"pass"}
```

Exit code is zero only when every constructed malformed case was rejected and
every valid round trip preserved its value. A panic aborts the process, which is
a finding: the parsers are expected to fail cleanly.

## Why no corpus directory

Inputs are generated deterministically from the iteration index and seed, so a
failure is reproducible with two numbers. There is no hidden corpus to drift.
