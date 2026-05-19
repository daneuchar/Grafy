//! Bench harness helpers.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

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
            exts.iter().find(|&&want| want == ext).map(|_| path.to_path_buf())
        })
        .collect()
}
