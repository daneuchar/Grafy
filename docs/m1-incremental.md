# M1 W6 — Incremental Reindex

## Overview

`grafy index` is now incremental by default. On each run it:

1. Walks the repository, classifying each file as `New | Unchanged | Modified`.
2. Skips parse entirely for `Unchanged` files (blake3 hash match).
3. Sweeps stale nodes for `Modified` files before re-emitting fresh ones.
4. Sweeps nodes for files deleted since the last run.

The engineering gate is: **single-file edit reindex p95 < 250 ms** on a 100k-LOC repo.

## Change detection algorithm

```
classify(prev: Option<&FileRecord>, path: &Path) -> FileStatus
```

1. Read file bytes from disk.
2. Compute blake3 hash.
3. Compare against `prev.blake3` (stored from last run).
4. If match → `Unchanged` (content identical; mtime change is irrelevant).
5. If no prev → `New`.
6. If hash differs → `Modified`.

mtime is **not used** to declare a file changed. It can be used as a
pre-check fast-skip in a future daemon (read mtime first; if identical
skip the read and hash), but the hash is always the ground truth.

## Secondary index: nodes_by_file

A `nodes_by_file` table (`(file_rel_path, node_id) → []`) is maintained
alongside `NODES_TABLE`. This enables O(file-nodes) stale-node sweeps:

```
DeleteNodesForFile(rel_path)
  → range-scan nodes_by_file for (rel_path, *)
  → delete each node from NODES_TABLE
  → delete secondary index entries
  → scan EDGES_TABLE for edges touching any deleted node_id, delete them
  → delete FileRecord from FILES_TABLE
```

The edge scan is O(all-edges). An edge-by-file index is a v1.x optimisation.

## Tree::edit — deferred to daemon mode

tree-sitter's `Tree::edit` API can update a live parse tree incrementally for a
single character range. The expected win is:

- For a small edit in a 1000-line file, re-tokenising only the changed subtree
  can be 10–50x faster than a full parse.

**Why it is not implemented in W6:**

tree-sitter's `Tree::edit` requires holding a `Tree` object in memory across
pipeline runs. Grafy currently runs as a stateless CLI: each `grafy index`
invocation opens a fresh process, loads the store from redb, runs the pipeline,
and exits. There is no in-process warm tree cache.

The incremental win achieved in W6 — skipping parse entirely for `Unchanged`
files — is actually **larger** than what `Tree::edit` would provide for a
small edit. A file with zero content change costs zero parse work. `Tree::edit`
only helps when a file has changed.

**Deferred path:**

- Daemon mode (v1.x): a long-lived `grafy serve` process will hold a
  `HashMap<PathBuf, (Tree, Vec<u8>)>` warm cache.
- On `inotify`/`FSEvents` notification for a file change, the daemon will
  compute the edit range, call `tree.edit(input_edit)`, and re-run the query
  only on the mutated subtree.
- The expected latency for a single-character edit in a 1000-line file is
  < 5 ms (parse) + < 1 ms (query) = < 10 ms end-to-end.

## CLI

```sh
# incremental (default)
grafy index .

# force full reindex (ignore cached hashes)
grafy index --rebuild .
```

## Report fields

`IndexReport` now includes:

| field       | meaning                                          |
|-------------|--------------------------------------------------|
| `unchanged` | files skipped (hash match)                       |
| `modified`  | files reparsed (hash changed)                    |
| `new_files` | files seen for the first time                    |
| `deleted`   | files in store but absent from this walk         |

## Plan §8 risks addressed

- **W2 double-open**: `count_nodes_from_store` previously opened a second
  `Store` handle after the writer closed. The fix stores a fresh `Store::open`
  handle for the read phase, removing the need to hold the writer-side handle
  open. (Full single-handle dedup requires a `read()` method on a shared `Arc<Store>`,
  which is v1.x scope once the daemon lands.)
- **Modified file ghost nodes**: addressed by `DeleteNodesForFile` sweep sent
  before fresh nodes are emitted. The writer processes deletions in the same
  batch order as inserts (FIFO channel), so deletions always precede the
  corresponding new nodes.
