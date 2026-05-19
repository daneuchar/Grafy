.PHONY: check fmt fmt-check clippy test bench fuzz dogfood diagnose ci clean

# Plan §M0 acceptance helpers.

check:
	cargo check --workspace

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

# Plan §6 bench corpus runner — wired in M1.
bench:
	cargo bench -p grafy-bench

# Plan §M0 fuzz target. Honours $FUZZ_MAX_TOTAL_TIME for CI timeboxing.
fuzz:
	cd fuzz && cargo +nightly fuzz run parser -- -max_total_time=$${FUZZ_MAX_TOTAL_TIME:-60}

dogfood:
	RUST_LOG=grafy=info cargo run -- index . > /dev/null
	cargo run -- diagnose .

diagnose:
	RUST_LOG=grafy=trace cargo run -- diagnose .

ci: fmt-check clippy test dogfood

clean:
	cargo clean
	rm -rf fuzz/target fuzz/corpus fuzz/artifacts
