//! FQN rules for Scala. Same package convention as Java.

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    super::java::fqn(root, file)
}
