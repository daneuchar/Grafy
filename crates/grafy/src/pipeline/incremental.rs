//! Incremental reindex — file-level change detection (M1 W6, plan §4).
//!
//! Strategy:
//!   1. Compute blake3 hash of file contents.
//!   2. Compare against the `FileRecord` stored in the `files` table.
//!   3. If blake3 matches → `Unchanged` (content identical; mtime change is a
//!      no-op for our purposes).
//!   4. If blake3 differs or no previous record exists → `New` / `Modified`.
//!
//! mtime is a **fast-skip only**: when blake3 matches we mark `Unchanged`
//! regardless of mtime. mtime alone is never used to declare a file changed,
//! because the hash is the ground truth.
//!
//! # Tree::edit note
//!
//! tree-sitter's `Tree::edit` API can incrementally update a cached parse tree
//! for a single edit region. That optimisation requires holding live parse trees
//! across pipeline runs — i.e., an in-process daemon with a warm tree cache.
//!
//! Grafy currently runs as a stateless CLI (no daemon). Therefore:
//!   - `Unchanged` files skip parse entirely (bigger win: zero parse work).
//!   - `Modified` files reparse from scratch (correct, deterministic).
//!   - `Tree::edit` is deferred to the daemon / v1.x milestone.
//!
//! See also `docs/m1-incremental.md`.

use std::path::Path;
use std::time::SystemTime;

use crate::store::FileRecord;

/// Classification of a single file relative to the previous index run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// No previous record. File will be fully indexed.
    New,
    /// blake3 hash matches previous record. Skip parse + queries.
    Unchanged,
    /// blake3 hash differs. Stale nodes swept; file fully reindexed.
    Modified,
}

/// Classify one file.
///
/// `prev` is the `FileRecord` stored from the last run, if any.
/// `path` is the absolute path to the file on disk.
///
/// Returns `New` when there is no previous record, `Unchanged` when the
/// blake3 content hash is identical, or `Modified` when it differs.
///
/// Errors reading the file (permissions, gone) result in `Modified` so
/// the pipeline can attempt a fresh read (which will log its own warning).
pub fn classify(prev: Option<&FileRecord>, path: &Path) -> FileStatus {
    let prev = match prev {
        None => return FileStatus::New,
        Some(r) => r,
    };

    // Read file bytes to compute content hash.
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => {
            // Can't read — treat as modified so the pipeline logs the error.
            return FileStatus::Modified;
        }
    };

    let current_hash = {
        let mut h = blake3::Hasher::new();
        h.update(&bytes);
        *h.finalize().as_bytes()
    };

    if current_hash == prev.blake3 {
        FileStatus::Unchanged
    } else {
        FileStatus::Modified
    }
}

/// Read the file's content, compute the blake3 hash and mtime, and return
/// a fresh `FileRecord`. Returns `None` if the file cannot be read.
pub fn file_record(path: &Path, lang_str: &str) -> Option<FileRecord> {
    let bytes = std::fs::read(path).ok()?;
    let blake3 = {
        let mut h = blake3::Hasher::new();
        h.update(&bytes);
        *h.finalize().as_bytes()
    };
    let mtime_ns = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Some(FileRecord {
        blake3,
        mtime_ns,
        lang: lang_str.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_record(bytes: &[u8]) -> FileRecord {
        let mut h = blake3::Hasher::new();
        h.update(bytes);
        FileRecord {
            blake3: *h.finalize().as_bytes(),
            mtime_ns: 0,
            lang: "rust".into(),
        }
    }

    #[test]
    fn new_when_no_prev() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.rs");
        std::fs::write(&p, b"fn x() {}").unwrap();
        assert_eq!(classify(None, &p), FileStatus::New);
    }

    #[test]
    fn unchanged_when_hash_matches() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.rs");
        let content = b"fn x() {}";
        std::fs::write(&p, content).unwrap();
        let rec = make_record(content);
        assert_eq!(classify(Some(&rec), &p), FileStatus::Unchanged);
    }

    #[test]
    fn modified_when_hash_differs() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.rs");
        std::fs::write(&p, b"fn y() {}").unwrap();
        let rec = make_record(b"fn x() {}"); // different content
        assert_eq!(classify(Some(&rec), &p), FileStatus::Modified);
    }

    #[test]
    fn unchanged_even_if_mtime_changed() {
        // Same bytes → Unchanged, regardless of mtime in the stored record.
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.rs");
        let content = b"fn x() {}";
        std::fs::write(&p, content).unwrap();
        let mut rec = make_record(content);
        rec.mtime_ns = 99_999_999_999; // stale mtime
        assert_eq!(classify(Some(&rec), &p), FileStatus::Unchanged);
    }
}
