# grafy

Polyglot, LLM-free code-intelligence engine. Drop-in alternative to [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) with stack-graphs-grade name resolution.

**Status:** pre-M0 placeholder (v0.1.0). Plan and roadmap: [github.com/daneuchar/Grafy](https://github.com/daneuchar/Grafy).

## What ships in v1.0

- 12 languages: Rust, Python, JS, TS, TSX, Go, Java, C++, C#, PHP, Lua, Scala.
- 11 MCP tools, schema-compatible with codebase-memory-mcp.
- Stack-graphs binding-precise cross-file resolution for Python, TS, Java.
- LSP server for Zed, VSCode, Neovim, Helix.
- Single static Rust binary. No LLM, no embeddings, no external services.

## Quickstart (target — not yet shipping)

```bash
cargo install grafy
grafy install            # auto-configures Claude Code / Codex / Cursor / Zed
grafy index .
```

## License

MIT OR Apache-2.0.
