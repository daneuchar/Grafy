# M2 demo fixture — expected resolution

## Shape

```
lib/notify.py:     def send_email(addr, body): ...
lib/__init__.py:   from .notify import send_email as send
app/main.py:       from lib import send
                   def alert(user, msg):
                       send(user.email, msg)
```

## Truth table

| Resolver | Edge `alert -> ?` | Verdict |
|---|---|---|
| M1 heuristic (pass-3) | nothing OR `alert -> send` (alias node, no body) | **wrong / missing** |
| scip-python via SCIP ingest | `alert -> send_email` (in `lib/notify.py`) | **correct** |

## Why heuristic loses

M1's pass-3 import-aware resolver does **not** follow aliases through a
package's `__init__.py`. It sees:

1. `from lib import send` in `app/main.py` — records `send` as imported from
   module `lib`.
2. The bare call site `send(...)` — looks up `send` in the local namespace
   and resolves to the **import binding**, not the underlying
   `send_email` definition in `lib/notify.py`.

In the redb store this surfaces as either:

- **No `CALLS` edge from `alert`** (heuristic refuses to emit when the
  callee isn't a defined function in the corpus — `send` was an import
  binding, not a `def`), or
- **`alert -> send`** where `send` is the alias re-export, which has no
  function body and is functionally a different node from `send_email`.

## Why SCIP wins

`scip-python` runs a real Python type system (pyright-derived). It follows
the alias through `lib/__init__.py`'s `from .notify import send_email as
send` and emits a reference at `app/main.py:20:4` whose symbol points at
`scip-python ... lib.notify/send_email().` — i.e. the *defining* function,
not the alias.

The grafy SCIP-ingest path translates that occurrence into an
`EdgeKind::Scip` edge `(alert_node_id, send_email_node_id)` — the
binding-precise edge that the heuristic cannot produce.

## What the integration test asserts

`crates/grafy/tests/m2_demo_fixture.rs`:

1. **Heuristic-only** run (`GRAFY_SCIP_DISABLE=1`): no `CALLS` edge from
   `alert` to `send_email` exists. The test asserts the *absence* — proves
   the heuristic gap.
2. **SCIP-augmented** run (no env var, scip-python auto-detected): an
   `EdgeKind::Scip` edge from `alert` to `send_email` exists.

The test is `#[ignore]`d on hosts where `scip-python` is not on `PATH`,
exiting cleanly. Asciinema script: `demos/m2-demo.md`.
