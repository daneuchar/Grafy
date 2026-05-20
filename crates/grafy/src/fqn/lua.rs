//! FQN rules for Lua. Module path with dots, like Lua's `require` convention.

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let parts: Vec<&str> = stem.iter().filter_map(|c| c.to_str()).collect();
    Some(parts.join("."))
}
