# Grafy

Rust, polyglot, LLM-free code-intelligence engine. Drop-in alternative to [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) with stack-graphs-grade name resolution.

**Status:** pre-M0. Plan v0.2 lives in [`plan.md`](./plan.md).

## Pitch

*Grafy is what codebase-memory-mcp would be if it were written in Rust and shipped with stack-graphs-grade name resolution — same MCP surface, ~2× faster indexing, half the memory, verifiably more precise call graphs.*

## Quickstart (target — not yet shipping)

```bash
cargo install grafy
grafy install                    # auto-configures Claude Code / Codex / Cursor / Zed MCP
grafy index .
```

## Build from source

```bash
cargo check --workspace
cargo test  --workspace
cargo clippy --all-targets -- -D warnings
RUST_LOG=grafy=info cargo run -- index .
```

## Repo

- `plan.md` — implementation plan of record.
- `grafy-design.md` — companion design doc (TBD).
- `AGENTS.md` — agent-facing navigation guide (dogfooded).
- `CLAUDE.md` — Claude-Code-specific conventions.

## License

Dual MIT / Apache-2.0 — see `LICENSE-MIT` and `LICENSE-APACHE`.
