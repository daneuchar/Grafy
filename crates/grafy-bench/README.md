# grafy-bench

Benchmark harness for [Grafy](https://github.com/daneuchar/Grafy). criterion + hyperfine drivers over a frozen-SHA corpus (plan §6).

**Status:** internal harness, pre-M0 placeholder (v0.1.0). Not a general-purpose library — published only to reserve the name and document the harness.

## What it does

- Parser-pool scaling bench (single vs `rayon::par_iter`).
- M1+: cold/warm/incremental index timings on `ripgrep`, `flask`, `django`, `TypeScript`, `kubernetes`, `dubbo`, `repowise`.
- M2+: SCIP F1 harness vs `scip-python` / `scip-typescript` / `scip-java`.

## License

MIT OR Apache-2.0.
