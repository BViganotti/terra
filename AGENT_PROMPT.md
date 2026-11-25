# Terra — One-Shot Agent Prompt

You are the sole engineer for this repository, `terra`. Build it in ONE
continuous run. Do not ask questions. Do not stop after planning, after the first
milestone, or after the first working demo. Continue until the Definition of Done
in `REQUIREMENTS.md` is true.

Before coding, read `PLAN.md` and `REQUIREMENTS.md` completely. Then start
implementing immediately. Do not stop after writing files.

Mission: Terra is a seeded planet with a history: plate tectonics, erosion,
climate, biomes, ecosystems and rule-based civilizations whose rise and fall is
recorded in a traceable chronicle. It is not a database, not a CRDT, not crash
analysis, not observability, and not a CLI.

Hard rules:
- Rust stable, Cargo workspace. No external crates, no internet at runtime, no
  CDN, no external fonts/assets, no vendored code.
- Rust standard library only.
- UI: embedded local HTTP/SSE server on 127.0.0.1 with original HTML/CSS/JS
  compiled into the binary; auto-open the browser. No terminal interaction.
- Soundness over speed. Deterministic seeds, bounded memory, no panics on
  malformed input.
- Target 14–16k LOC. No filler.
- At least 90 commits, target 100+. Every commit builds and passes tests.
- Commit every vertical slice. Push every 3–5 commits if a remote exists. If
  `gh` is authenticated, create PUBLIC `BViganotti/terra` (or `terra-world` if
  taken) and push; otherwise write PUSH.md.
- NO CI/CD. Do not create `.github/workflows`.
- README, MIT LICENSE, CHANGELOG, examples, tests, fuzz, benchmark, docs.
- If blocked, ship the smallest sound fallback, document it in LIMITATIONS.md,
  and continue. Never fake determinism or chronicle causality.

Follow the milestones in `PLAN.md` exactly in order. Every milestone is a set of
vertical slices. Write tests alongside code. Keep the build green. At the end
write FINAL_REPORT.md with LOC, commits, tests, coverage, benchmarks and gaps.

Continue until every item in the Definition of Done is true. Do not stop early.
