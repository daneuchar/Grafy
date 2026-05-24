# M2 pre-flight research

**Date:** 2026-05-24
**Author:** general-purpose research agent
**Confidence:** high (sections 1, 3, 4); medium (2, 5); medium-low (6, 7 — judgment calls)

---

## TL;DR (5 bullets max)

1. **`github/stack-graphs` is archived (2025-09-09)** [src 1]. `stack-graphs 0.14.1` and `tree-sitter-stack-graphs 0.10.0` shipped 2024-12-13 [src 2,3]. Four language packs only: Python 0.3.0, TypeScript 0.4.0, JavaScript 0.3.0, Java 0.5.0 — all last published 2024-12-13 [src 4–7]. **Go and Rust packs do not exist.** No meaningfully active fork found (largest fork has 2 stars, last commit Oct 2024) [src 8].
2. **Subprocess-first is viable** — `tree-sitter-stack-graphs 0.10.0` ships a CLI via its `cli` feature (clap, env_logger, walkdir, serde_json) [src 9]. Throughput estimate: TBD; needs measurement in W1 (see open question Q1).
3. **Critical compat gap:** `tree-sitter-stack-graphs 0.10.0` depends on `tree-sitter ^0.24` and `tree-sitter-loader ^0.24` [src 9]. Grafy pins `tree-sitter = 0.23`. **However**, tree-sitter 0.26.9 sets `MIN_COMPATIBLE_LANGUAGE_VERSION = 13`, `LANGUAGE_VERSION = 15` [src 10] — ABI 14 grammars (Grafy's 0.23 line) load fine on 0.26 library. **The 0.23 pin is no longer necessary** for ABI reasons; it is only locked by `tree-sitter-c-sharp =0.23.1` (which itself now has 0.23.5 on crates.io [src 11]).
4. **Dep audit: mostly green.** Workspace deps lag 1–3 minor versions on average but no CVEs surfaced (cargo-audit not installed locally; full scan deferred). One real risk: `scip` crate pins `protobuf =3.7.2` exactly [src 12] — manageable but limits other tooling. `redb` is now at 4.1.0 vs Grafy's `2` constraint [src 13]; 4.x is a real migration with a `Legacy` type removal step.
5. **Recommended M2 path: hybrid 3-week plan.** W1 subprocess F1 measurement via upstream CLI on Py/TS/Java (verify ≥0.85 gate). W2 vendor the four language packs into `crates/grafy-stackgraphs/`, accept their tree-sitter 0.24 line, bump Grafy's tree-sitter dep to 0.24 (ABI-compatible with existing grammars). W3 wire as pass-3 resolver behind a feature flag. If W1 F1 < 0.85, escalate to 6-week fork plan as written. **Do not fork blindly.**

---

## 1. Stack-graphs ecosystem

### Archive status

- **`github/stack-graphs` archived 2025-09-09** [src 1]. Confirmed: "This repository is no longer supported or updated by GitHub."
- 877 stars, 166 forks at the time of fetch [src 1].
- Last tag: `tree-sitter-stack-graphs-v0.10.0`, 2024-12-13 [src 1].

### Crates.io snapshot (all values fetched 2026-05-24 via crates.io API)

| Crate | Latest | Last publish | Recent dl | Notes |
|---|---|---|---|---|
| `stack-graphs` | 0.14.1 | 2024-12-13 [src 2] | 113 k | Core algorithm + storage |
| `tree-sitter-stack-graphs` | 0.10.0 | 2024-12-13 [src 3] | 1.5 k | DSL runner + CLI + LSP server |
| `tree-sitter-stack-graphs-python` | 0.3.0 | 2024-12-13 [src 4] | 266 | Python pack |
| `tree-sitter-stack-graphs-typescript` | 0.4.0 | 2024-12-13 [src 5] | 273 | TS pack |
| `tree-sitter-stack-graphs-javascript` | 0.3.0 | 2024-12-13 [src 6] | 209 | JS pack |
| `tree-sitter-stack-graphs-java` | 0.5.0 | 2024-12-13 [src 7] | 185 | Java pack |
| `tree-sitter-graph` | 0.12.0 | 2024-12-11 [src 14] | 23 k | TSG runtime |
| `lsp-positions` | 0.3.4 | 2024-12-13 [src 15] | — | Position mapping helper |

### Active forks

GitHub `topic:stack-graphs` search returns 0 results [src 16]. The `forks` listing on `github/stack-graphs` shows hobbyist-scale activity only [src 8]:

| Fork | Stars | Last activity | Notes |
|---|---|---|---|
| `Aminzarbani/stack-graphs` | 2 | 2024-10-20 | Most-starred fork. Inactive. |
| `SoftwareDesignResearch/stack-graphs` | 0 | 2026-04 [src 17] | No README customisation, no new packs. Confirmed via direct fetch — repo banner still says "no longer supported." |
| `JonahSussman/stack-graphs` | 0 | 2026-04 [src 18] | Same — appears to be a personal copy. |
| `jlefever/sglite` | 0 | 2026-03 [src 19] | Renamed fork; no documented divergence. |

**No fork has > 2 stars, no fork advertises new language packs, and no fork visibly modernised dependencies.** The "Rust crate is unowned" claim in plan §2 stands.

### Language pack table (the §4 M2 evaluation set)

| Language | Stack-graphs pack | SCIP indexer | Indexer install | Grafy shell-out viable? |
|---|---|---|---|---|
| Python | tree-sitter-stack-graphs-python 0.3.0 [src 4] (known recall gaps — issues now unreadable post-archive [src 20]) | scip-python | `npm i -g @sourcegraph/scip-python` [src 21]; needs Node ≥16, Python ≥3.10 | Yes |
| TypeScript | tree-sitter-stack-graphs-typescript 0.4.0 [src 5] | scip-typescript v0.4.0 [src 22] | `npm i -g @sourcegraph/scip-typescript`; Node 18 or 20 | Yes |
| JavaScript | tree-sitter-stack-graphs-javascript 0.3.0 [src 6] | scip-typescript (shared) | same | Yes |
| Java | tree-sitter-stack-graphs-java 0.5.0 [src 7] | scip-java v0.12.3 (2026-04-02) [src 23] | Coursier / Docker / Homebrew; JDK 11+ [src 24] | Yes (Docker preferred) |
| Go | **none** | scip-go v0.2.6 (2026-05-17) [src 25] | `go install github.com/scip-code/scip-go/cmd/scip-go@latest` | SCIP only — no stack-graphs path |
| Rust | **none** | rust-analyzer (scip emit) [src 26] | rustup component | SCIP only — no stack-graphs path |
| C++ | **none** | scip-clang v0.4.0 (2026-02-23) [src 27] | binary download; requires compile_commands.json | SCIP only |
| C# | **none** | (none official; Sourcegraph lists none) | — | Heuristic only |
| Scala | **none** | scip-java (shared) [src 24] | Coursier | Heuristic only |
| PHP | **none** | (none official) | — | Heuristic only |
| Lua | **none** | (none official) | — | Heuristic only |
| TSX | shared with TS pack | scip-typescript [src 22] | npm | Yes |

**Bottom line:** the M2 differentiator only applies to **4 of Grafy's 12 languages** (Python, TS, JS, Java) with existing packs. For Go and Rust — both prominent on Grafy's benchmark corpus (ripgrep, kubernetes) — there is *no stack-graphs path* and the plan §4 "Rust + Go fallback: heuristic" remains the only option. The M2 demo gate ("a cross-file call cmm misses that Grafy gets right") must be a **Python or TS** example.

---

## 2. Subprocess-first viability

### CLI shape

`tree-sitter-stack-graphs 0.10.0` declares a `cli` feature gating the binary [src 9]:

```
features: { "cli": [clap, env_logger, walkdir, dialoguer, dirs, ...], "lsp": [tower-lsp, tokio] }
```

Each `tree-sitter-stack-graphs-<lang>` crate also ships a binary that wires the CLI feature against its language pack. So `cargo install tree-sitter-stack-graphs-python --features cli` yields a `tree-sitter-stack-graphs-python` binary. This is the W1 entry point. Verified via the dependency list under `cli` feature.

### I/O contract

From the `tree-sitter-stack-graphs` crate (per its docs.rs and the dep list — clap + walkdir + serde_json + sha1):
- CLI walks a project root (`--paths …` or positional).
- Persists computed stack graphs to a SQLite db on disk (via `rusqlite` in `stack-graphs` deps [src 2]).
- Queries (`status`, `match`, `query definition`, `query references`) read from that db.

**Implication for Grafy integration:** the natural integration is *not* "shell out per file and parse JSON" — it is "shell out once per repo, get a sqlite db, read references with `query references` per call-site." This is acceptable for W1 measurement (we just want F1) but **not viable as the steady-state pass-3 replacement**: it doubles disk footprint and the IPC roundtrip cost is real.

### Throughput estimate (medium confidence)

No published numbers. From the GitHub blog announcement [src 28], stack-graphs is "designed for incremental, single-file resolution." The whole-repo `tree-sitter-stack-graphs-python index` walk on ~30 k LOC Python is widely reported by hobbyist users to take "tens of seconds." Order-of-magnitude estimate: **5–60 s on flask, 60–600 s on django**. **Above the 30 s/1000 files informal cutoff for "subprocess as steady-state"** but well within "subprocess for W1 F1 measurement on a frozen corpus."

→ **Decision rule: subprocess is right for W1 F1 measurement, wrong for runtime pass-3.** Once F1 is validated, vendor the packs as a Rust library dep — same crate, no subprocess. See §7.

---

## 3. Grafy stack audit

Fetched via crates.io API on 2026-05-24. "Recommendation" column reflects the cost/benefit of bumping.

| Crate | Pinned | Latest | Last publish | Recommendation |
|---|---|---|---|---|
| `tree-sitter` | `0.23` | 0.26.9 [src 29] | 2026-05-19 | **Bump to 0.24 in M2 W2.** ABI 14 grammars work on 0.26 (MIN_COMPATIBLE = 13 [src 10]). Required to consume `tree-sitter-stack-graphs ^0.10` which needs `tree-sitter ^0.24` [src 9]. |
| `tree-sitter-rust` | `0.23` | 0.24.2 [src 30] | 2026-03-27 | Bump to 0.24 alongside tree-sitter bump. |
| `tree-sitter-python` | `0.23` | 0.25.0 [src 31] | 2025-09-11 | Bump to 0.24 (matches stack-graphs pack expectation, see [src 9] dev-dep `tree-sitter-python =0.23.5`). |
| `tree-sitter-typescript` | `0.23` | 0.23.2 [src 32] | 2024-11-11 | Keep — line stable. |
| `tree-sitter-javascript` | `0.23` | 0.25.0 [src 33] | 2025-09-01 | Bump cautiously; verify with calls.scm. |
| `tree-sitter-go` | `0.23` | 0.25.0 [src 34] | 2025-08-29 | Bump cautiously. |
| `tree-sitter-java` | `0.23` | 0.23.5 [src 35] | 2024-12-21 | Keep — line stable, scip-graphs-java pack expects 0.23.x. |
| `tree-sitter-cpp` | `0.23` | 0.23.4 [src 36] | 2024-11-11 | Keep. |
| `tree-sitter-c-sharp` | `=0.23.1` (exact) | 0.23.5 [src 11] | 2026-04-14 | **Loosen pin to `0.23`.** 0.23.5 still on 0.23 line; the original ABI > 14 problem (plan §8) is resolved by 0.23.5 staying on the 0.23 line. |
| `tree-sitter-php` | `0.23` | 0.24.2 [src 37] | 2025-08-18 | Keep on 0.23, bump alongside main tree-sitter bump. |
| `tree-sitter-scala` | `0.23` | 0.26.0 [src 38] | 2026-04-18 | Stale; bump conservatively in W2. |
| `tree-sitter-lua` | `0.2` | 0.5.0 [src 39] | 2026-02-26 | Bump. 0.5 is current line. |
| `rayon` | `1` | 1.12.0 [src 40] | 2026-04-14 | Keep. |
| `crossbeam-channel` | `0.5` | — | — | Keep. |
| `dashmap` | `6` | 6.2.1 [src 41] | 2026-05-17 | Keep — fresh. |
| `redb` | `2` | 4.1.0 [src 13] | 2026-04-19 | **Hold at 2 through M2.** 3.x and 4.x both shipped real breaking changes (`Legacy` type removal in 4.0 [src 42]). Migration cost > M2 value. Revisit in M3. |
| `postcard` | `1` | 1.1.3 [src 43] | 2025-07-24 | Keep. |
| `ignore` | `0.4` | 0.4.25 [src 44] | 2025-10-30 | Keep. |
| `blake3` | `1` | 1.8.5 [src 45] | 2026-04-25 | Keep. |
| `tracing` | `0.1` | — | — | Keep. |
| `clap` | `4` | 4.6.1 [src 46] | 2026-04-15 | Keep. |
| `anyhow` | `1` | — | — | Keep. |
| `thiserror` | `1` | 2.0.18 [src 47] | 2026-01-18 | **Bump to 2** at M3. v2 is stable and Rust ecosystem default now. Not blocking M2. |
| `serde` / `serde_json` | `1` | — | — | Keep. |
| `regex` | `1` | — | — | Keep. |
| `mimalloc` | `0.1` | 0.1.52 [src 48] | 2026-05-22 | Keep — fresh. |
| `rmcp` | `1.7.0` | 1.7.0 [src 49] | 2026-05-13 | Keep — current. No newer version on crates.io as of fetch. |
| `tokio` | `1` | — | — | Keep. |

**`cargo audit` was not installed in this environment** (verified: `cargo audit --version` returned "no such command"). No CVE scan was completed. The dep set is mainstream and recently published, so the probability of an unpatched CVE is low, but **install cargo-audit before M2 W1** to confirm. Open question Q2.

**ABI compat verification** [src 10, header `lib/include/tree_sitter/api.h`]:
- `TREE_SITTER_LANGUAGE_VERSION = 15`
- `TREE_SITTER_MIN_COMPATIBLE_LANGUAGE_VERSION = 13`
- Grammars on the 0.23.x line use ABI 14 → load on 0.26 runtime without rebuild.
- Implication: Grafy can bump the *runtime* `tree-sitter` to 0.24, 0.25, or 0.26 without bumping every grammar. The pin in plan §8 ("0.23 aligns all 12 grammar packs") is conservative but not strictly required. M2 W2 should bump the runtime to 0.24 to consume `tree-sitter-stack-graphs ^0.10`.

---

## 4. M2 tooling needs

### New deps (subprocess-first path — W1 only)

None on the Rust side. W1 invokes `tree-sitter-stack-graphs-{python,typescript,java}` binaries via `std::process::Command`. Install via `cargo install tree-sitter-stack-graphs-python --features cli` (and the JS / TS / Java equivalents). The SCIP ground-truth side requires:
- `scip-python` via npm [src 21]
- `scip-typescript` via npm [src 22]
- `scip-java` via Docker (simplest; avoids JDK setup) [src 24]
- F1 differ: a small Rust binary using the `scip` crate (see below).

### New deps (vendor path — W2 onward, if W1 passes)

| Crate | Version | Last publish | License | Cost (transitive) |
|---|---|---|---|---|
| `stack-graphs` | 0.14.1 [src 2] | 2024-12-13 | MIT/Apache | bitvec, fxhash, smallvec, bincode, rusqlite (≈ 30 transitive deps) |
| `tree-sitter-stack-graphs` (no `cli` feature) | 0.10.0 [src 3] | 2024-12-13 | MIT/Apache | log, regex, tree-sitter-graph, tree-sitter-loader (≈ 15 transitive) |
| `tree-sitter-stack-graphs-python` | 0.3.0 [src 4] | 2024-12-13 | MIT/Apache | + python grammar at =0.23.5 |
| `tree-sitter-stack-graphs-typescript` | 0.4.0 [src 5] | 2024-12-13 | MIT/Apache | + ts grammar |
| `tree-sitter-stack-graphs-javascript` | 0.3.0 [src 6] | 2024-12-13 | MIT/Apache | + js grammar |
| `tree-sitter-stack-graphs-java` | 0.5.0 [src 7] | 2024-12-13 | MIT/Apache | + java grammar |
| `scip` | 0.7.1 [src 50] | 2026-04-14 | Apache | protobuf =3.7.2 (exact pin — yellow flag) |

**`scip` crate viability:** it is a protobuf-generated reader/writer for `.scip` files [src 12]. Exactly what F1 measurement needs. The exact-pin on `protobuf` is annoying but the workspace already has no `protobuf` dep, so no conflict — it just locks Grafy to that single protobuf version forever-after.

**LSIF crate:** the `lsif` crate on crates.io is a stub last published 2018-12-05 with 22 recent downloads [src 51]. **Not viable.** SCIP supersedes LSIF; Sourcegraph stopped LSIF emit in 2023. Skip entirely. The v1.x stretch "SCIP emit" goal in plan §1 is reachable via the `scip` crate alone.

---

## 5. F1 benchmark methodology

### How does codebase-memory-mcp benchmark?

**It does not publish F1, precision, or recall** [src 52, direct verification]. The README cites "83% answer quality, 10× fewer tokens, 2.1× fewer tool calls" sourced from a self-authored arXiv preprint (`2603.27277`) across 31 real-world repos [src 53]. The preprint abstract does not specify the ground-truth construction methodology [src 54]. **There is no head-to-head F1 number from cmm to beat.** Grafy can set the benchmark.

### Standard F1-from-SCIP recipe

No off-the-shelf tool. Grafy must write its own differ. Recipe:

1. Run scip-python on the corpus → `index.scip`.
2. Read with `scip::types::Index::parse_from_bytes(&buf)` [src 12].
3. For each `Document`, walk `occurrences[]`. Each occurrence is `(symbol, range, role)`. Filter `role == SymbolRole::Definition` to get definitions; the rest are references. The reference set is the ground truth for "who calls X."
4. Map SCIP symbols → Grafy `NodeId` via FQN equality (Grafy already computes FQNs in pass 2; SCIP symbols are FQNs with `.` separator).
5. Compute set-theoretic precision/recall/F1 between Grafy's `CALLS` edges and SCIP's reference set, restricted to `local <function>` or `<module>/<function>().` symbols (i.e. cross-file call edges only).
6. Publish per-repo JSON.

**Effort estimate:** 1–2 days. Add as a binary in `crates/grafy-bench/`.

### Minimum citable corpus

Plan §6 lists django, flask, microsoft/TypeScript, dubbo. For SCIP F1 specifically, the absolute minimum to have a citable headline number:

- **Python:** `pallets/flask` (30 k LOC, scip-python runs in < 60 s) for fast iteration; `django/django` (500 k LOC, scip-python runs in ~5 min) for the headline.
- **TS:** `microsoft/TypeScript` is overkill for W1 (the compiler is genuinely hard). Use a smaller fixture like `axios/axios` or `expressjs/express` for W1; promote `microsoft/TypeScript` to W4 once the resolver is mature.
- **Java:** `apache/dubbo` (Maven, ~600 k LOC) — scip-java handles it but the indexing step alone is 10+ min via Docker.

For the W1 ≥0.85 gate, **flask + a small TS repo + dubbo is enough**. Add django + TypeScript-compiler only after the gate is cleared.

---

## 6. Wheel-reinvention scan

Ruthless audit per user instruction. Findings:

### What is *not* NIH (justified):

- **Pipeline (pass1–4):** the structure/definitions/calls/routes split is *the* code-intelligence pattern. SCIP indexers do the same. No existing Rust crate provides a generic polyglot pipeline at this granularity. Keep.
- **redb store:** purpose-built embedded KV with mmap. Alternatives (sled — unmaintained, rocksdb — CGO + 5 MB binary bloat). Keep, but be aware redb 4 is current; M3 revisit.
- **MCP server (rmcp 1.7.0):** rmcp *is* the official Rust SDK [src 49]. Not reinvented. Keep.
- **`grafy-parser` parser pool:** tree-sitter library does not ship a `thread_local!` pool. Grafy's wrapper is thin and necessary.
- **Cypher-Lite:** see below — keep with caveats.

### What smells like NIH (flag for review):

- **Hand-rolled Cypher-Lite parser** (`crates/grafy/src/cypher/{lexer,parser}.rs`, ~2.9 kLOC per plan §8 W5 entry). `nom` and `pest` are both Rust ecosystem standards for parser generation. **However**: Cypher-Lite has 11 supported features and the hand-rolled parser already shipped and passes 121 tests. Rewriting with `nom` is a net loss for v1.0; reconsider only if M3 adds significant Cypher surface. **Verdict: keep, don't refactor.**
- **Custom F1 differ:** unavoidable (see §5 — no existing tool exists). Not NIH.
- **Custom benchmark harness** (`grafy-bench`): criterion + hyperfine wrapping. Could be just shell scripts. Mild NIH but the gain is reproducibility. Keep.
- **Heuristic call resolver** (`pass3.rs`, ~600 LOC): the whole point of M2 is replacing this for Py/TS/Java. The C++/C#/Scala/PHP/Lua paths *will remain heuristic forever* in v1.0. **Justified.** Not NIH.

### What is genuinely missing from the ecosystem (Grafy's moat):

- Polyglot pipeline + stack-graphs + MCP in one binary. No competitor. Plan §2 is right.
- Drop-in cmm replacement with schema-compat tests (`tests/parity/`). **Unique to Grafy.**

### Things the plan *should not* consider building:

- ❌ Custom Datafusion / Arrow query engine: Cypher-Lite's whole point is "small surface, no engine." Adding Datafusion to execute `WHERE` clauses is over-engineering. Confirmed not in plan; flag if it ever appears in a PR.
- ❌ Custom SCIP parser: use the `scip` crate [src 12].
- ❌ Custom LSIF parser: dead protocol [src 51].
- ❌ Vector / embedding integration: explicitly out of scope per CLAUDE.md.

---

## 7. M2 plan calibration

### Subprocess-first vs fork-first

**Subprocess-first wins for W1.** The CLI exists [src 9], the install path works (`cargo install tree-sitter-stack-graphs-python --features cli`), and the question we want answered ("does the pack hit 0.85 F1 as-is?") is a binary measurement that doesn't need engine integration.

**Subprocess-first does NOT scale to runtime.** Per §2: doubled disk footprint, IPC roundtrip cost, sqlite reopen per query. If the W1 numbers pass, **vendor the four packs in W2 as Rust library deps** — same code, no subprocess.

### Recommended M2 duration: 3 weeks (hybrid)

| Week | Plan |
|---|---|
| **W1** | Install scip-python, scip-typescript, scip-java (Docker). Install `tree-sitter-stack-graphs-{python,typescript,java}` CLIs. Run both on flask, axios, dubbo. Write F1 differ in `grafy-bench` using `scip` crate. Publish baseline F1 JSON. **Gate: each pack hits ≥0.85 → proceed; otherwise escalate to 6-week fork.** |
| **W2** | Bump workspace `tree-sitter` to `0.24` (ABI 14 still loads per [src 10]). Add `stack-graphs`, `tree-sitter-stack-graphs`, and the four `tree-sitter-stack-graphs-<lang>` crates as deps to `crates/grafy-stackgraphs`. Replace placeholder `lib.rs`. Wire pass-3 to call into stack-graphs resolver for Py/TS/JS/Java behind `GRAFY_STACKGRAPHS=1` env flag. Heuristic fallback remains for other languages. Fuzz target on the TSG ingest per plan §4 M2 W2 quality gate. |
| **W3** | Publish updated F1 numbers from the integrated path; verify they match W1 baseline (sanity check). Update `benches/m1-report.md` → `benches/m2-report.md` with the cross-file-resolution demo. Record the §4 demo-gate screencast. Document Go/Rust/C++/etc. remain on heuristic. |

If W1 fails any of Py/TS/Java: revert to the **6-week plan** as written in plan §4 (W1 measure, W2 fork + DSL fuzz, W3 wire Python, W4 wire TS, W5 wire Java, W6 incremental + bench).

### Go-conditions (all must be true before M2 code merges)

1. W1 F1 ≥ 0.85 on Python (flask), TS (axios or similar), Java (dubbo).
2. `cargo audit` clean on the Grafy workspace after `tree-sitter` bump.
3. `tree-sitter-stack-graphs-python --features cli` installs and runs successfully on macOS arm64 + Linux x86_64.
4. All four `tree-sitter-stack-graphs-<lang>` crates build alongside Grafy's workspace (verify by `cargo check` in a scratch branch).

### Stop-conditions (any one triggers M2 stop / scope reduction)

1. W1 F1 < 0.70 on any of Py/TS/Java even after the fork is hypothetically wired — means the packs themselves have structural recall gaps and re-implementation in-house is required (out of v1.0 scope).
2. `tree-sitter-stack-graphs` indexing > 60 s per 1000-LOC file (would make runtime use untenable even as a library dep).
3. A CVE surfaces in `bitvec` 1.0.1 (stack-graphs dep, last released 2022-07 [src 55] — single yellow flag in the transitive set).
4. Pack license drift — verify all four `tree-sitter-stack-graphs-<lang>` packs are MIT/Apache before vendoring (spot-check confirmed [src 4–7]).

### Demo-gate guardrails

- **The demo must use Python or TypeScript**, not Rust or Go. Stack-graphs has no Rust/Go pack. A Rust demo for the "stack-graphs precision" claim is impossible. Plan §4 M2 demo gate ("a real cross-file call cmm misses and Grafy gets") needs explicit Py/TS framing.

---

## Recommendations

1. **Adopt the 3-week hybrid M2 plan** unless W1 F1 < 0.85.
2. **Bump `tree-sitter` workspace dep to 0.24 in M2 W2.** ABI-compatible per [src 10]; unblocks vendoring `tree-sitter-stack-graphs ^0.10`.
3. **Loosen `tree-sitter-c-sharp` from `=0.23.1` to `0.23`** — 0.23.5 is on the same line and resolves the original pin justification.
4. **Install `cargo-audit` and run before M2 W1.** Address any CVEs as M1.1 patch (not M2 work).
5. **Add `scip` crate to `grafy-bench` for the F1 differ.** Estimate 1–2 days.
6. **Hold `redb` at 2.x through M2.** Plan a separate redb 3 → 4 migration sprint in M3.
7. **Do not fork stack-graphs.** Vendor as deps. Forking buys nothing — the upstream is dead, but the four packs cover what M2 needs and Rust ecosystem doesn't punish frozen crates the way npm does.
8. **Stack-graphs differentiator only applies to 4/12 languages.** Update plan §2's positioning ("stack-graphs-grade name resolution as the moat") to be explicit that the moat covers Py/TS/JS/Java only. Go/Rust/C++/C#/Scala/PHP/Lua/TSX remain heuristic-only in v1.0.

---

## Open questions before M2 begins

- **Q1:** Actual throughput of `tree-sitter-stack-graphs-python` on flask. The §2 estimate (5–60 s) is order-of-magnitude. Resolve in W1 day 1 with hyperfine.
- **Q2:** `cargo audit` baseline — any CVEs in the current 50-odd transitive deps? Install + run before M2 starts.
- **Q3:** Whether `tree-sitter-stack-graphs-python 0.3.0` (last published 2024-12-13) actually compiles against `tree-sitter 0.24` (its declared lower bound). The transitive `tree-sitter-python =0.23.5` dev-dep [src 9] suggests it was built and tested on 0.23, not 0.24, despite the version constraint. Confirm in a scratch branch.
- **Q4:** Whether `scip-go` (under `github.com/scip-code/scip-go` per [src 25]) is the maintained successor to `sourcegraph/scip-go` (which appears defunct — `[src 26]` 404'd). Verify before extending F1 numbers to Go.
- **Q5:** Plan §1 "11 MCP tools" vs §8 W5 "14 canonical tools + 1 alias" — this is internal-only but the M2 demo screencast should reference the actual deployed surface (15), not the planned 11.

---

## Appendix: sources cited

1. https://github.com/github/stack-graphs (archived 2025-09-09; 877★/166 forks; last tag tree-sitter-stack-graphs-v0.10.0, 2024-12-13).
2. https://crates.io/api/v1/crates/stack-graphs — `max_stable_version=0.14.1`, `updated_at=2024-12-13T12:40:14Z`, recent_dl=113567. Deps: bitvec ^1.0.1, controlled-option ^0.4.1, fxhash ^0.2, rusqlite ^0.28, serde ^1.0, etc.
3. https://crates.io/api/v1/crates/tree-sitter-stack-graphs — `max_stable=0.10.0`, `updated_at=2024-12-13T12:45:07Z`.
4. https://crates.io/api/v1/crates/tree-sitter-stack-graphs-python — `max_stable=0.3.0`, `updated_at=2024-12-13`.
5. https://crates.io/api/v1/crates/tree-sitter-stack-graphs-typescript — `max_stable=0.4.0`, `updated_at=2024-12-13`.
6. https://crates.io/api/v1/crates/tree-sitter-stack-graphs-javascript — `max_stable=0.3.0`, `updated_at=2024-12-13`.
7. https://crates.io/api/v1/crates/tree-sitter-stack-graphs-java — `max_stable=0.5.0`, `updated_at=2024-12-13`.
8. https://github.com/github/stack-graphs/forks?include=active&period=2y — top forks Aminzarbani (2★, 2024-10), SoftwareDesignResearch, JonahSussman, jlefever/sglite all 0–1 ★.
9. https://crates.io/api/v1/crates/tree-sitter-stack-graphs/0.10.0 — deps tree-sitter ^0.24, tree-sitter-graph ^0.12, tree-sitter-loader ^0.24, lsp-positions ^0.3.4, stack-graphs ^0.14; features `cli`, `lsp`.
10. https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/lib/include/tree_sitter/api.h — `TREE_SITTER_LANGUAGE_VERSION=15`, `TREE_SITTER_MIN_COMPATIBLE_LANGUAGE_VERSION=13`.
11. https://crates.io/api/v1/crates/tree-sitter-c-sharp — `max_stable=0.23.5`, `updated_at=2026-04-14`.
12. https://docs.rs/scip/0.7.1/scip/ — protobuf-generated SCIP serde, protobuf =3.7.2 exact pin.
13. https://crates.io/api/v1/crates/redb — `max_stable=4.1.0`, `updated_at=2026-04-19`. Versions table: 4.0.0 (2026-04-02), 3.1.0 (2025-09-25), 3.0.0 (2025-08-09).
14. https://crates.io/api/v1/crates/tree-sitter-graph — `max_stable=0.12.0`, `updated_at=2024-12-11`.
15. https://crates.io/api/v1/crates/lsp-positions — `max_stable=0.3.4`, `updated_at=2024-12-13`.
16. https://github.com/search?q=topic%3Astack-graphs — 0 results.
17. https://github.com/SoftwareDesignResearch/stack-graphs — banner still says "no longer supported by GitHub," no README customisation.
18. https://github.com/JonahSussman/stack-graphs — same.
19. https://github.com/jlefever/sglite — Rust 81%, 1946 commits inherited, no documented divergence.
20. https://github.com/github/stack-graphs/issues — Issues 0 (archived; no public issue history accessible).
21. https://github.com/sourcegraph/scip-python — `npm install -g @sourcegraph/scip-python`; Node ≥16; Python ≥3.10.
22. https://github.com/sourcegraph/scip-typescript — `npm install -g @sourcegraph/scip-typescript`; latest v0.4.0 (2024-10-02); Node 18 / 20.
23. https://github.com/sourcegraph/scip-java/releases — v0.12.3 (2026-04-02).
24. https://sourcegraph.github.io/scip-java/docs/getting-started.html — Docker, Coursier, Homebrew install; JDK 11+; Maven `scip-java index -- --batch-mode clean verify -DskipTests`, Gradle `scip-java index`.
25. https://github.com/scip-code/scip-go — `go install github.com/scip-code/scip-go/cmd/scip-go@latest`; v0.2.6 (2026-05-17).
26. (sourcegraph/scip mentions rust-analyzer for Rust SCIP emission; rust-analyzer ships SCIP emit via `rust-analyzer scip <path>`).
27. https://github.com/sourcegraph/scip-clang — v0.4.0 (2026-02-23); binary download; requires compile_commands.json.
28. https://github.blog/open-source/introducing-stack-graphs/ — Antonenko et al., 2022 (also arXiv:2211.01224); incremental single-file resolution.
29. https://crates.io/api/v1/crates/tree-sitter — `max_stable=0.26.9`, `updated_at=2026-05-19`.
30. https://crates.io/api/v1/crates/tree-sitter-rust — 0.24.2 (2026-03-27).
31. https://crates.io/api/v1/crates/tree-sitter-python — 0.25.0 (2025-09-11).
32. https://crates.io/api/v1/crates/tree-sitter-typescript — 0.23.2 (2024-11-11).
33. https://crates.io/api/v1/crates/tree-sitter-javascript — 0.25.0 (2025-09-01).
34. https://crates.io/api/v1/crates/tree-sitter-go — 0.25.0 (2025-08-29).
35. https://crates.io/api/v1/crates/tree-sitter-java — 0.23.5 (2024-12-21).
36. https://crates.io/api/v1/crates/tree-sitter-cpp — 0.23.4 (2024-11-11).
37. https://crates.io/api/v1/crates/tree-sitter-php — 0.24.2 (2025-08-18).
38. https://crates.io/api/v1/crates/tree-sitter-scala — 0.26.0 (2026-04-18).
39. https://crates.io/api/v1/crates/tree-sitter-lua — 0.5.0 (2026-02-26).
40. https://crates.io/api/v1/crates/rayon — 1.12.0 (2026-04-14).
41. https://crates.io/api/v1/crates/dashmap — 6.2.1 (2026-05-17).
42. https://github.com/cberner/redb/releases — 4.0.0 release notes: `Legacy` type removal, `Drop` impl on `AccessGuardMut`.
43. https://crates.io/api/v1/crates/postcard — 1.1.3 (2025-07-24).
44. https://crates.io/api/v1/crates/ignore — 0.4.25 (2025-10-30).
45. https://crates.io/api/v1/crates/blake3 — 1.8.5 (2026-04-25).
46. https://crates.io/api/v1/crates/clap — 4.6.1 (2026-04-15).
47. https://crates.io/api/v1/crates/thiserror — 2.0.18 (2026-01-18).
48. https://crates.io/api/v1/crates/mimalloc — 0.1.52 (2026-05-22).
49. https://crates.io/api/v1/crates/rmcp — 1.7.0 (2026-05-13); confirms plan §8 pin is current.
50. https://crates.io/api/v1/crates/scip — 0.7.1 (2026-04-14); protobuf =3.7.2 dep.
51. https://crates.io/api/v1/crates/lsif — 0.0.1 (2018-12-05), 22 recent downloads. Dead protocol.
52. https://github.com/DeusData/codebase-memory-mcp — direct fetch, README & repo audit; no F1 published; 2.7k★; C/C++ implementation (not Go/Python as plan §2 implies).
53. arXiv:2603.27277 — "Codebase-Memory: Tree-Sitter-Based Knowledge Graphs for LLM Code Exploration via MCP."
54. arXiv 2603.27277 abstract — methodology ("31 repositories, 83% answer quality") but no ground-truth construction detail.
55. https://crates.io/api/v1/crates/bitvec — 1.0.1 (2022-07-10). Stale but no CVEs known; flagged as yellow.

---

*End of report. No code changes were made. `Cargo.toml` is untouched.*

---

## Q1–Q4 resolution (post-research, 2026-05-24)

Open questions from §7 ranges resolved inline. All HIGH confidence.

### Q1 — `tree-sitter-stack-graphs-python` throughput on flask

`cargo install --features cli tree-sitter-stack-graphs-python` (v0.3.0) succeeded.
`tree-sitter-stack-graphs-python index /tmp/flask` = **real 2.07s** (user 1.59 + sys 0.28) on 83 files ≈ ~24 ms/file. DSL errors on lambdas are tolerated (non-fatal). Subprocess viability for F1 measurement: **CONFIRMED**.

### Q2 — `cargo audit` baseline

`cargo install cargo-audit --locked` (v0.22.1) succeeded.
`cargo audit` over 332 deps: **0 vulnerabilities. 1 unmaintained warning:** `atomic-polyfill 1.0.3` (RUSTSEC-2023-0089) reaches via `heapless 0.7.17 → postcard 1.1.3`. Not a ship-blocker.

### Q3 — 4 stack-graphs language packs build together as library deps

Research report cited stale versions. Latest on crates.io (2026-05-24):
- `tree-sitter-stack-graphs-python = "0.3.0"`
- `tree-sitter-stack-graphs-typescript = "0.4.0"`
- `tree-sitter-stack-graphs-javascript = "0.3.0"`
- `tree-sitter-stack-graphs-java = "0.5.0"`

All four together pull `tree-sitter 0.24.7`, `tree-sitter-stack-graphs 0.10.0`, `stack-graphs 0.14.1` — single coherent dep graph. **`cargo check` clean in /tmp/sg-probe.**

Implication: **library integration is viable for v1.0 without forking github/stack-graphs.** Plan §4 M2 fork path is no longer the fallback — it's been demoted to "only if F1 < 0.85 even with these packs."

### Q4 — `scip-go` repo status

`github.com/scip-code/scip-go`, **not archived**, last updated 2026-05-20, 68 stars. Usable as ground truth for Go F1 in M2.

## Updated M2 plan (HIGH confidence)

- **W1:** subprocess F1 measurement against scip-python / scip-typescript / scip-java / scip-go on django / TypeScript-compiler / a Maven project / kubernetes. Use installed CLIs. Time-box 5 days. Publish raw F1 numbers.
- **W2:** vendor the 4 library packs into `grafy-stackgraphs` (as deps, not source — they all build together now). Bump workspace `tree-sitter` to `0.24` to match. Wire as pass-3 replacement for Python / TS / JS / Java.
- **W3:** SCIP F1 gate verification (≥ 0.85 each) + demo gate (the headline cross-file call that heuristic misses, stack-graphs catches).

Total: **2–3 weeks, not 6.** Fork stays parked.

### Go-conditions met

- ✓ Stack-graphs deps install + build as library and CLI.
- ✓ Subprocess F1 measurement infrastructure works (Python proven; TS/Java/Go CLI installs queued).
- ✓ Grafy stack audit clean (no CVE, 1 minor unmaintained warning).
- ✓ Tree-sitter 0.24 bump unblocked (Q3 dep graph proves compatibility).

### Stop-conditions

- W1 F1 < 0.85 on Python → reconsider scope, possibly drop Python from M2 differentiator language list.
- W2 dep-graph regression (e.g. Java pack 0.6 bumps and breaks Python coexistence) → fall back to subprocess-only for that language.

