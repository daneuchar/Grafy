//! Grafy library crate.
//!
//! Modules first, crates later (plan §3). Promote a module to its own
//! crate only when a second external consumer appears.

pub mod cypher;
pub mod fqn;
pub mod lang;
pub mod mcp;
pub mod pipeline;
pub mod routes;
pub mod store;
pub mod watch;

pub use pipeline::Pipeline;
