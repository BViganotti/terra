# Requirements

## Mission
Terra is a seeded planet with a history: tectonics, erosion, climate, biomes,
ecosystems and rule-based civilizations, with every chronicle event traceable to
a cause. It is a browser world simulator, not a database or a game.

## Scope
- Icosphere world, plates, boundaries, uplift and subduction.
- Erosion, sediment, rivers and lakes.
- Temperature, precipitation, wind and ocean currents.
- Biomes and simple ecosystems.
- Civilizations: settlement, expansion, trade, war, collapse.
- Chronicle with causal links and queries.
- Map layers and timeline in a local browser Studio.
- Deterministic seed replay and map/PNG export.

## Project-specific acceptance
- 10 seeds replay identically.
- Plate area within 0.1%; mountains at convergent edges; rivers downhill.
- Every chronicle event traceable to simulation state or a prior event.
- 1,000 malformed scenarios fail cleanly.
- Studio Playwright: seed, maps, timeline.

## Language and dependencies
- Rust stable, Cargo workspace.
- No external crates, no vendored code, no runtime package downloads.
- Rust standard library only.
- No internet at runtime, no CDN, no external fonts/assets.
- Pushing to GitHub during development is allowed.

## Product shape
- The product is a local desktop developer workbench, not a CLI and not a TUI.
- The UI is a local web app served only on 127.0.0.1 with an embedded HTTP/SSE
  server, a random auth token, and original HTML/CSS/JS compiled into the binary.
- The launcher starts the server and opens the browser. No terminal interaction.
- The core must be usable and testable headlessly.
- Original work. Not related to Git, databases, CRDTs, crash analysis or
  observability products.

## Quality
- Soundness over speed. Correctness, determinism, bounded memory and no panics
  on malformed input matter more than performance.
- Every parse has hard limits on length, nesting and recursion.
- Unit tests, property tests, golden tests and fuzz tests.
- No fake results or fabricated benchmarks.

## Delivery
- Target 12–18k LOC depending on the project. Do not pad with boilerplate.
- At least 90 commits, target 100+. Every commit builds and passes tests.
- Commit every vertical slice. Push every 3–5 commits if a remote exists.
- If `gh` is authenticated, create the PUBLIC repo listed above and push;
  otherwise write PUSH.md.
- No CI/CD. Do not create `.github/workflows`.
- README, MIT LICENSE, CHANGELOG, examples, tests, fuzz, benchmark, docs.
- If blocked, ship the smallest sound fallback, document it in LIMITATIONS.md,
  and continue. Never fake results.

## Definition of Done
1. `cargo build --release` succeeds with zero warnings.
2. `cargo test` and `cargo test --release` pass.
3. The project-specific acceptance tests in `PLAN.md` pass.
4. 1,000 malformed inputs fail cleanly, never panic.
5. Browser Studio opens and the demo scenario works via Playwright.
6. README, docs, CHANGELOG, LICENSE, LIMITATIONS and FINAL_REPORT exist.
7. FINAL_REPORT lists LOC, commits, tests, coverage, benchmarks and gaps.
8. Repo committed and pushed, or PUSH.md exists.
9. No external crate, no CI, no CLI-only product.
