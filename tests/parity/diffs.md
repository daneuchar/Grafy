# Parity diffs vs codebase-memory-mcp

## Tool count

The task spec says "11 tools". The codebase-memory-mcp source (`src/mcp/mcp.c`, TOOLS[] array) declares 14 tools. Grafy implements all 14 canonical tools.

## Tool name: `trace_path` / `trace_call_path`

codebase-memory-mcp's C source registers the tool as `trace_path` in the TOOLS[] array, but its dispatch block also accepts `trace_call_path` as a legacy alias:

```c
if (strcmp(tool_name, "trace_path") == 0 || strcmp(tool_name, "trace_call_path") == 0) {
```

Grafy exposes both names: the primary tool is registered as `trace_path`; a `trace_call_path` alias routes to the same handler. The schema under `tests/parity/schemas/trace_path.json` is authoritative.

## Tools with no store backend (stubs)

These tools have no equivalent in grafy's redb store model and return `Unsupported` errors with a one-line action pointing users to a structured alternative:

| Tool | Status | Alternative |
|------|--------|-------------|
| `delete_project` | Stub | `rm -rf .grafy/` manually |
| `detect_changes` | Stub | `git diff` + re-index |
| `manage_adr` | Stub | Edit docs/adr/ directly |
| `ingest_traces` | Stub | Not applicable (no runtime trace model in v1) |
| `index_status` | Stub | Re-run `grafy index` |

## `semantic_query` parameter in `search_graph`

codebase-memory-mcp's `search_graph` supports a `semantic_query` array for vector-cosine search backed by bundled Nomic embeddings. Grafy's `search_graph` handler ignores `semantic_query` (no embedding backend) and returns an empty `semantic_results` array with a warning. This is consistent with the project non-negotiable: no embedding or vector backend.

## `query_graph` vs `cypher_query`

The task spec lists `cypher_query` as a best-effort tool name. The authoritative upstream name is `query_graph`. Grafy registers `query_graph` as the primary name and wires it to `crate::cypher::execute`.
