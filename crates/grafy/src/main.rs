//! `grafy` CLI. M0 commands: `index`, `diagnose`, `version`.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use grafy::pipeline::{to_dot, Pipeline};

#[derive(Parser, Debug)]
#[command(name = "grafy", version, about = "polyglot, LLM-free code-intelligence engine")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Index a repository and emit Graphviz `.dot` to stdout (M0 stub).
    Index { path: PathBuf },
    /// Print per-phase timings for the structure pass over `path`.
    Diagnose { path: PathBuf },
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("grafy=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true).compact())
        .init();
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Index { path } => {
            let pipe = Pipeline::new(&path);
            let summary = pipe.index()?;
            println!("{}", to_dot(&summary, &path));
        }
        Cmd::Diagnose { path } => {
            let pipe = Pipeline::new(&path);
            let started = Instant::now();
            let summary = pipe.index()?;
            let elapsed = started.elapsed();
            info!(target: "grafy.diagnose",
                  total_ms = elapsed.as_millis() as u64,
                  files_seen = summary.files_seen,
                  files_parsed = summary.files_parsed,
                  files_skipped = summary.files_skipped,
                  files_failed = summary.files_failed,
                  "diagnose complete");
            eprintln!(
                "grafy diagnose: total={:?} seen={} parsed={} skipped={} failed={}",
                elapsed,
                summary.files_seen,
                summary.files_parsed,
                summary.files_skipped,
                summary.files_failed
            );
        }
    }
    Ok(())
}
