//! `scip-f1`: F1 differ binary. Plan §4 M2 week 1 (positional) + week 3
//! (edge-pair).
//!
//! Two modes:
//! - **W1 positional mode** (`--tool <scip>`): compare two `.scip` files at
//!   per-occurrence `(file, line, col)` resolution. Used by `m2-w1.sh`.
//! - **W3 edge-pair mode** (`--grafy-store <redb>` + `--include-edges
//!   <filter>`): read grafy's redb store directly and compare its
//!   `(caller, callee)` edge set against the SCIP ground truth's
//!   `(enclosing_def, ref_symbol)` set. Used by `m2-w3.sh`.
//!
//! Exactly one of `--tool` / `--grafy-store` must be provided.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::Parser;
use grafy_bench::grafy_store::{load_edge_pairs, EdgeKindFilter};
use grafy_bench::scip_f1::{compute_edge_pair_f1, compute_f1, load_index};

#[derive(Parser, Debug)]
#[command(
    name = "scip-f1",
    about = "Compute precision/recall/F1 over SCIP references or grafy edges"
)]
struct Args {
    /// Ground-truth SCIP file (e.g. scip-python output).
    #[arg(long)]
    ground_truth: PathBuf,
    /// W1 mode: tool SCIP file (e.g. stack-graphs synthetic SCIP from sg-to-scip).
    #[arg(long, conflicts_with = "grafy_store")]
    tool: Option<PathBuf>,
    /// W3 mode: path to a grafy `.grafy/index.redb` store. When present, the
    /// differ computes edge-pair F1 instead of positional F1.
    #[arg(long, conflicts_with = "tool")]
    grafy_store: Option<PathBuf>,
    /// W3 mode: which edge kinds to include from the grafy store.
    /// One of: `calls`, `scip`, `calls,scip`. Required when --grafy-store is set.
    #[arg(long, requires = "grafy_store")]
    include_edges: Option<String>,
    /// Language tag for the JSON output (`python`, `typescript`, ...).
    #[arg(long)]
    lang: String,
    /// Repo tag (e.g. `pallets/flask`).
    #[arg(long, default_value = "unknown")]
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

    let json = match (&args.tool, &args.grafy_store) {
        (Some(tool), None) => {
            // W1 positional mode.
            let tool_idx = load_index(tool)?;
            let r = compute_f1(&args.lang, &args.repo, &args.sha, &gt, &tool_idx);
            serde_json::to_string_pretty(&r)?
        }
        (None, Some(store)) => {
            // W3 edge-pair mode.
            let include = args
                .include_edges
                .as_deref()
                .ok_or_else(|| anyhow!("--include-edges required with --grafy-store"))?;
            let filter = EdgeKindFilter::parse(include)?;
            let tool_pairs = load_edge_pairs(store, filter)?;
            let r =
                compute_edge_pair_f1(&args.lang, &args.repo, &args.sha, include, &gt, &tool_pairs);
            serde_json::to_string_pretty(&r)?
        }
        (None, None) => {
            return Err(anyhow!(
                "must pass exactly one of --tool <scip> or --grafy-store <redb>"
            ));
        }
        (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
    };

    match args.out {
        Some(p) => std::fs::write(&p, json)?,
        None => println!("{json}"),
    }
    Ok(())
}
