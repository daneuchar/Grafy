---
description: Run the Grafy pre-commit checklist — fmt check, clippy -D warnings, test, dogfood index.
allowed-tools: Bash(cargo fmt:*), Bash(cargo clippy:*), Bash(cargo test:*), Bash(cargo run:*)
---

Run these in order, stop on first failure, report each result:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo run -- index .` (dogfood — must produce a valid graph; M0+ only)

Report a one-line pass/fail per step. On failure, quote the first error block verbatim.
