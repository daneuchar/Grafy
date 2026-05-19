---
description: Run criterion benches + report deltas vs last results JSON.
allowed-tools: Bash(cargo bench:*), Bash(hyperfine:*), Read, Bash(ls:*)
---

1. Run `cargo bench -p grafy-bench`.
2. Compare new results in `benches/results/` vs the previous run.
3. Report regressions ≥5% as a table.

Do not commit results. Do not write under `benches/results/local/`.
