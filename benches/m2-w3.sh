#!/usr/bin/env bash
# M2 W3 — django F1 gate verification (plan §6).
#
# 1. Ensure /tmp/django (depth=1 clone) exists.
# 2. Run scip-python end-to-end → /tmp/django.scip (15-min wall budget).
#    If that times out: fall back to flask (already at /tmp/flask).
# 3. Index the corpus with grafy twice:
#      a. GRAFY_SCIP_DISABLE=1 — heuristic-only baseline.
#      b. Default — heuristic + SCIP ingest (auto-detects scip-python).
# 4. Run scip-f1 in W3 edge-pair mode against the ground truth, once per
#    grafy store + edge filter combination:
#      - heuristic-only:    --grafy-store .../h/index.redb --include-edges calls
#      - heuristic + SCIP:  --grafy-store .../s/index.redb --include-edges calls,scip
# 5. Drop both JSON files in benches/results/m2-w3/.
#
# `timeout` is not available on macOS by default. We fork scip-python into
# the background and kill the PID if it overruns. Exit codes:
#   0 — django F1 ran end-to-end.
#   2 — django scip-python timed out; flask fallback wrote results instead.
#   1 — something irrecoverable (clone failed, indexer missing, etc.).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$REPO_ROOT/benches/results/m2-w3"
WORK_DIR="${WORK_DIR:-/tmp/grafy-m2-w3}"
DJANGO_DIR="${DJANGO_DIR:-/tmp/django}"
FLASK_DIR="${FLASK_DIR:-/tmp/flask}"
SCIP_BUDGET_SECS="${SCIP_BUDGET_SECS:-900}"

GRAFY_BIN="$REPO_ROOT/target/release/grafy"
SCIP_F1="$REPO_ROOT/target/release/scip-f1"

mkdir -p "$WORK_DIR" "$RESULTS_DIR"

if [ ! -x "$GRAFY_BIN" ] || [ ! -x "$SCIP_F1" ]; then
  echo "[m2-w3] building release binaries…"
  cargo build --release -p grafy -p grafy-bench --bins
fi

if ! command -v scip-python >/dev/null; then
  echo "[m2-w3] scip-python not on PATH; abort"
  exit 1
fi

ensure_corpus() {
  local dir="$1" url="$2"
  if [ ! -d "$dir/.git" ]; then
    echo "[m2-w3] cloning $url into $dir…"
    git clone --depth 1 "$url" "$dir" >/dev/null
  fi
  git -C "$dir" rev-parse HEAD
}

# Run scip-python with a wall budget enforced via background-PID + sleep.
# Returns 0 on success, 124 on timeout (mirrors `timeout` exit code).
run_scip_python() {
  local corpus_dir="$1" out_file="$2" project_name="$3" budget="$4"
  rm -f "$out_file"
  (
    cd "$corpus_dir"
    scip-python index --output "$out_file" --project-name "$project_name" .
  ) &
  local pid=$!
  local elapsed=0
  while kill -0 "$pid" 2>/dev/null; do
    sleep 5
    elapsed=$((elapsed + 5))
    if [ "$elapsed" -ge "$budget" ]; then
      echo "[m2-w3] scip-python exceeded ${budget}s — killing PID $pid"
      kill -9 "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      return 124
    fi
  done
  wait "$pid"
}

run_f1_pair() {
  local repo="$1" sha="$2" gt="$3" corpus_dir="$4" prefix="$5"

  local h_dir="$WORK_DIR/$repo-h"
  local s_dir="$WORK_DIR/$repo-s"
  rm -rf "$h_dir" "$s_dir"
  mkdir -p "$h_dir" "$s_dir"
  # Shadow-copy corpus into work dirs (so grafy's .grafy/ doesn't pollute corpus).
  # rsync mirror, exclude .git for speed.
  rsync -a --delete --exclude '.git' "$corpus_dir"/ "$h_dir"/
  rsync -a --delete --exclude '.git' "$corpus_dir"/ "$s_dir"/

  echo "[m2-w3] indexing $repo (heuristic-only)…"
  /usr/bin/time -p env GRAFY_SCIP_DISABLE=1 "$GRAFY_BIN" index "$h_dir" 2>&1 | tail -5
  echo "[m2-w3] indexing $repo (heuristic + SCIP)…"
  /usr/bin/time -p "$GRAFY_BIN" index "$s_dir" 2>&1 | tail -5

  echo "[m2-w3] computing F1 — heuristic only"
  "$SCIP_F1" \
    --lang python --repo "$repo" --sha "$sha" \
    --ground-truth "$gt" \
    --grafy-store "$h_dir/.grafy/index.redb" \
    --include-edges calls \
    --out "$RESULTS_DIR/${prefix}-heuristic-only.json"

  echo "[m2-w3] computing F1 — heuristic + SCIP"
  "$SCIP_F1" \
    --lang python --repo "$repo" --sha "$sha" \
    --ground-truth "$gt" \
    --grafy-store "$s_dir/.grafy/index.redb" \
    --include-edges calls,scip \
    --out "$RESULTS_DIR/${prefix}-with-scip.json"

  echo
  echo "== $prefix heuristic-only =="
  cat "$RESULTS_DIR/${prefix}-heuristic-only.json" | head -25
  echo
  echo "== $prefix heuristic+SCIP =="
  cat "$RESULTS_DIR/${prefix}-with-scip.json" | head -25
}

main_django() {
  local sha
  sha=$(ensure_corpus "$DJANGO_DIR" https://github.com/django/django)
  echo "[m2-w3] django sha=$sha"

  echo "[m2-w3] running scip-python on django (budget=${SCIP_BUDGET_SECS}s)…"
  if ! run_scip_python "$DJANGO_DIR" /tmp/django.scip django "$SCIP_BUDGET_SECS"; then
    echo "[m2-w3] django scip-python timed out — falling back to flask"
    main_flask
    return 2
  fi

  run_f1_pair django "$sha" /tmp/django.scip "$DJANGO_DIR" django
}

main_flask() {
  if [ ! -d "$FLASK_DIR/.git" ]; then
    ensure_corpus "$FLASK_DIR" https://github.com/pallets/flask >/dev/null
  fi
  local sha
  sha=$(git -C "$FLASK_DIR" rev-parse HEAD)
  echo "[m2-w3] flask sha=$sha"

  echo "[m2-w3] running scip-python on flask…"
  if ! run_scip_python "$FLASK_DIR" /tmp/flask.scip flask 600; then
    echo "[m2-w3] flask scip-python timed out (unexpected)"
    exit 1
  fi

  # scip-python on flask emits paths from the project root (e.g.
  # `src/flask/app.py`). grafy walks the same root, so paths align.
  run_f1_pair flask "$sha" /tmp/flask.scip "$FLASK_DIR" flask
}

case "${1:-django}" in
  django) main_django ;;
  flask) main_flask ;;
  both)
    set +e
    main_django; django_status=$?
    set -e
    main_flask
    if [ "$django_status" = 2 ]; then
      echo "[m2-w3] django timed out; both results have flask only"
      exit 2
    fi
    ;;
  *) echo "usage: $0 {django|flask|both}"; exit 1 ;;
esac
