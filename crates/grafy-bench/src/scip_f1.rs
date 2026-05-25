//! SCIP F1 differ. Plan §4 M2 week 1.
//!
//! Reads two SCIP files (ground truth + tool output) and computes precision,
//! recall, and F1 over **references** — i.e. occurrences whose
//! `symbol_roles` does not have the `Definition` bit (1) set.
//!
//! Symbol comparison is structural modulo the project-name/version prefix
//! of the SCIP symbol grammar:
//!
//! ```text
//!   scheme manager package_name package_version descriptors+
//! ```
//!
//! We normalize a symbol by replacing its package descriptor (the second
//! whitespace-delimited token group, of length 3) with a placeholder, so
//! that `scip-python flask 1.0.0 …/foo#` and `grafy-stackgraphs flask 0.1.0 …/foo#`
//! compare equal. "local …" symbols are kept as-is.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use scip::types::{Document, Index, Occurrence};
use serde::Serialize;

/// Bit flag set on `Occurrence.symbol_roles` when the occurrence is a definition.
/// See `scip::types::SymbolRole::Definition` — value 1.
const DEFINITION_ROLE: i32 = 1;

/// One reference occurrence, keyed by file + position. Symbol is the normalized
/// SCIP symbol string (project prefix stripped).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefKey {
    /// Path relative to the corpus root (e.g. `src/flask/app.py`).
    pub path: String,
    /// 0-based start line, as emitted by SCIP.
    pub line: i32,
    /// 0-based start column.
    pub col: i32,
}

#[derive(Debug, Clone)]
pub struct RefOcc {
    pub key: RefKey,
    pub symbol: String,
}

/// Per-(lang, repo) F1 result. JSON-serializable for the bench driver.
#[derive(Debug, Serialize)]
pub struct F1Result {
    pub lang: String,
    pub repo: String,
    pub sha: String,
    pub ground_truth_refs: usize,
    pub tool_refs: usize,
    /// References present in both files with matching normalized symbol.
    pub true_pos: usize,
    /// References present in tool output but absent or mismatching in ground truth.
    pub false_pos: usize,
    /// References present in ground truth but absent or mismatching in tool output.
    pub false_neg: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

/// Parse a `.scip` file into an `Index`.
pub fn load_index(path: &Path) -> Result<Index> {
    let buf = fs::read(path).with_context(|| format!("read scip file {}", path.display()))?;
    use protobuf::Message;
    Index::parse_from_bytes(&buf).with_context(|| format!("parse scip {}", path.display()))
}

/// Normalize a SCIP symbol by stripping the `manager package-name package-version`
/// prefix so two indexers emitting the same logical symbol with different
/// project identifiers compare equal.
///
/// Grammar (per `scip.proto`): `scheme ' ' manager ' ' package_name ' ' package_version (' ' descriptor)+`.
/// `local <id>` symbols have no manager/package and are returned unchanged.
pub fn normalize_symbol(sym: &str) -> String {
    if sym.starts_with("local ") {
        return sym.to_owned();
    }
    // Split into at most 5 parts: scheme manager package version descriptors.
    // We normalize *all four* of scheme/manager/package/version to a sentinel
    // so that cross-scheme comparisons (e.g. scip-python vs sg-resolved) match
    // on the descriptor portion only.
    let mut parts = sym.splitn(5, ' ');
    let _scheme = parts.next();
    let _manager = parts.next();
    let _pkg = parts.next();
    let _ver = parts.next();
    let descriptors = parts.next().unwrap_or("");
    format!(". . . . {descriptors}")
}

/// Extract reference occurrences from a SCIP `Index`. Occurrences with the
/// `Definition` role bit set are excluded. The returned key uses the document's
/// `relative_path` exactly as recorded.
pub fn extract_refs(index: &Index) -> Vec<RefOcc> {
    let mut out = Vec::new();
    for doc in &index.documents {
        for occ in &doc.occurrences {
            if (occ.symbol_roles & DEFINITION_ROLE) != 0 {
                continue;
            }
            if occ.symbol.is_empty() {
                continue;
            }
            let (line, col) = start_position(occ);
            out.push(RefOcc {
                key: RefKey {
                    path: doc.relative_path.clone(),
                    line,
                    col,
                },
                symbol: normalize_symbol(&occ.symbol),
            });
        }
    }
    out
}

/// SCIP `Occurrence.range` is `[start_line, start_col, end_line?, end_col]` or
/// `[start_line, start_col, end_col]` (single-line). Return start (line, col).
fn start_position(occ: &Occurrence) -> (i32, i32) {
    let r = &occ.range;
    let line = r.first().copied().unwrap_or(0);
    let col = r.get(1).copied().unwrap_or(0);
    (line, col)
}

/// Iterate documents of a SCIP `Index` in deterministic order.
pub fn documents_sorted(index: &Index) -> Vec<&Document> {
    let mut docs: Vec<&Document> = index.documents.iter().collect();
    docs.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    docs
}

/// Compute F1 between ground truth and tool output. References from each side
/// are bucketed by `RefKey`; a position present on both sides is a TP iff the
/// normalized symbols match.
pub fn compute_f1(
    lang: &str,
    repo: &str,
    sha: &str,
    ground_truth: &Index,
    tool: &Index,
) -> F1Result {
    let gt = extract_refs(ground_truth);
    let tl = extract_refs(tool);

    // Bucket by RefKey. A single position may have multiple occurrences in
    // pathological cases; we keep the first symbol seen.
    let mut gt_map: HashMap<RefKey, String> = HashMap::with_capacity(gt.len());
    for r in &gt {
        gt_map.entry(r.key.clone()).or_insert(r.symbol.clone());
    }
    let mut tl_map: HashMap<RefKey, String> = HashMap::with_capacity(tl.len());
    for r in &tl {
        tl_map.entry(r.key.clone()).or_insert(r.symbol.clone());
    }

    let mut tp = 0usize;
    let mut fp = 0usize;
    let mut fn_ = 0usize;

    for (k, s) in &tl_map {
        match gt_map.get(k) {
            Some(gs) if gs == s => tp += 1,
            Some(_) => fp += 1, // resolved-to-different-symbol = wrong
            None => fp += 1,    // position not in ground truth = noise
        }
    }
    for k in gt_map.keys() {
        if !tl_map.contains_key(k) {
            fn_ += 1;
        }
    }

    let precision = if tp + fp == 0 {
        0.0
    } else {
        tp as f64 / (tp + fp) as f64
    };
    let recall = if tp + fn_ == 0 {
        0.0
    } else {
        tp as f64 / (tp + fn_) as f64
    };
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };

    F1Result {
        lang: lang.to_owned(),
        repo: repo.to_owned(),
        sha: sha.to_owned(),
        ground_truth_refs: gt_map.len(),
        tool_refs: tl_map.len(),
        true_pos: tp,
        false_pos: fp,
        false_neg: fn_,
        precision,
        recall,
        f1,
    }
}

// ---------------------------------------------------------------------------
// Edge-pair F1 (M2 W3)
//
// `compute_f1` above keys references by `(file, line, col)` — the W1
// methodology. Grafy's redb store has no per-call-site positions; its edges
// are `(caller_node_id, callee_node_id)`. To compare apples-to-apples we
// project both sides into the set `{(caller_fqn_tail, callee_fqn_tail)}`
// and compute set-membership F1.
//
// Both sides use the same `fqn_tail` rule (last segment after `.` or `::`).
// The SCIP side derives caller from the enclosing-range definition
// occurrence whose `[start, end)` contains the reference position; the
// callee is the reference occurrence's own symbol. A SCIP reference whose
// resolved symbol lives outside the corpus (no Definition occurrence
// anywhere in the index) contributes no pair — that's the fair scope for
// grafy comparison since grafy never emits edges to symbols it didn't see.
// ---------------------------------------------------------------------------

use std::collections::HashSet;

/// Edge-pair F1 result. Set-membership only — no positional info.
#[derive(Debug, Serialize)]
pub struct EdgePairF1 {
    pub lang: String,
    pub repo: String,
    pub sha: String,
    pub include_edges: String,
    pub ground_truth_pairs: usize,
    pub tool_pairs: usize,
    pub true_pos: usize,
    pub false_pos: usize,
    pub false_neg: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    /// Sample of 10 pairs in ground truth but absent from tool — for triage.
    pub missing_sample: Vec<(String, String)>,
    /// Sample of 10 pairs in tool but absent from ground truth.
    pub extra_sample: Vec<(String, String)>,
}

/// Build `{(caller_tail, callee_tail)}` pairs from a SCIP `Index`. For each
/// non-definition reference occurrence we locate the smallest enclosing
/// definition in the same document and use its symbol as the caller. Refs
/// that fall outside any in-corpus definition (module-level imports etc.)
/// are skipped — grafy doesn't emit caller-less edges either.
pub fn scip_edge_pairs(index: &Index) -> HashSet<(String, String)> {
    let mut pairs: HashSet<(String, String)> = HashSet::new();
    for doc in &index.documents {
        // Definition body ranges in this document. SCIP encodes the
        // identifier-only span in `Occurrence.range` and the full body
        // span in `Occurrence.enclosing_range`; we want the latter so
        // references inside a function body resolve to that function.
        // For Python, scip-python sets `enclosing_range` on every
        // definition occurrence whose body spans more than the name.
        let mut defs: Vec<(i32, i32, i32, i32, &str)> = Vec::new();
        for occ in &doc.occurrences {
            if (occ.symbol_roles & DEFINITION_ROLE) == 0 {
                continue;
            }
            if occ.symbol.is_empty() {
                continue;
            }
            // Prefer enclosing_range when present; fall back to range.
            let (sl, sc, el, ec) = if occ.enclosing_range.len() >= 4 {
                let r = &occ.enclosing_range;
                (r[0], r[1], r[2], r[3])
            } else if occ.enclosing_range.len() == 3 {
                let r = &occ.enclosing_range;
                (r[0], r[1], r[0], r[2])
            } else {
                let (sl, sc) = start_position(occ);
                let (el, ec) = end_position(occ);
                (sl, sc, el, ec)
            };
            defs.push((sl, sc, el, ec, occ.symbol.as_str()));
        }
        defs.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(b.2.cmp(&a.2))
                .then(b.3.cmp(&a.3))
        });

        for occ in &doc.occurrences {
            if (occ.symbol_roles & DEFINITION_ROLE) != 0 {
                continue;
            }
            if occ.symbol.is_empty() {
                continue;
            }
            let (rl, rc) = start_position(occ);
            // Find smallest def whose range contains (rl, rc).
            let mut best: Option<&str> = None;
            let mut best_size: i64 = i64::MAX;
            for (sl, sc, el, ec, sym) in &defs {
                let starts_before = *sl < rl || (*sl == rl && *sc <= rc);
                let ends_after = *el > rl || (*el == rl && *ec > rc);
                if starts_before && ends_after {
                    let size = ((*el - *sl) as i64) * 10_000 + (*ec - *sc) as i64;
                    if size < best_size {
                        best_size = size;
                        best = Some(*sym);
                    }
                }
            }
            let Some(caller_sym) = best else {
                continue;
            };
            let caller_tail = scip_symbol_tail(caller_sym);
            let callee_tail = scip_symbol_tail(&occ.symbol);
            if caller_tail.is_empty() || callee_tail.is_empty() {
                continue;
            }
            if caller_tail == callee_tail {
                // Self-loops are uninteresting. Grafy filters these out as well.
                continue;
            }
            pairs.insert((caller_tail.to_owned(), callee_tail.to_owned()));
        }
    }
    pairs
}

/// Extract the bare identifier from a SCIP symbol. SCIP descriptor grammar:
/// `scheme manager package version (descriptor)+`; the last descriptor
/// ends in `#` (type), `().` (method), `.` (term), or `/` (namespace).
/// `local <id>` symbols are unique to a single function body and have no
/// stable tail — return empty.
fn scip_symbol_tail(sym: &str) -> &str {
    if sym.starts_with("local ") {
        return "";
    }
    let descriptors = match sym.splitn(5, ' ').nth(4) {
        Some(d) => d,
        None => return "",
    };
    // SCIP descriptors are concatenated; each descriptor terminates in a
    // sigil (`#`, `.`, `/`, `:`) and may include `()` for method arity.
    // Strip trailing sigils first, then split on the **last** sigil to get
    // the bare identifier.
    let trimmed = descriptors.trim_end_matches(['#', '.', '/', ':', '(', ')']);
    let bare = trimmed
        .rsplit(['#', '.', '/', ':', '(', ')'])
        .next()
        .unwrap_or("");
    bare.trim_matches('`')
}

/// End position of a SCIP occurrence's range. Encoding:
/// 3-tuple `[sl, sc, ec]` → end_line = sl; 4-tuple `[sl, sc, el, ec]`.
fn end_position(occ: &Occurrence) -> (i32, i32) {
    let r = &occ.range;
    match r.len() {
        3 => (r[0], r[2]),
        4 => (r[2], r[3]),
        _ => (
            r.first().copied().unwrap_or(0),
            r.get(1).copied().unwrap_or(0),
        ),
    }
}

/// Compute edge-pair F1 between a SCIP ground truth and a set of grafy
/// `(caller_tail, callee_tail)` pairs. The SCIP side is projected through
/// `scip_edge_pairs`.
#[allow(clippy::too_many_arguments)]
pub fn compute_edge_pair_f1(
    lang: &str,
    repo: &str,
    sha: &str,
    include_edges: &str,
    ground_truth: &Index,
    tool_pairs: &HashSet<(String, String)>,
) -> EdgePairF1 {
    let gt_pairs = scip_edge_pairs(ground_truth);

    let mut tp = 0usize;
    let mut fp = 0usize;
    let mut fn_ = 0usize;
    let mut missing: Vec<(String, String)> = Vec::new();
    let mut extra: Vec<(String, String)> = Vec::new();

    for p in tool_pairs {
        if gt_pairs.contains(p) {
            tp += 1;
        } else {
            fp += 1;
            if extra.len() < 10 {
                extra.push(p.clone());
            }
        }
    }
    for p in &gt_pairs {
        if !tool_pairs.contains(p) {
            fn_ += 1;
            if missing.len() < 10 {
                missing.push(p.clone());
            }
        }
    }

    let precision = if tp + fp == 0 {
        0.0
    } else {
        tp as f64 / (tp + fp) as f64
    };
    let recall = if tp + fn_ == 0 {
        0.0
    } else {
        tp as f64 / (tp + fn_) as f64
    };
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };

    EdgePairF1 {
        lang: lang.to_owned(),
        repo: repo.to_owned(),
        sha: sha.to_owned(),
        include_edges: include_edges.to_owned(),
        ground_truth_pairs: gt_pairs.len(),
        tool_pairs: tool_pairs.len(),
        true_pos: tp,
        false_pos: fp,
        false_neg: fn_,
        precision,
        recall,
        f1,
        missing_sample: missing,
        extra_sample: extra,
    }
}

// Re-export so the bin can reach `fqn_tail` through a single path.
pub use crate::grafy_store::fqn_tail as grafy_fqn_tail;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_package_prefix() {
        let a = "scip-python pypi flask 1.0.0 `src/flask/app`/Flask#";
        let b = "grafy-stackgraphs pypi flask 0.1.0 `src/flask/app`/Flask#";
        assert_eq!(normalize_symbol(a), normalize_symbol(b));
    }

    #[test]
    fn normalize_preserves_local() {
        let s = "local 0";
        assert_eq!(normalize_symbol(s), s);
    }

    #[test]
    fn f1_perfect_match() {
        // Build two minimal indexes with one matching reference.
        let mut index = Index::new();
        let mut doc = Document::new();
        doc.relative_path = "a.py".into();
        let mut occ = Occurrence::new();
        occ.symbol = "scip-python . . . `mod`/foo#".into();
        occ.range = vec![1, 2, 5];
        occ.symbol_roles = 0;
        doc.occurrences.push(occ);
        index.documents.push(doc);

        let r = compute_f1("python", "test", "deadbeef", &index, &index);
        assert_eq!(r.true_pos, 1);
        assert_eq!(r.false_pos, 0);
        assert_eq!(r.false_neg, 0);
        assert!((r.f1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn scip_symbol_tail_extracts_last_id() {
        // scip-python style
        let s = "scip-python pypi flask 1.0.0 `src/flask/app`/Flask#";
        assert_eq!(scip_symbol_tail(s), "Flask");
        let m = "scip-python pypi flask 1.0.0 `src/flask/app`/Flask#run().";
        assert_eq!(scip_symbol_tail(m), "run");
        let t = "scip-python pypi flask 1.0.0 `src/flask/app`/CONFIG.";
        assert_eq!(scip_symbol_tail(t), "CONFIG");
        // local symbols → empty
        assert_eq!(scip_symbol_tail("local 42"), "");
    }

    #[test]
    fn edge_pair_self_loops_skipped() {
        // Construct a doc where a function calls itself; the only ref should
        // produce (caller=foo, callee=foo) and be dropped.
        let mut index = Index::new();
        let mut doc = Document::new();
        doc.relative_path = "a.py".into();

        let mut def = Occurrence::new();
        def.symbol = "scip-python . . . `a`/foo#".into();
        def.symbol_roles = 1;
        def.range = vec![0, 0, 10, 0]; // lines 0..10
        doc.occurrences.push(def);

        let mut r = Occurrence::new();
        r.symbol = "scip-python . . . `a`/foo#".into();
        r.symbol_roles = 0;
        r.range = vec![3, 4, 10];
        doc.occurrences.push(r);

        index.documents.push(doc);
        let pairs = scip_edge_pairs(&index);
        assert!(pairs.is_empty(), "self-loop should be filtered: {pairs:?}");
    }

    #[test]
    fn edge_pair_basic() {
        // Document defines `foo` and `bar`; bar references foo. Expect
        // pair (bar, foo).
        let mut index = Index::new();
        let mut doc = Document::new();
        doc.relative_path = "a.py".into();

        let mut foo_def = Occurrence::new();
        foo_def.symbol = "scip-python . . . `a`/foo#".into();
        foo_def.symbol_roles = 1;
        foo_def.range = vec![0, 0, 3, 0];
        doc.occurrences.push(foo_def);

        let mut bar_def = Occurrence::new();
        bar_def.symbol = "scip-python . . . `a`/bar#".into();
        bar_def.symbol_roles = 1;
        bar_def.range = vec![5, 0, 9, 0];
        doc.occurrences.push(bar_def);

        let mut foo_ref = Occurrence::new();
        foo_ref.symbol = "scip-python . . . `a`/foo#".into();
        foo_ref.symbol_roles = 0;
        foo_ref.range = vec![6, 4, 7];
        doc.occurrences.push(foo_ref);

        index.documents.push(doc);
        let pairs = scip_edge_pairs(&index);
        let want = ("bar".to_owned(), "foo".to_owned());
        assert!(pairs.contains(&want), "got {pairs:?}");
    }

    #[test]
    fn compute_edge_pair_perfect() {
        let mut index = Index::new();
        let mut doc = Document::new();
        doc.relative_path = "a.py".into();

        let mut foo_def = Occurrence::new();
        foo_def.symbol = "scip-python . . . `a`/foo#".into();
        foo_def.symbol_roles = 1;
        foo_def.range = vec![0, 0, 3, 0];
        doc.occurrences.push(foo_def);

        let mut bar_def = Occurrence::new();
        bar_def.symbol = "scip-python . . . `a`/bar#".into();
        bar_def.symbol_roles = 1;
        bar_def.range = vec![5, 0, 9, 0];
        doc.occurrences.push(bar_def);

        let mut foo_ref = Occurrence::new();
        foo_ref.symbol = "scip-python . . . `a`/foo#".into();
        foo_ref.symbol_roles = 0;
        foo_ref.range = vec![6, 4, 7];
        doc.occurrences.push(foo_ref);

        index.documents.push(doc);

        let tool: HashSet<(String, String)> =
            [("bar".to_owned(), "foo".to_owned())].into_iter().collect();
        let r = compute_edge_pair_f1("python", "test", "deadbeef", "calls", &index, &tool);
        assert_eq!(r.true_pos, 1);
        assert_eq!(r.false_pos, 0);
        assert_eq!(r.false_neg, 0);
        assert!((r.f1 - 1.0).abs() < 1e-9);
    }
}
