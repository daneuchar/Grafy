# Session: Cypher cross-file function call query

## Question

"MATCH (a:Function)-[:CALLS]->(b:Function) WHERE a.file CONTAINS 'pass3' RETURN a.fqn, b.fqn LIMIT 10"

## Expected shape

`query_graph` should execute the Cypher query and return rows where both `a.fqn` and
`b.fqn` are non-empty strings, and `a.file` contains the string `pass3`.

```json
{
  "rows": [
    {
      "a.fqn": "some::function::in::pass3",
      "b.fqn": "called::function"
    }
  ],
  "total": 3
}
```

At least 1 row is expected when the Grafy codebase is indexed (pass3 extracts call edges
across files).

## Request payload (grafy mcp)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "query_graph",
    "arguments": {
      "project": "grafy",
      "query": "MATCH (a:Function)-[:CALLS]->(b:Function) WHERE a.file CONTAINS 'pass3' RETURN a.fqn, b.fqn LIMIT 10"
    }
  }
}
```

## Structural assertions (CI checks)

- Response has `rows` (array) and `total` (integer).
- Each row has at least one key (function FQN).
- `total` >= 0 (0 is acceptable if the dogfood index has no pass3 edges yet).

## codebase-memory-mcp comparison

codebase-memory-mcp exposes this as `query_graph` with identical parameter names
(`query`, `project`, `max_rows`). The row format may differ in key naming conventions
(`a.fqn` vs `a_fqn`). Grafy uses the column alias as written in the RETURN clause.
This is a known difference — see `tests/parity/diffs.md`.
