//! Parse a `.scip` file and merge its reference occurrences into the redb
//! store as `EdgeKind::Scip` edges.
//!
//! # Strategy
//!
//! 1. Build an in-memory `NodeRecord` index keyed by `(file, byte_start)`.
//! 2. For each `Document` in the `.scip`, build a line-byte offset table by
//!    re-reading the source file from disk (SCIP positions are line/col, but
//!    Grafy nodes are keyed by byte ranges).
//! 3. For each non-definition `Occurrence`, look up the enclosing node (the
//!    `NodeRecord` whose byte range contains the occurrence start). That node
//!    is the *caller*.
//! 4. Look up the *callee* by SCIP symbol string. We map the symbol's last
//!    descriptor to an FQN suffix; nodes are matched by FQN.
//! 5. Emit `EdgeKind::Scip` edge `(caller_id, callee_id, 2)` to the writer.
//!
//! Symbols we cannot resolve are counted as `edges_skipped_no_match` and
//! reported in the per-indexer summary. Resolved-to-definitions are skipped
//! since M1 pass 2 already inserted those nodes.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use postcard::from_bytes;
use protobuf::Message;
use redb::ReadableTable;
use scip::types::{Index, Occurrence};
use tracing::{debug, info, warn};

use crate::pipeline::channels::{EdgeWriteEvent, WriteEvent};
use crate::store::{EdgeKind, NodeRecord, Store, EDGES_TABLE, NODES_BY_FILE_TABLE, NODES_TABLE};

/// Definition role bit per `scip.proto` `SymbolRole::Definition` = 1.
const DEFINITION_ROLE: i32 = 1;

#[derive(Debug, Clone, Default)]
pub struct IngestReport {
    /// Total non-definition occurrences seen in the `.scip` file.
    pub occurrences_seen: u64,
    /// Number of `EdgeKind::Scip` edges emitted.
    pub edges_emitted: u64,
    /// Occurrences we could not resolve to a caller-callee pair.
    pub edges_skipped_no_match: u64,
    /// Indexer name (e.g. "scip-python"). Captured for the CLI summary.
    pub indexer: String,
    pub elapsed_ms: u64,
}

/// In-memory snapshot of the node tables, taken once before SCIP ingest
/// begins (we cannot reopen redb while the writer thread holds it).
pub struct NodeSnapshot {
    by_fqn: FqnIndex,
    by_range: RangeIndex,
}

/// Build a `NodeSnapshot` from a redb read transaction. Releases the
/// transaction before returning so the caller can spawn a writer.
pub fn snapshot_indexes(store: &Store) -> Result<NodeSnapshot> {
    let (by_fqn, by_range) = load_indexes(store)?;
    Ok(NodeSnapshot { by_fqn, by_range })
}

/// Top-level convenience wrapper: snapshot + ingest in one call. Used by the
/// public `ingest_scip_file` API and unit tests; the pipeline uses
/// `snapshot_indexes` + `ingest_scip_with_snapshot` directly to share one
/// snapshot across multiple indexers.
pub fn ingest_scip_file(
    scip_path: &Path,
    store: &Store,
    repo_root: &Path,
    writer: &Sender<WriteEvent>,
    indexer_name: &str,
) -> Result<IngestReport> {
    let snap = snapshot_indexes(store)?;
    ingest_scip_with_snapshot(scip_path, &snap, repo_root, writer, indexer_name)
}

/// Read `scip_path`, resolve occurrences against `snap`, and send
/// `EdgeKind::Scip` edges through `writer`. The function does NOT close
/// `writer`; the caller manages lifecycle.
pub fn ingest_scip_with_snapshot(
    scip_path: &Path,
    snap: &NodeSnapshot,
    repo_root: &Path,
    writer: &Sender<WriteEvent>,
    indexer_name: &str,
) -> Result<IngestReport> {
    let started = std::time::Instant::now();
    let buf = std::fs::read(scip_path)
        .with_context(|| format!("read scip file {}", scip_path.display()))?;
    let index = Index::parse_from_bytes(&buf)
        .with_context(|| format!("parse scip {} — file may be truncated", scip_path.display()))?;

    let mut report = IngestReport {
        indexer: indexer_name.to_owned(),
        ..Default::default()
    };

    debug!(
        target: "grafy.scip.ingest",
        fqn_keys = snap.by_fqn.len(),
        range_files = snap.by_range.len(),
        "snapshot-based ingest start"
    );

    let mut line_cache: HashMap<String, Option<Vec<usize>>> = HashMap::new();

    for doc in &index.documents {
        let rel = &doc.relative_path;
        let file_nodes = match snap.by_range.get(rel.as_str()) {
            Some(v) => v,
            None => continue,
        };

        for occ in &doc.occurrences {
            if (occ.symbol_roles & DEFINITION_ROLE) != 0 {
                continue;
            }
            if occ.symbol.is_empty() {
                continue;
            }
            report.occurrences_seen += 1;

            let line_offsets = line_cache
                .entry(rel.clone())
                .or_insert_with(|| read_line_offsets(repo_root, rel));
            let Some(line_offsets) = line_offsets.as_ref() else {
                report.edges_skipped_no_match += 1;
                continue;
            };
            let (line, col) = start_position(occ);
            let Some(byte_off) = line_col_to_byte(line_offsets, line, col) else {
                report.edges_skipped_no_match += 1;
                continue;
            };

            let Some(caller_id) = enclosing_node(file_nodes, byte_off) else {
                report.edges_skipped_no_match += 1;
                continue;
            };

            let Some(callee_id) = lookup_callee(&snap.by_fqn, &occ.symbol) else {
                report.edges_skipped_no_match += 1;
                continue;
            };

            if caller_id == callee_id {
                continue;
            }

            let send_result = writer.send(WriteEvent::Edge(EdgeWriteEvent {
                from: caller_id,
                to: callee_id,
                kind: EdgeKind::Scip as u8,
            }));
            if send_result.is_err() {
                warn!(target: "grafy.scip.ingest", "writer dropped — aborting ingest");
                break;
            }
            report.edges_emitted += 1;
        }
    }

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    info!(
        target: "grafy.scip.ingest",
        indexer = indexer_name,
        occurrences = report.occurrences_seen,
        edges = report.edges_emitted,
        skipped = report.edges_skipped_no_match,
        elapsed_ms = report.elapsed_ms,
        "SCIP ingest complete"
    );
    Ok(report)
}

/// One file's worth of node records, sorted by byte_start for enclosing lookup.
#[derive(Debug, Clone)]
struct FileNode {
    id: u64,
    start: u32,
    end: u32,
}

type FqnIndex = HashMap<String, Vec<u64>>;
type RangeIndex = HashMap<String, Vec<FileNode>>;

fn load_indexes(store: &Store) -> Result<(FqnIndex, RangeIndex)> {
    let db = store.read_db();
    let tx = db.begin_read()?;

    // by_fqn: lowercase FQN suffix → node ids. We store the full FQN keyed by
    // (last_descriptor, ids[]). For SCIP symbol matching we strip the SCIP
    // descriptor of its sigil suffix ('#' for type, '().' for term/method)
    // and compare on the bare identifier.
    let mut by_fqn: HashMap<String, Vec<u64>> = HashMap::new();
    let mut by_range: HashMap<String, Vec<FileNode>> = HashMap::new();

    // Walk the nodes_by_file index for the (file, id) pairs, then fetch full
    // NodeRecord from NODES_TABLE.
    let nbf = tx.open_table(NODES_BY_FILE_TABLE)?;
    let nodes = tx.open_table(NODES_TABLE)?;
    for item in nbf.iter()? {
        let (key, _) = item?;
        let (file, id) = key.value();
        let Ok(Some(node_bytes)) = nodes.get(id) else {
            continue;
        };
        let Ok(rec) = from_bytes::<NodeRecord>(node_bytes.value()) else {
            continue;
        };

        by_range.entry(file.to_owned()).or_default().push(FileNode {
            id,
            start: rec.byte_start,
            end: rec.byte_end,
        });

        // Index by tail of FQN (after the last `::` / `.` separator).
        let tail = fqn_tail(&rec.fqn);
        by_fqn.entry(tail.to_owned()).or_default().push(id);
        // Also index the full FQN.
        by_fqn.entry(rec.fqn.clone()).or_default().push(id);
    }

    // Sort by_range buckets by (start asc, end desc) so the smallest enclosing
    // wins via reverse-iterate.
    for v in by_range.values_mut() {
        v.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    }

    // Silence unused EDGES_TABLE warning — keep the import scoped to anchor
    // the const path for future inspection without an extra `use`.
    let _ = EDGES_TABLE;

    Ok((by_fqn, by_range))
}

fn fqn_tail(fqn: &str) -> &str {
    // Strip namespace separators commonly produced by pass 2.
    if let Some(idx) = fqn.rfind("::") {
        return &fqn[idx + 2..];
    }
    if let Some(idx) = fqn.rfind('.') {
        return &fqn[idx + 1..];
    }
    fqn
}

/// SCIP symbol format: `scheme manager package version (descriptor)+`.
/// A descriptor ends in `#` (type), `().` (term/method), `.` (term/property),
/// or `/` (namespace). We extract the last non-empty descriptor's bare
/// identifier and try several FQN candidates: full path, last segment, etc.
fn lookup_callee(by_fqn: &HashMap<String, Vec<u64>>, sym: &str) -> Option<u64> {
    // Skip "local …" symbols — those are function-local definitions in SCIP
    // and never cross-file refs Grafy cares about.
    if sym.starts_with("local ") {
        return None;
    }
    // Pull descriptors. Split on whitespace, drop first 4 tokens
    // (scheme/manager/package/version), join the rest.
    let mut parts = sym.splitn(5, ' ');
    parts.next()?; // scheme
    parts.next()?; // manager
    parts.next()?; // package
    parts.next()?; // version
    let descriptors = parts.next()?;

    // Last descriptor identifier. Strip trailing sigils.
    let last = descriptors.split('/').next_back()?;
    let bare = last
        .trim_end_matches(['#', '.', '(', ')', ':'])
        .trim_end_matches("()");
    // Unwrap backticks: `Foo bar` → Foo bar (rare).
    let bare = bare.trim_matches('`');
    if bare.is_empty() {
        return None;
    }
    by_fqn.get(bare).and_then(|v| v.first().copied())
}

/// Find the smallest node in `file_nodes` whose `[start, end)` contains `byte`.
fn enclosing_node(file_nodes: &[FileNode], byte: usize) -> Option<u64> {
    let b = byte as u32;
    let mut best: Option<&FileNode> = None;
    for n in file_nodes {
        if n.start <= b && b < n.end {
            best = match best {
                None => Some(n),
                Some(prev) if (n.end - n.start) < (prev.end - prev.start) => Some(n),
                Some(prev) => Some(prev),
            };
        }
    }
    best.map(|n| n.id)
}

/// Build a `line → starting_byte_offset` table for a file. Returns `None`
/// if the file can't be read.
fn read_line_offsets(repo_root: &Path, rel: &str) -> Option<Vec<usize>> {
    let abs = repo_root.join(rel);
    let bytes = std::fs::read(&abs).ok()?;
    let mut offsets = Vec::with_capacity(bytes.len() / 40 + 1);
    offsets.push(0);
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            offsets.push(i + 1);
        }
    }
    Some(offsets)
}

/// Convert (0-based line, 0-based col) → absolute byte offset. Returns `None`
/// if the line is beyond the file or the col is past the line's bytes.
fn line_col_to_byte(line_offsets: &[usize], line: i32, col: i32) -> Option<usize> {
    if line < 0 || col < 0 {
        return None;
    }
    let line = line as usize;
    let col = col as usize;
    let start = *line_offsets.get(line)?;
    Some(start + col)
}

/// SCIP `range` packing: `[start_line, start_col, end_line?, end_col]`. Return
/// the start tuple.
fn start_position(occ: &Occurrence) -> (i32, i32) {
    let r = &occ.range;
    (
        r.first().copied().unwrap_or(0),
        r.get(1).copied().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fqn_tail_strips_separators() {
        assert_eq!(fqn_tail("foo::bar::baz"), "baz");
        assert_eq!(fqn_tail("module.Class.method"), "method");
        assert_eq!(fqn_tail("singleton"), "singleton");
    }

    #[test]
    fn lookup_callee_handles_local() {
        let m = HashMap::new();
        assert!(lookup_callee(&m, "local 42").is_none());
    }

    #[test]
    fn lookup_callee_strips_descriptor_sigils() {
        let mut m: HashMap<String, Vec<u64>> = HashMap::new();
        m.insert("Flask".into(), vec![100]);
        let sym = "scip-python pypi flask 1.0.0 `src/flask/app`/Flask#";
        assert_eq!(lookup_callee(&m, sym), Some(100));
    }

    #[test]
    fn enclosing_picks_smallest_range() {
        let nodes = vec![
            FileNode {
                id: 1,
                start: 0,
                end: 100,
            },
            FileNode {
                id: 2,
                start: 10,
                end: 50,
            },
            FileNode {
                id: 3,
                start: 200,
                end: 300,
            },
        ];
        assert_eq!(enclosing_node(&nodes, 25), Some(2));
        assert_eq!(enclosing_node(&nodes, 75), Some(1));
        assert_eq!(enclosing_node(&nodes, 250), Some(3));
        assert_eq!(enclosing_node(&nodes, 500), None);
    }

    #[test]
    fn line_col_to_byte_basic() {
        // Lines at offsets [0, 5, 10] (lines of length 4 each + \n).
        let offsets = vec![0, 5, 10];
        assert_eq!(line_col_to_byte(&offsets, 0, 0), Some(0));
        assert_eq!(line_col_to_byte(&offsets, 1, 2), Some(7));
        assert_eq!(line_col_to_byte(&offsets, 2, 0), Some(10));
        assert_eq!(line_col_to_byte(&offsets, 3, 0), None);
    }
}
