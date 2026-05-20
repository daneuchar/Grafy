//! FQN rules for TSX. Shares TypeScript shape.

use std::path::Path;

#[must_use]
pub fn fqn(root: &Path, file: &Path) -> Option<String> {
    super::typescript::fqn(root, file)
}
