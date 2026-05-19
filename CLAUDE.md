# Grafy — Claude Code Instructions

Rust, polyglot, LLM-free code-intelligence engine. Drop-in alternative to codebase-memory-mcp with stack-graphs-grade name resolution. See `plan.md` for full design.

## Quick context

- **Language:** Rust (workspace: `grafy`, `grafy-parser`, `grafy-stackgraphs`, `grafy-bench`)
- **Status:** pre-M0. Plan v0.2 in `plan.md`. Companion design doc `grafy-design.md`.
- **License:** dual MIT / Apache-2.0 (locked).
- **No CGO** except tree-sitter. **No LLM** calls, **no embeddings**, no external services.

## Commands

```bash
cargo check                       # fast compile check
cargo test                        # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo bench -p grafy-bench        # criterion benches
cargo fuzz run parser             # M0 fuzz target
RUST_LOG=grafy=info cargo run -- index .   # dogfood
cargo run -- diagnose .           # per-phase timings
```

## Conventions

- **Modules first, crates later.** Split a module into its own crate only when a second external consumer appears. See plan §3.
- **`tracing` from commit 1.** Every phase emits structured spans; no `println!` in pipeline code.
- **Error UX policy:** every user-visible error names the file, the language, and a one-line "what to do next." Exercised in `tests/integration/errors.rs`.
- **No `Node<'a>` across threads.** Public crate API surface must not return tree-sitter `Node<'a>`. Enforce via `clippy.toml`.
- **5-second per-file timeout** in the pipeline as DoS backstop. Don't remove without replacing.
- **Fuzz before fork.** `cargo fuzz` parser target ships in M0; stack-graphs DSL fuzz ships in M2 week 2.
- **Cypher-Lite scope is fixed.** See plan §5. Unsupported features must return the documented error, not silently degrade.
- **Dogfood gate:** `grafy index .` on this repo must produce a valid graph at every milestone.

## What NOT to do

- Don't add LLM, embedding, or vector-search dependencies. Out of scope for v1.0.
- Don't add Cypher write features (`CREATE`, `MERGE`, `SET`, …). Read-only only.
- Don't split crates speculatively. Plan calls out the v0.1 over-modularization mistake.
- Don't bypass the per-file timeout or the structured-error policy to "make tests pass."
- Don't commit benchmark output under `benches/results/local/` — that path is gitignored for a reason.

## Repo layout (target)

```
grafy/
├── Cargo.toml                       # workspace manifest
├── crates/
│   ├── grafy/                       # binary + pipeline/store/MCP/cypher/watch/fqn/lang modules
│   ├── grafy-parser/                # tree-sitter wrapper + parser pool
│   ├── grafy-stackgraphs/           # ported/forked from github/stack-graphs
│   └── grafy-bench/                 # criterion + hyperfine drivers
├── benches/{corpus.toml, results/}
├── fuzz/                            # cargo-fuzz targets
└── tests/{integration,scip-truth,parity}/
```

## Plan & milestones

`plan.md` is the source of truth. Milestones M0 (spike + foundations, 2w), M1 (parity MVP, 6w), M2 (stack-graphs differentiator, 6w), M3 (LSP + release, 4w). Every milestone has **three** gates: engineering, quality, demo. Don't skip the demo gate — plan §4 calls this out explicitly.

## When proposing changes

- Reference the plan section your change belongs to (e.g. "extends §6 benchmark plan").
- New dependencies need a one-line justification in the PR body.
- For anything resembling architecture drift, update `plan.md` first.
