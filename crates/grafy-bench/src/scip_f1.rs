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
}
