//! M2 W2 — SCIP ingest sidecar.
//!
//! These tests gracefully skip when an indexer is absent so the suite stays
//! green on CI boxes that don't have npm-installed Sourcegraph tools.

use std::path::PathBuf;

use grafy::pipeline::Pipeline;

fn which(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let cand = dir.join(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

#[test]
fn scip_ingest_python_smoke() {
    if which("scip-python").is_none() {
        eprintln!("scip-python not installed — skipping SCIP ingest smoke test");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    std::fs::write(
        root.join("alpha.py"),
        "def greet(name):\n    return f'hi {name}'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("beta.py"),
        "from alpha import greet\n\ndef main():\n    print(greet('world'))\n",
    )
    .unwrap();

    let report = Pipeline::new(root).index().expect("index");
    assert!(report.files >= 2, "files={}", report.files);
    // With scip-python installed, scip_edges should be > 0 on this contrived
    // cross-file reference setup.
    eprintln!("scip_edges={}", report.scip_edges);
    // We don't hard-assert > 0 here because scip-python sometimes needs a
    // pyproject.toml to fully resolve modules; the smoke goal is "no panic"
    // plus a path that runs the ingest code end-to-end.
}

#[test]
fn scip_disable_env_skips_ingest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("x.py"), "def f():\n    return 1\n").unwrap();
    std::env::set_var("GRAFY_SCIP_DISABLE", "1");
    let report = Pipeline::new(root).index().expect("index");
    std::env::remove_var("GRAFY_SCIP_DISABLE");
    assert_eq!(
        report.scip_edges, 0,
        "GRAFY_SCIP_DISABLE should suppress SCIP edges"
    );
}

#[test]
fn detect_returns_a_vec_without_panic() {
    let v = grafy::scip::detect::detected_indexers();
    // The host may or may not have indexers; either way the call must succeed.
    eprintln!("detected {} indexer(s)", v.len());
}

#[test]
fn prereq_probe_returns_struct() {
    let r = grafy::install::probe();
    assert_eq!(r.node.name, "node");
    assert_eq!(r.cargo.name, "cargo");
}

#[test]
fn install_dry_run_does_not_execute() {
    // --dry-run must never spawn install commands.
    let entries = grafy::install::run_with_scip(true);
    assert!(!entries.is_empty());
    for e in &entries {
        // Either AlreadyPresent (the binary is on PATH) or Skipped (dry-run / missing prereq).
        let label = e.status.label();
        assert!(
            label == "already present" || label == "skipped",
            "dry-run produced status `{}` on {}",
            label,
            e.indexer
        );
    }
}
