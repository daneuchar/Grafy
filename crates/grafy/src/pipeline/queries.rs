//! Cached `tree_sitter::Query` instances + thread-local `QueryCursor` pool.
//! Compiling a Query is non-trivial; caching one per `(language, kind)`
//! saves ~4× per-file work on cold index. Reusing the cursor avoids the
//! per-file allocation of cursor state.

use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use grafy_parser::Language;
use tree_sitter::{Query, QueryCursor};

use crate::lang::{calls_scm, definitions_scm, imports_scm};
use crate::pipeline::pass3::ts_language_for;

thread_local! {
    /// One `QueryCursor` per worker thread, reused across files.
    pub static CURSOR: RefCell<QueryCursor> = RefCell::new(QueryCursor::new());
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub enum QueryKind {
    Definitions,
    Calls,
    Imports,
}

type Key = (Language, QueryKind);

fn cache() -> &'static DashMap<Key, Arc<Query>> {
    static CACHE: OnceLock<DashMap<Key, Arc<Query>>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

/// Returns a cached `Arc<Query>` for `(lang, kind)`. Compiles once.
///
/// Returns `Err(String)` only on the first compile attempt; subsequent
/// hits short-circuit. The error string is the raw `QueryError::Display`.
pub fn get(lang: Language, kind: QueryKind) -> Result<Arc<Query>, String> {
    let key = (lang, kind);
    if let Some(q) = cache().get(&key) {
        return Ok(Arc::clone(&q));
    }
    let scm = match kind {
        QueryKind::Definitions => definitions_scm(lang),
        QueryKind::Calls => calls_scm(lang),
        QueryKind::Imports => imports_scm(lang),
    };
    let ts_lang = ts_language_for(lang);
    let q = Query::new(&ts_lang, scm).map_err(|e| format!("{e}"))?;
    let arc = Arc::new(q);
    cache().insert(key, Arc::clone(&arc));
    Ok(arc)
}
