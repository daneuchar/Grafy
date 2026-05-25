//! M2 W3 demo fixture — citable precision win for the M2 pitch.
//!
//! Indexes `tests/fixtures/demo/` two ways and asserts the heuristic-only
//! path *misses* the `alert -> send_email` call while the SCIP-augmented
//! path resolves it correctly.
//!
//! Skips when `scip-python` is not installed (so CI on hosts without npm
//! tools stays green).

use std::fs;
use std::path::{Path, PathBuf};

use grafy::store::{EdgeKind, NodeRecord, EDGES_TABLE, NODES_TABLE};
use postcard::from_bytes;
use redb::{Database, ReadableTable};
use tempfile::tempdir;

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

/// Copy the demo fixture tree into `dst`. Walks the fixture dir at runtime
/// rather than embedding so the source files stay reviewable.
fn copy_demo_fixture(dst: &Path) {
    // CARGO_MANIFEST_DIR points at crates/grafy/. The fixture lives at
    // workspace_root/tests/fixtures/demo.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixture = Path::new(manifest_dir)
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("demo");
    copy_dir_recursive(&fixture, dst);
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dst");
    for entry in fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("dir entry");
        let p = entry.path();
        let target = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir_recursive(&p, &target);
        } else {
            fs::copy(&p, &target).expect("copy file");
        }
    }
}

/// Returns the set of `(caller_fqn, callee_fqn, kind)` edges in the redb
/// store. Used by both assertions below.
fn read_edges(db_path: &Path) -> Vec<(String, String, u8)> {
    let db = Database::open(db_path).expect("open redb");
    let tx = db.begin_read().expect("begin read");
    let nodes = tx.open_table(NODES_TABLE).expect("nodes table");
    let edges = tx.open_table(EDGES_TABLE).expect("edges table");

    let mut id_to_fqn: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    for item in nodes.iter().expect("iter nodes") {
        let (k, v) = item.expect("node item");
        if let Ok(rec) = from_bytes::<NodeRecord>(v.value()) {
            id_to_fqn.insert(k.value(), rec.fqn);
        }
    }

    let mut out = Vec::new();
    for item in edges.iter().expect("iter edges") {
        let (k, _v) = item.expect("edge item");
        let (from, to, kind) = k.value();
        let Some(caller) = id_to_fqn.get(&from) else {
            continue;
        };
        let Some(callee) = id_to_fqn.get(&to) else {
            continue;
        };
        out.push((caller.clone(), callee.clone(), kind));
    }
    out
}

/// Heuristic-only path: the demo fixture must produce zero `CALLS` edges
/// from `app.main.alert` to `lib.notify.send_email`. The aliased re-export
/// in `lib/__init__.py` defeats the import-aware resolver.
#[test]
fn m2_demo_heuristic_misses_aliased_call() {
    let dir = tempdir().expect("tempdir");
    copy_demo_fixture(dir.path());

    // Shell out to the release binary so the GRAFY_SCIP_DISABLE env var
    // doesn't race the sibling test's invocation (cargo runs unit tests in
    // parallel and `std::env::set_var` is process-global).
    let exe = release_grafy_bin();
    let status = std::process::Command::new(&exe)
        .args(["index", "."])
        .current_dir(dir.path())
        .env("GRAFY_SCIP_DISABLE", "1")
        .env("GRAFY_LOG_LEVEL", "warn")
        .status()
        .expect("run grafy index");
    assert!(status.success(), "grafy index failed");

    let edges = read_edges(&dir.path().join(".grafy").join("index.redb"));
    let calls: Vec<_> = edges
        .iter()
        .filter(|(_, _, k)| *k == EdgeKind::Calls as u8)
        .collect();
    // SCIP must have been suppressed: no Scip edges in the store.
    let scip: Vec<_> = edges
        .iter()
        .filter(|(_, _, k)| *k == EdgeKind::Scip as u8)
        .collect();
    assert!(
        scip.is_empty(),
        "GRAFY_SCIP_DISABLE didn't suppress SCIP ingest: {scip:?}"
    );

    let bad = calls
        .iter()
        .find(|(caller, callee, _)| caller.ends_with("alert") && callee.contains("send_email"));
    assert!(
        bad.is_none(),
        "heuristic resolved aliased call (fixture is no longer a precision-win demo): {bad:?}"
    );

    // Optional sanity log — what edges *did* the heuristic produce?
    eprintln!("heuristic-only CALLS edges:");
    for (a, b, _) in &calls {
        eprintln!("  {a} -> {b}");
    }
}

/// Path to the grafy release binary. Built once by the cargo test
/// harness via the `env!("CARGO_BIN_EXE_grafy")` reflection, falling back
/// to `target/release/grafy` if the env var isn't set (older cargo).
fn release_grafy_bin() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_grafy") {
        return PathBuf::from(p);
    }
    // Walk up from CARGO_MANIFEST_DIR to find target/release/grafy.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidate = Path::new(manifest_dir)
        .join("..")
        .join("..")
        .join("target")
        .join("release")
        .join("grafy");
    candidate
}

/// SCIP-augmented path: with `scip-python` on PATH, ingest must emit
/// an `EdgeKind::Scip` edge from `alert` to `send_email`.
#[test]
fn m2_demo_scip_resolves_aliased_call() {
    if which("scip-python").is_none() {
        eprintln!("scip-python not installed — skipping M2 demo SCIP assertion");
        return;
    }

    let dir = tempdir().expect("tempdir");
    copy_demo_fixture(dir.path());

    // Shell out so we don't race the heuristic test's env-var mutation.
    let exe = release_grafy_bin();
    let output = std::process::Command::new(&exe)
        .args(["index", "."])
        .current_dir(dir.path())
        .env_remove("GRAFY_SCIP_DISABLE")
        .env("GRAFY_LOG_LEVEL", "warn")
        .output()
        .expect("run grafy index");
    assert!(
        output.status.success(),
        "grafy index failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let edges = read_edges(&dir.path().join(".grafy").join("index.redb"));
    let alert_to_send_email = edges.iter().find(|(caller, callee, kind)| {
        *kind == EdgeKind::Scip as u8 && caller.ends_with("alert") && callee.ends_with("send_email")
    });
    assert!(
        alert_to_send_email.is_some(),
        "missing SCIP edge alert -> send_email; got edges: {:?}",
        edges
            .iter()
            .filter(|(_, _, k)| *k == EdgeKind::Scip as u8)
            .collect::<Vec<_>>()
    );
}
