//! Cypher-Lite integration tests.
//!
//! Covers the full stack: lexer round-trips, parser supported/unsupported
//! subset, executor on an in-memory test store, and the hard-row-cap.

use grafy::cypher::{execute, CypherError};
use grafy::store::{EdgeKind, NodeKind, NodeRecord, EDGES_TABLE, NODES_TABLE};

use redb::Database;
use tempfile::tempdir;

// Re-export postcard for use in make_test_db (dev-dependency).
extern crate postcard;

// ---------------------------------------------------------------------------
// Test-store helpers
// ---------------------------------------------------------------------------

fn node_id_deterministic(fqn: &str, kind: NodeKind) -> u64 {
    grafy::store::node_id("test.rs", fqn, kind, 0)
}

/// Build a small in-memory test store with:
///   2 File nodes, 3 Function nodes, 2 Calls edges.
fn make_test_db() -> (Database, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.redb");
    let db = Database::create(&db_path).expect("create db");

    {
        let tx = db.begin_write().expect("begin_write");
        {
            let mut nodes = tx.open_table(NODES_TABLE).expect("open nodes");
            let mut edges = tx.open_table(EDGES_TABLE).expect("open edges");

            let insert_node = |tbl: &mut redb::Table<u64, &[u8]>, id: u64, rec: &NodeRecord| {
                let bytes = postcard::to_allocvec(rec).expect("encode node");
                tbl.insert(id, bytes.as_slice()).expect("insert node");
            };

            // File nodes
            let file1_id = node_id_deterministic("src/main.rs", NodeKind::File);
            insert_node(
                &mut nodes,
                file1_id,
                &NodeRecord {
                    fqn: "src/main.rs".into(),
                    kind: NodeKind::File,
                    file: "src/main.rs".into(),
                    byte_start: 0,
                    byte_end: 100,
                },
            );
            let file2_id = node_id_deterministic("src/lib.rs", NodeKind::File);
            insert_node(
                &mut nodes,
                file2_id,
                &NodeRecord {
                    fqn: "src/lib.rs".into(),
                    kind: NodeKind::File,
                    file: "src/lib.rs".into(),
                    byte_start: 0,
                    byte_end: 200,
                },
            );

            // Function nodes
            let main_id = node_id_deterministic("crate::main", NodeKind::Function);
            insert_node(
                &mut nodes,
                main_id,
                &NodeRecord {
                    fqn: "crate::main".into(),
                    kind: NodeKind::Function,
                    file: "src/main.rs".into(),
                    byte_start: 10,
                    byte_end: 50,
                },
            );
            let helper_id = node_id_deterministic("crate::helper", NodeKind::Function);
            insert_node(
                &mut nodes,
                helper_id,
                &NodeRecord {
                    fqn: "crate::helper".into(),
                    kind: NodeKind::Function,
                    file: "src/lib.rs".into(),
                    byte_start: 5,
                    byte_end: 30,
                },
            );
            let util_id = node_id_deterministic("crate::util", NodeKind::Function);
            insert_node(
                &mut nodes,
                util_id,
                &NodeRecord {
                    fqn: "crate::util".into(),
                    kind: NodeKind::Function,
                    file: "src/lib.rs".into(),
                    byte_start: 35,
                    byte_end: 80,
                },
            );

            // Module node
            let mod_id = node_id_deterministic("crate", NodeKind::Module);
            insert_node(
                &mut nodes,
                mod_id,
                &NodeRecord {
                    fqn: "crate".into(),
                    kind: NodeKind::Module,
                    file: "src/lib.rs".into(),
                    byte_start: 0,
                    byte_end: 500,
                },
            );

            // Calls edges: main → helper, helper → util
            edges
                .insert((main_id, helper_id, EdgeKind::Calls as u8), [].as_slice())
                .expect("edge 1");
            edges
                .insert((helper_id, util_id, EdgeKind::Calls as u8), [].as_slice())
                .expect("edge 2");
        }
        tx.commit().expect("commit");
    }

    (db, dir)
}

// ---------------------------------------------------------------------------
// Lexer round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn lexer_match_keyword() {
    use grafy::cypher::lexer::{tokenize, Token};
    let toks = tokenize("MATCH").unwrap();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].token, Token::Match);
    assert_eq!(toks[0].start, 0);
    assert_eq!(toks[0].end, 5);
}

#[test]
fn lexer_arrow_symbols() {
    use grafy::cypher::lexer::{tokenize, Token};
    let toks = tokenize("-> <- --").unwrap();
    let tokens: Vec<_> = toks.iter().map(|s| &s.token).collect();
    assert!(tokens.contains(&&Token::Arrow));
    assert!(tokens.contains(&&Token::LeftArrow));
    assert!(tokens.contains(&&Token::DoubleDash));
}

#[test]
fn lexer_string_with_escape() {
    use grafy::cypher::lexer::{tokenize, Token};
    let toks = tokenize(r#"'hello\nworld'"#).unwrap();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].token, Token::StringLit("hello\nworld".into()));
}

#[test]
fn lexer_integer_and_float() {
    use grafy::cypher::lexer::{tokenize, Token};
    let toks = tokenize("42 1.5").unwrap();
    assert_eq!(toks[0].token, Token::IntLit(42));
    assert!(matches!(toks[1].token, Token::FloatLit(_)));
}

#[test]
fn lexer_backtick_ident() {
    use grafy::cypher::lexer::{tokenize, Token};
    let toks = tokenize("`weird ident`").unwrap();
    assert_eq!(toks[0].token, Token::Ident("weird ident".into()));
}

#[test]
fn lexer_forbidden_keywords_recognised() {
    use grafy::cypher::lexer::{tokenize, Token};
    let toks = tokenize("UNWIND MERGE CREATE DELETE SET REMOVE OPTIONAL").unwrap();
    let kinds: Vec<_> = toks.iter().map(|s| &s.token).collect();
    assert!(kinds.contains(&&Token::Unwind));
    assert!(kinds.contains(&&Token::Merge));
    assert!(kinds.contains(&&Token::Create));
    assert!(kinds.contains(&&Token::Delete));
    assert!(kinds.contains(&&Token::Set));
    assert!(kinds.contains(&&Token::Remove));
    assert!(kinds.contains(&&Token::Optional));
}

#[test]
fn lexer_spans_are_accurate() {
    use grafy::cypher::lexer::tokenize;
    let toks = tokenize("MATCH (n)").unwrap();
    assert_eq!(toks[0].start, 0);
    assert_eq!(toks[0].end, 5);
    assert_eq!(toks[1].start, 6);
    assert_eq!(toks[1].end, 7);
}

// ---------------------------------------------------------------------------
// Parser supported subset
// ---------------------------------------------------------------------------

#[test]
fn parse_simple_match_return() {
    use grafy::cypher::parser::parse;
    let q = parse("MATCH (n:Function) RETURN n.fqn").unwrap();
    assert_eq!(q.match_clauses.len(), 1);
    let head = &q.match_clauses[0].pattern.head;
    assert_eq!(head.label.as_deref(), Some("Function"));
    assert_eq!(head.var.as_deref(), Some("n"));
}

#[test]
fn parse_where_contains_order_limit() {
    use grafy::cypher::parser::parse;
    let q = parse(
        r#"MATCH (n:Function) WHERE n.fqn CONTAINS "main" RETURN n.fqn ORDER BY n.fqn LIMIT 10"#,
    )
    .unwrap();
    assert!(q.where_clause.is_some());
    assert!(q.order_by.is_some());
    assert_eq!(q.limit, Some(10));
}

#[test]
fn parse_relationship_pattern() {
    use grafy::cypher::ast::Direction;
    use grafy::cypher::parser::parse;
    let q = parse("MATCH (a:Function)-[:CALLS]->(b:Function) RETURN a.fqn, b.fqn").unwrap();
    let segs = &q.match_clauses[0].pattern.segments;
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].0.types, vec!["CALLS"]);
    assert_eq!(segs[0].0.direction, Direction::LeftToRight);
}

#[test]
fn parse_three_hop_pattern() {
    use grafy::cypher::parser::parse;
    let q = parse("MATCH (a)-[r]->(b)-[r2]->(c) RETURN a, b, c").unwrap();
    assert_eq!(q.match_clauses[0].pattern.segments.len(), 2);
}

#[test]
fn parse_distinct_skip_limit() {
    use grafy::cypher::parser::parse;
    let q = parse(
        r#"MATCH (n:Module) WHERE n.file STARTS WITH "src/" AND NOT n.fqn = "" RETURN DISTINCT n.fqn SKIP 5 LIMIT 5"#,
    )
    .unwrap();
    assert!(q.distinct);
    assert_eq!(q.skip, Some(5));
    assert_eq!(q.limit, Some(5));
}

#[test]
fn parse_ends_with_predicate() {
    use grafy::cypher::parser::parse;
    let q = parse(r#"MATCH (n:Function) WHERE n.fqn ENDS WITH "main" RETURN n.fqn"#).unwrap();
    assert!(q.where_clause.is_some());
}

// ---------------------------------------------------------------------------
// Parser unsupported subset
// ---------------------------------------------------------------------------

fn assert_unsupported_with_docs(query: &str) {
    let e = grafy::cypher::parser::parse(query)
        .expect_err(&format!("expected error for {query:?}"));
    assert!(
        matches!(e, CypherError::Unsupported(_)),
        "expected Unsupported but got: {e:?}"
    );
    let msg = e.to_string();
    assert!(msg.starts_with("ERROR:"), "missing ERROR: prefix in: {msg}");
    assert!(
        msg.contains("docs/cypher-lite.md"),
        "missing docs reference in: {msg}"
    );
}

#[test]
fn unsupported_with() {
    assert_unsupported_with_docs("MATCH (n) WITH n RETURN n");
}

#[test]
fn unsupported_unwind() {
    assert_unsupported_with_docs("UNWIND [1,2] AS x RETURN x");
}

#[test]
fn unsupported_merge() {
    assert_unsupported_with_docs("MERGE (n:Function {fqn: 'foo'})");
}

#[test]
fn unsupported_create() {
    assert_unsupported_with_docs("CREATE (n:Function)");
}

#[test]
fn unsupported_delete() {
    assert_unsupported_with_docs("MATCH (n) DELETE n");
}

#[test]
fn unsupported_set() {
    assert_unsupported_with_docs("MATCH (n) SET n.x = 1");
}

#[test]
fn unsupported_remove() {
    assert_unsupported_with_docs("MATCH (n) REMOVE n.x");
}

#[test]
fn unsupported_optional_match() {
    assert_unsupported_with_docs("OPTIONAL MATCH (n) RETURN n");
}

#[test]
fn unsupported_variable_length_path() {
    assert_unsupported_with_docs("MATCH (a)-[*]->(b) RETURN a");
}

#[test]
fn unsupported_path_variable() {
    assert_unsupported_with_docs("MATCH p = (a)-[r]->(b) RETURN p");
}

#[test]
fn unsupported_function_in_return() {
    assert_unsupported_with_docs("MATCH (n) RETURN id(n)");
}

#[test]
fn unsupported_aggregation_checked_at_planner() {
    // The planner rejects multiple MATCH clauses.
    use grafy::cypher::ast::*;
    use grafy::cypher::plan::Planner;
    let q = Query {
        match_clauses: vec![
            MatchClause {
                pattern: Pattern {
                    head: NodePat { var: Some("a".into()), label: None, properties: vec![] },
                    segments: vec![],
                },
            },
            MatchClause {
                pattern: Pattern {
                    head: NodePat { var: Some("b".into()), label: None, properties: vec![] },
                    segments: vec![],
                },
            },
        ],
        where_clause: None,
        return_clause: ReturnClause {
            items: vec![ReturnItem { expr: ReturnExpr::Var("a".into()), alias: None }],
        },
        order_by: None,
        skip: None,
        limit: None,
        distinct: false,
    };
    let err = Planner::build(q).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("ERROR:"), "got: {msg}");
    assert!(msg.contains("docs/cypher-lite.md"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// Executor tests
// ---------------------------------------------------------------------------

#[test]
fn executor_scan_all_functions() {
    let (db, _dir) = make_test_db();
    let rows = execute(&db, "MATCH (n:Function) RETURN n.fqn").unwrap();
    // We inserted 3 functions.
    assert_eq!(rows.len(), 3, "expected 3 functions, got: {rows:?}");
    let fqns: Vec<_> = rows
        .iter()
        .filter_map(|r| r.get("n.fqn"))
        .filter_map(|v| match v {
            grafy::cypher::Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert!(fqns.contains(&"crate::main".to_string()));
    assert!(fqns.contains(&"crate::helper".to_string()));
    assert!(fqns.contains(&"crate::util".to_string()));
}

#[test]
fn executor_scan_with_label_filter() {
    let (db, _dir) = make_test_db();
    let rows = execute(&db, "MATCH (n:Module) RETURN n.fqn").unwrap();
    assert_eq!(rows.len(), 1);
    let fqn = match rows[0].get("n.fqn").unwrap() {
        grafy::cypher::Value::Str(s) => s.as_str(),
        _ => panic!("expected string"),
    };
    assert_eq!(fqn, "crate");
}

#[test]
fn executor_unknown_label_returns_zero_rows() {
    let (db, _dir) = make_test_db();
    // Unknown label should produce Execute error (from planner)
    // or return zero rows. Our planner returns an error for unknown labels.
    let result = execute(&db, "MATCH (n:NonExistent) RETURN n.fqn");
    // Either zero rows (if implemented as zero-rows) or Execute error.
    // Our impl returns Execute error for unknown labels.
    assert!(result.is_err() || result.unwrap().is_empty());
}

#[test]
fn executor_where_contains_filter() {
    let (db, _dir) = make_test_db();
    let rows = execute(
        &db,
        r#"MATCH (n:Function) WHERE n.fqn CONTAINS "helper" RETURN n.fqn"#,
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    match rows[0].get("n.fqn").unwrap() {
        grafy::cypher::Value::Str(s) => assert_eq!(s, "crate::helper"),
        _ => panic!("expected string"),
    }
}

#[test]
fn executor_expand_calls_edge() {
    let (db, _dir) = make_test_db();
    let rows =
        execute(&db, "MATCH (a:Function)-[:CALLS]->(b:Function) RETURN a.fqn, b.fqn").unwrap();
    assert_eq!(rows.len(), 2, "expected 2 call edges, got: {rows:?}");
    // Check that main→helper and helper→util are both present.
    let pairs: Vec<(String, String)> = rows
        .iter()
        .map(|r| {
            let a = match r.get("a.fqn").unwrap() {
                grafy::cypher::Value::Str(s) => s.clone(),
                _ => String::new(),
            };
            let b = match r.get("b.fqn").unwrap() {
                grafy::cypher::Value::Str(s) => s.clone(),
                _ => String::new(),
            };
            (a, b)
        })
        .collect();
    assert!(
        pairs.contains(&("crate::main".into(), "crate::helper".into())),
        "missing main->helper: {pairs:?}"
    );
    assert!(
        pairs.contains(&("crate::helper".into(), "crate::util".into())),
        "missing helper->util: {pairs:?}"
    );
}

#[test]
fn executor_order_by_fqn() {
    let (db, _dir) = make_test_db();
    let rows =
        execute(&db, "MATCH (n:Function) RETURN n.fqn ORDER BY n.fqn").unwrap();
    assert_eq!(rows.len(), 3);
    let fqns: Vec<_> = rows
        .iter()
        .filter_map(|r| match r.get("n.fqn") {
            Some(grafy::cypher::Value::Str(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    let mut sorted = fqns.clone();
    sorted.sort();
    assert_eq!(fqns, sorted, "rows not sorted: {fqns:?}");
}

#[test]
fn executor_limit_and_skip() {
    let (db, _dir) = make_test_db();
    let rows = execute(&db, "MATCH (n:Function) RETURN n.fqn ORDER BY n.fqn SKIP 1 LIMIT 1").unwrap();
    assert_eq!(rows.len(), 1, "expected exactly 1 row after skip+limit");
}

#[test]
fn executor_distinct() {
    let (db, _dir) = make_test_db();
    // All functions have distinct fqns, so DISTINCT doesn't change count.
    let rows = execute(&db, "MATCH (n:Function) RETURN DISTINCT n.fqn").unwrap();
    assert_eq!(rows.len(), 3);
}

#[test]
fn executor_row_var_returns_node() {
    let (db, _dir) = make_test_db();
    let rows = execute(&db, "MATCH (n:Function) RETURN n LIMIT 1").unwrap();
    assert_eq!(rows.len(), 1);
    let val = rows[0].get("n").unwrap();
    assert!(matches!(val, grafy::cypher::Value::Node(_)));
}

// ---------------------------------------------------------------------------
// Hard row cap test
// ---------------------------------------------------------------------------

#[test]
fn executor_hard_row_cap() {
    use grafy::cypher::executor::MAX_ROWS;

    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("big.redb");
    let db = Database::create(&db_path).expect("create db");

    // Write MAX_ROWS + 1 function nodes
    {
        let tx = db.begin_write().expect("begin_write");
        {
            let mut nodes = tx.open_table(NODES_TABLE).expect("open nodes");
            let _ = tx.open_table(EDGES_TABLE).expect("open edges"); // ensure exists
            for i in 0u32..=(MAX_ROWS as u32) {
                let fqn = format!("fn_{i}");
                let id = grafy::store::node_id("f.rs", &fqn, NodeKind::Function, i);
                let rec = NodeRecord {
                    fqn,
                    kind: NodeKind::Function,
                    file: "f.rs".into(),
                    byte_start: i,
                    byte_end: i + 1,
                };
                let bytes = postcard::to_allocvec(&rec).expect("encode");
                nodes.insert(id, bytes.as_slice()).expect("insert");
            }
        }
        tx.commit().expect("commit");
    }

    let result = execute(&db, "MATCH (n:Function) RETURN n.fqn");
    match result {
        Err(CypherError::Execute { msg }) => {
            assert!(
                msg.contains("LIMIT"),
                "cap error should suggest LIMIT, got: {msg}"
            );
        }
        other => panic!("expected Execute error, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// execute() top-level entry point
// ---------------------------------------------------------------------------

#[test]
fn top_level_execute_returns_rows() {
    let (db, _dir) = make_test_db();
    let rows = execute(&db, "MATCH (n:Function) RETURN n.fqn LIMIT 2").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn top_level_execute_unsupported_returns_error() {
    let (db, _dir) = make_test_db();
    let err = execute(&db, "MATCH (n) WITH n RETURN n").unwrap_err();
    assert!(matches!(err, CypherError::Unsupported(_)));
}

#[test]
fn top_level_execute_parse_error() {
    let (db, _dir) = make_test_db();
    let err = execute(&db, "NOTAQUERY").unwrap_err();
    assert!(matches!(err, CypherError::Parse { .. }));
}
