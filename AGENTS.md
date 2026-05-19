# AGENTS.md — Grafy

This file is the dogfood gate: any code-intelligence agent (Claude Code, Codex, Cursor, an LSP client, Grafy itself once it ships) should be able to navigate this repo from this document plus `plan.md`. If an agent can't, the doc is wrong — fix it.

## TL;DR

- Rust workspace, 4 crates: `grafy` (binary + most logic), `grafy-parser`, `grafy-stackgraphs`, `grafy-bench`.
- Plan of record: `plan.md`. Companion: `grafy-design.md`.
- License: MIT OR Apache-2.0.
- See `CLAUDE.md` for Claude-Code-specific conventions.

## Build & test

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Run

```bash
RUST_LOG=grafy=info cargo run -- index <path>
cargo run -- diagnose <path>
cargo run -- mcp                  # MCP server over stdio (M1+)
```

## Bench

```bash
cargo bench -p grafy-bench
make bench                        # full hyperfine + criterion + SCIP F1
```

## Where things live (target layout)

| Concern | Location |
|---|---|
| Pipeline (passes 1–4) | `crates/grafy/src/pipeline/` |
| Storage (redb) | `crates/grafy/src/store/` |
| MCP server (rmcp) | `crates/grafy/src/mcp/` |
| Cypher-Lite | `crates/grafy/src/cypher/` (spec: `cypher/README.md`) |
| File watch + incremental | `crates/grafy/src/watch/` |
| FQN rules per language | `crates/grafy/src/fqn/` |
| Language specs (`.scm` + rules) | `crates/grafy/src/lang/<lang>/` |
| Parser pool | `crates/grafy-parser/` |
| Stack-graphs fork | `crates/grafy-stackgraphs/` |
| Benchmarks | `crates/grafy-bench/` + `benches/` |
| Fuzz targets | `fuzz/` |
| SCIP truth fixtures | `tests/scip-truth/` |
| Schema parity vs codebase-memory-mcp | `tests/parity/` |

## House rules

- Modules first, crates later. Don't split speculatively.
- `tracing` from commit 1; no `println!` in pipeline.
- Every user-visible error: file + language + one-line next-step action.
- No `Node<'a>` across threads. Don't expose it from public API.
- 5-second per-file timeout is a backstop — don't remove without replacement.
- Cypher-Lite scope is fixed in plan §5; unsupported features return the documented error.

## Out of scope for v1.0

LLM calls, embeddings, vector search, Cypher writes, web UI, cross-repo indexing. Don't introduce dependencies for these.

## Plan reference shortcuts

- §3 Repo layout
- §4 Milestones M0–M3 (each has engineering / quality / demo gate)
- §5 Cypher-Lite scope
- §6 Benchmark plan
- §7 Risks
- §10 Day-1 task list
