# Grafy

**Status: archived 2026-05-25.** Working v0.2 codebase, no further development planned. Source is preserved for reference and is permissively licensed; fork freely.

Rust, polyglot, LLM-free code-intelligence engine. Drop-in alternative to [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp). Same 14 MCP tools, ~3× faster heuristic cold index, optional binding-precise references via Sourcegraph SCIP indexer ingest.

## What's in the box

- **Indexer** (`grafy index <path>`) — four-pass pipeline (structure / definitions / heuristic calls / HTTP routes) backed by a single redb store. 12 languages: Rust, Python, JavaScript, TypeScript, TSX, Go, Java, C++, C#, PHP, Lua, Scala.
- **SCIP ingest sidecar** — auto-detects `scip-python`, `scip-typescript`, `scip-go`, `scip-java`, `scip-clang`, `rust-analyzer scip` on PATH and merges their references as `EdgeKind::Scip` edges. Lift on django: heuristic F1 0.425 → +SCIP 0.613 (+44 %). Heuristic-only mode stays available via `GRAFY_SCIP_DISABLE=1`.
- **Cypher-Lite query engine** — read-only subset of Cypher over the graph (`MATCH`, `WHERE`, `RETURN`, `ORDER BY`, `LIMIT`, `SKIP`, `DISTINCT`). Spec in `docs/cypher-lite.md`.
- **MCP server** (`grafy mcp`) — rmcp 1.7.0 stdio, 14 tools matching codebase-memory-mcp's surface. Schema parity tested in CI.
- **Installer** (`grafy install --with-scip`) — npm / go install / coursier / rustup driver for the SCIP indexers (macOS + Linux).
- **Diagnostics** (`grafy diagnose <path>`) — per-phase timings, per-language indexer detection.

## Bench numbers (M2 final, macOS arm64 M1 Pro, vs codebase-memory-mcp 0.6.1)

| Repo | cmm cold | Grafy heuristic | Ratio |
|---|---|---|---|
| ripgrep (~50k LOC) | 566 ms | 290 ms | 1.95× |
| flask (~30k LOC) | 373 ms | 147 ms | 2.55× |
| grafy self | 290 ms | 142 ms | 2.04× |
| django (~500k LOC) | 15.87 s | 2.77 s | **5.74×** |

Geo mean 2.91× cmm cold. Incremental p95 < 250 ms. Peak RSS 0.4× cmm. Full report: [`benches/m2-final-report.md`](./benches/m2-final-report.md).

## Quickstart (from a clean clone)

```bash
git clone https://github.com/daneuchar/Grafy
cd Grafy
cargo build --release --workspace
./target/release/grafy index .
./target/release/grafy query . 'MATCH (n:Function) RETURN n.fqn LIMIT 5'
```

Indexers (optional, for binding-precise references):

```bash
./target/release/grafy install --with-scip   # npm + go + coursier driver
./target/release/grafy diagnose .            # show what got detected
```

## Build, test, lint

```bash
make ci                             # fmt-check + clippy + test + dogfood
cargo test --workspace              # 132 unit/integration tests
cargo test -p grafy --features testing --test parity_schemas --test parity_sessions   # +21 parity
cargo clippy --workspace --all-targets -- -D warnings
```

159 tests pass on this commit. Clippy `-D warnings` clean. `cargo audit` shows 0 CVEs (1 unmaintained warning in `atomic-polyfill` via `postcard`).

## Repo layout

```
crates/
├── grafy/                  bin + lib (pipeline, store, mcp, cypher, scip, install)
├── grafy-parser/           thread_local! tree-sitter parser pool (12 langs)
├── grafy-stackgraphs/      placeholder; M2 pivoted away from stack-graphs (see plan §4)
└── grafy-bench/            hyperfine driver + scip-f1 differ
benches/                    corpus.toml, run scripts, results JSON, m2-final-report.md
docs/                       cypher-lite.md, m1-mcp.md, m1-incremental.md, m1-parity.md,
                            m1-pgo.md, m2-research.md
tests/                      fixtures, parity schemas + sessions, demo fixture
demos/                      m2-demo.md (asciinema script)
fuzz/                       cargo-fuzz parser target
```

## Why archived

The codebase reached a real v0.2 ship-state: M1 parity MVP + M2 SCIP ingest. M2 W1 (`docs/m2-research.md`) showed that the original plan §2 "stack-graphs binding-precise resolution" moat doesn't exist in shippable form — the published `tree-sitter-stack-graphs-*` packs fail in production (F1 < 0.32 on real Python/TS/JS repos due to DSL evaluation crashes on common syntax). The M2 pivot to SCIP ingest works and ships, but the unique selling point reduces to "first-class SCIP ingest + 3× faster heuristic" — not enough differentiation over codebase-memory-mcp + Sourcegraph's existing tooling to justify continued solo development.

Active dev stops here. Source is intact, tests pass, the install path works, and anyone who wants the pieces can fork. Crates on crates.io stay published at v0.1.0 (the placeholder reservations); no v0.2 release.

## Plan + history

- [`plan.md`](./plan.md) — full v0.2 implementation plan (M0–M3) and §8 decision log with every milestone closeout pinned to its commit SHA.
- [`AGENTS.md`](./AGENTS.md) — agent navigation guide.
- [`CLAUDE.md`](./CLAUDE.md) — Claude Code conventions used during development.
- [`docs/m2-research.md`](./docs/m2-research.md) — pre-M2 research, including the stack-graphs ecosystem reality check.
- [`benches/m2-final-report.md`](./benches/m2-final-report.md) — final bench report (heuristic + SCIP).
- [`demos/m2-demo.md`](./demos/m2-demo.md) — 55-second asciinema script (unrecorded).

## License

Dual MIT / Apache-2.0. See [`LICENSE-MIT`](./LICENSE-MIT) and [`LICENSE-APACHE`](./LICENSE-APACHE).
