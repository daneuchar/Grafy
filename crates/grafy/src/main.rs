//! `grafy` CLI. M1 commands: `index`, `diagnose`, `query`, `mcp`.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use grafy::pipeline::{to_dot, Pipeline};
use grafy::store::Store;

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
    ///
    /// By default, unchanged files (blake3 hash match) are skipped for faster
    /// incremental reindexing. Use `--rebuild` to force a full reindex.
    Index {
        path: PathBuf,
        /// Force a full reindex, ignoring cached hashes.
        #[arg(long)]
        rebuild: bool,
    },
    /// Print per-phase timings and node-kind counts for `path`.
    Diagnose { path: PathBuf },
    /// Execute a read-only Cypher-Lite query against an indexed repository.
    ///
    /// Opens `<path>/.grafy/index.redb`, runs the query, and prints rows as
    /// JSON Lines to stdout (one per line). On error prints to stderr and exits 2.
    Query {
        /// Path to the indexed repository root (must contain `.grafy/index.redb`).
        path: PathBuf,
        /// Cypher-Lite query string (read-only, plan §5 subset).
        cypher: String,
    },
    /// Start the MCP stdio server (plan §4 M1 W5).
    Mcp {
        /// Repository root to serve. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Validate the 14 tool registrations and exit 0 without entering the stdio loop.
        #[arg(long)]
        check: bool,
    },
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("grafy=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_target(true)
                .compact()
                .with_writer(std::io::stderr),
        )
        .init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // For `mcp` without --check, we must NOT call init_tracing before the
    // tokio runtime is running and before stdio is owned by rmcp. For all
    // other commands we initialise tracing eagerly.
    match &cli.cmd {
        Cmd::Mcp { check: false, .. } => {}
        _ => init_tracing(),
    }

    match cli.cmd {
        Cmd::Index { path, rebuild } => {
            let pipe = Pipeline::new(&path);
            let report = if rebuild {
                pipe.index_rebuild()?
            } else {
                pipe.index()?
            };
            // Graphviz .dot to stdout.
            println!("{}", to_dot(&report, &path));
            // Summary to stderr (doesn't pollute .dot piped to graphviz).
            eprintln!(
                "files={} modules={} functions={} classes={} structs={} enums={} traits={} methods={} calls={} routes={} unchanged={} modified={} new={} deleted={}",
                report.files,
                report.modules,
                report.functions,
                report.classes,
                report.structs,
                report.enums,
                report.traits,
                report.methods,
                report.calls,
                report.routes,
                report.unchanged,
                report.modified,
                report.new_files,
                report.deleted,
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
                calls = report.calls,
                routes = report.routes,
                "diagnose complete"
            );
            eprintln!(
                "grafy diagnose: total={:?}  files={} modules={} functions={} classes={} structs={} enums={} traits={} methods={} calls={} routes={}",
                elapsed,
                report.files,
                report.modules,
                report.functions,
                report.classes,
                report.structs,
                report.enums,
                report.traits,
                report.methods,
                report.calls,
                report.routes,
            );
        }
        Cmd::Query { path, cypher } => {
            let store = match Store::open(&path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ERROR: {e}");
                    std::process::exit(2);
                }
            };
            match grafy::cypher::execute(store.read_db(), &cypher) {
                Ok(rows) => {
                    for row in rows {
                        println!(
                            "{}",
                            serde_json::to_string(&row)
                                .unwrap_or_else(|e| { format!("{{\"error\": \"{e}\"}}") })
                        );
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(2);
                }
            }
        }
        Cmd::Mcp { root, check } => {
            if check {
                // init_tracing() was already called above (Mcp { check: true } hits the `_` arm)
                return grafy::mcp::server::check();
            }

            // Canonicalise the root path before handing it to the async runtime.
            let root = root.canonicalize().unwrap_or(root);

            // Build and run the tokio runtime that owns the MCP stdio loop.
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(async {
                    // Init tracing now that we're in the async context.
                    // Tracing goes to stderr; stdio belongs to rmcp.
                    let filter = EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new("grafy=info"));
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(
                            fmt::layer()
                                .with_target(true)
                                .compact()
                                .with_writer(std::io::stderr),
                        )
                        .init();

                    grafy::mcp::server::serve(root).await
                })?;
        }
    }
    Ok(())
}
