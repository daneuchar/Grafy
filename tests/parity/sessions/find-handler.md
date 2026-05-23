# Session: What handles GET /users/:id?

## Question

"What handles `GET /users/:id`?"

## Expected shape

`search_graph` with `label=Route` and `name_pattern` matching the route should return
the Route node. The `trace_path` tool (direction=outbound) then follows CALLS edges
from the Route node to the handler function.

```json
{
  "results": [
    {
      "name": "GET /users/:id",
      "label": "Route",
      "file": "express-svc/app.js",
      "byte_start": 100,
      "byte_end": 130
    }
  ],
  "total": 1,
  "has_more": false
}
```

And for `trace_path` (outbound from the Route node):

```json
{
  "function_name": "GET /users/:id",
  "direction": "outbound",
  "hops": [
    {
      "name": "getUser",
      "label": "Function",
      "file": "express-svc/app.js",
      "depth": 1
    }
  ],
  "hop_count": 1
}
```

## Request payloads (grafy mcp)

### Step 1 — find the Route node

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
      "name_pattern": "GET.*users.*id"
    }
  }
}
```

### Step 2 — trace outbound to handler

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "trace_path",
    "arguments": {
      "function_name": "GET /users/:id",
      "project": "routes",
      "direction": "outbound",
      "depth": 2
    }
  }
}
```

## Structural assertions (CI checks)

- `search_graph` result has at least 1 Route node with `name` matching the pattern.
- `trace_path` `hops` array is present (may be empty in M1 — Route→handler CALLS edges
  are extracted by pass4 only when the framework wiring pattern is detected).

## codebase-memory-mcp comparison

codebase-memory-mcp follows the same two-step pattern. The main difference is that
codebase-memory-mcp may resolve the handler edge at index time (static analysis of
`app.get('/users/:id', getUser)`), whereas Grafy M1 extracts the Route node and the
handler function separately. Route→handler edges via CALLS are a pass4 enhancement —
present when the Express/FastAPI/Gin wiring call is within the same file. See
`tests/parity/diffs.md` for details.
