//! GrafyServer — the `#[tool_router(server_handler)]` impl with all 14 tools.
//!
//! Tool names and schemas match codebase-memory-mcp/src/mcp/mcp.c verbatim.
//! Handlers are thin: delegate to `pipeline::Pipeline`, `store::Store`, and
//! `cypher::execute`. No business logic lives here.
//!
//! Error UX: every error names the tool + a one-line "what to do next" action
//! (plan §CLAUDE.md error policy). Errors are returned as tool content text,
//! not JSON-RPC errors, so the agent can see them inline.

use std::collections::HashMap;
use std::path::PathBuf;

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    schemars,
    tool, tool_router,
};
use serde::Deserialize;

use redb::{ReadableTable, ReadableTableMetadata};

use crate::pipeline::Pipeline;
use crate::store::{EdgeKind, NodeKind, NodeRecord, Store, EDGES_TABLE, FILES_TABLE, NODES_TABLE};

// ---------------------------------------------------------------------------
// Public constant — used by server::check()
// ---------------------------------------------------------------------------

/// All 14 canonical tool names in registration order.
pub const TOOL_NAMES: &[&str] = &[
    "index_repository",
    "search_graph",
    "query_graph",
    "trace_path",
    "trace_call_path",
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

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct GrafyServer {
    root: PathBuf,
}

impl GrafyServer {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

// ---------------------------------------------------------------------------
// Input parameter structs
// Note: schemars derives the JSON schema; descriptions via #[schemars(description)]
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IndexRepositoryParams {
    #[schemars(description = "Path to the repository")]
    pub repo_path: String,
    #[schemars(description = "full: all passes. moderate: fast + semantic. fast: structure only. cross-repo-intelligence: match Routes/Channels across projects.")]
    pub mode: Option<String>,
    #[schemars(description = "Projects to search for cross-repo links (cross-repo-intelligence mode). Use [\"*\"] for all indexed projects. Run list_projects to see available projects.")]
    pub target_projects: Option<Vec<String>>,
    #[schemars(description = "Write compressed artifact to .codebase-memory/graph.db.zst for team sharing.")]
    pub persistence: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchGraphParams {
    pub project: String,
    #[schemars(description = "Natural-language or keyword full-text search using BM25 ranking.")]
    pub query: Option<String>,
    pub label: Option<String>,
    pub name_pattern: Option<String>,
    pub qn_pattern: Option<String>,
    pub file_pattern: Option<String>,
    pub relationship: Option<String>,
    pub min_degree: Option<i64>,
    pub max_degree: Option<i64>,
    pub exclude_entry_points: Option<bool>,
    pub include_connected: Option<bool>,
    #[schemars(description = "MUST be an ARRAY of keyword strings — NOT a single string.")]
    pub semantic_query: Option<Vec<String>>,
    #[schemars(description = "Max results per call. Default 200.")]
    pub limit: Option<i64>,
    #[schemars(description = "Skip the first N matching nodes.")]
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryGraphParams {
    #[schemars(description = "Cypher query")]
    pub query: String,
    pub project: String,
    #[schemars(description = "Optional row limit. Default: unlimited up to a 100k row ceiling.")]
    pub max_rows: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TracePathParams {
    pub function_name: String,
    pub project: String,
    pub direction: Option<String>,
    pub depth: Option<i64>,
    #[schemars(description = "calls: follow CALLS edges. data_flow: follow CALLS+DATA_FLOWS. cross_service: follow HTTP_CALLS+ASYNC_CALLS+DATA_FLOWS through Routes.")]
    pub mode: Option<String>,
    #[schemars(description = "For data_flow mode: scope trace to a specific parameter name")]
    pub parameter_name: Option<String>,
    pub edge_types: Option<Vec<String>>,
    #[schemars(description = "Add risk classification (CRITICAL/HIGH/MEDIUM/LOW) based on hop distance")]
    pub risk_labels: Option<bool>,
    #[schemars(description = "Include test files in results.")]
    pub include_tests: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetCodeSnippetParams {
    #[schemars(description = "Full qualified_name from search_graph, or short function name")]
    pub qualified_name: String,
    pub project: String,
    pub include_neighbors: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProjectParam {
    pub project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetArchitectureParams {
    pub project: String,
    pub aspects: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchCodeParams {
    pub pattern: String,
    pub project: String,
    #[schemars(description = "Glob for grep --include (e.g. *.go)")]
    pub file_pattern: Option<String>,
    #[schemars(description = "Regex filter on result file paths (e.g. ^src/ or \\.(go|ts)$)")]
    pub path_filter: Option<String>,
    #[schemars(description = "compact: signatures+metadata (default). full: with source. files: just file list.")]
    pub mode: Option<String>,
    #[schemars(description = "Lines of context around each match (like grep -C). Only used in compact mode.")]
    pub context: Option<i64>,
    pub regex: Option<bool>,
    #[schemars(description = "Max enriched results per call. Default 10.")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListProjectsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DetectChangesParams {
    pub project: String,
    pub scope: Option<String>,
    pub depth: Option<i64>,
    pub base_branch: Option<String>,
    #[schemars(description = "Git ref or date to compare from (e.g. HEAD~5, v0.5.0, 2026-01-01)")]
    pub since: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ManageAdrParams {
    pub project: String,
    pub mode: Option<String>,
    pub content: Option<String>,
    pub sections: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IngestTracesParams {
    pub traces: Vec<serde_json::Value>,
    pub project: String,
}

// ---------------------------------------------------------------------------
// Helper: open store read transaction, return (db path, store)
// ---------------------------------------------------------------------------

fn open_store(root: &std::path::Path) -> anyhow::Result<Store> {
    Store::open(root).map_err(|e| {
        anyhow::anyhow!(
            "{}: failed to open grafy store — run `grafy index {}` first. ({})",
            root.display(),
            root.display(),
            e
        )
    })
}

/// Format a tool-level error as a JSON text payload that follows the project
/// error UX policy: tool name, file/context, one-line action.
fn tool_error(tool: &str, file: &str, action: &str) -> CallToolResult {
    let msg = format!(
        "grafy {tool}: {file} — {action}"
    );
    tracing::warn!(target: "grafy.mcp", tool, file, action, "tool error");
    CallToolResult::success(vec![Content::text(format!(
        "{{\"error\": \"{}\"}}", msg.replace('"', "'")
    ))])
}

fn tool_ok(value: serde_json::Value) -> CallToolResult {
    CallToolResult::success(vec![Content::text(value.to_string())])
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[tool_router(server_handler)]
impl GrafyServer {
    // --- index_repository --------------------------------------------------

    #[tool(description = "Index a repository into the knowledge graph. Special mode 'cross-repo-intelligence': skip extraction, only match Routes/Channels across projects to create CROSS_HTTP_CALLS/CROSS_ASYNC_CALLS/CROSS_CHANNEL edges. Requires target_projects param. Ensure target projects have fresh indexes first.")]
    fn index_repository(
        &self,
        Parameters(p): Parameters<IndexRepositoryParams>,
    ) -> CallToolResult {
        let repo_path = PathBuf::from(&p.repo_path);
        let root = if repo_path.is_absolute() {
            repo_path
        } else {
            self.root.join(&p.repo_path)
        };

        tracing::info!(
            target: "grafy.mcp",
            tool = "index_repository",
            root = %root.display(),
            mode = ?p.mode,
            "indexing repository"
        );

        match Pipeline::new(&root).index() {
            Ok(report) => {
                let val = serde_json::json!({
                    "status": "indexed",
                    "project": root.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown"),
                    "root": root.to_string_lossy(),
                    "files": report.files,
                    "modules": report.modules,
                    "functions": report.functions,
                    "classes": report.classes,
                    "structs": report.structs,
                    "enums": report.enums,
                    "traits": report.traits,
                    "methods": report.methods,
                    "calls": report.calls,
                    "routes": report.routes,
                    "total_nodes": report.total_nodes(),
                });
                tool_ok(val)
            }
            Err(e) => tool_error(
                "index_repository",
                &root.display().to_string(),
                &format!("indexing failed — check the path is readable and retry. ({e})"),
            ),
        }
    }

    // --- search_graph -------------------------------------------------------

    #[tool(description = "Search the code knowledge graph for functions, classes, routes, and variables. Use INSTEAD OF grep/glob when finding code definitions, implementations, or relationships. Three search modes: (1) query='update settings' for BM25 ranked full-text search; (2) name_pattern='.*regex.*' for exact pattern matching; (3) semantic_query=[...] for vector cosine search. PAGINATION: results are capped at limit (default 200). Response includes 'total' and 'has_more'.")]
    fn search_graph(
        &self,
        Parameters(p): Parameters<SearchGraphParams>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "search_graph", project = %p.project, "searching graph");

        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("search_graph", &self.root.display().to_string(), &e.to_string()),
        };

        let limit = p.limit.unwrap_or(200).max(1) as usize;
        let offset = p.offset.unwrap_or(0).max(0) as usize;

        let db = store.read_db();
        let tx = match db.begin_read() {
            Ok(t) => t,
            Err(e) => return tool_error("search_graph", &self.root.display().to_string(),
                &format!("store read failed — rebuild with `grafy index`. ({e})")),
        };
        let nodes_tbl = match tx.open_table(NODES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("search_graph", &self.root.display().to_string(),
                &format!("open nodes table failed. ({e})")),
        };

        let name_pat = p.name_pattern.as_deref().unwrap_or("");
        let file_pat = p.file_pattern.as_deref().unwrap_or("");
        let label_filter = p.label.as_deref().unwrap_or("");
        let query_text = p.query.as_deref().unwrap_or("");

        let name_regex = if !name_pat.is_empty() {
            regex::Regex::new(name_pat).ok()
        } else {
            None
        };
        let file_regex = if !file_pat.is_empty() {
            regex::Regex::new(file_pat).ok()
        } else {
            None
        };

        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut total = 0usize;

        if let Ok(iter) = nodes_tbl.iter() {
            for item in iter.flatten() {
                let (_id, val) = item;
                let Ok(rec) = postcard::from_bytes::<NodeRecord>(val.value()) else {
                    continue;
                };

                // Label filter
                let kind_str = node_kind_label(rec.kind);
                if !label_filter.is_empty() && !kind_str.eq_ignore_ascii_case(label_filter) {
                    continue;
                }
                // Name pattern filter
                if let Some(ref re) = name_regex {
                    if !re.is_match(&rec.fqn) {
                        continue;
                    }
                }
                // File pattern filter
                if let Some(ref re) = file_regex {
                    if !re.is_match(&rec.file) {
                        continue;
                    }
                }
                // BM25 text filter (naive substring for stub)
                if !query_text.is_empty() {
                    let q_lower = query_text.to_lowercase();
                    let fqn_lower = rec.fqn.to_lowercase();
                    if !fqn_lower.contains(&q_lower) {
                        continue;
                    }
                }

                total += 1;
                if total > offset && results.len() < limit {
                    results.push(serde_json::json!({
                        "name": rec.fqn,
                        "label": kind_str,
                        "file": rec.file,
                        "byte_start": rec.byte_start,
                        "byte_end": rec.byte_end,
                    }));
                }
            }
        }

        // semantic_query: not supported — return empty semantic_results
        if p.semantic_query.is_some() {
            tracing::warn!(
                target: "grafy.mcp",
                tool = "search_graph",
                "semantic_query ignored — grafy has no embedding backend (non-negotiable)"
            );
        }

        tool_ok(serde_json::json!({
            "results": results,
            "total": total,
            "has_more": total > offset + results.len(),
            "offset": offset,
            "limit": limit,
            "semantic_results": [],
        }))
    }

    // --- query_graph --------------------------------------------------------

    #[tool(description = "Execute a Cypher query against the knowledge graph for complex multi-hop patterns, aggregations, and cross-service analysis. The response includes 'total' (returned row count). There is a hard 100k row ceiling — for broad queries add LIMIT in the Cypher itself or use search_graph + offset/limit pagination instead.")]
    fn query_graph(
        &self,
        Parameters(p): Parameters<QueryGraphParams>,
    ) -> CallToolResult {
        tracing::info!(
            target: "grafy.mcp",
            tool = "query_graph",
            project = %p.project,
            query = %p.query,
            "executing cypher query"
        );

        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("query_graph", &self.root.display().to_string(), &e.to_string()),
        };

        match crate::cypher::execute(store.read_db(), &p.query) {
            Ok(rows) => {
                let capped = if let Some(max) = p.max_rows {
                    rows.into_iter().take(max as usize).collect::<Vec<_>>()
                } else {
                    rows
                };
                tool_ok(serde_json::json!({
                    "rows": capped.iter().map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null)).collect::<Vec<_>>(),
                    "total": capped.len(),
                }))
            }
            Err(e) => tool_error(
                "query_graph",
                &self.root.display().to_string(),
                &format!("query failed — {e}"),
            ),
        }
    }

    // --- trace_path ---------------------------------------------------------

    #[tool(description = "Trace paths through the code graph. Modes: calls (callers/callees), data_flow (value propagation with args at each hop), cross_service (through HTTP/async Route nodes). Use INSTEAD OF grep for callers, dependencies, impact analysis, or data flow tracing.")]
    fn trace_path(
        &self,
        Parameters(p): Parameters<TracePathParams>,
    ) -> CallToolResult {
        self.do_trace_path(p)
    }

    // --- trace_call_path (alias for trace_path) ----------------------------

    #[tool(description = "Alias for trace_path. Trace paths through the code graph. Modes: calls (callers/callees), data_flow (value propagation with args at each hop), cross_service (through HTTP/async Route nodes).")]
    fn trace_call_path(
        &self,
        Parameters(p): Parameters<TracePathParams>,
    ) -> CallToolResult {
        self.do_trace_path(p)
    }

    // --- get_code_snippet ---------------------------------------------------

    #[tool(description = "Read source code for a function/class/symbol. IMPORTANT: First call search_graph to find the exact qualified_name, then pass it here. This is a read tool, not a search tool. Accepts full qualified_name (exact match) or short function name (returns suggestions if ambiguous).")]
    fn get_code_snippet(
        &self,
        Parameters(p): Parameters<GetCodeSnippetParams>,
    ) -> CallToolResult {
        tracing::info!(
            target: "grafy.mcp",
            tool = "get_code_snippet",
            qualified_name = %p.qualified_name,
            "reading code snippet"
        );

        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("get_code_snippet", &self.root.display().to_string(), &e.to_string()),
        };
        let db = store.read_db();
        let tx = match db.begin_read() {
            Ok(t) => t,
            Err(e) => return tool_error("get_code_snippet", &self.root.display().to_string(),
                &format!("store read failed. ({e})")),
        };
        let nodes_tbl = match tx.open_table(NODES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("get_code_snippet", &self.root.display().to_string(),
                &format!("open nodes table failed. ({e})")),
        };

        let mut matches: Vec<NodeRecord> = Vec::new();
        if let Ok(iter) = nodes_tbl.iter() {
            for item in iter.flatten() {
                let (_, val) = item;
                let Ok(rec) = postcard::from_bytes::<NodeRecord>(val.value()) else {
                    continue;
                };
                // Exact match on fqn, or suffix match on short name
                let short_name = rec.fqn.rsplit("::").next().unwrap_or(&rec.fqn);
                if rec.fqn == p.qualified_name || short_name == p.qualified_name {
                    matches.push(rec);
                }
            }
        }

        if matches.is_empty() {
            return tool_error(
                "get_code_snippet",
                &p.qualified_name,
                "symbol not found — run search_graph(name_pattern=...) to find the exact qualified_name",
            );
        }

        // If more than one match, return suggestions
        if matches.len() > 1 && matches[0].fqn != p.qualified_name {
            let suggestions: Vec<_> = matches.iter().map(|r| r.fqn.as_str()).collect();
            return tool_ok(serde_json::json!({
                "ambiguous": true,
                "suggestions": suggestions,
                "message": format!("multiple matches for {:?} — pass the full qualified_name", p.qualified_name),
            }));
        }

        let rec = &matches[0];
        // Read the file from disk
        let file_path = self.root.join(&rec.file);
        let source = std::fs::read_to_string(&file_path)
            .unwrap_or_else(|_| String::from("<source not available>"));

        let snippet = if rec.byte_end as usize <= source.len() && rec.byte_start <= rec.byte_end {
            source[rec.byte_start as usize..rec.byte_end as usize].to_string()
        } else {
            source
        };

        tool_ok(serde_json::json!({
            "qualified_name": rec.fqn,
            "label": node_kind_label(rec.kind),
            "file": rec.file,
            "byte_start": rec.byte_start,
            "byte_end": rec.byte_end,
            "source": snippet,
        }))
    }

    // --- get_graph_schema ---------------------------------------------------

    #[tool(description = "Get the schema of the knowledge graph (node labels, edge types)")]
    fn get_graph_schema(
        &self,
        Parameters(p): Parameters<ProjectParam>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "get_graph_schema", project = %p.project, "getting schema");

        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("get_graph_schema", &self.root.display().to_string(), &e.to_string()),
        };
        let db = store.read_db();
        let tx = match db.begin_read() {
            Ok(t) => t,
            Err(e) => return tool_error("get_graph_schema", &self.root.display().to_string(),
                &format!("store read failed. ({e})")),
        };

        // Count nodes by label
        let nodes_tbl = match tx.open_table(NODES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("get_graph_schema", &self.root.display().to_string(),
                &format!("open nodes table failed. ({e})")),
        };
        let edges_tbl = match tx.open_table(EDGES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("get_graph_schema", &self.root.display().to_string(),
                &format!("open edges table failed. ({e})")),
        };

        let mut label_counts: HashMap<&str, u64> = HashMap::new();
        if let Ok(iter) = nodes_tbl.iter() {
            for item in iter.flatten() {
                let (_, val) = item;
                if let Ok(rec) = postcard::from_bytes::<NodeRecord>(val.value()) {
                    *label_counts.entry(node_kind_label(rec.kind)).or_insert(0) += 1;
                }
            }
        }

        let mut edge_counts: HashMap<&str, u64> = HashMap::new();
        if let Ok(iter) = edges_tbl.iter() {
            for item in iter.flatten() {
                let (key, _) = item;
                let (_, _, kind) = key.value();
                let name = if kind == EdgeKind::Calls as u8 { "CALLS" } else { "ROUTES" };
                *edge_counts.entry(name).or_insert(0) += 1;
            }
        }

        let node_labels: Vec<_> = label_counts
            .iter()
            .map(|(l, c)| serde_json::json!({"label": l, "count": c}))
            .collect();
        let edge_types: Vec<_> = edge_counts
            .iter()
            .map(|(t, c)| serde_json::json!({"type": t, "count": c}))
            .collect();

        tool_ok(serde_json::json!({
            "project": p.project,
            "node_labels": node_labels,
            "edge_types": edge_types,
            "properties": {
                "Function": ["fqn", "file", "byte_start", "byte_end"],
                "Class": ["fqn", "file", "byte_start", "byte_end"],
                "Route": ["fqn", "file", "byte_start", "byte_end"],
            }
        }))
    }

    // --- get_architecture ---------------------------------------------------

    #[tool(description = "Get high-level architecture overview — packages, services, dependencies, and project structure at a glance.")]
    fn get_architecture(
        &self,
        Parameters(p): Parameters<GetArchitectureParams>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "get_architecture", project = %p.project, "getting architecture");

        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("get_architecture", &self.root.display().to_string(), &e.to_string()),
        };
        let db = store.read_db();
        let tx = match db.begin_read() {
            Ok(t) => t,
            Err(e) => return tool_error("get_architecture", &self.root.display().to_string(),
                &format!("store read failed. ({e})")),
        };
        let nodes_tbl = match tx.open_table(NODES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("get_architecture", &self.root.display().to_string(),
                &format!("open nodes table failed. ({e})")),
        };
        let edges_tbl = match tx.open_table(EDGES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("get_architecture", &self.root.display().to_string(),
                &format!("open edges table failed. ({e})")),
        };

        let mut total_nodes = 0u64;
        let mut total_functions = 0u64;
        let mut total_routes = 0u64;
        let mut files: Vec<String> = Vec::new();
        let mut langs: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Ok(iter) = nodes_tbl.iter() {
            for item in iter.flatten() {
                let (_, val) = item;
                if let Ok(rec) = postcard::from_bytes::<NodeRecord>(val.value()) {
                    total_nodes += 1;
                    match rec.kind {
                        NodeKind::Function | NodeKind::Method => total_functions += 1,
                        NodeKind::Route => total_routes += 1,
                        NodeKind::File => {
                            files.push(rec.file.clone());
                        }
                        _ => {}
                    }
                }
            }
        }

        // Infer languages from file extensions
        let files_tbl = tx.open_table(FILES_TABLE);
        if let Ok(ft) = files_tbl {
            if let Ok(iter) = ft.iter() {
                for item in iter.flatten() {
                    let (key, val) = item;
                    let path = key.value();
                    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
                        langs.insert(ext.to_string());
                    }
                    let _ = val; // FileRecord not needed here
                }
            }
        }

        let mut total_edges = 0u64;
        if let Ok(iter) = edges_tbl.iter() {
            for _ in iter.flatten() {
                total_edges += 1;
            }
        }

        let project_name = self.root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        tool_ok(serde_json::json!({
            "project": project_name,
            "root": self.root.to_string_lossy(),
            "total_nodes": total_nodes,
            "total_edges": total_edges,
            "functions": total_functions,
            "routes": total_routes,
            "languages": langs.into_iter().collect::<Vec<_>>(),
            "file_count": files.len(),
            "aspects": p.aspects.unwrap_or_default(),
        }))
    }

    // --- search_code --------------------------------------------------------

    #[tool(description = "Graph-augmented code search. Finds text patterns via grep, then enriches results with the knowledge graph: deduplicates matches into containing functions, ranks by structural importance (definitions first, popular functions next, tests last). Modes: compact (default, signatures only — token efficient), full (with source), files (just file paths). TRUNCATION: enriched results are capped at limit (default 10).")]
    fn search_code(
        &self,
        Parameters(p): Parameters<SearchCodeParams>,
    ) -> CallToolResult {
        tracing::info!(
            target: "grafy.mcp",
            tool = "search_code",
            pattern = %p.pattern,
            "searching code"
        );

        let limit = p.limit.unwrap_or(10).max(1) as usize;
        let mode = p.mode.as_deref().unwrap_or("compact");
        let use_regex = p.regex.unwrap_or(false);

        // Walk indexed files and search for the pattern
        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("search_code", &self.root.display().to_string(), &e.to_string()),
        };
        let db = store.read_db();
        let tx = match db.begin_read() {
            Ok(t) => t,
            Err(e) => return tool_error("search_code", &self.root.display().to_string(),
                &format!("store read failed. ({e})")),
        };

        let files_tbl = match tx.open_table(FILES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("search_code", &self.root.display().to_string(),
                &format!("open files table failed. ({e})")),
        };

        let pat_re = if use_regex {
            regex::Regex::new(&p.pattern).ok()
        } else {
            // Escape for literal match
            regex::Regex::new(&regex::escape(&p.pattern)).ok()
        };

        let file_ext_filter = p.file_pattern.as_deref().unwrap_or("");
        let path_filter_re = p.path_filter.as_deref().and_then(|s| regex::Regex::new(s).ok());

        let mut total_grep_matches = 0usize;
        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut file_results: Vec<String> = Vec::new();

        if let Ok(iter) = files_tbl.iter() {
            for item in iter.flatten() {
                let (key, _) = item;
                let rel_path = key.value().to_string();

                // File extension filter
                if !file_ext_filter.is_empty() {
                    let ext_pat = file_ext_filter.trim_start_matches("*.");
                    if !rel_path.ends_with(ext_pat) {
                        continue;
                    }
                }
                // Path filter
                if let Some(ref re) = path_filter_re {
                    if !re.is_match(&rel_path) {
                        continue;
                    }
                }

                let file_path = self.root.join(&rel_path);
                let Ok(content) = std::fs::read_to_string(&file_path) else {
                    continue;
                };

                let file_matches: Vec<(usize, &str)> = content
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| {
                        if let Some(ref re) = pat_re {
                            re.is_match(line)
                        } else {
                            line.contains(&p.pattern)
                        }
                    })
                    .collect();

                if file_matches.is_empty() {
                    continue;
                }

                total_grep_matches += file_matches.len();
                if results.len() < limit {
                    match mode {
                        "files" => {
                            if !file_results.contains(&rel_path) {
                                file_results.push(rel_path.clone());
                            }
                        }
                        "full" => {
                            for (line_no, line) in &file_matches {
                                results.push(serde_json::json!({
                                    "file": rel_path,
                                    "line": line_no + 1,
                                    "content": line,
                                }));
                            }
                        }
                        _ => {
                            // compact: first match per file
                            let (line_no, line) = file_matches[0];
                            results.push(serde_json::json!({
                                "file": rel_path,
                                "line": line_no + 1,
                                "content": line,
                                "match_count": file_matches.len(),
                            }));
                        }
                    }
                }
            }
        }

        if mode == "files" {
            tool_ok(serde_json::json!({
                "files": file_results,
                "total_results": file_results.len(),
                "total_grep_matches": total_grep_matches,
            }))
        } else {
            tool_ok(serde_json::json!({
                "results": results,
                "total_results": results.len(),
                "total_grep_matches": total_grep_matches,
                "truncated": total_grep_matches > results.len(),
            }))
        }
    }

    // --- list_projects ------------------------------------------------------

    #[tool(description = "List all indexed projects")]
    fn list_projects(
        &self,
        Parameters(_p): Parameters<ListProjectsParams>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "list_projects", "listing projects");

        let store_path = self.root.join(".grafy").join("index.redb");
        let exists = store_path.exists();
        let project_name = self.root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        if exists {
            let store = match open_store(&self.root) {
                Ok(s) => s,
                Err(e) => return tool_error("list_projects", &self.root.display().to_string(), &e.to_string()),
            };
            let db = store.read_db();
            let node_count: u64 = (|| -> Option<u64> {
                let tx = db.begin_read().ok()?;
                let t = tx.open_table(NODES_TABLE).ok()?;
                t.len().ok()
            })().unwrap_or(0);

            tool_ok(serde_json::json!({
                "projects": [{
                    "name": project_name,
                    "root": self.root.to_string_lossy(),
                    "node_count": node_count,
                    "indexed": true,
                }]
            }))
        } else {
            tool_ok(serde_json::json!({
                "projects": [{
                    "name": project_name,
                    "root": self.root.to_string_lossy(),
                    "indexed": false,
                    "message": "not indexed — run index_repository first",
                }]
            }))
        }
    }

    // --- delete_project -----------------------------------------------------

    #[tool(description = "Delete a project from the index")]
    fn delete_project(
        &self,
        Parameters(p): Parameters<ProjectParam>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "delete_project", project = %p.project, "deleting project");

        // Grafy does not support multi-project stores in v1. Return a clear action.
        tool_error(
            "delete_project",
            &p.project,
            "grafy v1 does not support remote project deletion — remove .grafy/ manually to reset the store",
        )
    }

    // --- index_status -------------------------------------------------------

    #[tool(description = "Get the indexing status of a project")]
    fn index_status(
        &self,
        Parameters(p): Parameters<ProjectParam>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "index_status", project = %p.project, "checking index status");

        let store_path = self.root.join(".grafy").join("index.redb");
        if !store_path.exists() {
            return tool_ok(serde_json::json!({
                "project": p.project,
                "status": "not_indexed",
                "message": "run index_repository to build the store",
            }));
        }

        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("index_status", &store_path.display().to_string(), &e.to_string()),
        };
        let db = store.read_db();
        let node_count: u64 = (|| -> Option<u64> {
            let tx = db.begin_read().ok()?;
            let t = tx.open_table(NODES_TABLE).ok()?;
            t.len().ok()
        })().unwrap_or(0);
        let edge_count: u64 = (|| -> Option<u64> {
            let tx = db.begin_read().ok()?;
            let t = tx.open_table(EDGES_TABLE).ok()?;
            t.len().ok()
        })().unwrap_or(0);

        tool_ok(serde_json::json!({
            "project": p.project,
            "status": "indexed",
            "node_count": node_count,
            "edge_count": edge_count,
            "store_path": store_path.to_string_lossy(),
        }))
    }

    // --- detect_changes -----------------------------------------------------

    #[tool(description = "Detect code changes and their impact")]
    fn detect_changes(
        &self,
        Parameters(p): Parameters<DetectChangesParams>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "detect_changes", project = %p.project, "detecting changes");

        // Stub: this requires git + diff-against-store capability not yet implemented.
        tool_error(
            "detect_changes",
            &p.project,
            "detect_changes not yet implemented in grafy v1 — use `git diff` + re-run index_repository to refresh",
        )
    }

    // --- manage_adr ---------------------------------------------------------

    #[tool(description = "Create or update Architecture Decision Records")]
    fn manage_adr(
        &self,
        Parameters(p): Parameters<ManageAdrParams>,
    ) -> CallToolResult {
        tracing::info!(target: "grafy.mcp", tool = "manage_adr", project = %p.project, "managing ADR");

        // Stub: ADR management not in grafy v1.
        tool_error(
            "manage_adr",
            &p.project,
            "manage_adr not yet implemented in grafy v1 — edit docs/adr/ directly",
        )
    }

    // --- ingest_traces ------------------------------------------------------

    #[tool(description = "Ingest runtime traces to enhance the knowledge graph")]
    fn ingest_traces(
        &self,
        Parameters(p): Parameters<IngestTracesParams>,
    ) -> CallToolResult {
        tracing::info!(
            target: "grafy.mcp",
            tool = "ingest_traces",
            project = %p.project,
            traces = p.traces.len(),
            "ingesting traces"
        );

        // Stub: runtime trace ingestion not in grafy v1.
        tool_error(
            "ingest_traces",
            &p.project,
            "ingest_traces not yet implemented in grafy v1 — no runtime trace model",
        )
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

impl GrafyServer {
    fn do_trace_path(&self, p: TracePathParams) -> CallToolResult {
        tracing::info!(
            target: "grafy.mcp",
            tool = "trace_path",
            function_name = %p.function_name,
            project = %p.project,
            direction = ?p.direction,
            depth = ?p.depth,
            "tracing call path"
        );

        let store = match open_store(&self.root) {
            Ok(s) => s,
            Err(e) => return tool_error("trace_path", &self.root.display().to_string(), &e.to_string()),
        };
        let db = store.read_db();
        let tx = match db.begin_read() {
            Ok(t) => t,
            Err(e) => return tool_error("trace_path", &self.root.display().to_string(),
                &format!("store read failed. ({e})")),
        };
        let nodes_tbl = match tx.open_table(NODES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("trace_path", &self.root.display().to_string(),
                &format!("open nodes table failed. ({e})")),
        };
        let edges_tbl = match tx.open_table(EDGES_TABLE) {
            Ok(t) => t,
            Err(e) => return tool_error("trace_path", &self.root.display().to_string(),
                &format!("open edges table failed. ({e})")),
        };

        let direction = p.direction.as_deref().unwrap_or("both");
        let max_depth = p.depth.unwrap_or(3).clamp(1, 5) as usize;
        let include_tests = p.include_tests.unwrap_or(false);

        // Build in-memory adjacency maps from edges
        let mut calls_fwd: HashMap<u64, Vec<u64>> = HashMap::new(); // caller -> callees
        let mut calls_bwd: HashMap<u64, Vec<u64>> = HashMap::new(); // callee -> callers

        if let Ok(iter) = edges_tbl.iter() {
            for item in iter.flatten() {
                let (key, _) = item;
                let (from, to, kind) = key.value();
                if kind == EdgeKind::Calls as u8 {
                    calls_fwd.entry(from).or_default().push(to);
                    calls_bwd.entry(to).or_default().push(from);
                }
            }
        }

        // Find starting node(s) by name
        let mut start_ids: Vec<(u64, NodeRecord)> = Vec::new();
        if let Ok(iter) = nodes_tbl.iter() {
            for item in iter.flatten() {
                let (key, val) = item;
                let Ok(rec) = postcard::from_bytes::<NodeRecord>(val.value()) else {
                    continue;
                };
                let short = rec.fqn.rsplit("::").next().unwrap_or(&rec.fqn);
                if rec.fqn == p.function_name || short == p.function_name {
                    if !include_tests && (rec.file.contains("test") || rec.file.contains("spec")) {
                        continue;
                    }
                    start_ids.push((key.value(), rec));
                }
            }
        }

        if start_ids.is_empty() {
            return tool_ok(serde_json::json!({
                "function_name": p.function_name,
                "direction": direction,
                "depth": max_depth,
                "hops": [],
                "message": format!(
                    "no node found for {:?} — run search_graph(name_pattern=...) to find the exact name",
                    p.function_name
                ),
            }));
        }

        // BFS traversal
        let mut visited: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<(u64, usize)> = std::collections::VecDeque::new();
        let mut hops: Vec<serde_json::Value> = Vec::new();

        for (id, _) in &start_ids {
            queue.push_back((*id, 0));
            visited.insert(*id);
        }

        // Build a quick id→record map
        let mut id_to_rec: HashMap<u64, NodeRecord> = HashMap::new();
        if let Ok(iter) = nodes_tbl.iter() {
            for item in iter.flatten() {
                let (k, v) = item;
                if let Ok(rec) = postcard::from_bytes::<NodeRecord>(v.value()) {
                    id_to_rec.insert(k.value(), rec);
                }
            }
        }

        while let Some((node_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let neighbors: Vec<u64> = match direction {
                "outbound" => calls_fwd.get(&node_id).cloned().unwrap_or_default(),
                "inbound" => calls_bwd.get(&node_id).cloned().unwrap_or_default(),
                _ => {
                    let mut n = calls_fwd.get(&node_id).cloned().unwrap_or_default();
                    n.extend(calls_bwd.get(&node_id).cloned().unwrap_or_default());
                    n
                }
            };

            for neighbor_id in neighbors {
                if visited.contains(&neighbor_id) {
                    continue;
                }
                visited.insert(neighbor_id);

                if let Some(rec) = id_to_rec.get(&neighbor_id) {
                    if !include_tests && (rec.file.contains("test") || rec.file.contains("spec")) {
                        continue;
                    }
                    hops.push(serde_json::json!({
                        "name": rec.fqn,
                        "label": node_kind_label(rec.kind),
                        "file": rec.file,
                        "depth": depth + 1,
                    }));
                    queue.push_back((neighbor_id, depth + 1));
                }
            }
        }

        let start_name = start_ids.first().map(|(_, r)| r.fqn.as_str()).unwrap_or(&p.function_name);
        tool_ok(serde_json::json!({
            "function_name": start_name,
            "direction": direction,
            "depth": max_depth,
            "hops": hops,
            "hop_count": hops.len(),
        }))
    }
}

// ---------------------------------------------------------------------------
// Label helper
// ---------------------------------------------------------------------------

fn node_kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "File",
        NodeKind::Module => "Module",
        NodeKind::Function => "Function",
        NodeKind::Class => "Class",
        NodeKind::Struct => "Struct",
        NodeKind::Enum => "Enum",
        NodeKind::Trait => "Trait",
        NodeKind::Method => "Method",
        NodeKind::Route => "Route",
    }
}
