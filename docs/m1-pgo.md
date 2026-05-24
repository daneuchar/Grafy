# PGO (Profile-Guided Optimisation) for Grafy

## What it is

PGO is a three-phase compiler technique:

1. **Instrument** — build a binary that records branch frequencies and call counts at runtime.
2. **Profile** — run the instrumented binary on representative workloads (ripgrep, flask, grafy-self).
3. **Optimise** — rebuild the release binary using the collected profile to guide inlining, branch layout, and register allocation.

Expected speedup: 5–15 % on CPU-bound pipeline work (tree-sitter query execution, rayon dispatch). Actual gain depends on how well the training corpus represents production workloads.

## Requirements

- `rustc` stable 1.88+ (stable PGO has been stable since 1.46).
- `llvm-profdata` version matching rustc's LLVM. The `make pgo` target probes:
  1. `/opt/homebrew/opt/llvm/bin/llvm-profdata` (homebrew LLVM, version-matched to rustc)
  2. `/Library/Developer/CommandLineTools/usr/bin/llvm-profdata` (Apple CLT, minor version may differ)
- `/tmp/ripgrep` — `git clone https://github.com/BurntSushi/ripgrep /tmp/ripgrep`
- `/tmp/flask` — `git clone https://github.com/pallets/flask /tmp/flask`

## How to run

```bash
make pgo
```

This runs all three phases in sequence and overwrites `target/release/grafy` with the PGO-optimised binary. The standard `make ci` / `make test` pipeline continues to use the standard `[profile.release]` build and is unaffected.

### Individual phases

```bash
make pgo-instrument   # phase 1 only — produces target/release-pgo/grafy
make pgo-collect      # phase 2 only — runs corpus, merges profiles to /tmp/grafy-pgo.profdata
make pgo-link         # phase 3 only — rebuilds target/release/grafy with profile-use
```

## Cargo.toml profile

`[profile.release-pgo]` inherits from `[profile.release]` but sets `strip = false` so the instrumentation runtime can emit `.profraw` files. The final PGO binary is built under `[profile.release]` (with `strip = "symbols"`) so the deployed binary is the same size as a non-PGO release.

## When to refresh the profile

Refresh when:
- Passes 1–4 have significant algorithmic changes (new query patterns, new language families).
- The corpus grows substantially (new languages or repo size doubles).
- rustc or LLVM toolchain is upgraded (profiles from a different LLVM major version are rejected by `-Cprofile-use`).

Rule of thumb: refresh every M-level milestone boundary (M1 → M2 → M3).

## Benchmarking PGO vs standard release

Build both, bench separately:

```bash
# Standard release
cargo build --release
hyperfine --warmup 1 --runs 10 \
  --prepare 'rm -rf /tmp/ripgrep/.grafy' \
  'target/release/grafy index /tmp/ripgrep'

# PGO release (overwrites target/release/grafy)
make pgo
hyperfine --warmup 1 --runs 10 \
  --prepare 'rm -rf /tmp/ripgrep/.grafy' \
  'target/release/grafy index /tmp/ripgrep'
```

Keep both sets of numbers in `benches/m1-report.md` so the PGO trade-off (longer build time vs runtime gain) is visible.

## Known limitations

- PGO profiles are machine-specific (Apple M1 Pro profiles should not be committed to CI).
- `-Cllvm-args=-pgo-warn-missing-function` emits warnings for functions never called during profiling. These are informational and do not block the build.
- macOS arm64 only: Linux x86-64 may need a different `LLVM_PROFDATA` path. Override with `make pgo LLVM_PROFDATA=/path/to/llvm-profdata`.
