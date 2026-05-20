//! FQN rules for Java. `package` declaration is authoritative; M1 W1 stub
//! uses path-derived dotted name (after `src/main/java` if present).

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let parts: Vec<&str> = stem.iter().filter_map(|c| c.to_str()).collect();
    let trimmed = trim_maven_prefix(&parts);
    Some(trimmed.join("."))
}

fn trim_maven_prefix<'a>(parts: &'a [&'a str]) -> Vec<&'a str> {
    let triple = ["src", "main", "java"];
    for i in 0..parts.len().saturating_sub(3) {
        if parts[i..i + 3] == triple {
            return parts[i + 3..].to_vec();
        }
    }
    parts.to_vec()
}
