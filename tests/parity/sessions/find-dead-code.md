# Session: Find dead code

## Question

"Find dead code."

## Expected shape

`search_graph` filtered to functions with no incoming CALLS edges. In Grafy's current
implementation this is achieved by combining `label=Function` with
`exclude_entry_points=true` (which filters nodes with degree 0) or by running a Cypher
query that returns functions with no inbound edges.

```json
{
  "results": [
    {
      "name": "<fqn of dead function>",
      "label": "Function",
      "file": "<relative path>",
      "byte_start": 0,
      "byte_end": 100
    }
  ],
  "total": 3,
  "has_more": false
}
```

## Request payload (grafy mcp)

### Option A — search_graph (structural filter)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "search_graph",
    "arguments": {
      "project": "grafy",
      "label": "Function",
      "exclude_entry_points": true,
      "limit": 50
    }
  }
}
```

### Option B — query_graph (Cypher)

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "query_graph",
    "arguments": {
      "project": "grafy",
      "query": "MATCH (n:Function) WHERE NOT ()-[:CALLS]->(n) RETURN n.fqn, n.file LIMIT 20"
    }
  }
}
```

## Structural assertions (CI checks)

- `results` is an array.
- Each result element has `name`, `label`, `file`.
- `total` is an integer >= 0.
- `has_more` is a boolean.

## codebase-memory-mcp comparison

codebase-memory-mcp supports `min_degree`/`max_degree` filters on `search_graph` and
can directly filter to degree-0 nodes. Grafy's `search_graph` includes these parameters
in the schema (pass-through) but degree counting is planned for M2. The Cypher path
(Option B) works in Grafy M1 for repos already indexed with pass3.
