# Cypher-Lite Reference

Cypher-Lite is the read-only, fixed-scope Cypher dialect used by `grafy query` and the `cypher_query` MCP tool. Scope is governed by plan §5 and is frozen for v1.0. Expansions require updating plan §5 and §8 first.

---

## Supported constructs

### MATCH

Fixed-length node and relationship patterns, up to 3 chained relationships.

```cypher
-- Simple node scan
MATCH (n:Function) RETURN n.fqn

-- One-hop relationship
MATCH (a:Function)-[:CALLS]->(b:Function) RETURN a.fqn, b.fqn

-- Two-hop chain
MATCH (a)-[r]->(b)-[r2]->(c) RETURN a.fqn, b.fqn, c.fqn

-- Three-hop chain (maximum)
MATCH (a)-[]->(b)-[]->(c)-[]->(d) RETURN a.fqn, d.fqn

-- Undirected relationship
MATCH (a)-[r]-(b) RETURN a.fqn, b.fqn

-- Right-to-left direction
MATCH (b:Function)<-[:CALLS]-(a:Function) RETURN a.fqn, b.fqn

-- Inline property filter
MATCH (n:Function {fqn: "crate::main"}) RETURN n.file
```

**Supported node labels:** `File`, `Module`, `Function`, `Class`, `Struct`, `Enum`, `Trait`, `Method`, `Route`.

**Supported edge types:** `CALLS`, `ROUTES`. Unrecognised type strings match zero edges (no error).

### WHERE

Boolean predicates over node and edge properties.

```cypher
MATCH (n:Function) WHERE n.fqn CONTAINS "main" RETURN n.fqn
MATCH (n:Function) WHERE n.fqn STARTS WITH "crate::" RETURN n.fqn
MATCH (n:Function) WHERE n.fqn ENDS WITH "_test" RETURN n.fqn
MATCH (n:Function) WHERE n.file = "src/lib.rs" RETURN n.fqn
MATCH (n:Function) WHERE n.byte_start >= 0 AND n.byte_end <= 500 RETURN n.fqn
MATCH (n:Module)   WHERE NOT n.fqn = "" RETURN n.fqn
MATCH (n:Function) WHERE n.fqn IS NOT NULL RETURN n.fqn
MATCH (n:Function) WHERE n.fqn IS NULL RETURN n.fqn
```

**Supported operators:**

| Operator | Description |
|---|---|
| `=` | Equality |
| `!=`, `<>` | Inequality |
| `<`, `>`, `<=`, `>=` | Numeric/string comparison |
| `CONTAINS` | String contains |
| `STARTS WITH` | String prefix |
| `ENDS WITH` | String suffix |
| `AND`, `OR`, `NOT` | Logical connectives |
| `IS NULL`, `IS NOT NULL` | Null check |

### RETURN

Return node/edge variables and their properties.

```cypher
MATCH (n:Function) RETURN n              -- entire node record
MATCH (n:Function) RETURN n.fqn         -- single property
MATCH (a)-[r]->(b) RETURN a.fqn, b.fqn -- multiple
MATCH (n:Function) RETURN n.fqn AS name -- alias
MATCH (n:Function) RETURN DISTINCT n.fqn -- deduplicate
```

**Node properties:** `fqn`, `file`, `byte_start`, `byte_end`, `kind`, `id`.

**Edge properties:** `kind`, `from`, `to`.

### ORDER BY, SKIP, LIMIT

```cypher
MATCH (n:Function) RETURN n.fqn ORDER BY n.fqn
MATCH (n:Function) RETURN n.fqn ORDER BY n.fqn DESC
MATCH (n:Function) RETURN n.fqn ORDER BY n.fqn SKIP 10 LIMIT 20
```

---

## Unsupported constructs (v1.0)

Each unsupported feature returns a structured error pointing to the recommended alternative.

### WITH clauses

**Error:** `ERROR: Cypher-Lite does not support \`WITH\` clauses (v1.0). For this query, use the structured tool \`search_graph\`. See docs/cypher-lite.md.`

**Alternative:** Use the `search_graph` structured tool for multi-step filtering.

### UNWIND

**Error:** `ERROR: Cypher-Lite does not support \`UNWIND\` (v1.0). For this query, iterate from the client side. See docs/cypher-lite.md.`

**Alternative:** Perform iteration on the client side.

### MERGE

**Error:** `ERROR: Cypher-Lite does not support \`MERGE\` (v1.0). For this query, Cypher-Lite is read-only. See docs/cypher-lite.md.`

**Alternative:** Cypher-Lite is read-only. Use `grafy index` to rebuild the store.

### CREATE

**Error:** `ERROR: Cypher-Lite does not support \`CREATE\` (v1.0). For this query, Cypher-Lite is read-only. See docs/cypher-lite.md.`

### DELETE

**Error:** `ERROR: Cypher-Lite does not support \`DELETE\` (v1.0). For this query, Cypher-Lite is read-only. See docs/cypher-lite.md.`

### SET

**Error:** `ERROR: Cypher-Lite does not support \`SET\` (v1.0). For this query, Cypher-Lite is read-only. See docs/cypher-lite.md.`

### REMOVE

**Error:** `ERROR: Cypher-Lite does not support \`REMOVE\` (v1.0). For this query, Cypher-Lite is read-only. See docs/cypher-lite.md.`

### Variable-length paths

**Error:** `ERROR: Cypher-Lite does not support variable-length paths (\`*\`, \`*1..3\`) (v1.0). For this query, use the structured tool \`trace_call_path\` with bounded hops. See docs/cypher-lite.md.`

**Examples of unsupported syntax:** `(a)-[*]->(b)`, `(a)-[*1..3]->(b)`, `(a)-[r*]->(b)`

**Alternative:** Use the `trace_call_path` structured tool with a bounded `depth` parameter.

### OPTIONAL MATCH

**Error:** `ERROR: Cypher-Lite does not support \`OPTIONAL MATCH\` (v1.0). For this query, use multiple \`search_graph\` calls. See docs/cypher-lite.md.`

**Alternative:** Issue separate `search_graph` calls and merge client-side.

### Aggregations

**Error:** `ERROR: Cypher-Lite does not support aggregations (\`count\`, \`sum\`, \`collect\`, …) (v1.0). For this query, aggregate on the client side. See docs/cypher-lite.md.`

**Alternative:** Retrieve the raw rows and aggregate on the client.

### Functions

**Error:** `ERROR: Cypher-Lite does not support functions (\`toLower\`, \`coalesce\`, …) (v1.0). For this query, filter on raw properties. See docs/cypher-lite.md.`

**Alternative:** Use raw property comparisons instead.

### Multiple MATCH clauses

**Error:** `ERROR: Cypher-Lite does not support multiple \`MATCH\` clauses in one query (v1.0). For this query, issue separate queries. See docs/cypher-lite.md.`

**Alternative:** Issue separate queries and join results client-side.

### Path variables

**Error:** `ERROR: Cypher-Lite does not support path variables (\`p = (a)-[*]->(b)\`) (v1.0). For this query, use the structured tool \`trace_call_path\`. See docs/cypher-lite.md.`

**Alternative:** Use the `trace_call_path` structured tool.

---

## Diagnosing query errors

### `CypherError::Parse`

Syntax error in the query. The error message includes the byte offset.

**Action:** Check the query syntax at the reported offset. Cypher-Lite uses standard Cypher syntax for the supported subset.

**Example:** `ERROR: query parse error — expected RETURN clause (check syntax at offset 42). See docs/cypher-lite.md.`

### `CypherError::Unsupported`

The query uses a feature outside the v1.0 scope.

**Action:** Read the error message — it names the exact feature and the recommended structured-tool replacement.

**Example:** `ERROR: Cypher-Lite does not support \`WITH\` clauses (v1.0). For this query, use the structured tool \`search_graph\`. See docs/cypher-lite.md.`

### `CypherError::Execute`

Runtime execution error. Most commonly the 100,000-row cap.

**Action:** Add a `LIMIT` clause to your query to bound the result set.

**Example:** `ERROR: query execution failed — query exceeded internal row cap of 100000; add a LIMIT clause to your query. See docs/cypher-lite.md.`

### `CypherError::Storage`

The redb store is missing or corrupt.

**Action:** Rebuild the store with `grafy index <path>`.

**Example:** `ERROR: storage error — open nodes table failed. Rebuild the store with \`grafy index\`. See docs/cypher-lite.md.`

---

## Hard limits

| Limit | Value | Rationale |
|---|---|---|
| Maximum row count | 100,000 | DoS backstop. Add `LIMIT` to your query. |
| Maximum chained hops | 3 | Plan §5 scope freeze. Use `trace_call_path` for deeper BFS. |

---

## CLI usage

```bash
# Index a repository first
grafy index /path/to/repo

# Run a query
grafy query /path/to/repo 'MATCH (n:Function) RETURN n.fqn LIMIT 10'

# Rows are emitted as JSON Lines to stdout
# Errors go to stderr, exit code 2
```

---

## MCP usage

The `cypher_query` tool (registered as `query_graph`) accepts the same Cypher-Lite queries and returns rows as a JSON array.

```json
{
  "tool": "query_graph",
  "params": {
    "query": "MATCH (n:Function) WHERE n.fqn CONTAINS 'main' RETURN n.fqn LIMIT 5",
    "project": "my-repo"
  }
}
```
