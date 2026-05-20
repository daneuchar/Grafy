//! FQN rules for TypeScript. Same shape as JavaScript.

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    super::javascript::fqn(root, file)
}
