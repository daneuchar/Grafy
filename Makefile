.PHONY: check fmt fmt-check clippy test bench bench-m1 fuzz dogfood diagnose mcp parity ci clean pgo

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

# ─── PGO (profile-guided optimisation) ──────────────────────────────────────
# Three-phase build: instrument → profile → optimise.
# Requires: llvm-profdata (homebrew LLVM 20 or Xcode CLT — must match rustc's LLVM).
# Usage: make pgo
#   Output: target/release/grafy (PGO-optimised binary).
#   To re-run without rebuilding: make pgo-collect pgo-link
#
# Detect llvm-profdata: prefer homebrew LLVM 20 (matches rustc 1.88 / LLVM 20).
# Fall back to Xcode CLT (Apple LLVM 21 — minor version mismatch is usually OK
# for profdata merge but not for profile-use; log a warning if used).
PGO_DIR ?= /tmp/grafy-pgo-profiles
PGO_PROFDATA ?= /tmp/grafy-pgo.profdata
LLVM_PROFDATA ?= $(shell command -v /opt/homebrew/opt/llvm/bin/llvm-profdata 2>/dev/null \
                   || command -v /Library/Developer/CommandLineTools/usr/bin/llvm-profdata 2>/dev/null \
                   || echo llvm-profdata)

pgo: pgo-instrument pgo-collect pgo-link

# Phase 1 — build instrumented binary.
pgo-instrument:
	@echo "[PGO 1/3] Building instrumented binary (profile.release-pgo)…"
	rm -rf "$(PGO_DIR)"
	mkdir -p "$(PGO_DIR)"
	RUSTFLAGS="-Cprofile-generate=$(PGO_DIR)" \
	  cargo build --profile release-pgo -p grafy
	@echo "[PGO 1/3] Instrumented binary: target/release-pgo/grafy"

# Phase 2 — run corpus to populate profiles.
pgo-collect: pgo-instrument
	@echo "[PGO 2/3] Running corpus to collect profiles…"
	@test -d /tmp/ripgrep || (echo "WARN: /tmp/ripgrep not found — clone with: git clone https://github.com/BurntSushi/ripgrep /tmp/ripgrep" && exit 1)
	@test -d /tmp/flask   || (echo "WARN: /tmp/flask not found — clone with: git clone https://github.com/pallets/flask /tmp/flask" && exit 1)
	rm -rf /tmp/ripgrep/.grafy
	./target/release-pgo/grafy index /tmp/ripgrep > /dev/null 2>&1 || true
	rm -rf /tmp/flask/.grafy
	./target/release-pgo/grafy index /tmp/flask > /dev/null 2>&1 || true
	rm -rf .grafy
	./target/release-pgo/grafy index . > /dev/null 2>&1 || true
	@echo "[PGO 2/3] Merging raw profiles…"
	"$(LLVM_PROFDATA)" merge -sparse "$(PGO_DIR)"/*.profraw -o "$(PGO_PROFDATA)"
	@echo "[PGO 2/3] Profile data: $(PGO_PROFDATA)"

# Phase 3 — build optimised binary using merged profile.
pgo-link:
	@echo "[PGO 3/3] Building PGO-optimised binary (profile.release)…"
	RUSTFLAGS="-Cprofile-use=$(PGO_PROFDATA) -Cllvm-args=-pgo-warn-missing-function" \
	  cargo build --release -p grafy
	@echo "[PGO 3/3] PGO binary ready: target/release/grafy"
	@ls -lh target/release/grafy
