# Grafy — execution status

Updated: 2026-05-19. Source of truth: `plan.md`.

## Shipped

### Repo bootstrap
- `.gitignore`, `CLAUDE.md`, `AGENTS.md`, `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`.
- `.claude/settings.json` (allow/deny lists, env), `.claude/commands/{plan,check,bench}.md`.
- `.claude/agents/` — orchestrator + 9 specialists (see "Agent team" below).

### M0 day-1 (plan §10) — **done 2026-05-19, tag `m0-day1`**
- 4-crate workspace: `grafy`, `grafy-parser`, `grafy-stackgraphs`, `grafy-bench`.
- Modules inside `crates/grafy/src/`: `pipeline`, `store`, `mcp`, `cypher` (+ `scope.rs` with full §5 error set), `watch`, `fqn` (rust+python), `lang` (rust+python `.scm`).
- `grafy-parser`: thread_local! pool, 5 s per-file timeout, structured `ParseError` with file + next-step action.
- `grafy index <path>` emits Graphviz `.dot`. `grafy diagnose <path>` prints per-phase tracing timings. `RUST_LOG=grafy=info` works.
- `cargo bench -p grafy-bench` parser pool: single vs rayon.
- `cargo fuzz` parser target.
- Dogfood gate green: `grafy index .` parses 20 .rs files in ~15 ms, 0 failed.
- `make ci` passes: fmt-check, clippy `-D warnings`, test, dogfood.

## Remaining for M0 acceptance (plan §4)

| Gate | What | Owner | Blocker |
|---|---|---|---|
| Engineering | Parser pool ≥5× on 8 cores vs 1 core (50k-LOC Rust repo) | `bench-engineer` | needs frozen-SHA corpus checkout (`benches/corpus.toml` fills in ripgrep SHA) |
| Engineering | Fuzz parser ≥60 min, no panic | `fuzz-safety-engineer` | needs nightly toolchain + uninterrupted run |
| Quality | Broken-UTF-8 file produces readable error, not panic | `fuzz-safety-engineer` | quick — add fixture under `crates/grafy/tests/` |
| Demo | 30-sec asciinema of `grafy index .` on a real repo | `release-installer` | needs asciinema recording session |

## Next sprint (M1 week 1 — plan §4)

Owner: `parser-pool-engineer` + `pipeline-architect`.

1. Add the remaining 10 language grammars + `.scm` queries + FQN rules: JS, TS, TSX, Go, Java, C++, C#, PHP, Lua, Scala.
2. Pipeline phase channels (crossbeam) skeleton; pass-1 (structure) writing through to a redb single-writer thread.
3. Heuristic call resolver spec written; implementation drops in M1 week 3.
4. CI runs `make ci` on Linux + macOS.

## Agent team

| Agent | Plan scope |
|---|---|
| `orchestrator` | Plan alignment, gate enforcement, routing |
| `parser-pool-engineer` | `grafy-parser`, `.scm`, FQN rules |
| `pipeline-architect` | 4-pass pipeline, redb, incremental |
| `stackgraphs-engineer` | `grafy-stackgraphs`, SCIP F1 (M2) |
| `mcp-server-engineer` | rmcp, 11 tools, schema parity (M1) |
| `cypher-lite-engineer` | Cypher-Lite per §5 (M1 week 5) |
| `bench-engineer` | criterion + hyperfine, F1, dashboards |
| `fuzz-safety-engineer` | fuzz/, timeouts, sandbox defaults |
| `lsp-engineer` | `grafy-lsp`, editor integrations (M3) |
| `release-installer` | Binaries, `grafy install`, docs site (M3) |
| `rust-reviewer` | Pre-merge idiomatic-Rust gate |

## Open items requiring user input

- Crates.io name reservation for `grafy`, `grafy-parser`, `grafy-stackgraphs`, `grafy-bench` (plan §10 prerequisite).
- Frozen-SHA fill-in for `benches/corpus.toml` (currently `TBD`).
- GitHub repo URL — `repository` + `homepage` in `Cargo.toml` point at `grafy/grafy` and `grafy.dev` placeholders.
- Author identity — `LICENSE-MIT`/`LICENSE-APACHE` and `Cargo.toml` use "The Grafy Authors"; change if a legal entity is preferred.
