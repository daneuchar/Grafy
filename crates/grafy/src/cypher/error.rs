//! Cypher-Lite error type. Every variant carries a one-line next-step
//! phrase per the project-wide error UX policy.

use thiserror::Error;

use crate::cypher::scope::Unsupported;

#[derive(Debug, Error)]
pub enum CypherError {
    /// Syntax error produced by the lexer or parser.
    /// `span` is `(byte_start, byte_end)` within the original query string.
    #[error("ERROR: query parse error — {msg} (check syntax at offset {span_start}). See docs/cypher-lite.md.")]
    Parse {
        msg: String,
        span: (usize, usize),
        #[doc(hidden)]
        span_start: usize,
    },

    /// The query uses a feature that is outside the Cypher-Lite v1.0 scope.
    #[error("{0}")]
    Unsupported(#[source] UnsupportedError),

    /// Runtime execution error (e.g. row-cap exceeded, internal logic fault).
    #[error("ERROR: query execution failed — {msg}. See docs/cypher-lite.md.")]
    Execute { msg: String },

    /// Storage / redb layer error.
    #[error("ERROR: storage error — {0}. Rebuild the store with `grafy index`. See docs/cypher-lite.md.")]
    Storage(#[source] anyhow::Error),
}

impl CypherError {
    /// Convenience constructor for parse errors.
    pub fn parse(msg: impl Into<String>, span: (usize, usize)) -> Self {
        let span_start = span.0;
        CypherError::Parse {
            msg: msg.into(),
            span,
            span_start,
        }
    }
}

/// Newtype so `Unsupported` can implement `std::error::Error` (it's `Copy`).
#[derive(Debug)]
pub struct UnsupportedError(pub Unsupported);

impl std::fmt::Display for UnsupportedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.message())
    }
}

impl std::error::Error for UnsupportedError {}

impl From<Unsupported> for CypherError {
    fn from(u: Unsupported) -> Self {
        CypherError::Unsupported(UnsupportedError(u))
    }
}
