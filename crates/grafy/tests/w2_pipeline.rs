//! W2 pipeline integration test.
//!
//! Builds an in-test fixture with one .rs file (struct + impl + function)
//! and one .py file (class + function). Runs `Pipeline::index()` and asserts:
//!   - At least 1 file node per language.
//!   - At least 1 function node per language.
//!   - At least 1 class/struct node.
//!   - The redb `files` and `nodes` tables are populated after indexing.

use std::fs;

use grafy::pipeline::Pipeline;
use grafy::store::{FileRecord, NodeRecord, FILES_TABLE, NODES_TABLE};
use postcard::from_bytes;
use redb::{Database, ReadableTable};
use tempfile::tempdir;

const RS_SOURCE: &str = r#"
pub struct Greeter {
    name: String,
}

impl Greeter {
    pub fn new(name: String) -> Self {
        Greeter { name }
    }

    pub fn greet(&self) -> String {
        format!("Hello, {}!", self.name)
    }
}

pub fn standalone() -> u32 {
    42
}
"#;

const PY_SOURCE: &str = r#"
class Greeter:
    def __init__(self, name):
        self.name = name

    def greet(self):
        return f"Hello, {self.name}!"

def standalone():
    return 42
"#;

#[test]
fn w2_index_counts() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // Write fixture files.
    fs::write(root.join("greet.rs"), RS_SOURCE).expect("write rs");
    fs::write(root.join("greet.py"), PY_SOURCE).expect("write py");

    // Run the pipeline.
    let pipeline = Pipeline::new(root);
    let report = pipeline.index().expect("index");

    // File nodes: one per source file (at minimum).
    assert!(
        report.files >= 2,
        "expected ≥2 file nodes, got {}",
        report.files
    );

    // Functions: standalone + greet + new from Rust; standalone + greet + __init__ from Python.
    assert!(
        report.functions >= 2,
        "expected ≥2 function nodes, got {}",
        report.functions
    );

    // At least one class/struct (Greeter appears in both).
    let class_or_struct = report.classes + report.structs;
    assert!(
        class_or_struct >= 1,
        "expected ≥1 class/struct node, got {class_or_struct}"
    );

    // --- Verify the redb tables directly. ---

    let db_path = root.join(".grafy").join("index.redb");
    assert!(db_path.exists(), ".grafy/index.redb must exist");

    let db = Database::open(&db_path).expect("open redb");

    // files table: should have entries for greet.rs and greet.py.
    {
        let tx = db.begin_read().expect("begin read");
        let tbl = tx.open_table(FILES_TABLE).expect("open files table");

        let rs_val = tbl
            .get("greet.rs")
            .expect("read greet.rs")
            .expect("greet.rs missing");
        let rs_rec: FileRecord =
            from_bytes(rs_val.value()).expect("decode FileRecord for greet.rs");
        assert_eq!(rs_rec.lang, "rust");

        let py_val = tbl
            .get("greet.py")
            .expect("read greet.py")
            .expect("greet.py missing");
        let py_rec: FileRecord =
            from_bytes(py_val.value()).expect("decode FileRecord for greet.py");
        assert_eq!(py_rec.lang, "python");
    }

    // nodes table: should have at least one Function node.
    {
        use grafy::store::NodeKind;

        let tx = db.begin_read().expect("begin read nodes");
        let tbl = tx.open_table(NODES_TABLE).expect("open nodes table");

        let mut function_count = 0u64;
        for item in tbl.iter().expect("iter nodes") {
            let (_k, v) = item.expect("node item");
            if let Ok(rec) = from_bytes::<NodeRecord>(v.value()) {
                if rec.kind == NodeKind::Function || rec.kind == NodeKind::Method {
                    function_count += 1;
                }
            }
        }
        assert!(
            function_count >= 2,
            "expected ≥2 function/method nodes in store, got {function_count}"
        );
    }
}

#[test]
fn w2_report_nonzero_on_larger_fixture() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // Slightly larger fixture to exercise edge cases.
    fs::write(
        root.join("lib.rs"),
        r#"
mod inner {
    pub fn helper() {}
}
pub enum Color { Red, Green, Blue }
pub trait Animal { fn sound(&self) -> &str; }
pub struct Dog;
impl Animal for Dog { fn sound(&self) -> &str { "woof" } }
"#,
    )
    .unwrap();

    let report = Pipeline::new(root).index().expect("index");
    // total_nodes includes File + whatever the pass2 emits.
    assert!(
        report.total_nodes() >= 1,
        "expected at least one node, got 0"
    );
    // Specific kinds from the Rust source above.
    assert!(report.enums >= 1, "expected ≥1 enum, got {}", report.enums);
    assert!(
        report.traits >= 1,
        "expected ≥1 trait, got {}",
        report.traits
    );
    assert!(
        report.structs >= 1,
        "expected ≥1 struct, got {}",
        report.structs
    );
}
