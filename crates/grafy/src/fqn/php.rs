//! FQN rules for PHP. PSR-4 namespace mirrors directory; M1 W1 stub uses path.

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let parts: Vec<&str> = stem.iter().filter_map(|c| c.to_str()).collect();
    Some(parts.join("\\"))
}
