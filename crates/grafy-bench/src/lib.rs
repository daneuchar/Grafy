//! Bench harness helpers.
//!
//! M1: `collect_sources` walks a tree with `ignore` and returns matching files.
//! M2 W1: `scip_f1` + `sg_to_scip` implement subprocess-driven F1 measurement
//! of stack-graphs vs per-language SCIP indexers. Plan §4 M2 week 1.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

pub mod grafy_store;
pub mod scip_f1;
pub mod sg_to_scip;

/// Collect source files under `root` matching one of the given extensions.
pub fn collect_sources(root: &Path, exts: &[&str]) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .standard_filters(true)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension()?.to_str()?;
            exts.iter()
                .find(|&&want| want == ext)
                .map(|_| path.to_path_buf())
        })
        .collect()
}
