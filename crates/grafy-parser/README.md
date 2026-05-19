# grafy-parser

Thread-safe tree-sitter parser pool for [Grafy](https://github.com/daneuchar/Grafy). Reusable as a library — designed to be embedded by other code-intelligence tools.

**Status:** pre-M0 placeholder (v0.1.0). API will land at M1 (plan §4).

## What it does

- `thread_local!` tree-sitter parsers, one per language per thread.
- 5-second per-file wall-clock timeout (DoS backstop).
- Structured `ParseError` naming the file + a one-line "what to do next."
- No `Node<'_>` ever crosses the crate API surface — Send/Sync safe by construction.

## Languages

M0: Rust, Python. M1 adds: JS, TS, TSX, Go, Java, C++, C#, PHP, Lua, Scala.

## License

MIT OR Apache-2.0.
