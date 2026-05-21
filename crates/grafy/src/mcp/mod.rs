//! MCP server — M1 W5 (plan §4).
//!
//! Implements a stdio-transport MCP server matching the codebase-memory-mcp
//! 14-tool surface. Tool names, descriptions, and input schemas are extracted
//! verbatim from codebase-memory-mcp/src/mcp/mcp.c (TOOLS[] array).
//!
//! Architecture:
//!   `server`  — entry point: `serve(root)` and `check()`.
//!   `handler` — the `GrafyServer` struct with `#[tool_router(server_handler)]`.

pub mod handler;
pub mod server;
