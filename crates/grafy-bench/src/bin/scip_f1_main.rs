//! `scip-f1`: F1 differ binary. Plan §4 M2 week 1.
//!
//! Reads two SCIP files (ground truth + tool output), applies the symbol
//! normalization documented in `grafy_bench::scip_f1`, and writes a JSON
//! result to `--out` (or stdout when omitted).

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use grafy_bench::scip_f1::{compute_f1, load_index};

#[derive(Parser, Debug)]
#[command(
    name = "scip-f1",
    about = "Compute precision/recall/F1 over SCIP references"
)]
struct Args {
    /// Ground-truth SCIP file (e.g. scip-python output).
    #[arg(long)]
    ground_truth: PathBuf,
    /// Tool SCIP file (e.g. stack-graphs synthetic SCIP from sg-to-scip).
    #[arg(long)]
    tool: PathBuf,
    /// Language tag for the JSON output (`python`, `typescript`, ...).
    #[arg(long)]
    lang: String,
    /// Repo tag (e.g. `pallets/flask`).
    #[arg(long)]
    repo: String,
    /// Commit SHA of the corpus at measurement time.
    #[arg(long, default_value = "unknown")]
    sha: String,
    /// JSON output path. Default: stdout.
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let gt = load_index(&args.ground_truth)?;
    let tool = load_index(&args.tool)?;
    let r = compute_f1(&args.lang, &args.repo, &args.sha, &gt, &tool);
    let json = serde_json::to_string_pretty(&r)?;
    match args.out {
        Some(p) => std::fs::write(&p, json)?,
        None => println!("{json}"),
    }
    Ok(())
}
