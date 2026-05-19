//! FQN rules for Python. M0 stub.

use std::path::Path;

pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let mut parts: Vec<&str> = stem.iter().filter_map(|c| c.to_str()).collect();
    if matches!(parts.last(), Some(&"__init__")) {
        parts.pop();
    }
    Some(parts.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn init_collapses() {
        let root = PathBuf::from("/r");
        let f = PathBuf::from("/r/pkg/sub/__init__.py");
        assert_eq!(fqn(&root, &f), Some("pkg.sub".into()));
    }
}
