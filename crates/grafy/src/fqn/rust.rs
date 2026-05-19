//! FQN rules for Rust. M0: directory→module mapping stub.

use std::path::Path;

/// Map a Rust source path under `root` to a module FQN.
/// Stub: strips extension and replaces path separators with `::`.
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let mut parts: Vec<&str> = stem.iter().filter_map(|c| c.to_str()).collect();
    if matches!(parts.last(), Some(&"mod" | &"lib" | &"main")) {
        parts.pop();
    }
    Some(parts.join("::"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn mod_collapses() {
        let root = PathBuf::from("/r");
        let f = PathBuf::from("/r/foo/bar/mod.rs");
        assert_eq!(fqn(&root, &f), Some("foo::bar".into()));
    }

    #[test]
    fn regular_file() {
        let root = PathBuf::from("/r");
        let f = PathBuf::from("/r/a/b.rs");
        assert_eq!(fqn(&root, &f), Some("a::b".into()));
    }
}
