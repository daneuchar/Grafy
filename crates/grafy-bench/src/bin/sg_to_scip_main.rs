//! `sg-to-scip`: drive `tree-sitter-stack-graphs-<lang>` and emit a synthetic
//! SCIP file whose occurrences carry resolved-definition positions in their
//! symbols. Plan §4 M2 week 1.
//!
//! Pipeline:
//!   1. Read the ground-truth SCIP (from scip-python/scip-typescript/…).
//!   2. Rewrite its symbols to `sg-resolved … :line:col` so the F1 differ
//!      can compare positionally (see `sg_to_scip::rewrite_ground_truth_symbols`).
//!   3. Enumerate reference positions from the rewritten ground truth.
//!   4. Run `tree-sitter-stack-graphs-<lang> index <corpus>` to populate the db.
//!   5. For each position, call `query definition`; collect resolved positions.
//!   6. Emit a synthetic SCIP file whose occurrences encode the resolutions.
//!   7. Also emit a rewritten ground-truth SCIP so the F1 differ compares
//!      apples-to-apples.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use grafy_bench::scip_f1::load_index;
use grafy_bench::sg_to_scip::{
    build_synthetic_index, reference_positions, resolve_all, rewrite_ground_truth_symbols,
    run_index, LangAdapter,
};
use scip::types::Index;

#[derive(Parser, Debug)]
#[command(name = "sg-to-scip")]
struct Args {
    /// One of: python, typescript, javascript, java.
    #[arg(long)]
    lang: String,
    /// Corpus root (absolute path).
    #[arg(long)]
    corpus: PathBuf,
    /// Ground-truth SCIP file (from scip-python/scip-typescript/scip-java).
    #[arg(long)]
    ground_truth: PathBuf,
    /// Output: synthetic SCIP from stack-graphs.
    #[arg(long)]
    tool_out: PathBuf,
    /// Output: rewritten ground-truth SCIP (positional symbols).
    #[arg(long)]
    rewritten_gt_out: PathBuf,
    /// Stack-graphs db path. Created if absent; reused if present.
    #[arg(long)]
    db: PathBuf,
    /// Skip index step if db already exists.
    #[arg(long)]
    skip_index: bool,
    /// Stack-graphs per-file timeout, seconds.
    #[arg(long, default_value_t = 5)]
    max_file_secs: u64,
    /// Total wall budget (seconds) for the per-position resolve loop.
    #[arg(long, default_value_t = 600)]
    resolve_budget_secs: u64,
    /// Cap on positions to resolve (debug / smoke runs). 0 = no cap.
    #[arg(long, default_value_t = 0)]
    max_positions: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let adapter = match args.lang.as_str() {
        "python" => LangAdapter::PYTHON,
        "typescript" => LangAdapter::TYPESCRIPT,
        "javascript" => LangAdapter::JAVASCRIPT,
        "java" => LangAdapter::JAVA,
        other => bail!("unsupported lang: {other}"),
    };

    eprintln!(
        "[sg-to-scip] loading ground truth: {}",
        args.ground_truth.display()
    );
    let gt = load_index(&args.ground_truth)?;
    eprintln!("[sg-to-scip] ground-truth docs: {}", gt.documents.len());

    let rewritten = rewrite_ground_truth_symbols(&gt);
    write_index(&args.rewritten_gt_out, &rewritten)?;

    let mut positions = reference_positions(&rewritten);
    eprintln!("[sg-to-scip] reference positions: {}", positions.len());
    if args.max_positions > 0 && positions.len() > args.max_positions {
        positions.truncate(args.max_positions);
        eprintln!("[sg-to-scip] capped to {} positions", positions.len());
    }

    if !args.skip_index || !args.db.exists() {
        eprintln!("[sg-to-scip] indexing corpus with {}", adapter.bin);
        let d = run_index(&adapter, &args.corpus, &args.db, args.max_file_secs)?;
        eprintln!("[sg-to-scip] index wall time: {d:?}");
    } else {
        eprintln!("[sg-to-scip] reusing existing db at {}", args.db.display());
    }

    let (resolutions, resolve_wall) = resolve_all(
        &adapter,
        &args.db,
        &args.corpus,
        &positions,
        Duration::from_secs(args.resolve_budget_secs),
    )?;
    eprintln!("[sg-to-scip] resolve wall time: {resolve_wall:?}");

    let synthetic = build_synthetic_index(&resolutions);
    write_index(&args.tool_out, &synthetic)?;

    eprintln!(
        "[sg-to-scip] wrote synthetic SCIP: {} ({} docs)",
        args.tool_out.display(),
        synthetic.documents.len()
    );
    Ok(())
}

fn write_index(path: &PathBuf, index: &Index) -> Result<()> {
    let mut buf = Vec::new();
    // The `scip` crate's `Index` uses protobuf serialization via the
    // protobuf crate; we use its `write_to_vec` for compactness.
    use protobuf::Message;
    index.write_to_vec(&mut buf)?;
    let mut f = File::create(path)?;
    f.write_all(&buf)?;
    Ok(())
}
