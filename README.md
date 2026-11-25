# Terra

Terra is a seeded planet with a history. It grows an icosphere world from a
single seed: plates drift and collide, mountains rise and erode, rivers cut
toward the sea, climate paints biomes, ecosystems settle in, and rule-based
civilizations found cities, trade, fight and fall. Every event in the chronicle
records what caused it, and the whole run replays bit-for-bit from its seed.

Terra is a local, browser-based world simulator written in Rust with **no
external crates**. The client is original HTML, CSS and JavaScript embedded in
the binary. Nothing is fetched from the network.

## Quick start

```sh
cargo run --release -p terra-studio
```

The launcher binds an HTTP/SSE server to `127.0.0.1` on a random free port,
prints the authenticated URL once, and opens your browser. There is no terminal
workflow: the Studio is the product.

Headless reproduction for tests and benchmarks:

```sh
cargo run --release -p terra-studio -- --seed 42 --years 10000 --report
```

## What is inside

| Crate | Responsibility |
|---|---|
| `terra-core` | icosphere, tectonics, erosion, hydrology, climate, biomes, ecosystems |
| `terra-history` | civilizations, diplomacy, war, chronicle with causal links |
| `terra-studio` | HTTP/SSE server, JSON, PNG encoder, map renderer, embedded UI |

Documentation lives in [`docs/`](docs/). The acceptance criteria are in
[`PLAN.md`](PLAN.md) and [`REQUIREMENTS.md`](REQUIREMENTS.md); the final measured
state is in [`FINAL_REPORT.md`](FINAL_REPORT.md).

## Guarantees

- Deterministic: the same seed and scenario produce the same world and the same
  chronicle hashes.
- Traceable: every chronicle event cites simulation state or earlier events;
  cause chains are acyclic and terminate in physical facts.
- Bounded: every parser enforces hard limits; simulations cap years, cells and
  civilizations; malformed input returns errors instead of panicking.
- Offline: standard library only, no runtime downloads, no external assets.

## Development

```sh
make build      # cargo build --release
make test       # debug tests
make test-rel   # release tests
make fuzz       # deterministic malformed-input campaign
make bench      # timing harness
make studio     # launch the Studio
```

No CI is configured, by design. Every slice is committed with a green build.

## License

MIT. See [LICENSE](LICENSE).
