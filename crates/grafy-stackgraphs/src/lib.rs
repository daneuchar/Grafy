//! Grafy stack-graphs — name resolution.
//!
//! M0: placeholder. M2 week 1 subprocess-validates against upstream
//! github/stack-graphs and decides fork-vs-vendor based on F1.
//! See plan §4 M2.

#![allow(clippy::module_name_repetitions)]

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error(
        "stack-graphs resolver not yet implemented (M2). Use the M1 heuristic resolver for now."
    )]
    NotYetImplemented,
}

/// Placeholder so the crate compiles and downstream wiring is real.
pub fn resolve() -> Result<(), ResolveError> {
    Err(ResolveError::NotYetImplemented)
}
