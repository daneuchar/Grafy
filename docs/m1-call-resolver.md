# M1 heuristic call resolver — spec

Owner: `m1-pipeline-passes`. Drafted W1, implemented W3, replaced for Python/TypeScript/Java in M2 by stack-graphs. Until then, this is what Grafy ships.

## Goal

For every `Function` definition node, identify the set of `Function` definition nodes it likely calls, with no LLM, no full type inference, and no symbol-table walk beyond import-aware lexical scope.

**Engineering gate (plan §4 M1 W3):** `grafy index django` CALLS count within ±10% of codebase-memory-mcp on the same SHA.

## Inputs

1. Output of pass 1 (structure) + pass 2 (definitions): all function/class/method symbols with their FQNs and source ranges.
2. Per-file tree-sitter `Tree` plus the language's `calls.scm` query.
3. Per-file import statements extracted with `imports.scm` (W1 deliverable).

## Output

Edges `(caller_node_id, callee_node_id, EdgeKind::Calls)` written through the pipeline channel to the redb writer.

## Per-language family strategy

Three strategies cover all 12 languages:

### Family A — Python, TypeScript, JavaScript, TSX, PHP, Lua (dynamic/lexical)

1. Run `calls.scm` to find call-site identifiers (`foo()`, `obj.method()`, `Class.method()`).
2. Resolve in this order, first match wins:
   1. **Local scope** — match against names bound by the enclosing function / closure (parameters, `let`/`const`/`var`/`def` inside the function body).
   2. **Module scope** — match against top-level definitions in the same file.
   3. **Import scope** — match `import` / `from x import y` / `require('x')` / `use x` to a known module FQN; resolve into that module's exported symbols if the module is in the index.
   4. **Type-inferred receiver** (TS/JS only) — if call is `obj.method()` and `obj` is `let obj: T = …`, treat the call as `T.method`. No flow-sensitive inference.
3. Unresolved calls → don't emit an edge. (False negative ≪ false positive.)

### Family B — Rust, Go, Scala, Java, C#, C++ (lexical with explicit types)

1. Run `calls.scm` to find call-site identifiers.
2. Resolve via:
   1. **Local + module scope** (same as Family A 2.1–2.2).
   2. **Use / import / using / namespace** — explicit imports map identifiers to FQNs.
   3. **Method receiver type** — if call is `x.method()`, use the declared type of `x`. For Rust, this means the function-signature type; for Go, the method-set receiver; for Java/C#/Scala/C++, the declared type. No generics inference.
   4. **Trait/interface dispatch** — for Rust traits, Go interfaces, Java/Scala interfaces: emit an edge to each impl/satisfier in the index. (Overshoots when traits are large; documented limitation.)
3. Unresolved → no edge.

### Family C — Pass-through (none in v1.0)

Reserved for future languages where the heuristic doesn't fit.

## Per-language deliverables for W3

| Language | `calls.scm` | `imports.scm` | Family |
|---|---|---|---|
| Python | yes | yes | A |
| TypeScript | yes | yes | A |
| TSX | shared with TS | shared with TS | A |
| JavaScript | yes | yes | A |
| PHP | yes | yes | A |
| Lua | yes | yes | A |
| Rust | yes | yes | B |
| Go | yes | yes | B |
| Java | yes | yes | B |
| C# | yes | yes | B |
| Scala | yes | yes | B |
| C++ | yes | yes | B |

## What this resolver does NOT do (documented gaps)

- No flow-sensitive type inference.
- No generic instantiation tracking.
- No reflection / metaprogramming (`getattr`, `__import__`, dynamic `eval`).
- No cross-language FFI (pyo3, napi, gRPC). Stretch goal v1.x.
- No virtual call resolution beyond trait/interface impl enumeration.

These gaps are the moat M2 fills with stack-graphs for Python / TypeScript / Java.

## Quality criteria

- Edge precision: prefer false negatives over false positives. A missed call is a gap; a wrong call is a bug.
- Determinism: same input → same edge set. No reliance on filesystem walk order.
- Timeout: respect the 5-second per-file budget; if resolution exceeds it, emit no edges for that file and log a `warn!` with the next-step action.

## Test plan

1. Per-language fixtures under `tests/fixtures/calls/<lang>/`. Small (≤ 20 LOC) hand-written cases for each resolution rule (local, module, import, type-receiver, trait dispatch).
2. Snapshot tests via `insta` against golden edge sets.
3. End-to-end on django (Python) and the TypeScript compiler (TS) — ±10% vs codebase-memory-mcp on the W3 gate corpus.

## What changes in M2

Family A's Python + TypeScript paths are replaced by `grafy-stackgraphs` binding-precise resolution (plan §4 M2). Family B's Java path also. The heuristic stays as fallback for Family B's other languages + Family A's PHP/Lua/JS.
