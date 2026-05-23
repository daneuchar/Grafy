//! Parse-tree cache shared between pass 1 (producer) and pass 3 + pass 4 (consumers).
//!
//! ## Design
//!
//! Pass 1 parses every changed file and stores `Arc<ParsedFile>` in a
//! `DashMap<PathBuf, Arc<ParsedFile>>` (`ParseCache`). After pass 1 finishes
//! (its thread joins), the cache is fully populated. Passes 3 and 4 borrow
//! immutably from the map, eliminating their per-file re-read + re-parse.
//!
//! ## Memory ceiling
//!
//! A `tree_sitter::Tree` is roughly 3× the source byte size in memory.
//! For ripgrep (~3.5 MB source), the tree cache is ~10 MB — fine.
//! For a large TypeScript monorepo (700 k LOC, ~200 MB source) the ceiling
//! would be ~600 MB. The env var `GRAFY_PARSE_CACHE_MAX_MB` (default: 1024)
//! caps the cache at that many MiB; once the budget is exhausted pass 1 stops
//! inserting new entries. Pass 3 / pass 4 will simply fall back to a fresh
//! re-read + re-parse for files that were not cached (cold-path only).
//!
//! ## Send + Sync
//!
//! `tree_sitter::Tree` owns its internal memory and is `Send + Sync`.
//! `Arc<[u8]>` is `Send + Sync`.
//! `ParsedFile` and `ParseCache` are therefore `Send + Sync`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use tree_sitter::Tree;

use grafy_parser::Language;

/// Threshold in bytes beyond which pass 1 stops inserting into the cache.
/// Derived from the `GRAFY_PARSE_CACHE_MAX_MB` env var (default 1024 MiB).
fn cache_budget_bytes() -> u64 {
    let mb: u64 = std::env::var("GRAFY_PARSE_CACHE_MAX_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024);
    mb * 1024 * 1024
}

/// One parsed file, shareable across threads.
///
/// Invariant: `source` is the exact bytes that were fed to `tree`. The tree's
/// byte ranges reference positions into `source`.
pub struct ParsedFile {
    /// Owned parse tree — no `Node<'_>` alive here.
    pub tree: Tree,
    /// Raw source bytes (referenced by tree byte ranges).
    pub source: Arc<[u8]>,
    /// Language used to produce `tree`.
    pub lang: Language,
}

// Safety: Tree is internally `Send + Sync` (it does not alias external memory
// and all mutations go through `&mut Tree`). The `source` Arc is trivially
// `Send + Sync`.
unsafe impl Send for ParsedFile {}
unsafe impl Sync for ParsedFile {}

/// Thread-safe map from absolute path → `Arc<ParsedFile>`.
/// Populated by pass 1; consumed (read-only) by pass 3 and pass 4.
pub type ParseCache = DashMap<PathBuf, Arc<ParsedFile>>;

/// Atomic byte-size budget tracker for the cache.
pub struct CacheBudget {
    used: AtomicU64,
    limit: u64,
}

impl CacheBudget {
    #[must_use]
    pub fn new() -> Self {
        Self {
            used: AtomicU64::new(0),
            limit: cache_budget_bytes(),
        }
    }

    /// Try to reserve `bytes` from the budget. Returns `true` if the
    /// reservation succeeded (i.e. the entry should be inserted into the
    /// cache). Returns `false` when the budget is exhausted.
    pub fn try_reserve(&self, bytes: u64) -> bool {
        // Relaxed ordering is fine: the budget is a soft cap, not a hard
        // synchronisation barrier. Minor overshoot under contention is
        // acceptable and expected.
        let prev = self.used.fetch_add(bytes, Ordering::Relaxed);
        prev + bytes <= self.limit
    }
}

impl Default for CacheBudget {
    fn default() -> Self {
        Self::new()
    }
}
