//! Read a grafy `.grafy/index.redb` store and project its edges into the
//! edge-pair form `(caller_fqn_tail, callee_fqn_tail)` for direct F1
//! comparison against a SCIP ground-truth `Index`.
//!
//! Plan §4 M2 W3. The W1 differ keys references by `(file, line, col)` and
//! requires both sides to emit per-occurrence positional symbols. Grafy's
//! redb store has no per-call-site rows — only `(caller_node_id,
//! callee_node_id, kind)` triples. Round-tripping grafy edges through a
//! synthetic `.scip` would mix positional and structural comparison and
//! obscure the picture. Instead we compute **edge-pair F1**: the set of
//! `(caller_tail, callee_tail)` pairs each side asserts. This is the metric
//! consumers actually care about ("did we connect A→B?").
//!
//! The matching key uses the last identifier of each FQN (e.g.
//! `app.module.foo` → `foo`). Tail matching is necessary because
//! `scip-python` emits `<pkg> <ver> module/Class#method().` style symbols
//! while grafy emits dotted FQNs; only the last segment is reliably
//! identical across the two grammars.
//!
//! Edge-kind filtering: `--include-edges calls` only keeps `EdgeKind::Calls`;
//! `--include-edges scip` only keeps `EdgeKind::Scip`; `--include-edges
//! calls,scip` keeps both (this is the "augmented" mode that proves the
//! SCIP ingest is doing real work).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

// Redb table definitions duplicated from `crates/grafy/src/store/mod.rs`. We
// cannot depend on the `grafy` crate from `grafy-bench` (cyclic via the
// workspace bench dev-dep), so the schema is mirrored here. If the schema
// changes, this file must change too — both ends are anchored on the
// `nodes` and `edges` table names.
const NODES_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("nodes");
const EDGES_TABLE: TableDefinition<(u64, u64, u8), &[u8]> = TableDefinition::new("edges");

/// Mirror of `grafy::store::NodeRecord` for postcard decoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeRecord {
    fqn: String,
    kind: u8,
    file: String,
    byte_start: u32,
    byte_end: u32,
}

/// Edge kinds — bit values must match `grafy::store::EdgeKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKindFilter {
    Calls,
    Scip,
    Both,
}

impl EdgeKindFilter {
    pub fn parse(s: &str) -> Result<Self> {
        let parts: HashSet<&str> = s.split(',').map(str::trim).collect();
        match (parts.contains("calls"), parts.contains("scip")) {
            (true, true) => Ok(Self::Both),
            (true, false) => Ok(Self::Calls),
            (false, true) => Ok(Self::Scip),
            (false, false) => anyhow::bail!(
                "--include-edges expected one of: calls, scip, calls,scip — got {s:?}"
            ),
        }
    }

    fn matches(self, kind: u8) -> bool {
        // 0 = Calls, 1 = Routes, 2 = Scip — see `grafy::store::EdgeKind`.
        match self {
            Self::Calls => kind == 0,
            Self::Scip => kind == 2,
            Self::Both => kind == 0 || kind == 2,
        }
    }
}

/// Last identifier of a dotted/colon-separated FQN (`a.b.c` → `c`,
/// `mod::Type` → `Type`). Falls back to the input when no separator is
/// present.
pub fn fqn_tail(fqn: &str) -> &str {
    if let Some(idx) = fqn.rfind("::") {
        return &fqn[idx + 2..];
    }
    if let Some(idx) = fqn.rfind('.') {
        return &fqn[idx + 1..];
    }
    fqn
}

/// Load all `(caller_fqn_tail, callee_fqn_tail)` pairs from a grafy redb
/// store, filtered to the requested edge kinds. Skips orphan edges whose
/// endpoints are absent from `NODES_TABLE` (can happen if the store was
/// captured mid-write, though redb's MVCC should prevent it).
pub fn load_edge_pairs(
    store_path: &Path,
    filter: EdgeKindFilter,
) -> Result<HashSet<(String, String)>> {
    let db = Database::open(store_path)
        .with_context(|| format!("open redb store {}", store_path.display()))?;
    let tx = db.begin_read().context("begin redb read transaction")?;

    // Build node-id → fqn map.
    let nodes = tx.open_table(NODES_TABLE).context("open nodes table")?;
    let mut id_to_fqn: HashMap<u64, String> = HashMap::new();
    for item in nodes.iter().context("iter nodes")? {
        let (k, v) = item.context("decode node entry")?;
        let id = k.value();
        let Ok(rec) = postcard::from_bytes::<NodeRecord>(v.value()) else {
            continue;
        };
        id_to_fqn.insert(id, rec.fqn);
    }

    let edges = tx.open_table(EDGES_TABLE).context("open edges table")?;
    let mut pairs: HashSet<(String, String)> = HashSet::new();
    for item in edges.iter().context("iter edges")? {
        let (k, _v) = item.context("decode edge entry")?;
        let (from, to, kind) = k.value();
        if !filter.matches(kind) {
            continue;
        }
        let Some(caller) = id_to_fqn.get(&from) else {
            continue;
        };
        let Some(callee) = id_to_fqn.get(&to) else {
            continue;
        };
        pairs.insert((fqn_tail(caller).to_owned(), fqn_tail(callee).to_owned()));
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filter_variants() {
        assert!(matches!(
            EdgeKindFilter::parse("calls").unwrap(),
            EdgeKindFilter::Calls
        ));
        assert!(matches!(
            EdgeKindFilter::parse("scip").unwrap(),
            EdgeKindFilter::Scip
        ));
        assert!(matches!(
            EdgeKindFilter::parse("calls,scip").unwrap(),
            EdgeKindFilter::Both
        ));
        assert!(matches!(
            EdgeKindFilter::parse("scip,calls").unwrap(),
            EdgeKindFilter::Both
        ));
        assert!(EdgeKindFilter::parse("routes").is_err());
        assert!(EdgeKindFilter::parse("").is_err());
    }

    #[test]
    fn filter_matches_correct_kinds() {
        // 0 = Calls, 1 = Routes, 2 = Scip.
        assert!(EdgeKindFilter::Calls.matches(0));
        assert!(!EdgeKindFilter::Calls.matches(1));
        assert!(!EdgeKindFilter::Calls.matches(2));
        assert!(EdgeKindFilter::Scip.matches(2));
        assert!(!EdgeKindFilter::Scip.matches(0));
        assert!(EdgeKindFilter::Both.matches(0));
        assert!(EdgeKindFilter::Both.matches(2));
        assert!(!EdgeKindFilter::Both.matches(1));
    }

    #[test]
    fn fqn_tail_strips_separators() {
        assert_eq!(fqn_tail("a.b.c"), "c");
        assert_eq!(fqn_tail("mod::Type"), "Type");
        assert_eq!(fqn_tail("flat"), "flat");
        assert_eq!(fqn_tail(""), "");
    }
}
