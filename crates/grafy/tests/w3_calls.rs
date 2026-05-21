//! W3 call resolver integration tests.
//!
//! Per-language fixture pairs that exercise the heuristic resolver. Each test:
//!   - Creates a tempdir with two source files.
//!   - Runs `Pipeline::index()`.
//!   - Re-opens the redb edges table and asserts at least one `EdgeKind::Calls`
//!     edge exists.
//!
//! Languages covered: Rust (Family B), Python (Family A), TypeScript (Family A),
//! Go (Family B).

use std::fs;

use grafy::pipeline::Pipeline;
use grafy::store::{EdgeKind, EDGES_TABLE};
use redb::{Database, ReadableTable};
use tempfile::tempdir;

fn count_call_edges(db_path: &std::path::Path) -> u64 {
    let db = Database::open(db_path).expect("open redb");
    let tx = db.begin_read().expect("begin read");
    let tbl = tx.open_table(EDGES_TABLE).expect("open edges table");
    let mut count = 0u64;
    for item in tbl.iter().expect("iter edges") {
        let (key, _val) = item.expect("edge item");
        let (_from, _to, kind) = key.value();
        if kind == EdgeKind::Calls as u8 {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Rust (Family B) — two-file test: a::foo called from b::bar
// ---------------------------------------------------------------------------

#[test]
fn w3_rust_cross_file_call() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // File a.rs: defines foo
    fs::write(
        root.join("a.rs"),
        r#"pub fn foo() -> u32 { 42 }
"#,
    )
    .expect("write a.rs");

    // File b.rs: calls foo — uses `use` import so import resolver can match
    fs::write(
        root.join("b.rs"),
        r#"use crate::a::foo;

pub fn bar() -> u32 {
    foo()
}
"#,
    )
    .expect("write b.rs");

    let report = Pipeline::new(root).index().expect("index");

    let db_path = root.join(".grafy").join("index.redb");
    assert!(db_path.exists(), ".grafy/index.redb must exist");

    let call_count = count_call_edges(&db_path);
    assert!(
        call_count >= 1,
        "expected ≥1 call edge for Rust cross-file call, got {call_count}. Report={report:?}"
    );
}

// ---------------------------------------------------------------------------
// Python (Family A) — two-file test: from a import foo; foo()
// ---------------------------------------------------------------------------

#[test]
fn w3_python_import_call() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // a.py: defines foo
    fs::write(
        root.join("a.py"),
        r#"def foo():
    return 42
"#,
    )
    .expect("write a.py");

    // b.py: imports and calls foo
    fs::write(
        root.join("b.py"),
        r#"from a import foo

def bar():
    return foo()
"#,
    )
    .expect("write b.py");

    let report = Pipeline::new(root).index().expect("index");

    let db_path = root.join(".grafy").join("index.redb");
    let call_count = count_call_edges(&db_path);
    assert!(
        call_count >= 1,
        "expected ≥1 call edge for Python import call, got {call_count}. Report={report:?}"
    );
}

// ---------------------------------------------------------------------------
// TypeScript (Family A) — two-file test: import { foo } from "./a"; foo()
// ---------------------------------------------------------------------------

#[test]
fn w3_typescript_import_call() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // a.ts: defines foo
    fs::write(
        root.join("a.ts"),
        r#"export function foo(): number { return 42; }
"#,
    )
    .expect("write a.ts");

    // b.ts: imports and calls foo
    fs::write(
        root.join("b.ts"),
        r#"import { foo } from "./a";

export function bar(): number {
    return foo();
}
"#,
    )
    .expect("write b.ts");

    let report = Pipeline::new(root).index().expect("index");

    let db_path = root.join(".grafy").join("index.redb");
    let call_count = count_call_edges(&db_path);
    assert!(
        call_count >= 1,
        "expected ≥1 call edge for TypeScript import call, got {call_count}. Report={report:?}"
    );
}

// ---------------------------------------------------------------------------
// Go (Family B) — same-package two-file test: bar calls foo
// ---------------------------------------------------------------------------

#[test]
fn w3_go_same_package_call() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // a.go: defines Foo
    fs::write(
        root.join("a.go"),
        r#"package main

func Foo() int { return 42 }
"#,
    )
    .expect("write a.go");

    // b.go: calls Foo
    fs::write(
        root.join("b.go"),
        r#"package main

func Bar() int {
    return Foo()
}
"#,
    )
    .expect("write b.go");

    let report = Pipeline::new(root).index().expect("index");

    let db_path = root.join(".grafy").join("index.redb");
    let call_count = count_call_edges(&db_path);
    assert!(
        call_count >= 1,
        "expected ≥1 call edge for Go same-package call, got {call_count}. Report={report:?}"
    );
}

// ---------------------------------------------------------------------------
// Sanity: pipeline-level calls field is non-zero on a Rust fixture
// ---------------------------------------------------------------------------

#[test]
fn w3_report_calls_nonzero() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("lib.rs"),
        r#"
fn helper() -> u32 { 1 }
fn caller() -> u32 { helper() }
"#,
    )
    .expect("write lib.rs");

    let report = Pipeline::new(root).index().expect("index");
    assert!(
        report.calls >= 1,
        "expected ≥1 call in IndexReport, got {}",
        report.calls
    );
}
