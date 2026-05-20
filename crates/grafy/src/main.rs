//! `grafy` CLI. M1 W2 commands: `index`, `diagnose`, `version`.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use grafy::pipeline::{to_dot, Pipeline};

#[derive(Parser, Debug)]
#[command(
    name = "grafy",
    version,
    about = "polyglot, LLM-free code-intelligence engine"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Index a repository and emit Graphviz `.dot` to stdout.
    Index { path: PathBuf },
    /// Print per-phase timings and node-kind counts for `path`.
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
            let report = pipe.index()?;
            // Graphviz .dot to stdout.
            println!("{}", to_dot(&report, &path));
            // Summary to stderr (doesn't pollute .dot piped to graphviz).
            eprintln!(
                "files={} modules={} functions={} classes={} structs={} enums={} traits={} methods={}",
                report.files,
                report.modules,
                report.functions,
                report.classes,
                report.structs,
                report.enums,
                report.traits,
                report.methods,
            );
        }
        Cmd::Diagnose { path } => {
            let pipe = Pipeline::new(&path);
            let started = Instant::now();
            let report = pipe.index()?;
            let elapsed = started.elapsed();
            info!(
                target: "grafy.diagnose",
                total_ms = elapsed.as_millis() as u64,
                files = report.files,
                modules = report.modules,
                functions = report.functions,
                classes = report.classes,
                structs = report.structs,
                enums = report.enums,
                traits = report.traits,
                methods = report.methods,
                "diagnose complete"
            );
            eprintln!(
                "grafy diagnose: total={:?}  files={} modules={} functions={} classes={} structs={} enums={} traits={} methods={}",
                elapsed,
                report.files,
                report.modules,
                report.functions,
                report.classes,
                report.structs,
                report.enums,
                report.traits,
                report.methods,
            );
        }
    }
    Ok(())
}
