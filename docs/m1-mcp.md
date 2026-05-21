# M1 W5 — MCP Server (plan §4)

`grafy mcp` exposes the codebase-memory-mcp tool surface over stdio transport
via rmcp 1.7.0. All 14 canonical tool schemas live under
`tests/parity/schemas/`. One alias (`trace_call_path`) is also registered for
backward compatibility.

Start the server:

```
grafy mcp --root /path/to/repo
```

Validate registrations (CI):

```
grafy mcp --check
```

---

## index_repository

**Schema:** `tests/parity/schemas/index_repository.json`

Runs the 4-pass pipeline (tree-sitter parse → store → call resolver → route
extractor) and persists the graph to `.grafy/index.redb`.

Input:

| field | type | required | description |
|---|---|---|---|
| `repo_path` | string | yes | Absolute or relative path to the repository |
| `mode` | string | no | `full` / `moderate` / `fast` / `cross-repo-intelligence` |
| `target_projects` | string[] | no | For cross-repo-intelligence mode |
| `persistence` | bool | no | Write compressed artifact |

Output shape:

```json
{
  "status": "indexed",
  "project": "myrepo",
  "root": "/abs/path",
  "files": 42,
  "modules": 10,
  "functions": 120,
  "classes": 5,
  "structs": 8,
  "enums": 3,
  "traits": 2,
  "methods": 40,
  "calls": 200,
  "routes": 15,
  "total_nodes": 230
}
```

Example prompt: "Index the repo at /home/user/myproject so I can query it."

---

## search_graph

**Schema:** `tests/parity/schemas/search_graph.json`

Search the graph for nodes (functions, classes, structs, routes, etc.) by
keyword, name pattern, label filter, or relationship. BM25 ranked when `query`
is set; exact regex when `name_pattern` is set.

`semantic_query` is documented in the schema but returns an unsupported error
in grafy v1.0 (no embedding backend). See `tests/parity/diffs.md`.

Input:

| field | type | required | description |
|---|---|---|---|
| `project` | string | yes | Repository name or root path |
| `query` | string | no | BM25 keyword search |
| `label` | string | no | Node kind filter: `Function`, `Class`, `Struct`, `Enum`, `Trait`, `Method`, `Route`, `Module`, `File` |
| `name_pattern` | string | no | Regex against node name |
| `qn_pattern` | string | no | Regex against qualified name |
| `file_pattern` | string | no | Regex against file path |
| `limit` | int | no | Max results (default 200) |
| `offset` | int | no | Pagination offset |

Output shape:

```json
{
  "nodes": [
    {
      "id": 1234567890,
      "name": "process_order",
      "qualified_name": "myrepo::orders::process_order",
      "kind": "Function",
      "file": "src/orders.rs",
      "start_line": 10
    }
  ],
  "total": 1,
  "has_more": false,
  "note": "semantic_query ignored — no embedding backend in grafy v1.0"
}
```

Example prompt: "Find all Route nodes in the payment service."

---

## query_graph

**Schema:** `tests/parity/schemas/query_graph.json`

Execute a read-only Cypher-Lite query against the indexed graph. Supported
subset: `MATCH`, `WHERE`, `RETURN`, `ORDER BY`, `LIMIT`, `SKIP`, `DISTINCT`,
up to 3 hops. Write operations return a structured unsupported error. See
`docs/cypher-lite.md` for the full grammar.

Input:

| field | type | required | description |
|---|---|---|---|
| `query` | string | yes | Cypher-Lite query string |
| `project` | string | yes | Repository name or root path |
| `max_rows` | int | no | Row cap (default: up to 100k ceiling) |

Output shape:

```json
{
  "rows": [
    { "n": { "name": "process_order", "kind": "Function", "file": "src/orders.rs" } }
  ],
  "count": 1
}
```

Example prompt: "Run MATCH (n:Function) WHERE n.name =~ '.*order.*' RETURN n LIMIT 20 against the orders repo."

---

## trace_path

**Schema:** `tests/parity/schemas/trace_path.json`

BFS call-path traversal from a named function. Returns the set of nodes and
edges reachable up to `depth` hops. Delegated to by `trace_call_path`.

Input:

| field | type | required | description |
|---|---|---|---|
| `function_name` | string | yes | Starting function name or qualified name |
| `project` | string | yes | Repository name or root path |
| `direction` | string | no | `outgoing` (default) / `incoming` / `both` |
| `depth` | int | no | Max hops 1–5 (default 3) |
| `mode` | string | no | `calls` / `data_flow` / `cross_service` |

Output shape:

```json
{
  "root": "process_order",
  "nodes": [
    { "id": 1, "name": "process_order", "kind": "Function", "file": "src/orders.rs" }
  ],
  "edges": [
    { "from": 1, "to": 2, "kind": "Calls" }
  ],
  "depth": 3,
  "truncated": false
}
```

Example prompt: "Show me the full call tree from process_order up to 4 hops."

---

## trace_call_path

Alias for `trace_path`. Identical input/output. Registered for backward
compatibility with codebase-memory-mcp clients.

Example prompt: "Trace the call path from validate_order."

---

## get_code_snippet

**Schema:** `tests/parity/schemas/get_code_snippet.json`

Return source lines for a function or class by qualified name. Reads the
source file directly; does not require the file to be parsed.

Input:

| field | type | required | description |
|---|---|---|---|
| `qualified_name` | string | yes | Fully-qualified name from search_graph |
| `project` | string | yes | Repository root |
| `include_neighbors` | bool | no | Include preceding/following definitions |

Output shape:

```json
{
  "qualified_name": "myrepo::orders::process_order",
  "file": "src/orders.rs",
  "start_line": 10,
  "end_line": 25,
  "snippet": "fn process_order(id: u64) -> bool {\n    ...\n}"
}
```

Example prompt: "Show me the source of myrepo::orders::process_order."

---

## get_graph_schema

**Schema:** `tests/parity/schemas/get_graph_schema.json`

Return the node kinds and edge kinds present in the indexed graph, plus counts.

Input:

| field | type | required | description |
|---|---|---|---|
| `project` | string | yes | Repository root |

Output shape:

```json
{
  "node_kinds": {
    "File": 10, "Module": 5, "Function": 80, "Class": 3,
    "Struct": 6, "Enum": 2, "Trait": 1, "Method": 30, "Route": 4
  },
  "edge_kinds": {
    "Calls": 150, "Routes": 4
  },
  "total_nodes": 141,
  "total_edges": 154
}
```

Example prompt: "What node types exist in the graph for this repo?"

---

## get_architecture

**Schema:** `tests/parity/schemas/get_architecture.json`

Return a high-level architectural summary: files, entry points (routes), and
module structure.

Input:

| field | type | required | description |
|---|---|---|---|
| `project` | string | yes | Repository root |
| `aspects` | string[] | no | Filter to `routes`, `modules`, `entry_points` |

Output shape:

```json
{
  "project": "myrepo",
  "total_files": 10,
  "total_nodes": 141,
  "routes": [
    { "id": 999, "name": "POST /orders", "kind": "Route", "file": "src/routes.py" }
  ],
  "entry_points": [],
  "note": "Use search_graph with label=Route for full route details."
}
```

Example prompt: "Give me an architectural overview of the payment service."

---

## search_code

**Schema:** `tests/parity/schemas/search_code.json`

Regex or literal search across source files (grep-equivalent). Returns file
paths and optional context lines.

Input:

| field | type | required | description |
|---|---|---|---|
| `pattern` | string | yes | Search pattern |
| `project` | string | yes | Repository root |
| `file_pattern` | string | no | Glob filter (e.g. `*.go`) |
| `path_filter` | string | no | Regex on result file paths |
| `mode` | string | no | `compact` (default) / `full` / `files` |
| `context` | int | no | Lines of context around match |
| `regex` | bool | no | Treat pattern as regex (default false) |
| `limit` | int | no | Max results (default 10) |

Output shape:

```json
{
  "matches": [
    {
      "file": "src/orders.rs",
      "line": 12,
      "text": "    let result = validate_order(id);"
    }
  ],
  "total": 1
}
```

Example prompt: "Find all usages of validate_order in Rust files."

---

## list_projects

**Schema:** `tests/parity/schemas/list_projects.json`

List all indexed repositories visible from the server's root.

Input: none

Output shape:

```json
{
  "projects": [
    { "name": "grafy", "root": "/home/user/grafy", "indexed": true }
  ]
}
```

Example prompt: "What projects have been indexed?"

---

## delete_project

**Schema:** `tests/parity/schemas/delete_project.json`

Remove a project's index. Stub in grafy v1.0 — returns a not-implemented
notice. Will remove `.grafy/index.redb` in a future release.

Input:

| field | type | required | description |
|---|---|---|---|
| `project` | string | yes | Project name or root path |

Output shape:

```json
{ "note": "delete_project: not yet implemented in grafy v1.0 — remove .grafy/index.redb manually." }
```

---

## index_status

**Schema:** `tests/parity/schemas/index_status.json`

Report whether a project has a current index and when it was last built.

Input:

| field | type | required | description |
|---|---|---|---|
| `project` | string | yes | Repository root |

Output shape:

```json
{
  "project": "myrepo",
  "indexed": true,
  "db_path": "/home/user/myrepo/.grafy/index.redb",
  "size_bytes": 204800
}
```

Example prompt: "Is the payment service indexed?"

---

## detect_changes

**Schema:** `tests/parity/schemas/detect_changes.json`

Stub in grafy v1.0. Git-diff-aware re-index is planned for M2. Returns a
not-implemented notice.

Input:

| field | type | required | description |
|---|---|---|---|
| `project` | string | yes | Repository root |
| `scope` | string | no | `changed_files` / `full` |
| `since` | string | no | Git ref or date |

Output shape:

```json
{ "note": "detect_changes: not yet implemented in grafy v1.0 — run index_repository to refresh." }
```

---

## manage_adr

**Schema:** `tests/parity/schemas/manage_adr.json`

Stub in grafy v1.0. ADR (Architecture Decision Record) management is planned
for M3. Returns a not-implemented notice.

Input:

| field | type | required | description |
|---|---|---|---|
| `project` | string | yes | Repository root |
| `mode` | string | no | `list` / `create` / `update` |
| `content` | string | no | ADR body (markdown) |

Output shape:

```json
{ "note": "manage_adr: not yet implemented in grafy v1.0." }
```

---

## ingest_traces

**Schema:** `tests/parity/schemas/ingest_traces.json`

Stub in grafy v1.0. Distributed-trace ingestion (OTEL / Jaeger) is planned
for M2. Returns a not-implemented notice.

Input:

| field | type | required | description |
|---|---|---|---|
| `traces` | object[] | yes | Array of trace span objects |
| `project` | string | yes | Repository root |

Output shape:

```json
{ "note": "ingest_traces: not yet implemented in grafy v1.0." }
```

---

## Parity notes

See `tests/parity/diffs.md` for a full accounting of differences between
grafy's tool surface and codebase-memory-mcp's upstream. Key points:

- `semantic_query` in `search_graph` is parsed but ignored (no embedding backend).
- `query_graph` corresponds to codebase-memory-mcp's `cypher_query` name.
- `delete_project`, `detect_changes`, `manage_adr`, `ingest_traces` are stubs.
- Tool JSON output is stable. Breaking changes require a deprecation cycle.
