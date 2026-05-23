# M1 Parity Gate: Schema-Compat + Recorded-Session Tests

Plan §4 — M1 quality gate. No "drop-in alternative to codebase-memory-mcp" claim ships
until both gates are green.

## Two gates

### Gate 1 — Schema compatibility (CI-enforced)

`make parity` runs `cargo test -p grafy --features testing --test parity_schemas`.

Fourteen schema files live under `tests/parity/schemas/<tool>.json`, one per
codebase-memory-mcp tool. Each file is the authoritative input-parameter schema
extracted from the upstream source (`codebase-memory-mcp/src/mcp/mcp.c`, TOOLS[]
array). The test suite:

1. Loads each schema file.
2. Compiles it with `jsonschema::validator_for` — schema parse errors are **blockers**.
3. Builds a representative request payload and validates it against the schema.
4. Invokes the matching `GrafyServer` handler via the `dispatch` test helper.
5. Asserts the response JSON has the expected top-level keys.

Schema drift — any upstream schema change that breaks validation — is a CI blocker, not
a warning. If the schema cannot be matched, document the reason in
`tests/parity/diffs.md` before merging.

### Gate 2 — Recorded-session parity (release-gated)

`make parity` also runs `cargo test -p grafy --features testing --test parity_sessions`.

Five representative Claude Code prompts live under `tests/parity/sessions/`. Each file
contains:
- The natural-language question.
- Expected response shape (JSON snippet).
- The exact MCP request payload.
- The structural assertions the CI test checks.
- Notes comparing the behaviour to codebase-memory-mcp.

The CI test checks **structural compatibility** only (correct keys, array shapes, integer
types). A maintainer eyeballs the actual content before each release and records
differences with rationale in `tests/parity/diffs.md`.

## Running the gates

```bash
# Both gates together
make parity

# Schema gate only
cargo test -p grafy --features testing --test parity_schemas

# Session gate only
cargo test -p grafy --features testing --test parity_sessions

# Verbose output (shows tracing::debug! lines)
RUST_LOG=grafy.parity=debug cargo test -p grafy --features testing --test parity_schemas -- --nocapture
```

## Adding a new session

1. Create `tests/parity/sessions/<slug>.md` following the format of existing sessions.
2. Add a corresponding test function to `crates/grafy/tests/parity_sessions.rs`.
3. Index the relevant fixture or build a temp dir in the test body.
4. Assert response shape using `assert_has_key` and array checks — not exact strings.
5. Run `make parity` and verify the test is green before merging.

## Handling upstream drift

When codebase-memory-mcp adds, renames, or modifies a tool:

1. Add an entry to `tests/parity/drift-log.md` (date, tool, upstream commit, status=pending).
2. Update the corresponding `tests/parity/schemas/<tool>.json` to match the new schema.
3. Update the handler in `crates/grafy/src/mcp/handler.rs` to match.
4. Update the test in `parity_schemas.rs` if the response shape changes.
5. Update the drift-log entry to `status=matched`.
6. If a change cannot be matched (e.g. vector-search feature), document it in
   `tests/parity/diffs.md` and set `status=deferred`.

## Interpreting a failure

**Schema compile failure** (`validator_for` error)

```
schema <name>.json failed to compile (BLOCKER):
  <jsonschema error>
  path: tests/parity/schemas/<name>.json
```

The schema file is malformed JSON or contains an unsupported JSON Schema keyword.
Compare against the upstream source and fix.

**Payload validation failure**

```
payload does not conform to schema <name>.json:
  instance_path=<path> — <error>
```

The representative payload in the test does not match the schema. Update the payload in
`parity_schemas.rs` to build a correct request.

**Response key failure**

```
<tool>: response missing key '<key>'. Got keys: [...]
```

The handler returns a different response shape than expected. Either update the expected
keys in `parity_schemas.rs` or fix the handler. If it's a legitimate incompatibility,
document it in `tests/parity/diffs.md`.

**Session structural failure**

```
session=<slug> tool=<tool>: response missing key '<key>'. Got: {...}
```

The handler returned an unexpected JSON shape. Check the `dispatch` call in
`parity_sessions.rs` and compare against the session file's expected shape.

## Stub tools

Four tools are stubs in Grafy v1 (no implementation yet). They always return
`{"error": "..."}`. The schema test asserts the error key is present. The stub status
is documented in `tests/parity/diffs.md`.

| Tool | Reason | Alternative |
|---|---|---|
| `delete_project` | No multi-project store | `rm -rf .grafy/` |
| `detect_changes` | Git diff integration not built | `git diff` + re-index |
| `manage_adr` | No ADR model in v1 | Edit docs/adr/ directly |
| `ingest_traces` | No runtime trace model | N/A |
