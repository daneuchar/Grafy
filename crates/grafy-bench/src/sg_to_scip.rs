//! Stack-graphs → SCIP adapter. Plan §4 M2 week 1.
//!
//! Runs `tree-sitter-stack-graphs-<lang> index <repo>` then resolves a set of
//! reference positions (one per `RefKey`) via `tree-sitter-stack-graphs-<lang>
//! query definition <path>:<line+1>:<col+1>` and emits a synthetic SCIP
//! `Index` whose `Occurrence.symbol` strings encode the resolved definition's
//! `(path, line, col)` so that the F1 differ can match them positionally.
//!
//! Why synthetic symbols: stack-graphs has no SCIP backend. Encoding the
//! *resolved definition position* as a symbol string is the simplest way to
//! turn the resolver's output into something the F1 differ can compare against
//! a per-language SCIP indexer's symbols — provided we apply the **same
//! transformation** to the ground truth on the input side. See `scip_f1.rs`
//! for the precise comparison rule used by the bench driver.
//!
//! NOTE: the upstream `tree-sitter-stack-graphs-<lang>` CLI accepts only
//! one position per invocation. We pay one fork per position. For W1's
//! ground-truth-driven measurement that is acceptable (flask is ~3 k refs
//! and the CLI startup is a few ms); for W2+ steady-state use the library
//! and skip the subprocess.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use scip::types::{Document, Index, Occurrence};

use crate::scip_f1::RefKey;

/// Per-language adapter config. Bin names match `~/.cargo/bin/tree-sitter-stack-graphs-*`.
pub struct LangAdapter {
    pub lang: &'static str,
    pub bin: &'static str,
}

impl LangAdapter {
    pub const PYTHON: LangAdapter = LangAdapter {
        lang: "python",
        bin: "tree-sitter-stack-graphs-python",
    };
    pub const TYPESCRIPT: LangAdapter = LangAdapter {
        lang: "typescript",
        bin: "tree-sitter-stack-graphs-typescript",
    };
    pub const JAVASCRIPT: LangAdapter = LangAdapter {
        lang: "javascript",
        bin: "tree-sitter-stack-graphs-javascript",
    };
    pub const JAVA: LangAdapter = LangAdapter {
        lang: "java",
        bin: "tree-sitter-stack-graphs-java",
    };
}

/// Index a corpus root with the appropriate stack-graphs CLI. The resulting
/// SQLite db is created at `db_path`. Returns the wall time spent indexing.
pub fn run_index(
    adapter: &LangAdapter,
    corpus_root: &Path,
    db_path: &Path,
    max_file_secs: u64,
) -> Result<Duration> {
    let t0 = Instant::now();
    let status = Command::new(adapter.bin)
        .arg("index")
        .arg("--max-file-time")
        .arg(max_file_secs.to_string())
        .arg("-D")
        .arg(db_path)
        .arg(corpus_root)
        .status()
        .with_context(|| format!("spawn {} index", adapter.bin))?;
    if !status.success() {
        // stack-graphs CLI exits non-zero whenever any file produced DSL errors,
        // but the db is still populated for files that resolved cleanly. We
        // treat non-zero as "partial" and proceed; the report records caveat.
        eprintln!(
            "[sg-to-scip] {} index returned non-zero (DSL errors expected — see report)",
            adapter.bin
        );
    }
    Ok(t0.elapsed())
}

/// One resolved definition.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub path: String,
    pub line: i32,
    pub col: i32,
}

/// Resolve a batch of positions in a single CLI invocation. Returns one
/// `Option<Resolved>` per input key in the same order. The CLI accepts
/// variadic positions on `query definition`; we batch up to `batch_size` per
/// call to keep stdout parsing tractable and avoid OS arg-list limits.
pub fn query_definitions_batch(
    adapter: &LangAdapter,
    db_path: &Path,
    corpus_root: &Path,
    keys: &[RefKey],
) -> Result<Vec<Option<Resolved>>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let positions: Vec<String> = keys
        .iter()
        .map(|k| {
            let abs = corpus_root.join(&k.path);
            format!("{}:{}:{}", abs.display(), k.line + 1, k.col + 1)
        })
        .collect();
    let out = Command::new(adapter.bin)
        .arg("query")
        .arg("-D")
        .arg(db_path)
        .arg("definition")
        .args(&positions)
        .output()
        .with_context(|| format!("spawn {} query definition (batch)", adapter.bin))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_batch_output(&stdout, corpus_root, keys)
}

/// Parse a batched stdout into a vector aligned with `keys`. The CLI emits one
/// "block" per input position, in order; each block begins with the header
/// line `<path>:L:C: <status>` and may be followed by indented lines.
///
/// We scan for header lines whose status is `found N definitions for 1 references`
/// with N>0, then look for the next non-empty *indented* line in that block to
/// extract the resolved position. Status `no references at location` or
/// `found 0 definitions …` yields `None`.
fn parse_batch_output(
    stdout: &str,
    corpus_root: &Path,
    keys: &[RefKey],
) -> Result<Vec<Option<Resolved>>> {
    // The CLI's per-block format is unstable across versions. We adopt the
    // simplest robust strategy: split the stdout on header lines that look
    // like "<abs-path>:<L>:<C>:" and then, for each block, scan all lines for
    // *any* indented line that itself matches "<abs-path>:<L>:<C>" — that's
    // the resolved definition's location.
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let is_header = |line: &str| -> bool {
        // Header: starts at column 0 with absolute path then `:L:C:` then a space.
        if line.starts_with(' ') || line.is_empty() {
            return false;
        }
        // Has at least two colons and ends with status text.
        if !line.starts_with('/') {
            return false;
        }
        // Check for `:digits:digits: ` pattern.
        let mut colons = 0;
        for (i, ch) in line.char_indices() {
            if ch == ':' {
                colons += 1;
                if colons == 3 {
                    return line.len() > i + 1 && line.as_bytes().get(i + 1) == Some(&b' ');
                }
            }
        }
        false
    };
    for line in stdout.lines() {
        if is_header(line) && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    let mut out = Vec::with_capacity(keys.len());
    for (i, block) in blocks.iter().enumerate() {
        if i >= keys.len() {
            break;
        }
        out.push(extract_resolved_from_block(block, corpus_root));
    }
    // If the CLI emitted fewer blocks than positions, pad with None.
    while out.len() < keys.len() {
        out.push(None);
    }
    Ok(out)
}

fn extract_resolved_from_block(block: &[&str], corpus_root: &Path) -> Option<Resolved> {
    // Canonicalize the corpus root so the macOS `/tmp` → `/private/tmp`
    // rewrite doesn't break path stripping.
    let canon = std::fs::canonicalize(corpus_root).unwrap_or_else(|_| corpus_root.to_path_buf());
    // The block contains the header (block[0]) plus context lines plus, if a
    // definition exists, lines of the form `      has definition` followed by
    // `      <path>:<L>:<C>:` (note the trailing colon — the CLI prints
    // positions in `<path>:L:C:` form everywhere). We scan for `has definition`
    // and take the next path-position line in the block.
    let mut found_def_marker = false;
    let echo_pos = parse_header_pos(block.first().copied().unwrap_or(""));
    for line in block.iter().skip(1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("has definition") {
            found_def_marker = true;
            continue;
        }
        if !found_def_marker {
            continue;
        }
        if trimmed.is_empty() || !trimmed.starts_with('/') {
            continue;
        }
        let Some(head) = trimmed.split_whitespace().next() else {
            continue;
        };
        // Strip trailing ':' if present.
        let head = head.trim_end_matches(':');
        let mut tail = head.rsplitn(3, ':');
        let (Some(col_str), Some(line_str), Some(path_str)) =
            (tail.next(), tail.next(), tail.next())
        else {
            continue;
        };
        let Ok(line_n) = line_str.parse::<i32>() else {
            continue;
        };
        let Ok(col_n) = col_str.parse::<i32>() else {
            continue;
        };
        // Reject the echoed reference position (same as the header's pos).
        if let Some((p, l, c)) = &echo_pos {
            if p == path_str && *l == line_n && *c == col_n {
                continue;
            }
        }
        // Try both raw and canonicalized corpus root for the strip.
        let pb = PathBuf::from(path_str);
        let rel = pb
            .strip_prefix(&canon)
            .or_else(|_| pb.strip_prefix(corpus_root))
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_else(|_| path_str.to_string());
        return Some(Resolved {
            path: rel,
            line: line_n - 1,
            col: col_n - 1,
        });
    }
    None
}

/// Parse `<abs-path>:<L>:<C>: ...` from a header line. Returns the absolute
/// path string and 1-based line/col, or `None` if the line isn't a header.
fn parse_header_pos(line: &str) -> Option<(String, i32, i32)> {
    if !line.starts_with('/') {
        return None;
    }
    // Split off the trailing `: <status>` portion.
    let (left, _right) = line.split_once(": ")?;
    let mut tail = left.rsplitn(3, ':');
    let col_str = tail.next()?;
    let line_str = tail.next()?;
    let path_str = tail.next()?;
    let line_n: i32 = line_str.parse().ok()?;
    let col_n: i32 = col_str.parse().ok()?;
    Some((path_str.to_string(), line_n, col_n))
}

/// Build a synthetic SCIP `Index` from resolved references. Each occurrence's
/// `symbol` encodes the resolved definition location as
/// `"sg-resolved <relpath>:<line>:<col>"` so the F1 differ can compare against
/// a ground-truth index whose symbols have been rewritten the same way (see
/// `rewrite_ground_truth_symbols` below).
pub fn build_synthetic_index(resolutions: &HashMap<RefKey, Option<Resolved>>) -> Index {
    let mut by_path: HashMap<String, Vec<Occurrence>> = HashMap::new();
    for (key, res) in resolutions {
        let Some(r) = res else { continue };
        let mut occ = Occurrence::new();
        occ.symbol = format!("sg-resolved . . . `{}`:{}:{}", r.path, r.line, r.col);
        occ.range = vec![key.line, key.col, key.col + 1];
        occ.symbol_roles = 0; // reference
        by_path.entry(key.path.clone()).or_default().push(occ);
    }
    let mut index = Index::new();
    for (path, occs) in by_path {
        let mut doc = Document::new();
        doc.relative_path = path;
        doc.occurrences = occs;
        index.documents.push(doc);
    }
    index
}

/// Rewrite the ground-truth SCIP index so each reference occurrence's symbol
/// encodes the resolved definition's (relpath, line, col). This is the
/// positional equivalent of the original SCIP symbol — same comparison key
/// that `build_synthetic_index` emits. Returns a new `Index`.
///
/// Ground-truth symbols on a reference are the *defined* symbol; we look up
/// the definition occurrence of that symbol in the same index and rewrite the
/// reference to use a `sg-resolved` symbol pointing at the definition's
/// position. If no definition exists in the index (cross-corpus reference),
/// the reference is dropped.
pub fn rewrite_ground_truth_symbols(gt: &Index) -> Index {
    // Build symbol → (relpath, line, col) from definition occurrences.
    let mut defs: HashMap<&str, (String, i32, i32)> = HashMap::new();
    for doc in &gt.documents {
        for occ in &doc.occurrences {
            const DEFINITION_ROLE: i32 = 1;
            if (occ.symbol_roles & DEFINITION_ROLE) != 0 && !occ.symbol.is_empty() {
                let line = occ.range.first().copied().unwrap_or(0);
                let col = occ.range.get(1).copied().unwrap_or(0);
                defs.entry(occ.symbol.as_str())
                    .or_insert((doc.relative_path.clone(), line, col));
            }
        }
    }
    let mut out = Index::new();
    for doc in &gt.documents {
        let mut new_doc = Document::new();
        new_doc.relative_path = doc.relative_path.clone();
        for occ in &doc.occurrences {
            const DEFINITION_ROLE: i32 = 1;
            if (occ.symbol_roles & DEFINITION_ROLE) != 0 {
                continue;
            }
            if let Some((p, l, c)) = defs.get(occ.symbol.as_str()) {
                let mut o = Occurrence::new();
                o.symbol = format!("sg-resolved . . . `{p}`:{l}:{c}");
                o.range = occ.range.clone();
                o.symbol_roles = 0;
                new_doc.occurrences.push(o);
            }
            // Drop references whose definition isn't in this corpus —
            // stack-graphs can't be expected to resolve external refs either
            // unless the dep is also indexed.
        }
        out.documents.push(new_doc);
    }
    out
}

/// Extract the unique reference positions from a (rewritten) SCIP index. These
/// drive the `query definition` calls.
pub fn reference_positions(index: &Index) -> Vec<RefKey> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for doc in &index.documents {
        for occ in &doc.occurrences {
            let line = occ.range.first().copied().unwrap_or(0);
            let col = occ.range.get(1).copied().unwrap_or(0);
            let key = RefKey {
                path: doc.relative_path.clone(),
                line,
                col,
            };
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    out
}

/// Resolve every position in `positions` against the on-disk db. Returns a
/// map; values are `None` for unresolved positions. Uses batched CLI calls of
/// `batch_size` positions per invocation to amortize subprocess startup.
/// Stops early if `wall_budget` is exceeded; remaining positions are mapped
/// to `None`.
pub fn resolve_all(
    adapter: &LangAdapter,
    db_path: &Path,
    corpus_root: &Path,
    positions: &[RefKey],
    wall_budget: Duration,
) -> Result<(HashMap<RefKey, Option<Resolved>>, Duration)> {
    const BATCH_SIZE: usize = 200;
    let t0 = Instant::now();
    let mut out: HashMap<RefKey, Option<Resolved>> = HashMap::with_capacity(positions.len());
    let mut resolved = 0;
    let mut budget_hit = false;
    for chunk in positions.chunks(BATCH_SIZE) {
        if t0.elapsed() > wall_budget {
            budget_hit = true;
            eprintln!(
                "[sg-to-scip] wall budget hit after {} positions; remainder left unresolved",
                out.len()
            );
            break;
        }
        let results = query_definitions_batch(adapter, db_path, corpus_root, chunk)?;
        for (key, r) in chunk.iter().zip(results.into_iter()) {
            if r.is_some() {
                resolved += 1;
            }
            out.insert(key.clone(), r);
        }
        if out.len() % 1000 == 0 {
            eprintln!(
                "[sg-to-scip] progress: {} / {} ({} resolved)",
                out.len(),
                positions.len(),
                resolved
            );
        }
    }
    // Pad unresolved.
    for k in positions {
        out.entry(k.clone()).or_insert(None);
    }
    if !budget_hit {
        eprintln!(
            "[sg-to-scip] resolved {} / {} positions in {:?}",
            resolved,
            positions.len(),
            t0.elapsed()
        );
    }
    Ok((out, t0.elapsed()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_batch_handles_empty() {
        let out = parse_batch_output("", Path::new("/tmp"), &[]).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn parse_batch_extracts_definitions() {
        // Real-shape stdout from tree-sitter-stack-graphs-python 0.3.0,
        // including the `has definition` marker and trailing-colon paths.
        let stdout = "\
/abs/src/a.py:10:5: found 1 definitions for 1 references
queried reference
/abs/src/a.py:10:5:
10 |     foo()
   |     ^^^
has definition
/abs/src/a.py:42:7:
42 | def Flask():
   |     ^^^^^
/abs/src/b.py:20:1: no references at location
/abs/src/c.py:5:3: found 1 definitions for 1 references
queried reference
/abs/src/c.py:5:3:
has definition
/abs/src/d.py:100:0:
100 | def helper():
";
        let keys = vec![
            RefKey {
                path: "src/a.py".into(),
                line: 9,
                col: 4,
            },
            RefKey {
                path: "src/b.py".into(),
                line: 19,
                col: 0,
            },
            RefKey {
                path: "src/c.py".into(),
                line: 4,
                col: 2,
            },
        ];
        let out = parse_batch_output(stdout, Path::new("/abs"), &keys).unwrap();
        assert_eq!(out.len(), 3);
        let r0 = out[0].as_ref().unwrap();
        assert_eq!(r0.path, "src/a.py");
        assert_eq!(r0.line, 41);
        assert_eq!(r0.col, 6);
        assert!(out[1].is_none());
        let r2 = out[2].as_ref().unwrap();
        assert_eq!(r2.path, "src/d.py");
        assert_eq!(r2.line, 99);
    }
}
