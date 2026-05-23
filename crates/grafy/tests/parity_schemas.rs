//! Schema-compat CI tests — M1 quality gate (plan §4).
//!
//! For every JSON file in `tests/parity/schemas/` this suite:
//!   1. Loads the input schema from the file.
//!   2. Compiles it with `jsonschema::validator_for` (Draft 7 auto-detect).
//!   3. Builds a representative request payload that satisfies the schema.
//!   4. Validates the payload against the schema — confirms the schema is
//!      loadable and our payload is well-formed.
//!   5. Invokes the handler via `GrafyServer::dispatch` (test-only helper).
//!   6. Asserts the response is valid JSON with the expected top-level keys.
//!
//! Stub tools (`delete_project`, `detect_changes`, `manage_adr`,
//! `ingest_traces`) assert response shape only — their `{"error": "…"}`
//! envelope is schema-valid.
//!
//! Schema drift is a blocker: any schema that fails to compile triggers a
//! test failure with the exact error path printed to stderr.

use std::path::PathBuf;

use grafy::{mcp::handler::GrafyServer, pipeline::Pipeline};
use serde_json::{json, Value};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/grafy layout")
        .join("tests")
        .join("parity")
        .join("schemas")
}

fn load_schema(name: &str) -> Value {
    let path = schema_dir().join(format!("{name}.json"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read schema {name}.json: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("invalid JSON in schema {name}.json: {e}"))
}

/// Compile schema and assert the payload is valid against it.
/// Reports the diverging instance path on failure.
fn assert_payload_valid(schema_name: &str, payload: &Value) {
    let schema = load_schema(schema_name);
    let validator = jsonschema::validator_for(&schema).unwrap_or_else(|e| {
        panic!(
            "schema {schema_name}.json failed to compile (BLOCKER):\n  {e}\n  path: tests/parity/schemas/{schema_name}.json"
        )
    });
    let errors: Vec<String> = validator
        .iter_errors(payload)
        .map(|e| format!("  instance_path={} — {}", e.instance_path(), e))
        .collect();
    if !errors.is_empty() {
        panic!(
            "payload does not conform to schema {schema_name}.json:\n{}",
            errors.join("\n")
        );
    }
}

/// Assert the response JSON has all required keys.
fn assert_response_keys(tool: &str, response: &Value, required: &[&str]) {
    let obj = response
        .as_object()
        .unwrap_or_else(|| panic!("{tool}: response is not a JSON object"));
    for key in required {
        assert!(
            obj.contains_key(*key),
            "{tool}: response missing key '{key}'. Got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }
}

/// Build an indexed temp dir with a simple Rust source file.
fn make_indexed_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.rs"),
        "pub fn index_entry(id: u64) -> bool { validate_entry(id) }\n\
         fn validate_entry(_: u64) -> bool { true }\n",
    )
    .unwrap();
    Pipeline::new(dir.path())
        .index()
        .expect("index should succeed for parity schema test");
    dir
}

// ---------------------------------------------------------------------------
// One test per tool
// ---------------------------------------------------------------------------

#[test]
fn schema_index_repository() {
    let dir = tempdir().unwrap();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "repo_path": dir.path().to_string_lossy(),
        "mode": "fast"
    });
    assert_payload_valid("index_repository", &payload);

    // index_repository actually runs the pipeline; use a dir with one file
    let dir2 = tempdir().unwrap();
    std::fs::write(dir2.path().join("main.rs"), "fn main() {}").unwrap();
    let server2 = GrafyServer::new(dir2.path().to_path_buf());
    let payload2 = json!({
        "repo_path": dir2.path().to_string_lossy()
    });
    let resp = server2
        .dispatch("index_repository", payload2)
        .unwrap_or_else(|e| panic!("index_repository dispatch failed: {e}"));

    // On success path
    if resp.get("status").is_some() {
        assert_response_keys("index_repository", &resp, &["status", "project", "files"]);
    } else {
        // error envelope
        assert_response_keys("index_repository", &resp, &["error"]);
    }
    tracing::debug!(target: "grafy.parity", tool = "index_repository", "schema-compat ok");
    let _ = server; // suppress unused
}

#[test]
fn schema_search_graph() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "project": "test",
        "query": "validate",
        "limit": 10
    });
    assert_payload_valid("search_graph", &payload);

    let resp = server
        .dispatch("search_graph", payload)
        .unwrap_or_else(|e| panic!("search_graph dispatch failed: {e}"));

    assert_response_keys("search_graph", &resp, &["results", "total", "has_more"]);
    assert!(
        resp["results"].is_array(),
        "search_graph: 'results' must be an array"
    );
    tracing::debug!(target: "grafy.parity", tool = "search_graph", "schema-compat ok");
}

#[test]
fn schema_query_graph() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "query": "MATCH (n:Function) RETURN n.fqn LIMIT 5",
        "project": "test"
    });
    assert_payload_valid("query_graph", &payload);

    let resp = server
        .dispatch("query_graph", payload)
        .unwrap_or_else(|e| panic!("query_graph dispatch failed: {e}"));

    // Success: has rows+total. Error: has error key.
    assert!(
        resp.get("rows").is_some() || resp.get("error").is_some(),
        "query_graph: response must have 'rows' or 'error'. Got: {resp}"
    );
    if resp.get("rows").is_some() {
        assert_response_keys("query_graph", &resp, &["rows", "total"]);
    }
    tracing::debug!(target: "grafy.parity", tool = "query_graph", "schema-compat ok");
}

#[test]
fn schema_trace_path() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "function_name": "index_entry",
        "project": "test",
        "direction": "both",
        "depth": 2
    });
    assert_payload_valid("trace_path", &payload);

    let resp = server
        .dispatch("trace_path", payload)
        .unwrap_or_else(|e| panic!("trace_path dispatch failed: {e}"));

    assert_response_keys("trace_path", &resp, &["function_name", "direction", "hops"]);
    assert!(
        resp["hops"].is_array(),
        "trace_path: 'hops' must be an array"
    );
    tracing::debug!(target: "grafy.parity", tool = "trace_path", "schema-compat ok");
}

#[test]
fn schema_trace_call_path_alias() {
    // trace_call_path is the alias; same schema as trace_path.
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "function_name": "validate_entry",
        "project": "test"
    });
    // Validate against the authoritative trace_path schema.
    assert_payload_valid("trace_path", &payload);

    let resp = server
        .dispatch("trace_call_path", payload)
        .unwrap_or_else(|e| panic!("trace_call_path dispatch failed: {e}"));

    assert_response_keys(
        "trace_call_path",
        &resp,
        &["function_name", "direction", "hops"],
    );
    tracing::debug!(target: "grafy.parity", tool = "trace_call_path", "schema-compat ok");
}

#[test]
fn schema_get_code_snippet() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "qualified_name": "index_entry",
        "project": "test"
    });
    assert_payload_valid("get_code_snippet", &payload);

    let resp = server
        .dispatch("get_code_snippet", payload)
        .unwrap_or_else(|e| panic!("get_code_snippet dispatch failed: {e}"));

    // Either a snippet or error or ambiguous response
    assert!(
        resp.get("qualified_name").is_some()
            || resp.get("ambiguous").is_some()
            || resp.get("error").is_some(),
        "get_code_snippet: expected 'qualified_name', 'ambiguous', or 'error' key. Got: {resp}"
    );
    tracing::debug!(target: "grafy.parity", tool = "get_code_snippet", "schema-compat ok");
}

#[test]
fn schema_get_graph_schema() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({"project": "test"});
    assert_payload_valid("get_graph_schema", &payload);

    let resp = server
        .dispatch("get_graph_schema", payload)
        .unwrap_or_else(|e| panic!("get_graph_schema dispatch failed: {e}"));

    assert_response_keys(
        "get_graph_schema",
        &resp,
        &["project", "node_labels", "edge_types"],
    );
    tracing::debug!(target: "grafy.parity", tool = "get_graph_schema", "schema-compat ok");
}

#[test]
fn schema_get_architecture() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({"project": "test"});
    assert_payload_valid("get_architecture", &payload);

    let resp = server
        .dispatch("get_architecture", payload)
        .unwrap_or_else(|e| panic!("get_architecture dispatch failed: {e}"));

    assert_response_keys(
        "get_architecture",
        &resp,
        &["project", "total_nodes", "total_edges"],
    );
    tracing::debug!(target: "grafy.parity", tool = "get_architecture", "schema-compat ok");
}

#[test]
fn schema_search_code() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "pattern": "validate",
        "project": "test",
        "mode": "compact",
        "limit": 10
    });
    assert_payload_valid("search_code", &payload);

    let resp = server
        .dispatch("search_code", payload)
        .unwrap_or_else(|e| panic!("search_code dispatch failed: {e}"));

    assert_response_keys("search_code", &resp, &["results", "total_grep_matches"]);
    tracing::debug!(target: "grafy.parity", tool = "search_code", "schema-compat ok");
}

#[test]
fn schema_list_projects() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    // list_projects has no required fields
    let payload = json!({});
    assert_payload_valid("list_projects", &payload);

    let resp = server
        .dispatch("list_projects", payload)
        .unwrap_or_else(|e| panic!("list_projects dispatch failed: {e}"));

    assert_response_keys("list_projects", &resp, &["projects"]);
    assert!(
        resp["projects"].is_array(),
        "list_projects: 'projects' must be an array"
    );
    tracing::debug!(target: "grafy.parity", tool = "list_projects", "schema-compat ok");
}

#[test]
fn schema_delete_project() {
    // Stub: returns error envelope. Assert shape, not behaviour.
    let dir = tempdir().unwrap();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({"project": "test"});
    assert_payload_valid("delete_project", &payload);

    let resp = server
        .dispatch("delete_project", payload)
        .unwrap_or_else(|e| panic!("delete_project dispatch failed: {e}"));

    // delete_project is a stub — must return {"error": "…"}
    assert_response_keys("delete_project", &resp, &["error"]);
    tracing::warn!(
        target: "grafy.parity",
        tool = "delete_project",
        "stub tool — error shape confirmed; see tests/parity/diffs.md"
    );
}

#[test]
fn schema_index_status() {
    let dir = make_indexed_dir();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({"project": "test"});
    assert_payload_valid("index_status", &payload);

    let resp = server
        .dispatch("index_status", payload)
        .unwrap_or_else(|e| panic!("index_status dispatch failed: {e}"));

    // Indexed path
    assert!(
        resp.get("status").is_some() || resp.get("error").is_some(),
        "index_status: expected 'status' or 'error' key. Got: {resp}"
    );
    if resp.get("status").is_some() {
        assert_response_keys("index_status", &resp, &["project", "status"]);
    }
    tracing::debug!(target: "grafy.parity", tool = "index_status", "schema-compat ok");
}

#[test]
fn schema_detect_changes() {
    // Stub: returns error envelope.
    let dir = tempdir().unwrap();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({"project": "test"});
    assert_payload_valid("detect_changes", &payload);

    let resp = server
        .dispatch("detect_changes", payload)
        .unwrap_or_else(|e| panic!("detect_changes dispatch failed: {e}"));

    assert_response_keys("detect_changes", &resp, &["error"]);
    tracing::warn!(
        target: "grafy.parity",
        tool = "detect_changes",
        "stub tool — error shape confirmed; see tests/parity/diffs.md"
    );
}

#[test]
fn schema_manage_adr() {
    // Stub: returns error envelope.
    let dir = tempdir().unwrap();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({"project": "test", "mode": "get"});
    assert_payload_valid("manage_adr", &payload);

    let resp = server
        .dispatch("manage_adr", payload)
        .unwrap_or_else(|e| panic!("manage_adr dispatch failed: {e}"));

    assert_response_keys("manage_adr", &resp, &["error"]);
    tracing::warn!(
        target: "grafy.parity",
        tool = "manage_adr",
        "stub tool — error shape confirmed; see tests/parity/diffs.md"
    );
}

#[test]
fn schema_ingest_traces() {
    // Stub: returns error envelope.
    let dir = tempdir().unwrap();
    let server = GrafyServer::new(dir.path().to_path_buf());

    let payload = json!({
        "traces": [{"span_id": "abc", "parent_span_id": null}],
        "project": "test"
    });
    assert_payload_valid("ingest_traces", &payload);

    let resp = server
        .dispatch("ingest_traces", payload)
        .unwrap_or_else(|e| panic!("ingest_traces dispatch failed: {e}"));

    assert_response_keys("ingest_traces", &resp, &["error"]);
    tracing::warn!(
        target: "grafy.parity",
        tool = "ingest_traces",
        "stub tool — error shape confirmed; see tests/parity/diffs.md"
    );
}

// ---------------------------------------------------------------------------
// Drift guard: all 14 schema files are present and load cleanly
// ---------------------------------------------------------------------------

#[test]
fn all_parity_schemas_load() {
    const TOOLS: &[&str] = &[
        "index_repository",
        "search_graph",
        "query_graph",
        "trace_path",
        "get_code_snippet",
        "get_graph_schema",
        "get_architecture",
        "search_code",
        "list_projects",
        "delete_project",
        "index_status",
        "detect_changes",
        "manage_adr",
        "ingest_traces",
    ];
    for name in TOOLS {
        let schema = load_schema(name);
        let _ = jsonschema::validator_for(&schema).unwrap_or_else(|e| {
            panic!("schema {name}.json failed to compile (DRIFT BLOCKER):\n  {e}")
        });
    }
}
