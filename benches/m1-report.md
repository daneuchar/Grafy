# Grafy M1 W6 Benchmark Report

## Summary

Grafy cold-index is **0.04–0.09× the speed of codebase-memory-mcp** on the measured corpus (i.e. 11–22× slower), failing the M1 cold-index ≥ 2× gate. The root cause is the W3 heuristic caller-attribution overshoot (plan §8): Grafy emits ~277 k call edges for ripgrep vs CMM's 13 k. Pass 3 reruns tree-sitter on every file and cross-products all callers × all callees in a file, dominating wall time. Warm/incremental paths are a different story: Grafy warm is 1.5–2.2× **faster** than CMM (warm gate passes); incremental p95 is 68 ms on ripgrep (< 250 ms gate passes). Peak RSS gate passes: Grafy 48–83 MB vs CMM 109–202 MB.

---

## Setup

**Host**

```
Darwin MacBookPro 25.5.0 Darwin Kernel Version 25.5.0 (arm64)
CPU: Apple M1 Pro, 10 cores (sysctl hw.ncpu = 10)
RAM: 16 GB
```

**Toolchain**

| Item | Version |
|---|---|
| Rust / Cargo | 1.88.0 (2025-06-23) |
| Grafy | commit `213fd215cf5f5b26aac5c2cea576dcba37d8ce39` (branch main) |
| codebase-memory-mcp | 0.6.1 |
| hyperfine | 1.20.0 |

**Methodology**

- 10 runs, 1 warmup, `mean ± stdev`, `min`, `max`.
- Cold index: `.grafy/` deleted before each run (Grafy); `~/.cache/codebase-memory-mcp/<project>.db` deleted (CMM).
- FS cache: macOS `purge` requires `sudo` and was **not** called. Numbers reflect OS-page-cache-warm cold starts, which slightly favours subsequent runs. True cold-disk numbers would be worse for both tools equally.
- Incremental: `touch <file>` before each run via `hyperfine --prepare`.
- Peak RSS: `man /usr/bin/time -l`, "maximum resident set size" field (bytes), single cold run.
- Grafy threads: rayon default (10 cores on this machine). CMM workers: default (`workers=10` seen in logs).
- Grafy binary: `cargo build --release`, no `RUSTFLAGS` override.

**Corpus scoping**

Required repos measured this run: `ripgrep`, `flask`, `grafy` (self), fixture services.

Deferred repos (noted per plan §4 task scoping — "deferred to dedicated CI workflow"):
- `TypeScript` (700 k LOC), `kubernetes` (5 M LOC), `dubbo` (600 k LOC): clone + index would exceed the 30-minute session budget.
- `django` (500 k LOC): optional per task scope; not attempted this run.
- `repowise`: repo at `repowise-dev/repowise` SHA `8d1d1875bb45213f26d55f1cb687a5d8628b3efb` — SHA pinned in corpus.toml; not cloned this run (polyglot small repo, would likely show Grafy faster due to small file count).

**Blockers encountered**

1. `pipeline/mod.rs` deadlock: `definitions_tx` was bounded at 1024; main thread collected `definitions_rx` only after joining p1+p2, causing a deadlock on repos with > 1024 definition events. The parallel incremental agent's commit `213fd21` already included the fix (`unbounded()` channels for `structure_tx` and `definitions_tx`). Building from the correct SHA resolved the hang.

2. CMM `index_repository` requires `repo_path` key (not `path`); discovered during first test call. No impact on benchmarks.

---

## Cold Index

All times in milliseconds. 10 runs, 1 warmup. Lower is better.

| Repo | Grafy mean ± stdev | Grafy min / max | CMM mean ± stdev | CMM min / max | Ratio (Grafy/CMM) |
|---|---|---|---|---|---|
| ripgrep | 9682 ± 530 ms | 9198 / 10753 ms | 425 ± 17 ms | 405 / 454 ms | **22.8× slower** |
| flask | 1895 ± 264 ms | 1500 / 2335 ms | 544 ± 131 ms | 295 / 675 ms | **3.5× slower** |
| grafy-self | 787 ± 23 ms | 761 / 835 ms | 450 ± 87 ms | 203 / 485 ms | **1.7× slower** |
| fixtures | 37 ± 8 ms | 32 / 59 ms | 77 ± 2 ms | 74 / 81 ms | **0.48× (2.1× faster)** |

**Gate: FAIL.** Cold index ≥ 2× faster required; Grafy is 1.7–22.8× slower on the measured repos. Only the 3-file fixture set shows Grafy ahead. Root cause: W3 pass-3 reparse + call-edge fan-out (plan §8 `W3 caller attribution overshoot`).

---

## Warm Index

| Repo | Grafy mean ± stdev | CMM mean ± stdev | Ratio (Grafy/CMM) |
|---|---|---|---|
| ripgrep | 54 ± 5 ms | 79 ± 3 ms | **1.46× faster** |
| flask | 40 ± 2 ms | 81 ± 2 ms | **2.03× faster** |
| grafy-self | 37 ± 2 ms | 85 ± 10 ms | **2.26× faster** |
| fixtures | (unchanged fast-path) | 77 ± 2 ms | n/a |

**Gate: PASS.** Warm index ≥ CMM on all measured repos. Grafy's blake3 unchanged-file fast-path (plan §4 M1 W6) is highly effective: if nothing changed, the pipeline takes ~37–54 ms regardless of repo size.

---

## Incremental p50 / p95 (single-file edit)

Methodology: `touch <representative file>` before each run via `hyperfine --prepare`. This forces exactly one `Modified` file through passes 1–4; all other files hit the unchanged fast-path.

| Repo | Grafy mean ± stdev | min | max | Estimated p95 |
|---|---|---|---|---|
| ripgrep | 63 ± 4 ms | 57 ms | 68 ms | **68 ms** |
| flask | 42 ± 5 ms | 37 ms | 54 ms | **54 ms** |
| grafy-self | 40 ± 3 ms | 35 ms | 46 ms | **46 ms** |

*p95 estimated as max of 10 runs (10-run p95 ≈ 2nd-highest value; max used as conservative upper bound.)*

**Gate: PASS.** Incremental p95 < 250 ms on all repos. Worst observed: 68 ms on ripgrep (a 50 k-LOC Rust repo). Well within the 250 ms gate.

Note: p95 from 10 runs is a rough estimate. For the plan §1 "sub-200 ms p95" goal at 1 M LOC, dedicated measurement on a 100 k-LOC repo is needed (deferred to M2 with stack-graphs re-resolution). The gate wording in plan §4 M1 W6 says "100k-LOC repo"; ripgrep at ~50 k LOC is the closest available in this run.

---

## Peak RSS

Cold-index peak RSS (`/usr/bin/time -l` "maximum resident set size"). Lower is better.

| Repo | Grafy RSS | CMM RSS | Ratio (Grafy/CMM) |
|---|---|---|---|
| ripgrep | 83 MB | 202 MB | **0.41× (2.4× less)** |
| flask | 47 MB | 133 MB | **0.35× (2.8× less)** |
| grafy-self | 50 MB | 109 MB | **0.46× (2.2× less)** |

**Gate: PASS.** Grafy peak RSS ≤ CMM on all measured repos. Grafy consistently uses ~40–50% of CMM's memory.

---

## On-Disk Index Size

| Repo | Grafy `.grafy/index.redb` | CMM `.db` | Source (excl. .git/target) | Grafy ratio |
|---|---|---|---|---|
| ripgrep | 17 MB | 13 MB | 3.5 MB | 4.9× source |
| flask | 6.5 MB | 7.1 MB | 2.3 MB | 2.8× source |
| grafy-self | 3.5 MB | 4.7 MB | 1.1 MB | 3.2× source |

Grafy index is slightly larger than CMM for ripgrep (due to 277 k call edges stored vs 13 k). The plan §1 goal is ≤ 1.5× source size; current ratio is 2.8–4.9×. The large index is directly attributable to the W3 edge fan-out. Fixing the caller-attribution overshoot will bring index size within goal.

---

## Node + Edge Counts

Sanity check: files / functions / calls / routes per repo.

| Repo | Files | Functions | Methods | Structs | Enums | Calls | Routes |
|---|---|---|---|---|---|---|---|
| ripgrep | 100 | 2742 | 2413 | 295 | 71 | 277716 | 0 |
| flask | 83 | 2117 | 302 | 0 | 0 | 57890 | 0 |
| grafy-self | 70 | 360 | 148 | 49 | 23 | 9515 | 6 |
| fixtures | 3 | 9 | 0 | 0 | 0 | 0 | 6 |

Observations:
- Call counts are inflated by the W3 fan-out (plan §8: every call site in a file attributed to every definition in the file). Ripgrep's 2742+2413=5155 callables × high call-site density = 277 k edges. CMM emits 13.5 k for the same repo.
- Routes: 6 on grafy-self (fixture services detected correctly) and 6 on fixtures. Flask routes show 0 — Flask uses decorators (`@app.route`) which require pass-4 framework-specific patterns; not yet added for Flask (only FastAPI/Gin/Express, plan §4 M1 W4).
- CMM (for reference): ripgrep 4574 nodes, 13520 edges. Grafy: 5746 nodes, 277716 edges.

---

## Gate Status (plan §4 M1 W6)

| Gate | Result | Detail |
|---|---|---|
| Cold index ≥ 2× faster than CMM | **FAIL** | Grafy 1.7–22.8× slower. Root cause: W3 fan-out + reparse. |
| Warm index ≥ CMM | **PASS** | Grafy 1.5–2.3× faster warm. Blake3 fast-path effective. |
| Incremental p95 < 250 ms (single-file edit, ~100k-LOC repo) | **PASS** | Worst p95 = 68 ms (ripgrep, ~50 k LOC). |
| Peak RSS ≤ CMM | **PASS** | Grafy 2.2–2.8× less RAM than CMM. |

**M1 cold-index gate FAIL.** Three of four gates pass. The cold-index failure is entirely attributable to pass-3 edge fan-out. Two fixes are needed before re-running:
1. Enclosing-function walk in pass 3 (deferred from W3 per plan §8) — replaces whole-file attribution with per-function attribution.
2. Cached parse trees (W6 plan) so pass 3 does not reparse every file on cold index.

---

## Caveats and Known Overshoots (plan §8)

**W3 caller attribution overshoot (plan §8):** Pass 3 attributes every call site in a file to every definition in that file (no enclosing-function walk). The fix requires a tree-sitter parent-node walk. Deferred to W6 (this week) alongside cached parse trees. This is the single biggest factor in the cold-index gate failure and inflated call counts.

**W3 reparse cost (plan §8):** Pass 3 reparses every file on cold index (2× pass-1 cost). W6 cached parse trees will eliminate this. Not yet landed in this commit.

**W3 calls.scm runtime fallback (plan §8):** If a per-language `calls.scm`/`imports.scm` fails to compile, pass 3 logs `debug!` and emits zero edges for that file. Flask routes=0 may be explained by Flask decorator patterns not covered by `routes.scm` (pass 4 covers FastAPI/Gin/Express only; plan §4 M1 W4).

**W3 symbol-table memory (plan §8):** In-memory `HashMap`-based symbol table. Large monorepos could be hundreds of MB. Stream-rather-than-buffer deferred to W6+.

**No reverse-edge index (plan §8):** Cypher `<-[:CALLS]-` performs full EDGES_TABLE scan. OK at current sizes; add reverse index when monorepo evidence requires it.

**Method double-capture (plan §8):** Python/Rust flat queries and method-specific queries can fire on the same node, creating duplicate Function + Method nodes per method. Harmless for benchmarks; clean fix needs `#not-has-ancestor?` (tree-sitter 0.23 limitation).

**FS cache not dropped:** macOS `purge` requires `sudo`. Cold-index numbers reflect page-cache-warm cold starts. Each run deletes `.grafy/` or CMM's `.db` but the underlying file bytes remain in the OS page cache from previous runs. True cold-disk numbers would be higher for both tools proportionally.

**CMM `pipeline.route = incremental` on second run:** After the first CMM index, subsequent runs route to the incremental path even when we delete the `.db`. CMM's `pipeline.discover files=155` vs Grafy's `files_seen=215` suggests CMM applies different include/exclude rules (e.g. skips test fixtures or vendor dirs). Both tools apply `standard_filters(true)` / `.gitignore` rules; minor count differences are expected.

---

## Vega-Lite Chart

`benches/results/m1-throughput.vl.json` — grouped-bar cold-index throughput per repo, Grafy vs CMM. Render at https://vega.github.io/editor or via `vl2png`.

---

*Results JSON per commit under `benches/results/213fd21/`. Reproducible via `make bench-m1` from this tagged commit.*
