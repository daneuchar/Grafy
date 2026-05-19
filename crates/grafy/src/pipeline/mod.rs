//! Indexing pipeline. Four passes (plan §4 M1):
//! 1. structure  — File / Module / Function / Class nodes
//! 2. definitions — symbols + signatures
//! 3. calls       — M1 heuristic resolver, M2 stack-graphs
//! 4. routes      — HTTP route ↔ call-site linking
//!
//! M0: structure pass only. The shape is the contract.

use std::path::{Path, PathBuf};
use std::time::Instant;

use grafy_parser::{parse, Language};
use ignore::WalkBuilder;
use rayon::prelude::*;
use tracing::{info, info_span, warn};

pub struct Pipeline {
    root: PathBuf,
}

#[derive(Debug, Default)]
pub struct IndexSummary {
    pub files_seen: usize,
    pub files_parsed: usize,
    pub files_skipped: usize,
    pub files_failed: usize,
}

impl Pipeline {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Run the M0 structure pass over `self.root`.
    pub fn index(&self) -> anyhow::Result<IndexSummary> {
        let span = info_span!("pipeline.index", root = %self.root.display());
        let _e = span.enter();

        let started = Instant::now();
        let files: Vec<PathBuf> = self.walk();
        info!(target: "grafy.pipeline", count = files.len(), "walked");

        let summary = files
            .par_iter()
            .map(|p| self.parse_one(p))
            .reduce(IndexSummary::default, IndexSummary::merge);

        info!(target: "grafy.pipeline",
              elapsed_ms = started.elapsed().as_millis() as u64,
              files_seen = summary.files_seen,
              files_parsed = summary.files_parsed,
              files_skipped = summary.files_skipped,
              files_failed = summary.files_failed,
              "indexed");
        Ok(summary)
    }

    fn walk(&self) -> Vec<PathBuf> {
        let span = info_span!("pipeline.walk");
        let _e = span.enter();
        WalkBuilder::new(&self.root)
            .standard_filters(true)
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .map(|e| e.path().to_path_buf())
            .collect()
    }

    fn parse_one(&self, path: &Path) -> IndexSummary {
        let mut s = IndexSummary {
            files_seen: 1,
            ..Default::default()
        };
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            s.files_skipped += 1;
            return s;
        };
        let Some(lang) = Language::from_extension(ext) else {
            s.files_skipped += 1;
            return s;
        };

        let Ok(bytes) = std::fs::read(path) else {
            warn!(target: "grafy.pipeline", path = %path.display(), "read failed");
            s.files_failed += 1;
            return s;
        };

        let path_str = path.display().to_string();
        match parse(&path_str, lang, &bytes) {
            Ok(_tree) => {
                s.files_parsed += 1;
            }
            Err(e) => {
                warn!(target: "grafy.pipeline", error = %e, "parse failed");
                s.files_failed += 1;
            }
        }
        s
    }
}

impl IndexSummary {
    fn merge(mut a: Self, b: Self) -> Self {
        a.files_seen += b.files_seen;
        a.files_parsed += b.files_parsed;
        a.files_skipped += b.files_skipped;
        a.files_failed += b.files_failed;
        a
    }
}

/// Emit a minimal Graphviz `.dot` representation of the indexed root.
/// M0 stub — produces a valid `.dot` so the dogfood gate passes.
pub fn to_dot(summary: &IndexSummary, root: &Path) -> String {
    let mut out = String::new();
    out.push_str("digraph grafy {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str(&format!("  root [label=\"{}\"];\n", root.display()));
    out.push_str(&format!(
        "  files [label=\"files: parsed={} skipped={} failed={}\"];\n",
        summary.files_parsed, summary.files_skipped, summary.files_failed
    ));
    out.push_str("  root -> files;\n");
    out.push_str("}\n");
    out
}
