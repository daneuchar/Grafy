//! FQN rules for Go. Package = directory path; identifier = file's package.
//! M1 W1 stub uses directory path; refined in W2 once we parse `package` clauses.

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let parent = rel.parent()?;
    let parts: Vec<&str> = parent.iter().filter_map(|c| c.to_str()).collect();
    Some(parts.join("/"))
}
