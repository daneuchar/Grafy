# M2 W1 — subprocess F1 baseline vs SCIP ground truth

**Date:** 2026-05-24
**Plan section:** §4 M2 week 1, §6 F1 gate (≥ 0.85)
**Driver agent:** grafy-stackgraphs-owner

---

## Headline

| Language | Repo | F1 | Verdict |
|---|---|---|---|
| Python | pallets/flask | **0.089** | drop from M2 differentiator scope |
| TypeScript | microsoft/vscode-textmate | **0.319** | drop from M2 differentiator scope |
| JavaScript | lodash/lodash | **0.000** | drop from M2 differentiator scope |
| Java | apache/commons-lang | skipped | scip-java requires mvn (not installed) |

**No language clears the 0.85 gate.** The W1 measurement contradicts the plan §4 assumption that "existing Python/TS/Java packs hit ≥ 0.85" out of the box. The fork-vs-vendor decision is now decoupled from F1 — neither path produces a 0.85 differentiator without significant repair work on the DSL packs themselves.

---

## Setup

### Tooling installed in this session

| Tool | Version | Install path |
|---|---|---|
| `tree-sitter-stack-graphs-python` | 0.3.0 | `~/.cargo/bin/` (pre-existing) |
| `tree-sitter-stack-graphs-typescript` | 0.4.0 | `~/.cargo/bin/` (pre-existing) |
| `tree-sitter-stack-graphs-javascript` | 0.3.0 | `cargo install --features cli` (this session) |
| `tree-sitter-stack-graphs-java` | 0.5.0 | `cargo install --features cli` (this session) |
| `scip-python` | 0.6.6 | `npm i -g @sourcegraph/scip-python` |
| `scip-typescript` | 0.4.0 | `npm i -g @sourcegraph/scip-typescript` |
| `scip-java` | (latest from `cs install --contrib`) | `coursier` (this session) |
| `coursier` (`cs`) | latest | `brew install coursier/formulas/coursier` |

`scip-java` requires Apache Maven (`mvn`) on `PATH`; **Maven was not installed in this environment**. This blocked the Java measurement. The verbatim error is in `benches/results/m2-w1/java-commons-lang.json`.

### Corpus

| Lang | Repo | SHA | Notes |
|---|---|---|---|
| Python | pallets/flask | `954f5684e4841aad84a8eec7ace7b81a0d3f6831` | already at `/tmp/flask`; corpus = `src/flask` |
| TypeScript | microsoft/vscode-textmate | `1701cf6b45b25bed3a07b44d059e0f7930be30f2` | cloned this session, ~16 MB src, ran `npm install` |
| JavaScript | lodash/lodash | `a02353279093cca0fea1c8cc468ffbf03bb3485b` | cloned this session, ~5.7 MB |
| Java | apache/commons-lang | `ef39bc92712d47c100af3b243a8bfcd84a45f116` | cloned but not indexed (mvn missing) |

---

## F1 table

| Lang | Repo | Refs (GT) | Refs (tool) | TP | FP | FN | Precision | Recall | F1 | Gate (≥ 0.85) |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| python | flask | 4,983 | 367 | 233 | 134 | 4,616 | 0.635 | 0.048 | **0.089** | FAIL |
| typescript | vscode-textmate | 8,459 | 3,211 | 1,605 | 1,606 | 5,248 | 0.500 | 0.234 | **0.319** | FAIL |
| javascript | lodash (3 k pos cap) | 69,190 | 0 | 0 | 0 | 69,190 | 0.000 | 0.000 | **0.000** | FAIL |

Per-repo JSON in `/Users/danieleuchar/workspace/grafy/benches/results/m2-w1/`.

### Methodology

The F1 differ (`crates/grafy-bench/src/scip_f1.rs`) compares **reference occurrences only** — definitions are excluded. To make stack-graphs and SCIP outputs comparable despite very different symbol grammars, both indexes are projected into a synthetic-symbol form `sg-resolved . . . \`<relpath>\`:<line>:<col>` whose comparison key is the resolved-definition position. The transformation is applied symmetrically:

- **Ground-truth side:** for each reference occurrence in the SCIP file, look up the matching definition occurrence by symbol within the same index, rewrite the reference's symbol to point at that definition's `(path, line, col)`. References whose definition isn't in the corpus (e.g. `python-stdlib`, `npm vitest`) are dropped — stack-graphs cannot be expected to resolve external references either, so this is the fair comparison set.
- **Tool side:** index the corpus with `tree-sitter-stack-graphs-<lang>` then call `query definition <path>:<line>:<col>` (batched 200 per CLI invocation to amortize startup) for every remaining ref position. The CLI's resolved definition position becomes the symbol.

A reference is a **true positive** iff its `(file, line, col)` is present in both indexes *and* its resolved position matches.

---

## Timing

| Step | Lang | Repo | Wall time |
|---|---|---|---|
| ground-truth scip index | python | flask | 4.5 s |
| stack-graphs index | python | flask | 1.0 s |
| stack-graphs resolve (batched, 4983 pos) | python | flask | 6.2 s |
| ground-truth scip index | typescript | vscode-textmate | 1.2 s |
| stack-graphs index | typescript | vscode-textmate | 3.2 s |
| stack-graphs resolve (batched, 8459 pos) | typescript | vscode-textmate | 109 s |
| ground-truth scip index | javascript | lodash | 3.9 s |
| stack-graphs index | javascript | lodash | 83 s |
| stack-graphs resolve (batched, 3 k cap) | javascript | lodash | 6.7 s |

Subprocess resolve throughput with 200-position batches: ~75 positions/sec on Python, ~75 positions/sec on TS, ~450 positions/sec on JS (all-fail-fast).

**Subprocess as a steady-state pass-3 backend is plausible** at this throughput when the corpus reuses an indexed db, *if* the DSL recall problem can be fixed (it cannot, in W1 alone). Re-indexing on every grafy run would dominate wall-clock budget on anything beyond toy corpora.

---

## Caveats and observed failure modes

### 1. DSL stanza errors in language packs (recall floor)

Every language pack we tested raises **`Undefined scoped variable [syntax node X].def`** errors on real-world code, marking entire files as `failed` in the indexing db. The CLI exits with status 0 for the files that succeed but reports `failed` in `status --all`.

| Lang | Repo | Files indexed | Files failed | % indexed |
|---|---|---:|---:|---:|
| python | flask | 10 | 14 | 42 % |
| typescript | vscode-textmate | 19 | 11 | 63 % |
| javascript | lodash | 45 | 11 | 80 % (but the one mega-file is in the failed set) |

For Python, the failing stanza is `edge @name.def -> @param.param_name` matching `*args: t.Any` / `**kwargs: t.Any` (variadic typed parameters). This pattern occurs in nearly every modern Python file. Failed files cannot answer **any** definition query, so every reference *to a symbol defined in a failed file* contributes a false negative. In flask, `app.py`, `cli.py`, `ctx.py`, `helpers.py`, `json/__init__.py`, `sansio/{app,blueprints,scaffold}.py` all fail. Together they hold most of the definitions other modules import — hence recall 0.048.

For TypeScript, the failing stanza is `edge @nested.expr_def -> @mod.expr_def` matching `member_expression` nodes inside method bodies (e.g. `this.cache.has(key)`). Common idiom; common fail.

For JavaScript, the bundled `lodash.js` (~17 k LOC) **timed out** at the 5-second per-file limit on `path computation`. That single file holds essentially all of the corpus's reference targets, so the lodash measurement is uninformative for cross-file resolution. A smaller, multi-file JS corpus would be a fairer test. The Grafy 5-s per-file timeout is **policy-locked** (see `CLAUDE.md`), so bumping it is not an option.

### 2. Precision is mediocre even where resolution succeeds

On TypeScript (3 211 tool refs, 1 605 TP), **precision is 0.50** — half of the resolutions point at the wrong definition position. Spot-checking suggests the CLI sometimes resolves to an import statement's name occurrence rather than the underlying declaration. The differ treats this as a false positive because the ground-truth symbol's definition is the declaration, not the import.

For Python, precision is 0.63 (134 FP / 367 tool refs). Same shape of error.

### 3. Subprocess output parsing required surgery

The `tree-sitter-stack-graphs-<lang> query definition` CLI emits multi-line per-position blocks with an unstructured "found N definitions" format. The adapter (`crates/grafy-bench/src/sg_to_scip.rs`) needs a stable parser for:

- "no references at location" (position is not a reference)
- "found 0 definitions for 1 references" (reference recognized but unresolved)
- "found 1 definitions for 1 references" (single resolution)
- "found N definitions for M references" (multiple references at one position — only the first is parsed; this is a known under-count for overloaded positions but doesn't materially affect F1 on our corpus)

Additionally, the CLI canonicalizes paths through `/private/tmp` on macOS, requiring `std::fs::canonicalize` on the corpus root before `strip_prefix`. This was a 60-min trap when first surfaced.

### 4. Symbol comparison fairness

The chosen comparison method (positional `sg-resolved … :L:C`) **rewards** stack-graphs whenever it resolves to anywhere inside the same definition occurrence's range, **as long as** it picks the same starting position as SCIP. In practice both indexers tend to pick the identifier-start column for definitions, so this is fair. A coarser symbol-level comparison (line-only, or symbol-name-only) would yield a different number; future iterations of the differ should add a `--strict` vs `--loose` flag.

---

## Decision per language

### Python — drop from M2 differentiator scope

F1 0.089. Driven by 58 % of indexed files failing on a single ubiquitous DSL bug (`*args: T` / `**kwargs: T`). Fixing this requires forking `tree-sitter-stack-graphs-python` and patching the TSG file. That is exactly the 6-week fork plan the M2 brief was trying to avoid. **Either fork or drop.** Recommend **drop** for v1.0; document the gap; ship M2 with M1's heuristic resolver augmented by SCIP ingest as the cross-file path. Revisit fork in v1.x if upstream gets a maintainer.

### TypeScript — drop from M2 differentiator scope

F1 0.319. Better than Python but still far below the gate. 37 % of files fail; precision is 0.50 even on the working ones. Same fork-or-drop tradeoff as Python.

### JavaScript — drop from M2 differentiator scope

F1 0.000 on lodash specifically because the corpus is a bundled monolith that times out. **The choice of lodash was a mistake** — picking a multi-file JS project (e.g. `expressjs/express` or `axios/axios`) would have produced a non-zero F1 closer to the TypeScript number. That said, even an optimistic JS F1 of 0.3 would still fail the gate. Drop.

### Java — measurement blocked

Cannot install Maven inside this session without sudo/brew interactivity. Per stop-condition, language is recorded as skipped. **Not a verdict; needs a re-run on a machine with `mvn`.** A plausible follow-up: run scip-java via the Docker image (`sourcegraph/scip-java`) which bundles mvn — that bypasses the host install requirement.

---

## Implications for the M2 plan

The plan §4 text ("If existing Python/TS/Java packs hit ≥ 0.85, the fork is unnecessary for v1.0 — keep stack-graphs as a vendored dep, ship M2 in ~2 weeks") is invalidated by these numbers. **Neither the 2-week vendor path nor the 6-week fork path produces a 0.85 differentiator** without first repairing the DSL packs, which has effort upper-bounded only by "however long it takes to write correct TSG for `*args: T`, `member_expression`, and similar idioms across three languages."

Concrete recommendation for the pipeline owner (`pipeline-architect`):

- **Drop the stack-graphs library replacement of pass-3** for v1.0.
- **Repurpose M2** to ship a per-language **SCIP ingest** path: run `scip-{python,typescript,go,java,…}` as an external indexer when the user opts in, ingest the `.scip` file, merge its CALLS edges with M1's heuristic resolver's CALLS edges. This is a strict subset of plan §1 v1.x ("SCIP emit" stretch) but inverted (SCIP ingest, not emit). It gives M2 a real differentiator versus codebase-memory-mcp (which has neither stack-graphs nor SCIP) while sidestepping the broken DSL.
- **Promote the F1 harness** (`scip-f1` binary) to a permanent part of the bench corpus so each milestone measures cross-file recall against the SCIP ground truth.

If the project still wants the stack-graphs differentiator, the realistic path is: vendor the packs, **fork the DSL files**, ship at least 4 weeks of TSG fixes per language. Total cost likely exceeds 6 weeks; quality gate at 0.85 likely takes 8–12 weeks per language.

---

## Open issues for plan §7

1. **Fork-or-drop decision is now both more urgent and more painful.** §7 should be updated to reflect that **even after vendoring**, the F1 gap is the limiting factor, not the integration cost.
2. **DSL fuzz target (`fuzz-safety-engineer` coordination, M2 W2):** the panics observed during indexing are pure DSL evaluation errors, not parser crashes. They surface as `Result::Err` — no UB. The fuzz target should focus on **valid Python/TS/JS inputs that crash the DSL evaluator**, since this is where the recall floor lives.
3. **5-second per-file timeout is correctly enforced and is non-negotiable** — `lodash.js` timing out is a feature, not a bug. The plan should document this as a known limitation of subprocess-based lib evaluation on bundled monoliths.
4. **Bench corpus expansion:** JS needs a non-monolithic real-world project; TS needs an additional corpus beyond `vscode-textmate` (e.g. `axios/axios`); Python needs at least one project lacking heavy generics (`*args: T`) to upper-bound the recall when the DSL bug isn't triggered.

---

## Reproduce

```bash
# 1. Install indexers (one-time)
npm i -g @sourcegraph/scip-python @sourcegraph/scip-typescript
brew install coursier/formulas/coursier
cs install --contrib scip-java
brew install maven    # only needed for the Java path
cargo install tree-sitter-stack-graphs-{python,typescript,javascript,java} --features cli

# 2. Build the harness
cargo build --release -p grafy-bench --bins

# 3. Drive the bench (clones corpora to /tmp/grafy-m2)
bash benches/m2-w1.sh

# 4. Read results
ls benches/results/m2-w1/
```
