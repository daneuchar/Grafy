//! Pass 4 — HTTP route ↔ call-site linking (M1 W4).
//!
//! Strategy:
//!
//! For each file in the indexed set, detect which HTTP framework (if any) it
//! uses via a cheap lexical scan (`routes::detect_framework`). If a framework
//! is detected, run the corresponding `routes.scm` tree-sitter query.
//!
//! For each matched route:
//!   1. Emit `WriteEvent::Node` for a synthetic Route node whose FQN is
//!      `"METHOD /pattern"` and kind is `NodeKind::Route`.
//!   2. Attempt to resolve the handler name in the same file's module scope
//!      via the shared `SymbolTable`. If resolved, emit a
//!      `WriteEvent::Edge { kind: EdgeKind::Routes }` from the Route node to
//!      the handler node.
//!   3. If the handler is anonymous (Express inline arrow/function expression),
//!      the synthetic handler name is `<file>:<line>`. The Route node is still
//!      emitted but no edge is produced (per spec: surface the route, don't
//!      resolve anonymous handlers).
//!
//! **5-second per-file timeout** is enforced; on overrun the file's routes are
//! skipped with a `warn!` log.
//!
//! **Single-writer discipline** — only `WriteEvent::Node` and `WriteEvent::Edge`
//! are emitted; no direct redb access.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::Sender;
use rayon::prelude::*;
use tracing::{debug, info_span, warn};
use tree_sitter::{Query, QueryCursor};

use grafy_parser::{Language, PER_FILE_TIMEOUT};

use crate::pipeline::cache::ParseCache;
use crate::pipeline::channels::{EdgeWriteEvent, NodeWriteEvent, WriteEvent};
use crate::pipeline::pass3::{build_symbol_table, ts_language_for, SymbolTable};
use crate::routes::{detect_framework, routes_scm, Framework};
use crate::store::{node_id, EdgeKind, NodeKind, NodeRecord};

// ---------------------------------------------------------------------------
// Per-route extracted data
// ---------------------------------------------------------------------------

struct RouteMatch {
    method: String,
    pattern: String,
    /// Named handler identifier; `None` if the handler is anonymous.
    handler_name: Option<String>,
    /// Byte offset of the route node in source (decorator or call expression).
    byte_start: u32,
    byte_end: u32,
    /// Line number for synthetic anonymous handler name (1-based).
    handler_line: u32,
}

// ---------------------------------------------------------------------------
// Query execution helpers
// ---------------------------------------------------------------------------

fn extract_routes(
    bytes: &[u8],
    source_str: &str,
    tree: &tree_sitter::Tree,
    fw: Framework,
    lang: Language,
) -> Vec<RouteMatch> {
    let ts_lang = ts_language_for(lang);
    let scm = routes_scm(fw);
    let query = match Query::new(&ts_lang, scm) {
        Ok(q) => q,
        Err(e) => {
            debug!(
                target: "grafy.pass4",
                framework = ?fw,
                language = lang.as_str(),
                "routes.scm compile error: {e}"
            );
            return vec![];
        }
    };

    let capture_names: Vec<String> = query
        .capture_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut cursor = QueryCursor::new();
    let mut routes = Vec::new();

    for m in cursor.matches(&query, tree.root_node(), bytes) {
        let mut method: Option<String> = None;
        let mut pattern: Option<String> = None;
        let mut handler_name: Option<String> = None;
        // Track position of the outermost node in this match.
        let mut match_start: u32 = 0;
        let mut match_end: u32 = 0;
        let mut inline_handler_line: u32 = 0;

        for cap in m.captures {
            let cap_name = capture_names[cap.index as usize].as_str();
            let text = &source_str[cap.node.start_byte()..cap.node.end_byte()];
            let start = cap.node.start_byte() as u32;
            let end = cap.node.end_byte() as u32;

            // Track the overall extent of the match (the route declaration).
            if match_start == 0 || start < match_start {
                match_start = start;
            }
            if end > match_end {
                match_end = end;
            }

            match cap_name {
                "route.method" => {
                    method = Some(text.to_uppercase());
                }
                "route.pattern" => {
                    // Strip surrounding quotes.
                    pattern = Some(strip_route_quotes(text));
                }
                "route.handler" => {
                    handler_name = Some(text.to_owned());
                }
                "route.inline_handler" => {
                    // Anonymous handler — record line for synthetic name.
                    inline_handler_line = cap.node.start_position().row as u32 + 1;
                }
                _ => {}
            }
        }

        let (method, pattern) = match (method, pattern) {
            (Some(m), Some(p)) => (m, p),
            _ => continue, // incomplete match — skip
        };

        // If we have an explicit handler name, use it; otherwise synthesise
        // from the inline_handler line (if the inline variant fired).
        let resolved_handler = if handler_name.is_some() {
            handler_name
        } else if inline_handler_line > 0 {
            // Pass4 spec: surface as route with synthetic name; don't resolve.
            None
        } else {
            None
        };

        routes.push(RouteMatch {
            method,
            pattern,
            handler_name: resolved_handler,
            byte_start: match_start,
            byte_end: match_end,
            handler_line: inline_handler_line,
        });
    }

    routes
}

/// Strip quotes and template-literal backticks from a route pattern string.
fn strip_route_quotes(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('\'') && s.ends_with('\''))
            || (s.starts_with('`') && s.ends_with('`')))
    {
        s[1..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Per-file route pass
// ---------------------------------------------------------------------------

/// Collect and emit route nodes + edges for a single source file.
/// Returns `Vec<WriteEvent>` which is then sent to the writer thread.
fn process_file(
    file_path: &Path,
    root: &Path,
    lang: Language,
    sym: &SymbolTable,
    cache: &ParseCache,
) -> Vec<WriteEvent> {
    let started = Instant::now();

    // --- Obtain bytes + tree: prefer cache, fall back to fresh read + parse. ---
    let cached = cache.get(file_path).map(|r| Arc::clone(&*r));

    let (bytes_owned, tree_owned);
    let (bytes, tree): (&[u8], &tree_sitter::Tree) = if let Some(ref pf) = cached {
        (&pf.source, &pf.tree)
    } else {
        bytes_owned = match std::fs::read(file_path) {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    target: "grafy.pass4",
                    file = %file_path.display(),
                    language = lang.as_str(),
                    "read failed — check file permissions. ({})", e
                );
                return vec![];
            }
        };
        tree_owned = match grafy_parser::parse(&file_path.display().to_string(), lang, &bytes_owned)
        {
            Ok(t) => t,
            Err(e) => {
                warn!(
                    target: "grafy.pass4",
                    file = %file_path.display(),
                    language = lang.as_str(),
                    "reparse failed — verify valid {} source. ({})", lang.as_str(), e
                );
                return vec![];
            }
        };
        (&bytes_owned, &tree_owned)
    };

    // Framework detection: cheap lexical scan.
    let fw = match detect_framework(lang, file_path, bytes) {
        Some(fw) => fw,
        None => return vec![],
    };

    if started.elapsed() > PER_FILE_TIMEOUT {
        warn!(
            target: "grafy.pass4",
            file = %file_path.display(),
            framework = ?fw,
            "per-file timeout exceeded before query extraction — skipping routes. Split the file or open an issue."
        );
        return vec![];
    }

    let source_str = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            warn!(
                target: "grafy.pass4",
                file = %file_path.display(),
                framework = ?fw,
                "non-UTF-8 source — re-encode to UTF-8 and retry."
            );
            return vec![];
        }
    };

    if started.elapsed() > PER_FILE_TIMEOUT {
        warn!(
            target: "grafy.pass4",
            file = %file_path.display(),
            framework = ?fw,
            "per-file timeout exceeded during query extraction — skipping routes. Split the file or open an issue."
        );
        return vec![];
    }

    let route_matches = extract_routes(bytes, source_str, tree, fw, lang);

    if route_matches.is_empty() {
        return vec![];
    }

    let rel_path = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .into_owned();

    let mut events: Vec<WriteEvent> = Vec::with_capacity(route_matches.len() * 2);

    for rm in &route_matches {
        // FQN for the Route node: "METHOD /pattern".
        let route_fqn = format!("{} {}", rm.method, rm.pattern);
        let route_id = node_id(&rel_path, &route_fqn, NodeKind::Route, rm.byte_start);

        // Emit the Route node.
        events.push(WriteEvent::Node(NodeWriteEvent {
            id: route_id,
            record: NodeRecord {
                fqn: route_fqn.clone(),
                kind: NodeKind::Route,
                file: rel_path.clone(),
                byte_start: rm.byte_start,
                byte_end: rm.byte_end,
            },
        }));

        // Attempt handler resolution.
        let handler_id = if let Some(handler_name) = &rm.handler_name {
            sym.resolve_short(handler_name, file_path)
        } else {
            // Anonymous inline handler — synthesise name but don't resolve.
            if rm.handler_line > 0 {
                debug!(
                    target: "grafy.pass4",
                    file = %file_path.display(),
                    route = %route_fqn,
                    line = rm.handler_line,
                    "anonymous inline handler — route node emitted without edge"
                );
            }
            None
        };

        if let Some(handler_id) = handler_id {
            // Emit the Routes edge: Route node → handler node.
            events.push(WriteEvent::Edge(EdgeWriteEvent {
                from: route_id,
                to: handler_id,
                kind: EdgeKind::Routes as u8,
            }));
        } else if let Some(handler_name) = &rm.handler_name {
            debug!(
                target: "grafy.pass4",
                file = %file_path.display(),
                route = %route_fqn,
                handler = %handler_name,
                "handler unresolved — route node emitted without edge"
            );
        }
    }

    events
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run pass 4 using a pre-built symbol table and pre-computed file list.
///
/// Called from the pipeline orchestrator after pass3 shares the symbol table.
/// Emits `WriteEvent::Node` (Route) and `WriteEvent::Edge` (Routes) entries
/// via `write_tx`.
/// `cache` provides pre-parsed trees from pass 1 so pass 4 avoids re-reading
/// and re-parsing each file (W6.5 fix).
pub fn run_with_table(
    root: &Path,
    files: &[(PathBuf, Language)],
    sym: &SymbolTable,
    write_tx: Sender<WriteEvent>,
    cache: &ParseCache,
) {
    let span = info_span!("pass4.routes");
    let _e = span.enter();

    tracing::info!(
        target: "grafy.pass4",
        files = files.len(),
        "scanning for HTTP routes"
    );

    // Parallel route extraction via rayon.
    let all_events: Vec<WriteEvent> = files
        .par_iter()
        .flat_map(|(path, lang)| process_file(path, root, *lang, sym, cache))
        .collect();

    let node_count = all_events
        .iter()
        .filter(|e| matches!(e, WriteEvent::Node(_)))
        .count();
    let edge_count = all_events
        .iter()
        .filter(|e| matches!(e, WriteEvent::Edge(_)))
        .count();

    for ev in all_events {
        let _ = write_tx.send(ev);
    }

    tracing::info!(
        target: "grafy.pass4",
        route_nodes = node_count,
        route_edges = edge_count,
        "pass4 complete"
    );
}

/// Convenience entry: drain `definitions` slice, build symbol table, run pass4.
/// Used by tests that don't go through the full pipeline.
pub fn run(
    root: &Path,
    definitions: &[crate::pipeline::channels::DefinitionEvent],
    files: &[(PathBuf, Language)],
    write_tx: Sender<WriteEvent>,
) {
    let sym = build_symbol_table(definitions, root);
    // No cache in standalone mode — each file falls back to fresh read + parse.
    let empty_cache = ParseCache::new();
    run_with_table(root, files, &sym, write_tx, &empty_cache);
}

// ---------------------------------------------------------------------------
// Unit tests for query compilation
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use grafy_parser::Language;
    use tree_sitter::Query;

    #[test]
    fn fastapi_scm_compiles() {
        let lang = ts_language_for(Language::Python);
        let result = Query::new(&lang, routes_scm(Framework::FastApi));
        assert!(result.is_ok(), "FastAPI routes.scm: {:?}", result.err());
    }

    #[test]
    fn gin_scm_compiles() {
        let lang = ts_language_for(Language::Go);
        let result = Query::new(&lang, routes_scm(Framework::Gin));
        assert!(result.is_ok(), "Gin routes.scm: {:?}", result.err());
    }

    #[test]
    fn express_scm_compiles() {
        let lang = ts_language_for(Language::JavaScript);
        let result = Query::new(&lang, routes_scm(Framework::Express));
        assert!(result.is_ok(), "Express routes.scm: {:?}", result.err());
    }
}
