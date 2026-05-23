# Session: Who calls Pipeline::index?

## Question

"Who calls `Pipeline::index`?"

## Expected shape

The response from `trace_path` (direction=inbound) should include a `hops` array.
Each hop has `name`, `label`, `file`, and `depth` fields.
For the Grafy codebase, the known callers include the MCP handler and the CLI entry point.

```json
{
  "function_name": "Pipeline::index",
  "direction": "inbound",
  "depth": 3,
  "hops": [
    {
      "name": "<caller fqn>",
      "label": "Function",
      "file": "<relative path>",
      "depth": 1
    }
  ],
  "hop_count": 2
}
```

Alternatively, `search_graph` with `name_pattern` can find the callers via nodes with
outgoing CALLS edges to `index`.

## Request payload (grafy mcp)

### Option A — trace_path (preferred)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "trace_path",
    "arguments": {
      "function_name": "Pipeline::index",
      "project": "grafy",
      "direction": "inbound",
      "depth": 3
    }
  }
}
```

### Option B — search_graph with relationship filter

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "search_graph",
    "arguments": {
      "project": "grafy",
      "query": "index",
      "label": "Function"
    }
  }
}
```

## Structural assertions (CI checks)

- `hops` is an array (may be empty if the Grafy index was built without pass3 edges).
- Each hop has `name` (string), `label` (string), `file` (string), `depth` (integer).
- `function_name` in response matches the queried name.

## codebase-memory-mcp comparison

codebase-memory-mcp uses the same `trace_path` tool with identical parameter names and
response shape. The `hops` array format is identical. Expected difference: Grafy may
return fewer hops in early milestones because stack-graphs name resolution (M2) is not
yet complete; heuristic call resolution (pass3) provides partial results.
