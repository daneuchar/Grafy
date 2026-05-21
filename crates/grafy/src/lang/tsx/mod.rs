//! TSX language pack. M1 W1: definitions (shared with TS). M1 W3: calls + imports.

pub const DEFINITIONS_SCM: &str = include_str!("../typescript/definitions.scm");
pub const CALLS_SCM: &str = include_str!("calls.scm");
pub const IMPORTS_SCM: &str = include_str!("imports.scm");
