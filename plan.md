# Grafy — Implementation Plan (v0.2)

*A phased build plan for a Rust, polyglot, LLM-free code-intelligence engine — drop-in alternative to codebase-memory-mcp with stack-graphs-grade name resolution as the moat.*

Owner: Danny
Status: Plan v0.2 (revised after critique pass)
Companion doc: `grafy-design.md`
Date: 2026-05-19

**Changes from v0.1:** consolidated 13 crates → 4; added demo screencast as a gate at every milestone; promoted dogfooding, fuzzing, telemetry, and error UX from "polish" to M0; defined Cypher-Lite scope explicitly; added subprocess-first validation of stack-graphs before any fork commitment; promoted LSP from "deferred" to M3 alongside release polish; tightened the "drop-in" acceptance gate into a testable schema-compatibility + recorded-session pair; locked the license decision.

---

## 1. Goals and non-goals

**Goals (v1.0):**
- Single static Rust binary, no CGO except tree-sitter, no external services.
- Polyglot: Rust, Python, JavaScript, TypeScript, TSX, Go, Java, C++, C#, PHP, Lua, Scala (parity with codebase-memory-mcp).
- 11 MCP tools matching codebase-memory-mcp's surface and JSON schema so it is genuinely drop-in.
- Stack-graphs-based cross-file name resolution as the differentiator; verifiable against per-language SCIP indexers.
- Sub-200 ms incremental reindex on a single-file edit (p95) for repos up to ~1 M LOC.
- Cold full-index ≥ 2× faster than codebase-memory-mcp on the benchmark corpus.
- On-disk index ≤ 1.5× source byte size.
- Every public error names the file, the language, and a one-line "what to do next."

**Non-goals (v1.0):**
- LLM calls, embeddings, vector search, summaries.
- Cypher write queries (read-only execution only).
- Web UI / dashboard.
- Cross-repo / multi-repo indexing into one graph.

**v1.x stretch (not v1.0 but designed-for):**
- `grafy-lsp` binary (Zed / VSCode / Neovim) — broadens audience beyond MCP, reuses 90% of v1.0 code.
- Workspace-aware indexing for Cargo / uv / pnpm / go.work.
- Cross-language FFI edges (pyo3, napi, gRPC, OpenAPI).
- SCIP emit for Sourcegraph interop.

**License:** dual MIT / Apache-2.0 (Rust ecosystem default). Locked. Stack-graphs fork stays at its inherited MIT/Apache dual.

---

## 2. Strategic positioning

**Pitch (one sentence):** *Grafy is what codebase-memory-mcp would be if it were written in Rust and shipped with stack-graphs-grade name resolution — same MCP surface, ~2× faster indexing, half the memory, verifiably more precise call graphs.*

**Why this wins (engineering):**
1. codebase-memory-mcp's pass-3 call resolution uses import-aware + type-inferred heuristics. Stack-graphs gives binding-precise resolution for Python, TS, Java. On a head-to-head SCIP F1 benchmark, Grafy can show measurably higher precision/recall on cross-file references — a hard, citable claim.
2. Rust's parser pool pattern (`rayon` + `thread_local!`) scales near-linearly past 16 cores. Go's runtime contention shows up earlier. On large monorepos this is visible.
3. github/stack-graphs was archived September 2025. The formalism is excellent and unowned. A maintained Rust crate is itself a contribution.

**Why this wins (positioning):** *adoption is won by demos and integration polish, not by F1 numbers.* The plan reflects this — every milestone has a demo gate alongside the engineering one. The benchmark dashboard is evidence; the screencast is the product page.

**Honest weaknesses:**
- codebase-memory-mcp has 2.4k stars and shipped Claude Code integration polish first.
- Most users never read benchmarks. The F1 number is a moat for credibility, not for click-through.
- Stack-graphs is hard. Python/TS language packs have known recall gaps. Go and Rust have no mature packs.

---

## 3. Repository layout

**Start small. Split when a second consumer appears, not before.**

```
grafy/
├── Cargo.toml                       # workspace manifest
├── README.md
├── AGENTS.md                        # this repo's AGENTS.md (dogfood)
├── crates/
│   ├── grafy/                       # binary + most logic (pipeline, store, MCP, watch, FQN, lang specs)
│   ├── grafy-parser/                # tree-sitter wrapper + parser pool — reusable
│   ├── grafy-stackgraphs/           # ported/forked from github/stack-graphs — reusable
│   └── grafy-bench/                 # benchmark harness (criterion + hyperfine drivers)
├── benches/
│   ├── corpus.toml                  # frozen-SHA repo list
│   └── results/                     # JSON + Vega-Lite dashboards
├── fuzz/                            # cargo-fuzz targets (parser + stackgraphs ingest)
└── tests/
    ├── integration/
    ├── scip-truth/                  # SCIP ground-truth fixtures
    └── parity/                      # codebase-memory-mcp JSON-schema parity tests
```

Four crates, not thirteen. Inside `grafy/` we still keep `pipeline/`, `store/`, `mcp/`, `cypher/`, `watch/`, `fqn/`, `lang/` as **modules**. They get promoted to crates only when an external consumer wants to depend on one specifically. This avoids the 13-CI-matrix, 13-README, 13-version-bump tax for what is effectively one product.

`grafy-parser` and `grafy-stackgraphs` are split out because they're the two pieces other projects (Aider, an LSP server, a vendor's coding agent) might plausibly embed.

---

## 4. Milestones

Each milestone has **three** acceptance gates: an engineering gate (does it work), a quality gate (does it not break in the field), and a demo gate (can a user *see* it work in 30 seconds).

### M0 — Spike + foundations (2 weeks)

**Goal:** prove parser-pool scaling, set the operational floor everything depends on, dogfood from day one.

**Engineering tasks:**
1. Cargo workspace scaffold (4 crates); CI on Linux/macOS with `cargo check`, `cargo test`, `cargo clippy -D warnings`.
2. `grafy-parser`: `thread_local!` `Parser` pool; benchmark single vs `n`-thread parsing on a 50k-LOC Rust repo.
3. Core types: `NodeId`, `EdgeId`, `Symbol`, `EdgeKind`; `postcard` serialization.
4. Tree-sitter `.scm` queries + FQN rules for Rust and Python.
5. `grafy index <path>` CLI dumping Graphviz `.dot` to stdout.
6. **`tracing` crate wired in from commit 1**, with `RUST_LOG=grafy=info` showing per-phase timings. `grafy diagnose <repo>` prints a per-phase breakdown.
7. **`cargo fuzz` targets for the parser** and a 5-second-per-file timeout in the pipeline as a DoS backstop.
8. **Error UX policy**: every user-visible error names the file, language, and a one-line action. `tests/integration/errors.rs` exercises the failure paths.
9. **Dogfood**: `grafy index .` on Grafy's own source produces a valid graph and a sensible `.dot`.

**Engineering gate:** parses 50k LOC ≥ 5× faster on 8 cores than 1 (parallel scaling validated); fuzz target runs for 60 min without panic.

**Quality gate:** `grafy diagnose .` prints clean per-phase timings; a deliberately broken UTF-8 file produces a readable error, not a panic.

**Demo gate:** 30-second asciinema showing `grafy index .` on a real repo + a `.dot` rendered as a small architecture poster of Grafy's own code.

**Exit decision:** if parallel scaling is sub-2× on 8 cores, the design needs rethinking before M1. (We don't expect this.)

---

### M1 — Parity MVP (6 weeks)

**Goal:** drop-in alternative to codebase-memory-mcp with the same 11 MCP tools and same 12 languages. Beat its indexing throughput ≥ 2× on the benchmark corpus.

**Drop-in is testable** (not "equivalent answers"):
- **Schema parity test:** Grafy's MCP tool response JSON is structurally compatible with codebase-memory-mcp's. We extract codebase-memory-mcp's tool output schemas from its README + a recorded session, encode them in `tests/parity/schemas/`, and run schema-compat tests in CI.
- **Recorded-session parity:** N representative Claude Code prompts (e.g. "who calls X", "find dead code", "list routes") are run against both servers in CI; the natural-language *outcomes* are eyeballed by the maintainer before each release.

**Week-by-week:**

| Week | Deliverable | Engineering gate |
|---|---|---|
| 1 | All 12 language specs (`.scm` + FQN rules); pipeline skeleton with phase channels; **heuristic call resolver fully specified** (the M2 stack-graphs fallback) | All 12 grammars parse their language's stdlib without crash |
| 2 | Pass 1 (structure) + pass 2 (definitions) writing to redb | `grafy index repowise` populates File/Module/Function/Class nodes correctly |
| 3 | Pass 3 (calls) — heuristic import-aware resolution (M2 will replace this for Py/TS/Java) | `grafy index django` CALLS count within ±10% of codebase-memory-mcp |
| 4 | Pass 4 (HTTP route ↔ call-site linking) for FastAPI / Gin / Express | Routes populated on known-good multi-service fixture |
| 5 | MCP server with all 11 tools via rmcp; **Cypher-Lite** (see §5) | Claude Code runs `index_repository` and `trace_call_path` against Grafy |
| 6 | Incremental reindex (blake3 + mtime + tree-sitter `Tree::edit`); benchmark vs codebase-memory-mcp | Cold index ≥ 2× faster; incremental p95 < 250 ms |

**Quality gate:** schema-parity tests pass; recorded-session parity reviewed; `grafy diagnose` shows clean phase timings on every benchmark repo; fuzz harness extended to the pipeline.

**Demo gate:** 60-second screencast: install Grafy → drop into `.mcp.json` → ask Claude Code three structural questions about a real repo → show the answers. Same questions side-by-side against codebase-memory-mcp with timing.

---

### M2 — Stack-graphs differentiator (6 weeks)

**Goal:** ship the moat. Replace M1's heuristic pass-3 with stack-graphs binding resolution for Python, TS, Java. Publish SCIP F1 numbers.

**Critical week-1 step: validate by subprocess before forking.** The github/stack-graphs CLI still builds. M2 week 1 shells out to it from `grafy-pipeline` and measures F1 on the benchmark corpus *as-is*. If existing language packs hit ≥ 0.85, the fork is unnecessary for v1 — keep stack-graphs as a vendored dependency, ship M2 in ~2 weeks instead of 6. If they don't, we know exactly which gaps need fixing before touching the engine. Cheaper validation, same end state.

**Week-by-week (assuming subprocess-only doesn't hit F1; fork required):**

| Week | Deliverable | Engineering gate |
|---|---|---|
| 1 | Subprocess integration; measure F1 on Py/TS/Java as-is | F1 baseline numbers published |
| 2 | Fork github/stack-graphs into `crates/grafy-stackgraphs`; vendor language packs; clean build | `cargo build` clean; existing tests pass; **fuzz target for stack-graphs DSL ingest** |
| 3 | Wire stack-graphs as pass-3 resolver for Python | Python CALLS F1 ≥ 0.85 vs scip-python on django |
| 4 | Same for TypeScript | TS CALLS F1 ≥ 0.85 vs scip-typescript on the TypeScript compiler |
| 5 | Same for Java | Java CALLS F1 ≥ 0.85 vs scip-java on a Maven project |
| 6 | Incremental stack-graph re-resolution (file-isolated subgraphs); publish benchmark report | Single-file edit reindex p95 < 200 ms on a 100k-LOC repo; results JSON + Vega-Lite live on GitHub Pages |

**Rust + Go fallback:** stay on M1's heuristic resolver. Documented as a gap. Stretch goal in v1.x.

**Quality gate:** stack-graphs DSL fuzz target survives 4 hours without panic; resolver respects the 5s-per-file timeout from M0.

**Demo gate:** the headline screencast — a real cross-file call that codebase-memory-mcp's heuristic resolver misses and Grafy's stack-graphs resolver gets right. Side-by-side, 45 seconds.

---

### M3 — LSP + release polish (4 weeks)

**Goal:** broaden the audience beyond Claude Code via LSP, and ship v1.0.

**The LSP move is strategic.** 90% of the engine that powers MCP also powers a Language Server. Shipping `grafy-lsp` opens Zed / VSCode / Neovim / Helix — multiple times the user base of "Claude Code users who configure MCP servers." Same engine, broader market, harder for codebase-memory-mcp to displace.

| Week | Deliverable |
|---|---|
| 1 | `grafy watch` (notify + debounce + incremental reindex); `grafy lsp` binary serving `textDocument/definition`, `references`, `documentSymbol`, `workspaceSymbol` against the existing store |
| 2 | LSP smoke-test integrations: Zed, VSCode (a thin extension that just wraps the binary), Neovim (built-in LSP client config) |
| 3 | Cross-platform release binaries via GitHub Actions: Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64; Windows skipped for v1.0 |
| 4 | Install UX: `cargo install grafy`, Homebrew formula, `curl \| sh` installer; `grafy install` auto-configures `.mcp.json` for Claude Code / Codex / Cursor / Zed / Continue; docs site at `grafy.dev` (or GitHub Pages); comparison page vs codebase-memory-mcp |

**Engineering gate:** Zed jumps to definition via Grafy LSP on a polyglot test repo; release binaries are reproducible from a tagged commit.

**Quality gate:** install-to-first-query under 5 minutes on a clean Linux box (recorded).

**Demo gate:** 90-second video — install Grafy, point Zed at a repo, watch jump-to-def + find-references work polyglot, then switch to Claude Code and ask a structural question, same engine. The "one tool, many surfaces" pitch.

---

## 5. Cypher-Lite — explicit scope

Cypher implementations are black holes if you say "support what they support." Explicit scope:

**Supported in v1.0:**
- `MATCH (a:Label)-[r:REL]->(b:Label)` — one or more chained relationships, up to 3 hops fixed length.
- `WHERE` — `=`, `!=`, `<`, `>`, `<=`, `>=`, `CONTAINS`, `STARTS WITH`, `ENDS WITH`, `=~` (regex), `IS NULL`, `IS NOT NULL`, `AND`, `OR`, `NOT`.
- `RETURN` — node properties, relationship properties, computed identity comparisons; no expressions/functions in v1.0.
- `ORDER BY`, `LIMIT`, `SKIP`, `DISTINCT`.
- Read-only execution only.

**Explicitly out of scope in v1.0:**
- `WITH`, `UNWIND`, `MERGE`, `CREATE`, `DELETE`, `SET`, `REMOVE`.
- Variable-length paths (`*`, `*1..3`) — covered instead by the structured `trace_call_path` tool, which is faster and bounded.
- `OPTIONAL MATCH`.
- Aggregations (`count`, `sum`, `collect`, etc.).
- Functions (`toLower`, `coalesce`, etc.).
- Multiple `MATCH` clauses in one query.
- Path variables (`p = (a)-[*]->(b)`).

**Unsupported features return a clear error:**

```
ERROR: Cypher-Lite does not support `WITH` clauses (v1.0).
       For this query, use the structured tool `search_graph` with
       `relationship` and `direction` filters. See docs/cypher-lite.md.
```

The full spec lives in `crates/grafy/src/cypher/README.md` and is enforced by a CI test that runs every example query in the spec.

---

## 6. Benchmark plan (gating M1 and M2)

Frozen corpus pinned by commit SHA in `benches/corpus.toml`:

| Repo | Language | ~LOC | Role |
|---|---|---|---|
| `BurntSushi/ripgrep` | Rust | 50 K | Rust dogfood |
| `pallets/flask` | Python | 30 K | Small Python |
| `django/django` | Python | 500 K | Python at scale |
| `microsoft/TypeScript` | TS | 700 K | Strict TS upper bound |
| `kubernetes/kubernetes` | Go | 5 M | Go at scale |
| `apache/dubbo` | Java | 600 K | Java |
| `repowise-dev/repowise` | Polyglot | 10 K | Real polyglot monorepo |
| `grafy` | Rust | — | Dogfood gate |

**Metrics (criterion + hyperfine, 10 runs, `mean ± stdev`, min, max):**
- Cold index time (FS cache dropped)
- Warm index time (pre-walked)
- Incremental update p50 / p95
- Peak RSS (sampled at 100 Hz)
- On-disk index size / source byte total
- Parallel scaling at 1 / 2 / 4 / 8 / 16 cores
- Query latency p50 / p95 for `find-def`, `find-refs`, `trace_call_path(hops=2)`, `dead_code`, `routes`

**Quality metrics:**
- SCIP F1 against per-language SCIP indexers on Python / TS / Java
- RepoBench-R Acc@1 / @3 / @5 (treat Grafy as the retriever)

**Baselines side-by-side:**
- ctags + ripgrep (floor)
- codebase-memory-mcp (the direct competitor)
- per-language SCIP indexers (ceiling for F1)
- Aider repo-map (ranking comparison)

**Reporting:** JSON results per release commit; two Vega-Lite charts on GitHub Pages — grouped-bar throughput per repo, and the headline Pareto frontier `quality vs cost`.

---

## 7. Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Stack-graphs port harder than expected | Medium | High | Subprocess-validate first (M2 week 1); pivot to "stack-graphs-inspired" custom resolver if engine work blocks |
| Tree-sitter Send/Sync sharp edges cause data races | Low | High | `clippy.toml` with strict lints; no `Node<'a>` across threads enforced via crate API surface (no public functions return `Node<'a>`) |
| codebase-memory-mcp ships a Rust port first | Low | Medium | Differentiate on stack-graphs precision + LSP audience, not just speed |
| redb single-writer becomes a bottleneck | Low | Medium | Dedicated writer thread fed by crossbeam channel; fallback to rocksdb if measured a problem |
| Tree-sitter grammar bugs / weird ASTs cause panics | Medium | Medium | `cargo fuzz` from M0 day one; 5-second-per-file timeout as backstop; CI runs the indexer over the full corpus on every PR |
| Malicious repo crashes/hangs the resolver (DoS in editor) | Medium | High | Fuzz the stack-graphs `.tsg` DSL ingest; timeouts; sandbox the file walker with explicit ignore for `.git/`, `node_modules/`, etc. by default |
| LLM-coding-agent market consolidates around one tool | Medium | High | LSP path (M3) hedges against MCP-only collapse; `grafy-parser` + `grafy-stackgraphs` are reusable as library crates |
| Over-modularization (the v0.1 mistake) | Mitigated | — | Workspace now 4 crates; split-when-needed policy stated |
| Telemetry retrofit when debugging "my repo is slow" | Mitigated | — | `tracing` wired from commit 1; `grafy diagnose` from M0 |

---

## 8. Open questions resolved in this revision

| Question | Decision |
|---|---|
| License | **Dual MIT / Apache-2.0**. Locked. |
| Workspace shape | **4 crates** (`grafy`, `grafy-parser`, `grafy-stackgraphs`, `grafy-bench`). Split further only when a second external consumer exists. |
| Cypher scope | **Cypher-Lite** as specified in §5. |
| Stack-graphs fork commit | **Subprocess-validate first**; fork only if F1 < 0.85 on as-is language packs. |
| LSP shape | **v1.0 (M3)**, not v1.x deferred. |
| Telemetry | **`tracing` from commit 1.** `grafy diagnose` ships in M0. |
| Drop-in acceptance | **JSON schema-compat tests + recorded-session parity**, not "equivalent answers." |
| Fuzz scope | **Parser fuzz target in M0**; stack-graphs DSL ingest fuzz target in M2 week 2. |
| Dogfood gate | **`grafy index .` on Grafy itself is an M0 acceptance.** |
| Org/crate name on crates.io | Check availability of `grafy`, `grafy-parser`, `grafy-stackgraphs`, `grafy-bench` **before M0 day 1**. |
| Clippy lint strictness | **`-D warnings` on default lints, not pedantic.** Pedantic too noisy for v1.0 scaffold; `rust-reviewer` agent enforces stricter idioms via review. |
| `rmcp` version pin | Deferred to M1 week 5. Workspace dep stays commented in `Cargo.toml` until then. |
| M0 day-1 status | **Done 2026-05-19.** Workspace scaffold (4 crates), tracing wired, parser pool + bench, fuzz target, CLI (`index`/`diagnose`), dogfood gate green: `grafy index .` parses 20 `.rs` files in ~15 ms; `cargo clippy -D warnings` clean. |
| Tree-sitter version line | **0.23**. Aligns all 12 grammar packs (M1 W1). Bump to 0.24+ only if a grammar in the v1.0 set drops 0.23 support before M1 close. `tree-sitter-c-sharp` pinned to `=0.23.1` (newer versions require tree-sitter ABI > 14). |
| Crates.io reservation | **Shipped 2026-05-19**, all four crates at 0.1.0: `grafy-parser`, `grafy-stackgraphs`, `grafy-bench`, `grafy`. Yank-and-bump to 0.2.0 at M1 close. |
| GitHub repo | **[daneuchar/Grafy](https://github.com/daneuchar/Grafy)**, public. Tag `m0-day1` pushed. |
| M1 W1 status | **Done 2026-05-20.** 12 grammars compile + smoke-parse a per-language fixture (13 tests). Phase-channel skeleton (`crates/grafy/src/pipeline/channels.rs`). Heuristic call-resolver spec at `docs/m1-call-resolver.md`. `make ci` clean. |
| M1 W2 status | **Done 2026-05-20.** Pass 1 (structure) + Pass 2 (definitions) shipped. redb schema in `crates/grafy/src/store/mod.rs` (FILES/NODES/EDGES tables), deterministic blake3-derived `NodeId`, single-writer thread batching 256 events / 50 ms. Dogfood on Grafy repo: `files=44 modules=43 functions=81 structs=16 enums=8` (192 nodes total). 29 tests pass, clippy clean. |
| Methods=0 on Rust dogfood (W2) | **Known gap.** Rust `definitions.scm` captures `impl_item` as Struct, not surfacing methods as `Method` nodes. Add `(impl_item body: (declaration_list (function_item name: (identifier) @method.name) @method.def))` in W3, before the call resolver lands — it depends on method nodes. |
| `count_nodes_from_store` reopens redb (W2) | Pipeline opens redb for writing, joins writer, then reopens read-only for tallies. Two `Database::open` calls on the same file. W6 incremental should keep a single long-lived handle. |
| Legacy `WriteEvent` variants | W2 added `File`/`Node`/`Edge`; legacy `Structure`/`Definition`/`Call`/`Route` retained for back-compat with the channels skeleton tests. Remove during W5 MCP cleanup. |
| M1 W3 status | **Done 2026-05-21.** Pass 3 heuristic call resolver shipped (`crates/grafy/src/pipeline/pass3.rs`). 12 `calls.scm` + 12 `imports.scm`; `EdgeKind::{Calls,Routes}` in store. Two-phase resolver: in-memory SymbolTable from full `DefinitionEvent` stream, then per-file edge emission. Dogfood on Grafy repo: `methods=43 calls=1192` in ~350 ms. 24 tests pass (5 new in `w3_calls.rs`), clippy clean. |
| Method capture double-count | Python/Rust flat `function_definition`/`function_item` queries fire for impl/class bodies *and* the new `method.def` patterns fire on the same nodes — each method gets a Function node + a Method node (different `node_id`). Harmless for W3; clean fix needs tree-sitter `#not-has-ancestor?` (unavailable on tree-sitter 0.23) or restructured queries. Track for M2. |
| W3 caller attribution overshoot | Pass 3 attributes every call site in a file to *every* definition in that file (no enclosing-function walk). Inflates edge counts on multi-function files. Fix is the enclosing-node walk — defer to W6 alongside cached parse trees. May skew the django ±10% gate; verify before declaring W6 done. |
| W3 calls.scm runtime fallback | If a per-language `calls.scm`/`imports.scm` fails to compile against the live tree-sitter grammar, pass 3 logs `debug!` and emits zero edges for that file. Add a per-language `Query::new` smoke test before the django gate run. |
| W3 reparse cost | Pass 3 reparses every file. ~2× pass 1 cost. W6 incremental caching eliminates it. |
| W3 symbol-table memory | `HashMap`-based in-memory table; large monorepos (CPython ~4k files) could be hundreds of MB. Stream-rather-than-buffer is W6 work. |
| M1 W4 status | **Done 2026-05-21.** Pass 4 routes shipped (`crates/grafy/src/pipeline/pass4.rs`). Three frameworks: FastAPI / Gin / Express. `crates/grafy/src/routes/<framework>/routes.scm` + lexical `detect_framework`. `NodeKind::Route` + `EdgeKind::Routes`. Pass 3 + pass 4 run in parallel scoped threads sharing `&SymbolTable`. Dogfood on `tests/fixtures/routes/`: `routes=6` (2 per framework, all handlers resolved). 47 tests pass, clippy clean. Commit `1bd5739`. |
| Go raw-string route patterns | Gin queries match `interpreted_string_literal` only. Backtick-quoted paths (rare) not captured. v1.0 acceptable. |
| Express middleware chaining | `app.route('/path').get(h)` not captured by `routes.scm`. v1.0 out of scope. |
| Inline arrow handlers (Express) | Synthetic `route.handler` name based on `<file>:<line>` so route node still exists; no edge to a real definition node. Per spec. |
| M1 W5 status | **Done 2026-05-21.** Cypher-Lite + MCP server shipped. `crates/grafy/src/cypher/{lexer,parser,plan,executor,ast,error,scope,mod}.rs` (~2.9 kLOC), hand-rolled recursive-descent parser, read-only executor with `MAX_ROWS=100_000` cap. rmcp 1.7.0 stdio server in `crates/grafy/src/mcp/{handler,server,mod}.rs`. 14 canonical tools + 1 alias (`trace_call_path` → `trace_path`). 121 tests pass (was 47). clippy clean. `grafy mcp --check`: 15/15 OK. Commits `d11dc12` + `0f28cc8`. |
| MCP tool surface | 14 canonical tools matching codebase-memory-mcp's `mcp.c` TOOLS[] array: `index_repository`, `search_graph`, `query_graph`, `trace_path`, `get_code_snippet`, `get_graph_schema`, `get_architecture`, `search_code`, `list_projects`, `delete_project`, `index_status`, `detect_changes`, `manage_adr`, `ingest_traces`. Plus `trace_call_path` as a `trace_path` alias for plan §1's "11 tool" wording — keeps both names live. Schemas under `tests/parity/schemas/`; differences vs upstream tracked in `tests/parity/diffs.md`. |
| Cypher unknown-label semantics | Planner currently errors on unknown labels rather than returning zero rows (Neo4j semantics). Either is defensible; tests cover both paths. Pick one before M1 close. |
| Cypher reverse-edge scan | `<-[:CALLS]-` does a full EDGES_TABLE scan (no reverse index). OK at current store sizes; adds an index when monorepo bench evidence requires it. |
| Cypher regex `=~` | Tokenised + parsed into `BinOp::Regex` but executor returns `false` for all comparisons (no `regex` dep at execute time). Wire to the workspace `regex` dep before M1 close OR demote to `Unsupported::Function` and document. |
| MCP stub tools | Tools that need external state (`delete_project`, `manage_adr`, `ingest_traces`) currently return well-formed empty responses. Wire to real state in M3 alongside `grafy install`. Tracked in `tests/parity/diffs.md`. |

---

## 9. Definition of done for v1.0

- All M0–M3 engineering, quality, and demo gates met.
- `cargo install grafy` works from a clean Linux/macOS box.
- `grafy install` configures Claude Code / Codex / Cursor / Continue MCP entries.
- README quickstart runs end-to-end in < 5 minutes on `BurntSushi/ripgrep` (recorded asciinema).
- `grafy-lsp` works in Zed (and ideally VSCode + Neovim) — recorded demo.
- Benchmark dashboard live, reproducible via `make bench`.
- SCIP F1 ≥ 0.85 on Python / TS / Java published with raw JSON.
- A blog post comparing Grafy to codebase-memory-mcp head-to-head, with the headline screencast.
- At least 3 external installs (informal signal that the install UX works).

---

## 10. Day-1 task list

For the literal first day of M0:

1. Check crates.io availability for `grafy`, `grafy-parser`, `grafy-stackgraphs`, `grafy-bench`. Reserve names.
2. `cargo new --bin grafy && cd grafy && git init`. Convert to 4-crate workspace with `lib.rs` stubs.
3. Add dependencies to the relevant manifests: `tree-sitter`, `tree-sitter-rust`, `tree-sitter-python`, `rayon`, `crossbeam-channel`, `dashmap`, `redb`, `ignore`, `blake3`, `rmcp`, `tracing`, `tracing-subscriber`, `postcard`.
4. Wire `tracing-subscriber` so `RUST_LOG=grafy=info cargo run -- index .` prints structured per-phase timings.
5. Write the 5-line `cargo bench` showing single-threaded vs `rayon::par_iter` parse loop over a directory of `.rs` files.
6. Add an MIT and Apache-2.0 license file pair. Add `[license = "MIT OR Apache-2.0"]` to every Cargo.toml.
7. Add `cargo fuzz init` and write the first parser fuzz target.
8. Initial commit; tag `m0-day1`; push.

If day 1 ends with the bench printing two numbers, the parallel one being meaningfully larger, the fuzz target running for at least a minute without panic, and `grafy diagnose .` printing phase timings, the project is real.

---

## Sources

- [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) — the direct competitor.
- [github/stack-graphs (archived)](https://github.com/github/stack-graphs) — the moat.
- [Stack Graphs paper](https://arxiv.org/abs/2211.01224) — Antonenko et al., 2022.
- [tree-sitter](https://github.com/tree-sitter/tree-sitter)
- [redb](https://github.com/cberner/redb)
- [rmcp](https://github.com/modelcontextprotocol/rust-sdk)
- [SCIP](https://github.com/sourcegraph/scip) — ground-truth source for F1.
- [RepoBench](https://arxiv.org/abs/2306.03091)
- [tracing](https://github.com/tokio-rs/tracing)
- [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
- [tower-lsp](https://github.com/ebkalderon/tower-lsp) (for M3 LSP work)
- Companion design doc: `grafy-design.md` in this folder.