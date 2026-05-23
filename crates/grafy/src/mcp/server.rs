//! MCP stdio-transport server entry point — M1 W5 (plan §4).

use std::path::PathBuf;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};

use super::handler::GrafyServer;

/// Start the stdio-transport MCP server rooted at `root`.
///
/// Blocks until the MCP client disconnects or the process is signalled.
/// All tracing output goes to stderr; the stdio transport owns stdout/stdin.
pub async fn serve(root: PathBuf) -> Result<()> {
    tracing::info!(
        target: "grafy.mcp",
        root = %root.display(),
        "MCP server starting (stdio transport)"
    );

    let server = GrafyServer::new(root);
    let service = server.serve(stdio()).await.map_err(|e| {
        anyhow::anyhow!(
            "mcp server: failed to start stdio transport — check stdin/stdout are a pipe. ({e})"
        )
    })?;

    service.waiting().await.map_err(|e| {
        anyhow::anyhow!("mcp server: transport error — restart the agent and retry. ({e})")
    })?;

    tracing::info!(target: "grafy.mcp", "MCP server shut down cleanly");
    Ok(())
}

/// Validate that all 14 canonical tools (+ trace_call_path alias) are registered.
/// Called by `grafy mcp --check`; exits 0 on success.
///
/// Tool count is 15: 14 canonical tool names from codebase-memory-mcp plus the
/// `trace_call_path` alias for backward compatibility (source: mcp.c line 3996).
pub fn check() -> Result<()> {
    use crate::mcp::handler::TOOL_NAMES;

    // 14 canonical + 1 alias = 15
    const EXPECTED: usize = 15;

    let count = TOOL_NAMES.len();
    println!(
        "grafy mcp --check: {count} tool entries registered ({} canonical + 1 alias)",
        EXPECTED - 1
    );
    for name in TOOL_NAMES {
        println!("  ok  {name}");
    }
    if count != EXPECTED {
        anyhow::bail!(
            "mcp --check: expected {EXPECTED} tool entries, found {count}. Update TOOL_NAMES in handler.rs."
        );
    }
    println!("grafy mcp --check: all {count} entries OK");
    Ok(())
}
