//! FQN rules for JavaScript. Path-derived module name with `index.js` collapsed.

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let mut parts: Vec<&str> = stem.iter().filter_map(|c| c.to_str()).collect();
    if matches!(parts.last(), Some(&"index")) {
        parts.pop();
    }
    Some(parts.join("/"))
}
