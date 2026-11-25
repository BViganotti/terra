.PHONY: build test test-rel fuzz bench studio e2e coverage fmt clean check

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

e2e:
	cargo build --release -p terra-studio
	python3 tests/studio/test_studio.py

coverage:
	cargo llvm-cov --workspace --tests --summary-only

studio:
	cargo run --release -p terra-studio

fmt:
	cargo fmt --all

clean:
	cargo clean
