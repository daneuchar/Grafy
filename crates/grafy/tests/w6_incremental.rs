//! M1 W6 incremental reindex tests (plan §4).
//!
//! Fixture: tempdir with Rust + Python + Go files.
//! Each test exercises one incremental scenario.

use std::thread;
use std::time::Duration;

use grafy::pipeline::Pipeline;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn wait_mtime() {
    // On HFS+ / APFS the mtime resolution is 1 second on some kernels.
    // Sleep 1 ms is enough for most filesystems; tests that specifically need
    // a fresh mtime use this.
    thread::sleep(Duration::from_millis(10));
}

fn make_repo(dir: &tempfile::TempDir) {
    std::fs::write(dir.path().join("lib.rs"), "fn alpha() {}\nstruct Beta;\n").unwrap();
    std::fs::write(
        dir.path().join("main.py"),
        "def gamma():\n    pass\nclass Delta:\n    pass\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("server.go"),
        "package main\nfunc Epsilon() {}\n",
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Test 1: cold index produces non-zero counts.
// ---------------------------------------------------------------------------

#[test]
fn cold_index_counts_all_files() {
    let dir = tempfile::tempdir().unwrap();
    make_repo(&dir);
    let report = Pipeline::new(dir.path()).index().expect("cold index");
    assert_eq!(
        report.new_files, 3,
        "expected 3 new files, got {}",
        report.new_files
    );
    assert_eq!(
        report.unchanged, 0,
        "cold run should have 0 unchanged, got {}",
        report.unchanged
    );
    assert!(report.total_nodes() > 0, "expected nodes");
}

// ---------------------------------------------------------------------------
// Test 2: mtime-only change → Unchanged (blake3 same).
// ---------------------------------------------------------------------------

#[test]
fn touch_only_no_content_change_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    make_repo(&dir);

    // First run: cold.
    Pipeline::new(dir.path()).index().expect("cold");

    // Touch lib.rs by rewriting the same bytes (mtime will change, hash same).
    wait_mtime();
    let content = std::fs::read(dir.path().join("lib.rs")).unwrap();
    std::fs::write(dir.path().join("lib.rs"), &content).unwrap();

    // Second run: incremental.
    let report = Pipeline::new(dir.path()).index().expect("warm");

    // All three files should be Unchanged (content identical).
    assert_eq!(
        report.unchanged, 3,
        "expected 3 unchanged files after touch-only, got {}",
        report.unchanged
    );
    assert_eq!(
        report.modified, 0,
        "expected 0 modified, got {}",
        report.modified
    );
    assert_eq!(
        report.new_files, 0,
        "expected 0 new, got {}",
        report.new_files
    );
}

// ---------------------------------------------------------------------------
// Test 3: content change → modified file nodes replaced, other files untouched.
// ---------------------------------------------------------------------------

#[test]
fn content_change_replaces_nodes_for_modified_file() {
    let dir = tempfile::tempdir().unwrap();
    make_repo(&dir);

    // Cold run.
    let cold = Pipeline::new(dir.path()).index().expect("cold");
    let cold_functions = cold.functions;

    // Modify lib.rs — add a new function.
    wait_mtime();
    std::fs::write(
        dir.path().join("lib.rs"),
        "fn alpha() {}\nstruct Beta;\nfn gamma_new() {}\n",
    )
    .unwrap();

    // Warm run.
    let warm = Pipeline::new(dir.path()).index().expect("warm");

    assert_eq!(
        warm.modified, 1,
        "expected 1 modified file, got {}",
        warm.modified
    );
    assert_eq!(
        warm.unchanged, 2,
        "expected 2 unchanged files, got {}",
        warm.unchanged
    );
    // The added function should appear.
    assert!(
        warm.functions > cold_functions,
        "function count should grow after add: cold={cold_functions} warm={}",
        warm.functions
    );
}

// ---------------------------------------------------------------------------
// Test 4: delete a file → its nodes removed from store.
// ---------------------------------------------------------------------------

#[test]
fn deleted_file_nodes_removed() {
    let dir = tempfile::tempdir().unwrap();
    make_repo(&dir);

    // Cold run.
    let cold = Pipeline::new(dir.path()).index().expect("cold");
    let cold_files = cold.files;

    // Delete server.go.
    std::fs::remove_file(dir.path().join("server.go")).unwrap();

    // Warm run.
    let warm = Pipeline::new(dir.path())
        .index()
        .expect("warm after delete");

    assert_eq!(warm.deleted, 1, "expected 1 deleted, got {}", warm.deleted);
    // File node count should drop.
    assert!(
        warm.files < cold_files,
        "file node count should decrease: cold={cold_files} warm={}",
        warm.files
    );
}

// ---------------------------------------------------------------------------
// Test 5: add a new file → New count = 1.
// ---------------------------------------------------------------------------

#[test]
fn new_file_increments_new_count() {
    let dir = tempfile::tempdir().unwrap();
    make_repo(&dir);

    // Cold run.
    Pipeline::new(dir.path()).index().expect("cold");

    // Add a new file.
    std::fs::write(dir.path().join("extra.rs"), "fn extra_fn() {}\n").unwrap();

    // Warm run.
    let warm = Pipeline::new(dir.path()).index().expect("warm after add");

    assert_eq!(
        warm.new_files, 1,
        "expected 1 new file, got {}",
        warm.new_files
    );
    assert_eq!(
        warm.unchanged, 3,
        "original 3 files should be unchanged, got {}",
        warm.unchanged
    );
}

// ---------------------------------------------------------------------------
// Test 6: rebuild flag forces full reindex (no unchanged).
// ---------------------------------------------------------------------------

#[test]
fn rebuild_flag_forces_full_reindex() {
    let dir = tempfile::tempdir().unwrap();
    make_repo(&dir);

    // Cold run.
    Pipeline::new(dir.path()).index().expect("cold");

    // Rebuild run — all files should appear as new (no unchanged short-circuit).
    let report = Pipeline::new(dir.path()).index_rebuild().expect("rebuild");

    assert_eq!(
        report.unchanged, 0,
        "rebuild must have 0 unchanged, got {}",
        report.unchanged
    );
}

// ---------------------------------------------------------------------------
// Test 7: store secondary index consistency — nodes_by_file populated.
// ---------------------------------------------------------------------------

#[test]
fn nodes_by_file_index_populated() {
    use grafy::store::{Store, NODES_BY_FILE_TABLE};
    use redb::ReadableTable;

    let dir = tempfile::tempdir().unwrap();
    make_repo(&dir);

    Pipeline::new(dir.path()).index().expect("index");

    let store = Store::open(dir.path()).unwrap();
    let db = store.read_db();
    let tx = db.begin_read().unwrap();
    let tbl = tx.open_table(NODES_BY_FILE_TABLE).unwrap();
    let count: usize = tbl.iter().unwrap().count();
    assert!(count > 0, "nodes_by_file should have entries after index");
}
