# Session: List all HTTP routes

## Question

"List all HTTP routes."

## Expected shape

`search_graph` with `label=Route` should return the Route nodes extracted by pass4.
The fixture at `tests/fixtures/routes/` contains three services (Express, FastAPI, Gin)
with 2 routes each = 6 Route nodes total.

```json
{
  "results": [
    {
      "name": "GET /users/:id",
      "label": "Route",
      "file": "express-svc/app.js",
      "byte_start": 100,
      "byte_end": 150
    }
  ],
  "total": 6,
  "has_more": false
}
```

## Request payload (grafy mcp)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "search_graph",
    "arguments": {
      "project": "routes",
      "label": "Route",
      "limit": 20
    }
  }
}
```

## Structural assertions (CI checks)

- `results` is an array with length >= 1 when the routes fixture is indexed.
- Each result has `name` (containing the HTTP method and path), `label` == "Route",
  and `file` (relative path within the fixture dir).
- `total` >= 1.

## codebase-memory-mcp comparison

codebase-memory-mcp extracts routes via its own AST passes. The response format is
identical: `search_graph` returns Route nodes with the same field names. The route FQN
format may differ: Grafy uses `<METHOD> <path>` (e.g. `GET /users/:id`) while
codebase-memory-mcp may include the service name as a prefix. This is documented in
`tests/parity/diffs.md`.
