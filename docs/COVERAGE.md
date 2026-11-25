# Coverage

Coverage was measured with `cargo llvm-cov`, which is development tooling only;
it is not a project dependency and is not required to build or run Terra.

```sh
cargo llvm-cov --workspace --tests --summary-only
```

The run instruments every workspace crate, executes the full test suite
including integration tests, and reports region, function and line coverage. It
includes the fuzz and bench harnesses, which have no automated tests, so the
workspace total is lower than the library totals.

## Result

Measured after the full acceptance suite was in place:

| Scope | Region | Function | Line |
|---|---:|---:|---:|
| Workspace total | 89.50% | 87.04% | 89.05% |

Selected modules:

| Module | Line coverage |
|---|---:|
| `terra-core/src/lib.rs` | 100.00% |
| `terra-core/src/noise.rs` | 98.72% |
| `terra-core/src/world.rs` | 97.95% |
| `terra-core/src/hydrology/fill.rs` | 96.81% |
| `terra-core/src/hydrology/flow.rs` | 95.68% |
| `terra-core/src/hydrology/lakes.rs` | 95.88% |
| `terra-core/src/sphere.rs` | 93.59% |
| `terra-core/src/scenario.rs` | 92.22% |
| `terra-history/src/chronicle/export.rs` | 99.38% |
| `terra-history/src/chronicle/query.rs` | 95.55% |
| `terra-history/src/civ/mod.rs` | 95.96% |
| `terra-history/src/civ/sim.rs` | 90.63% |
| `terra-studio/src/render.rs` | 95.36% |
| `terra-studio/src/png.rs` | 92.43% |
| `terra-studio/src/json.rs` | 91.70% |
| `terra-studio/src/server.rs` | 82.57% |

## What is not covered

- The launcher's browser-open branches and OS-specific command spawning.
- The fuzz and bench harnesses themselves (they are exercised manually).
- Some defensive branches that require a broken invariant to reach.
- OS-specific failure paths in token generation and socket errors.

## Honest caveat

Coverage is a tool for finding untested code, not a quality score. Terra's
central claims are invariants (replay, causality, conservation, bounds), and
those are tested directly. A line executed without an assertion is not evidence;
the acceptance tests in `docs/TESTING.md` are the evidence.

Numbers will change as tests are added or code is refactored. Re-run the command
above and update this table rather than citing a stale figure.
