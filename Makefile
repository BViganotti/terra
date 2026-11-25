.PHONY: build test test-rel fuzz bench studio fmt clean check

build:
	cargo build --release

check:
	cargo build --workspace --all-targets

test:
	cargo test --workspace

test-rel:
	cargo test --workspace --release

fuzz:
	cargo run --release -p terra-fuzz

bench:
	cargo run --release -p terra-bench

studio:
	cargo run --release -p terra-studio

fmt:
	cargo fmt --all

clean:
	cargo clean
