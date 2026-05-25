# M2 demo asciinema script

60-second screencast: heuristic resolver vs SCIP ingest on a hand-crafted
aliased-export Python project. Total wall budget: ~60 s; the recording is
the M2 milestone demo gate.

## Pre-flight (do once, off-camera)

```sh
# Install the release binary.
cargo build --release

# Make sure scip-python is on PATH. If not:
npm install -g @sourcegraph/scip-python
# or:
grafy install --with-scip
```

## Script

Each step lists a wall-clock budget. Total: 55 s.

### 0. Setup (5 s)

```sh
mkdir -p /tmp/grafy-demo
cp -r /Users/danieleuchar/workspace/grafy/tests/fixtures/demo/. /tmp/grafy-demo/
cd /tmp/grafy-demo
tree -L 3
```

Expected output:
```
.
├── app
│   ├── __init__.py
│   └── main.py
├── expected.md
└── lib
    ├── __init__.py
    └── notify.py
```

### 1. Heuristic only — show the missing edge (15 s)

```sh
GRAFY_SCIP_DISABLE=1 grafy index .
grafy query . 'MATCH (a:Function)-[r:CALLS]->(b:Function) WHERE a.fqn ENDS WITH "alert" RETURN a.fqn, b.fqn'
```

Expected stdout: **nothing**. No `CALLS` edge from `alert` — the heuristic
sees `send` as an import binding, not a function, and refuses to emit.

Narration: "Pass-3's heuristic resolver is import-aware but doesn't follow
aliased re-exports. It sees `from lib import send`, looks up `send`
locally, finds an import binding, and stops. No edge."

### 2. Install scip-python if needed (5 s)

```sh
grafy install --with-scip
```

Expected: report says `scip-python` ready (or already present). Skip if
pre-installed.

### 3. Re-index with SCIP ingest enabled (15 s)

```sh
rm -rf .grafy
grafy index .
```

Expected tail of output:
```
files=4 ... calls=0 routes=0 scip_edges=6 ...
```

Note: `calls=0` (heuristic still couldn't resolve), but `scip_edges=6`
(SCIP ingest provided the binding-precise edges).

### 4. Query the new SCIP edge (10 s)

```sh
grafy query . 'MATCH (a:Function)-[r:SCIP]->(b:Function) WHERE a.fqn ENDS WITH "alert" RETURN a.fqn, b.fqn'
```

Expected stdout:
```
{"a.fqn":"app.main.alert","b.fqn":"lib.notify.send_email"}
```

Narration: "scip-python ran pyright internally, followed the alias
through `lib/__init__.py`, and resolved `send(...)` to its actual
definition `lib.notify.send_email`. The grafy SCIP-ingest path translates
that into an `EdgeKind::Scip` edge in the redb store."

### 5. Dual-mode summary (10 s)

```sh
grafy query . 'MATCH (a:Function)-[r]->(b:Function) WHERE a.fqn ENDS WITH "alert" RETURN a.fqn, type(r), b.fqn'
```

Expected stdout: only the SCIP edge (heuristic didn't produce any). On a
real repo with both edge kinds the query would interleave them, which is
the M2 pitch: "heuristic everywhere by default; SCIP precision when you
ask for it."

## Recording tips

- Use `asciinema rec -i 2 m2-demo.cast` so idle terminal seconds are
  trimmed to 2 s.
- Embed via `agg m2-demo.cast m2-demo.gif` or upload to asciinema.org.
- The CI-verifiable backstop is `crates/grafy/tests/m2_demo_fixture.rs`;
  if anything in this script regresses, those tests fail.

## Citable claim for the M2 pitch

> "Heuristic call resolution misses aliased re-exports. SCIP ingest adds
> binding-precise edges from any of six supported indexers (scip-python,
> scip-typescript, scip-go, scip-java, scip-clang, rust-analyzer scip)
> without changing the grafy pipeline. The dual-mode store keeps both
> edge sets — `EdgeKind::Calls` for breadth, `EdgeKind::Scip` for
> precision."
