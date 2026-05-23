#!/usr/bin/env bash
# M1 W6 benchmark script. Plan §6.
# Usage: bash benches/run_m1.sh
# Prereqs: hyperfine, git, npm (for codebase-memory-mcp), cargo build --release
#
# macOS notes:
#   - `purge` requires sudo on macOS 14+; this script does NOT call sudo.
#   - Cold-cache simulation: each grafy cold run deletes .grafy/ before the run.
#     Each CMM cold run deletes the per-project .db in ~/.cache/codebase-memory-mcp/.
#     Without a true FS-cache flush, the OS page cache will retain recently-read
#     files. Results are therefore "cold warm-FS" rather than true cold-disk.
#     For true cold-disk numbers on macOS: sudo purge between each run.
#
# On Linux: replace `sync && purge` with `sync && echo 3 | sudo tee /proc/sys/vm/drop_caches`

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GRAFY="$REPO_ROOT/target/release/grafy"
BENCH_DIR="/tmp/grafy-bench"
RESULTS_DIR="$BENCH_DIR/results"
RUNS=10
WARMUP=1

# ------------------------------------------------------------------
# Ensure grafy release binary is fresh
# ------------------------------------------------------------------
echo "[bench] building grafy release..."
cargo build --release -p grafy --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1 | tail -3

GRAFY_SHA=$(git -C "$REPO_ROOT" rev-parse HEAD)
COMMIT_RESULTS="$REPO_ROOT/benches/results/${GRAFY_SHA:0:7}"
mkdir -p "$BENCH_DIR" "$RESULTS_DIR" "$COMMIT_RESULTS"

echo "[bench] grafy SHA: $GRAFY_SHA"
echo "[bench] results dir: $RESULTS_DIR"

# ------------------------------------------------------------------
# Ensure hyperfine is installed
# ------------------------------------------------------------------
if ! command -v hyperfine &>/dev/null; then
  echo "[bench] ERROR: hyperfine not found. Install with: brew install hyperfine"
  exit 1
fi

# ------------------------------------------------------------------
# Ensure codebase-memory-mcp is installed (attempt once)
# ------------------------------------------------------------------
CMM_AVAILABLE=0
if command -v codebase-memory-mcp &>/dev/null; then
  CMM_AVAILABLE=1
  CMM_VERSION=$(codebase-memory-mcp --version 2>/dev/null | head -1 || echo "unknown")
  echo "[bench] codebase-memory-mcp: $CMM_VERSION"
else
  echo "[bench] codebase-memory-mcp not found; attempting npm install -g..."
  if npm install -g codebase-memory-mcp 2>/dev/null; then
    CMM_AVAILABLE=1
    CMM_VERSION=$(codebase-memory-mcp --version 2>/dev/null | head -1 || echo "unknown")
    echo "[bench] codebase-memory-mcp installed: $CMM_VERSION"
  else
    echo "[bench] WARNING: codebase-memory-mcp install failed. CMM comparison unavailable."
    CMM_VERSION="unavailable"
  fi
fi

CMM_CACHE_DIR="$HOME/.cache/codebase-memory-mcp"

# ------------------------------------------------------------------
# Helper: derive CMM cache key from repo path
# e.g. /tmp/grafy-bench/ripgrep -> tmp-grafy-bench-ripgrep
# ------------------------------------------------------------------
cmm_cache_key() {
  echo "$1" | sed 's|^/||; s|/|-|g'
}

# ------------------------------------------------------------------
# Helper: run a single repo benchmark
# Args: $1=repo_name $2=repo_path $3=touched_file (for incremental)
# ------------------------------------------------------------------
bench_repo() {
  local name="$1"
  local path="$2"
  local touched_file="$3"

  echo ""
  echo "[bench] ===== $name ====="

  # -- Grafy cold --
  echo "[bench] grafy cold ($RUNS runs)..."
  hyperfine \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --prepare "rm -rf '$path/.grafy'" \
    --export-json "$RESULTS_DIR/${name}-grafy-cold.json" \
    "'$GRAFY' index '$path'" \
    2>&1 | grep -E "Time|Range|Warning"

  cp "$RESULTS_DIR/${name}-grafy-cold.json" "$COMMIT_RESULTS/" 2>/dev/null || true

  # -- Grafy warm --
  echo "[bench] grafy warm ($RUNS runs)..."
  rm -rf "$path/.grafy"
  "$GRAFY" index "$path" > /dev/null 2>&1 || true
  hyperfine \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --export-json "$RESULTS_DIR/${name}-grafy-warm.json" \
    "'$GRAFY' index '$path'" \
    2>&1 | grep -E "Time|Range|Warning"
  cp "$RESULTS_DIR/${name}-grafy-warm.json" "$COMMIT_RESULTS/" 2>/dev/null || true

  # -- Grafy incremental --
  if [ -n "$touched_file" ] && [ -f "$touched_file" ]; then
    echo "[bench] grafy incremental ($RUNS runs, touching $touched_file)..."
    hyperfine \
      --warmup "$WARMUP" \
      --runs "$RUNS" \
      --prepare "touch '$touched_file'" \
      --export-json "$RESULTS_DIR/${name}-grafy-incremental.json" \
      "'$GRAFY' index '$path'" \
      2>&1 | grep -E "Time|Range|Warning"
    cp "$RESULTS_DIR/${name}-grafy-incremental.json" "$COMMIT_RESULTS/" 2>/dev/null || true
  fi

  # -- Grafy peak RSS (cold) --
  echo "[bench] grafy cold RSS..."
  rm -rf "$path/.grafy"
  GRAFY_RSS=$({ /usr/bin/time -l "$GRAFY" index "$path" > /dev/null; } 2>&1 | grep "maximum resident" | awk '{print $1}')
  echo "[bench] grafy RSS: $GRAFY_RSS bytes ($(( GRAFY_RSS / 1024 / 1024 )) MB)"

  # -- CMM (if available) --
  if [ "$CMM_AVAILABLE" -eq 1 ]; then
    local cmm_key
    cmm_key=$(cmm_cache_key "$path")

    echo "[bench] codebase-memory-mcp cold ($RUNS runs)..."
    hyperfine \
      --warmup "$WARMUP" \
      --runs "$RUNS" \
      --prepare "rm -f '$CMM_CACHE_DIR/${cmm_key}.db'" \
      --export-json "$RESULTS_DIR/${name}-cmm-cold.json" \
      "codebase-memory-mcp cli index_repository '{\"repo_path\": \"$path\"}'" \
      2>&1 | grep -E "Time|Range|Warning"
    cp "$RESULTS_DIR/${name}-cmm-cold.json" "$COMMIT_RESULTS/" 2>/dev/null || true

    echo "[bench] codebase-memory-mcp warm ($RUNS runs)..."
    codebase-memory-mcp cli index_repository "{\"repo_path\": \"$path\"}" > /dev/null 2>&1 || true
    hyperfine \
      --warmup "$WARMUP" \
      --runs "$RUNS" \
      --export-json "$RESULTS_DIR/${name}-cmm-warm.json" \
      "codebase-memory-mcp cli index_repository '{\"repo_path\": \"$path\"}'" \
      2>&1 | grep -E "Time|Range|Warning"
    cp "$RESULTS_DIR/${name}-cmm-warm.json" "$COMMIT_RESULTS/" 2>/dev/null || true

    echo "[bench] codebase-memory-mcp cold RSS..."
    rm -f "$CMM_CACHE_DIR/${cmm_key}.db"
    CMM_RSS=$({ /usr/bin/time -l codebase-memory-mcp cli index_repository "{\"repo_path\": \"$path\"}" > /dev/null; } 2>&1 | grep "maximum resident" | awk '{print $1}')
    echo "[bench] codebase-memory-mcp RSS: $CMM_RSS bytes ($(( CMM_RSS / 1024 / 1024 )) MB)"
  else
    echo "[bench] SKIP: codebase-memory-mcp unavailable for $name"
  fi
}

# ------------------------------------------------------------------
# Clone helper: depth-1 clone at pinned SHA
# ------------------------------------------------------------------
clone_repo() {
  local name="$1"
  local url="$2"
  local sha="$3"
  local dest="$BENCH_DIR/$name"

  if [ -d "$dest/.git" ]; then
    local current
    current=$(git -C "$dest" rev-parse HEAD 2>/dev/null || echo "")
    if [ "$current" = "$sha" ]; then
      echo "[bench] $name already cloned at correct SHA"
      return
    fi
    echo "[bench] $name SHA mismatch — recloning"
    rm -rf "$dest"
  fi

  echo "[bench] cloning $name at $sha..."
  git clone --depth 1 "$url" "$dest" 2>&1 | tail -2
  # If shallow clone doesn't pin to exact SHA, try fetching
  local actual
  actual=$(git -C "$dest" rev-parse HEAD 2>/dev/null || echo "")
  if [ "$actual" != "$sha" ]; then
    echo "[bench] NOTE: shallow clone HEAD is $actual (expected $sha) — pinned SHA may differ from tip"
  fi
}

# ------------------------------------------------------------------
# Required repos (must measure)
# ------------------------------------------------------------------

# ripgrep
clone_repo "ripgrep" "https://github.com/BurntSushi/ripgrep" "4519153e5e461527f4bca45b042fff45c4ec6fb9"
bench_repo "ripgrep" "$BENCH_DIR/ripgrep" "$BENCH_DIR/ripgrep/crates/core/main.rs"

# flask
clone_repo "flask" "https://github.com/pallets/flask" "954f5684e4841aad84a8eec7ace7b81a0d3f6831"
bench_repo "flask" "$BENCH_DIR/flask" "$BENCH_DIR/flask/src/flask/app.py"

# grafy self (copy to avoid locking the workspace .grafy)
if [ ! -d "$BENCH_DIR/grafy-self/Cargo.toml" ]; then
  echo "[bench] copying grafy repo to $BENCH_DIR/grafy-self..."
  rsync -a --exclude='.grafy' --exclude='target' "$REPO_ROOT/" "$BENCH_DIR/grafy-self/"
fi
bench_repo "grafy-self" "$BENCH_DIR/grafy-self" "$BENCH_DIR/grafy-self/crates/grafy/src/main.rs"

# fixture services
bench_repo "fixtures" "$REPO_ROOT/tests/fixtures/routes" "$REPO_ROOT/tests/fixtures/routes/fastapi-svc/main.py"

# ------------------------------------------------------------------
# Optional: django (only if time permits — skip with SKIP_DJANGO=1)
# ------------------------------------------------------------------
if [ "${SKIP_DJANGO:-0}" = "0" ]; then
  echo ""
  echo "[bench] attempting optional django clone (set SKIP_DJANGO=1 to skip)..."
  DJANGO_SHA="a53e02af6d7a07d0367b534f7107a0e2a37e3d34"
  START_T=$SECONDS
  clone_repo "django" "https://github.com/django/django" "$DJANGO_SHA" 2>&1
  ELAPSED=$(( SECONDS - START_T ))
  if [ -d "$BENCH_DIR/django/.git" ] && [ "$ELAPSED" -lt 300 ]; then
    bench_repo "django" "$BENCH_DIR/django" "$BENCH_DIR/django/django/core/management/__init__.py"
  else
    echo "[bench] SKIP django: clone took too long ($ELAPSED s)"
  fi
fi

# ------------------------------------------------------------------
# Deferred (note in report)
# ------------------------------------------------------------------
echo ""
echo "[bench] Deferred repos (TypeScript, kubernetes, dubbo): dedicated CI workflow only."
echo "[bench] Reason: clone + full index exceeds 30-min session budget per plan §4 M1 scoping."

echo ""
echo "[bench] Done. Results in $RESULTS_DIR and $COMMIT_RESULTS"
echo "[bench] Run 'make bench-m1' to reproduce from a tagged commit."
