.PHONY: check fmt fmt-check clippy test bench bench-m1 fuzz dogfood diagnose mcp parity ci clean

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

# M1 W6 head-to-head benchmark: Grafy vs codebase-memory-mcp.
# Requires: hyperfine, git, npm (codebase-memory-mcp).
# Cold-index runs delete .grafy / CMM .db before each hyperfine iteration.
# macOS note: sudo purge is NOT called (requires root). Numbers reflect
# OS-page-cache-warm cold starts. See benches/m1-report.md §Setup.
bench-m1:
	bash benches/run_m1.sh

# Plan §M0 fuzz target. Honours $FUZZ_MAX_TOTAL_TIME for CI timeboxing.
fuzz:
	cd fuzz && cargo +nightly fuzz run parser -- -max_total_time=$${FUZZ_MAX_TOTAL_TIME:-60}

dogfood:
	RUST_LOG=grafy=info cargo run -- index . > /dev/null
	cargo run -- diagnose .

diagnose:
	RUST_LOG=grafy=trace cargo run -- diagnose .

# M1 W5: validate 14-tool MCP surface (+ trace_call_path alias = 15 entries).
# Runs grafy mcp --check: exits 0 if all tool registrations are present.
mcp:
	cargo run -- mcp --check

# M1 quality gate: schema-compat + recorded-session parity (plan §4).
# Runs 16 schema-compat tests + 5 recorded-session tests.
# Schema drift is a blocker — see tests/parity/diffs.md and tests/parity/drift-log.md.
parity:
	cargo test -p grafy --features testing --test parity_schemas
	cargo test -p grafy --features testing --test parity_sessions

ci: fmt-check clippy test dogfood mcp parity

clean:
	cargo clean
	rm -rf fuzz/target fuzz/corpus fuzz/artifacts
