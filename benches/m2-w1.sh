#!/usr/bin/env bash
# M2 W1: subprocess F1 baseline for stack-graphs vs per-language SCIP indexers.
# Plan §4 M2 week 1, §6 F1 gate (≥ 0.85).
#
# For each language in {python, typescript, javascript, java}:
#   1. Clone corpus repo at default-branch HEAD.
#   2. Run per-language SCIP indexer → ground-truth `.scip`.
#   3. Run sg-to-scip (drives `tree-sitter-stack-graphs-<lang>` CLI):
#        a. Rewrite ground-truth symbols to positional form.
#        b. Index corpus with stack-graphs.
#        c. Resolve each reference position via batched `query definition`.
#        d. Emit synthetic SCIP whose symbols mirror the rewritten gt.
#   4. Run scip-f1 → per-repo JSON in benches/results/m2-w1/.
#
# Time budget per language: 30 minutes. Set LANG_TIMEOUT_SECS to override.
#
# Stop conditions:
#   - SCIP indexer or its build-tool dep (mvn, npm) not installed → skip language.
#   - stack-graphs CLI not installed → skip language.
#   - Resolve loop exceeds RESOLVE_BUDGET_SECS → mark partial.
#
# Path note: the corpus path passed to sg-to-scip must be the dir that, when
# joined with a SCIP `Document.relative_path`, yields the absolute source path.
# scip-typescript emits paths relative to the *project root* (incl. the `src/`
# prefix when sources live under src/), so the corpus arg should be the
# project root, not `<root>/src`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR="/tmp/grafy-m2"
RESULTS_DIR="$REPO_ROOT/benches/results/m2-w1"
RESOLVE_BUDGET_SECS="${RESOLVE_BUDGET_SECS:-900}"
MAX_POSITIONS="${MAX_POSITIONS:-0}"

SCIP_F1="$REPO_ROOT/target/release/scip-f1"
SG_TO_SCIP="$REPO_ROOT/target/release/sg-to-scip"

mkdir -p "$WORK_DIR/work" "$RESULTS_DIR"

echo "[m2-w1] WORK_DIR=$WORK_DIR"
echo "[m2-w1] RESULTS_DIR=$RESULTS_DIR"
echo "[m2-w1] RESOLVE_BUDGET_SECS=$RESOLVE_BUDGET_SECS"
echo "[m2-w1] MAX_POSITIONS=$MAX_POSITIONS"

if [ ! -x "$SCIP_F1" ] || [ ! -x "$SG_TO_SCIP" ]; then
  echo "[m2-w1] building release binaries..."
  cargo build --release -p grafy-bench --bins 2>&1 | tail -3
fi

clone_repo() {
  local name="$1" url="$2"
  local dest="$WORK_DIR/$name"
  if [ -d "$dest/.git" ]; then
    echo "[m2-w1] reusing $dest" >&2
  else
    echo "[m2-w1] cloning $name from $url..." >&2
    git clone --depth 1 "$url" "$dest" >&2
  fi
  git -C "$dest" rev-parse HEAD
}

# Args: $1=lang $2=repo_name $3=corpus_subdir(relative) $4=gt_cmd_template
# gt_cmd_template runs inside repo_dir; "$gt_file" is appended as final arg.
run_lang_repo() {
  local lang="$1" repo_name="$2" corpus_sub="$3" gt_cmd="$4"
  local repo_dir="$WORK_DIR/$repo_name"
  local corpus="$repo_dir"
  [ -n "$corpus_sub" ] && corpus="$repo_dir/$corpus_sub"

  local sha
  sha=$(git -C "$repo_dir" rev-parse HEAD)

  local gt_file="$WORK_DIR/work/$repo_name-gt.scip"
  local rewritten_gt="$WORK_DIR/work/$repo_name-gt-rewritten.scip"
  local tool_file="$WORK_DIR/work/$repo_name-tool.scip"
  local db_file="$WORK_DIR/work/$repo_name.sg.db"
  local result_file="$RESULTS_DIR/$lang-$repo_name.json"

  echo ""
  echo "[m2-w1] ===== $lang : $repo_name ====="
  echo "[m2-w1] sha=$sha corpus=$corpus"

  if [ ! -s "$gt_file" ]; then
    echo "[m2-w1] ground-truth indexer: $gt_cmd $gt_file"
    if ! ( cd "$repo_dir" && eval "$gt_cmd \"$gt_file\"" ) 2>&1 | tail -3 ; then
      echo "[m2-w1] ground truth FAILED for $repo_name"
      return 1
    fi
  fi
  [ -s "$gt_file" ] || { echo "[m2-w1] empty ground truth — abort"; return 1; }

  if ! "$SG_TO_SCIP" \
      --lang "$lang" \
      --corpus "$corpus" \
      --ground-truth "$gt_file" \
      --tool-out "$tool_file" \
      --rewritten-gt-out "$rewritten_gt" \
      --db "$db_file" \
      --max-file-secs 5 \
      --resolve-budget-secs "$RESOLVE_BUDGET_SECS" \
      --max-positions "$MAX_POSITIONS"; then
    echo "[m2-w1] sg-to-scip FAILED"
    return 1
  fi

  "$SCIP_F1" \
    --ground-truth "$rewritten_gt" \
    --tool "$tool_file" \
    --lang "$lang" \
    --repo "$repo_name" \
    --sha "$sha" \
    --out "$result_file"
  echo "[m2-w1] wrote $result_file"
  cat "$result_file"
  echo ""
}

# --- Python : pallets/flask -----------------------------------------------
if command -v scip-python &>/dev/null && command -v tree-sitter-stack-graphs-python &>/dev/null; then
  if [ ! -d "$WORK_DIR/flask/.git" ]; then
    if [ -d "/tmp/flask/.git" ]; then
      ln -sfn /tmp/flask "$WORK_DIR/flask"
    else
      clone_repo flask https://github.com/pallets/flask > /dev/null
    fi
  fi
  # scip-python emits paths relative to src/flask when --target-only is set.
  run_lang_repo python flask "src/flask" \
    "scip-python index --project-name flask --project-version w1 --target-only src/flask --quiet --output" \
    || echo "[m2-w1] python/flask FAILED"
else
  echo "[m2-w1] SKIP python — scip-python or tree-sitter-stack-graphs-python missing"
fi

# --- TypeScript : microsoft/vscode-textmate -------------------------------
if command -v scip-typescript &>/dev/null && command -v tree-sitter-stack-graphs-typescript &>/dev/null; then
  clone_repo vscode-textmate https://github.com/microsoft/vscode-textmate > /dev/null || true
  if [ ! -d "$WORK_DIR/vscode-textmate/node_modules" ]; then
    ( cd "$WORK_DIR/vscode-textmate" && npm install --no-audit --no-fund ) 2>&1 | tail -2 || true
  fi
  # scip-typescript paths include `src/` — corpus must be the project root.
  run_lang_repo typescript vscode-textmate "" \
    "scip-typescript index --output" \
    || echo "[m2-w1] typescript/vscode-textmate FAILED"
else
  echo "[m2-w1] SKIP typescript — scip-typescript or tree-sitter-stack-graphs-typescript missing"
fi

# --- JavaScript : lodash/lodash -------------------------------------------
if command -v scip-typescript &>/dev/null && command -v tree-sitter-stack-graphs-javascript &>/dev/null; then
  clone_repo lodash https://github.com/lodash/lodash > /dev/null || true
  run_lang_repo javascript lodash "" \
    "scip-typescript index --infer-tsconfig --output" \
    || echo "[m2-w1] javascript/lodash FAILED"
else
  echo "[m2-w1] SKIP javascript — scip-typescript or tree-sitter-stack-graphs-javascript missing"
fi

# --- Java : apache/commons-lang -------------------------------------------
# scip-java requires `mvn` on PATH. We check for it explicitly.
if command -v scip-java &>/dev/null \
    && command -v tree-sitter-stack-graphs-java &>/dev/null \
    && command -v mvn &>/dev/null; then
  clone_repo commons-lang https://github.com/apache/commons-lang > /dev/null || true
  run_lang_repo java commons-lang "src/main/java" \
    "scip-java index --output" \
    || echo "[m2-w1] java/commons-lang FAILED"
else
  echo "[m2-w1] SKIP java — scip-java, tree-sitter-stack-graphs-java, or mvn missing"
fi

echo ""
echo "[m2-w1] Done. Results in $RESULTS_DIR"
ls -la "$RESULTS_DIR" 2>&1
