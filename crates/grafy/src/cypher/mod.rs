//! Cypher-Lite. Read-only subset. Scope is fixed in plan §5.
//! Lands in M1 week 5.

pub mod ast;
pub mod error;
pub mod executor;
pub mod lexer;
pub mod parser;
pub mod plan;
pub mod scope;

pub use error::CypherError;
pub use executor::{Row, Value};

/// Top-level entry point used by both the CLI and the MCP `cypher_query` tool.
/// Read-only — opens a `ReadTransaction` and never mutates the store.
pub fn execute(db: &redb::Database, query: &str) -> Result<Vec<Row>, CypherError> {
    let ast = parser::parse(query)?;
    let pipeline = plan::Planner::build(ast)?;
    executor::run(db, &pipeline)
}
