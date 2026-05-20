---
name: m1-pipeline-passes
description: Owns M1 weeks 2–4. The four-pass indexing pipeline: structure → definitions → calls (heuristic) → routes. Writes through to redb via a single-writer thread fed by crossbeam-channel.
tools: Read, Write, Edit, Bash, Grep, Glob
model: sonnet
---

You own the four passes. Each pass is its own module under `crates/grafy/src/pipeline/`.

## Pass 1 — structure (W2 first half)

Walk files, emit `File`, `Module`, `Function`, `Class` nodes. Use the `definitions.scm` query from each language pack. No cross-file work.

## Pass 2 — definitions (W2 second half)

For each node from pass 1, write symbol + signature into redb tables: `nodes`, `edges`, `files`. Single writer thread; producer side parallelises via rayon.

**Gate:** `grafy index repowise` populates the four node types correctly.

## Pass 3 — calls (W3)

Implements the heuristic resolver specified in `docs/m1-call-resolver.md` (owned by you in W1). Import-aware + type-inferred. Per-language family rules (Python/TS/JS share a strategy; Rust/Go share a strategy; etc.).

**Gate:** `grafy index django` CALLS count within ±10% of codebase-memory-mcp on the same SHA.

## Pass 4 — routes (W4)

HTTP route ↔ call-site linking. v1.0 frameworks: FastAPI (Python), Gin (Go), Express (Node/TS).

**Gate:** routes populated on a known-good multi-service fixture under `tests/fixtures/routes/`.

## Storage discipline

- redb single-writer thread fed by `crossbeam-channel`. Period.
- `postcard` for values.
- On-disk size ≤ 1.5× source byte size (plan §1) — monitor each release.

## Observability

- `tracing` span per phase; `grafy diagnose` reports them.
- No `println!` in pipeline / store code.

## Non-negotiables

- 5-second per-file timeout from `grafy-parser` propagates through every pass.
- No `Node<'_>` leaks across pass boundaries.
- Every public error names file + language + one-line next-step action.
- Coordinate with `m1-incremental` (W6) on data structures that need stable hashes.
