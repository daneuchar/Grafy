//! M1 W6 synthetic incremental benchmark (plan §4 M1 gate).
//!
//! Creates a 1000-file Rust tempdir, measures:
//!   - Cold index time.
//!   - Warm reindex time (mtime touch, content unchanged).
//!   - Modified reindex time (single file content change), p50 + p95 over 10 runs.
//!
//! Prints: `m1-w6 incremental: cold=Xms warm=Yms modified-p95=Zms`
//! Exits 0 always.

use std::io::Write as IoWrite;
use std::time::Instant;

use grafy::pipeline::Pipeline;

const FILE_COUNT: usize = 1000;
const MODIFIED_RUNS: usize = 10;

fn main() {
    // Silence tracing — we only want the bench numbers on stdout.
    // RUST_LOG can override if desired.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("off")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // ------------------------------------------------------------------
    // Generate 1000 Rust files with distinct content.
    // ------------------------------------------------------------------
    eprint!("generating {FILE_COUNT} files... ");
    std::io::stderr().flush().ok();
    for i in 0..FILE_COUNT {
        let content = format!(
            "// synthetic file {i}\npub fn func_{i}() -> u64 {{ {i} }}\npub struct Struct_{i};\n"
        );
        std::fs::write(root.join(format!("f{i:04}.rs")), content).unwrap();
    }
    eprintln!("done");

    // ------------------------------------------------------------------
    // Cold index.
    // ------------------------------------------------------------------
    let cold_start = Instant::now();
    Pipeline::new(root).index().expect("cold index");
    let cold_ms = cold_start.elapsed().as_millis() as u64;

    // ------------------------------------------------------------------
    // Warm reindex: touch one file's mtime, content unchanged (5 runs, take median).
    // ------------------------------------------------------------------
    let target = root.join("f0500.rs");
    let original_bytes = std::fs::read(&target).unwrap();

    let mut warm_times: Vec<u64> = Vec::with_capacity(5);
    for _ in 0..5 {
        // Rewrite same bytes → mtime changes, hash identical.
        std::fs::write(&target, &original_bytes).unwrap();
        let t = Instant::now();
        Pipeline::new(root).index().expect("warm index");
        warm_times.push(t.elapsed().as_millis() as u64);
    }
    warm_times.sort_unstable();
    let warm_median_ms = warm_times[warm_times.len() / 2];

    // ------------------------------------------------------------------
    // Modified reindex: change one file's content, p50 + p95 over 10 runs.
    // ------------------------------------------------------------------
    let mut modified_times: Vec<u64> = Vec::with_capacity(MODIFIED_RUNS);
    for run in 0..MODIFIED_RUNS {
        // Change the content slightly each run to ensure Modified classification.
        let new_content = format!(
            "// modified run {run}\npub fn func_modified_{run}() -> u64 {{ {run} }}\npub struct Modified_{run};\n"
        );
        std::fs::write(&target, new_content).unwrap();
        let t = Instant::now();
        Pipeline::new(root).index().expect("modified index");
        modified_times.push(t.elapsed().as_millis() as u64);
    }
    modified_times.sort_unstable();
    let modified_p50_ms = modified_times[MODIFIED_RUNS / 2];
    let modified_p95_ms = modified_times[(MODIFIED_RUNS * 95) / 100];

    println!(
        "m1-w6 incremental: cold={cold_ms}ms warm={warm_median_ms}ms modified-p95={modified_p95_ms}ms"
    );
    eprintln!(
        "  cold={cold_ms}ms  warm-median={warm_median_ms}ms  modified-p50={modified_p50_ms}ms  modified-p95={modified_p95_ms}ms"
    );

    // Gate check (informational — does not fail the process).
    if modified_p95_ms > 250 {
        eprintln!("  WARN: modified-p95={modified_p95_ms}ms exceeds M1 gate of 250ms");
    } else {
        eprintln!("  OK: modified-p95={modified_p95_ms}ms <= 250ms gate");
    }
}
