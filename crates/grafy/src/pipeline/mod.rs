//! Indexing pipeline. Four passes (plan §4 M1):
//! 1. structure   — File / Module / Function / Class / Struct / Enum / Trait / Method nodes
//! 2. definitions — FQN resolution + NodeId assignment
//! 3. calls       — M1 heuristic resolver (W3)
//! 4. routes      — HTTP route ↔ call-site linking (W4)
//!
//! M1 W6: incremental reindex via blake3 content hashing.

pub mod cache;
pub mod channels;
pub mod incremental;
pub mod pass1;
pub mod pass2;
pub mod pass3;
pub mod pass4;
pub mod queries;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{bounded, unbounded};
use ignore::WalkBuilder;
use postcard::from_bytes;
use redb::ReadableTable;
use tracing::{info, info_span, warn};

use crate::store::EdgeKind;

use grafy_parser::Language;

use crate::pipeline::cache::{CacheBudget, ParseCache};
use crate::store::{FileRecord, NodeKind, Store, WriterStats, FILES_TABLE};

use channels::{FileWork, WriteEvent, CHANNEL_BUFFER};

pub struct Pipeline {
    root: PathBuf,
}

/// Aggregated counts per node kind + call edges, returned from `Pipeline::index`.
#[derive(Debug, Default, Clone)]
pub struct IndexReport {
    pub files: u64,
    pub modules: u64,
    pub functions: u64,
    pub classes: u64,
    pub structs: u64,
    pub enums: u64,
    pub traits: u64,
    pub methods: u64,
    /// Number of resolved call edges written by pass 3.
    pub calls: u64,
    /// Number of HTTP route nodes written by pass 4.
    pub routes: u64,
    /// Number of binding-precise edges ingested from SCIP indexers (M2 W2).
    pub scip_edges: u64,
    /// Set of distinct `grafy_parser::Language` discriminants whose files
    /// were touched during this run. Used by SCIP ingest to skip indexers
    /// for languages not present in the repo.
    pub languages: HashSet<u8>,
    // Incremental counts (M1 W6).
    pub unchanged: u64,
    pub modified: u64,
    pub new_files: u64,
    pub deleted: u64,
}

impl IndexReport {
    #[allow(clippy::too_many_arguments)]
    fn from_counts(
        counts: &HashMap<u8, u64>,
        calls: u64,
        routes: u64,
        scip_edges: u64,
        languages: HashSet<u8>,
        unchanged: u64,
        modified: u64,
        new_files: u64,
        deleted: u64,
    ) -> Self {
        Self {
            files: *counts.get(&(NodeKind::File as u8)).unwrap_or(&0),
            modules: *counts.get(&(NodeKind::Module as u8)).unwrap_or(&0),
            functions: *counts.get(&(NodeKind::Function as u8)).unwrap_or(&0),
            classes: *counts.get(&(NodeKind::Class as u8)).unwrap_or(&0),
            structs: *counts.get(&(NodeKind::Struct as u8)).unwrap_or(&0),
            enums: *counts.get(&(NodeKind::Enum as u8)).unwrap_or(&0),
            traits: *counts.get(&(NodeKind::Trait as u8)).unwrap_or(&0),
            methods: *counts.get(&(NodeKind::Method as u8)).unwrap_or(&0),
            calls,
            routes,
            scip_edges,
            languages,
            unchanged,
            modified,
            new_files,
            deleted,
        }
    }

    /// True if any indexed file used `lang`. SCIP ingest uses this to skip
    /// indexers whose language doesn't appear in the repo.
    #[must_use]
    pub fn has_language(&self, lang: grafy_parser::Language) -> bool {
        self.languages.contains(&(lang as u8))
    }

    pub fn total_nodes(&self) -> u64 {
        self.files
            + self.modules
            + self.functions
            + self.classes
            + self.structs
            + self.enums
            + self.traits
            + self.methods
            + self.routes
    }
}

impl Pipeline {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Run all four passes over `self.root`, persist to redb, return counts.
    ///
    /// This is now a thin wrapper around `index_incremental` with
    /// `rebuild = false`. Pass `rebuild = true` to skip the unchanged check
    /// (full reindex).
    pub fn index(&self) -> anyhow::Result<IndexReport> {
        self.index_incremental(false)
    }

    /// Full reindex — ignores previous hashes and reindexes every file.
    pub fn index_rebuild(&self) -> anyhow::Result<IndexReport> {
        self.index_incremental(true)
    }

    /// Incremental reindex. When `rebuild` is `false`, unchanged files are
    /// detected by blake3 hash comparison and skipped. When `rebuild` is
    /// `true`, all files are processed regardless of hash (same as a cold
    /// index).
    ///
    /// # Algorithm
    ///
    /// 1. Walk all files; classify each as `New | Unchanged | Modified`.
    /// 2. For `Unchanged` files: skip parse entirely; keep existing nodes.
    /// 3. For `Modified` or deleted files: send `DeleteNodesForFile` before
    ///    re-emitting nodes (stale-node sweep).
    /// 4. For `New` or `Modified` files: run full pass 1–4.
    /// 5. For files present in the store but absent from the walk (deleted):
    ///    send `DeleteNodesForFile`.
    pub fn index_incremental(&self, rebuild: bool) -> anyhow::Result<IndexReport> {
        let span = info_span!("pipeline.index_incremental", root = %self.root.display(), rebuild);
        let _e = span.enter();

        // ------------------------------------------------------------------
        // Open store once — held for the full pipeline lifetime.
        // Plan §8 W2: eliminates the double-open / re-open for count phase.
        // ------------------------------------------------------------------
        let store = Store::open(&self.root)?;

        // ------------------------------------------------------------------
        // Load previous file records for incremental classification.
        // ------------------------------------------------------------------
        let prev_records: HashMap<String, FileRecord> = if rebuild {
            HashMap::new()
        } else {
            load_file_records(&store)?
        };

        // ------------------------------------------------------------------
        // Walk and classify files.
        // ------------------------------------------------------------------
        let mut new_count: u64 = 0;
        let mut unchanged_count: u64 = 0;
        let mut modified_count: u64 = 0;

        // Files seen during this walk (rel-paths).
        let mut seen_files: HashSet<String> = HashSet::new();

        // Work items for pass 1 (New + Modified only).
        let mut work_items: Vec<FileWork> = Vec::new();
        // Rel-paths of files to sweep before re-indexing.
        let mut to_sweep: Vec<String> = Vec::new();
        // Languages observed in the walk (for SCIP indexer dispatch).
        let mut languages_seen: HashSet<u8> = HashSet::new();

        {
            let span = info_span!("pipeline.walk");
            let _e = span.enter();
            let mut files_seen = 0usize;

            for result in WalkBuilder::new(&self.root).standard_filters(true).build() {
                let entry = match result {
                    Ok(e) => e,
                    Err(e) => {
                        warn!(target: "grafy.pipeline", error = %e, "walk entry error");
                        continue;
                    }
                };
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                files_seen += 1;
                let path = entry.path().to_path_buf();
                let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                    continue;
                };
                let Some(lang) = Language::from_extension(ext) else {
                    continue;
                };

                let rel_path = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();

                seen_files.insert(rel_path.clone());
                languages_seen.insert(lang as u8);

                let prev = prev_records.get(&rel_path);
                let status = incremental::classify(prev, &path);

                match status {
                    incremental::FileStatus::Unchanged => {
                        unchanged_count += 1;
                        // Skip — keep existing store data.
                    }
                    incremental::FileStatus::New => {
                        new_count += 1;
                        work_items.push(FileWork {
                            path,
                            language: lang,
                        });
                    }
                    incremental::FileStatus::Modified => {
                        modified_count += 1;
                        to_sweep.push(rel_path);
                        work_items.push(FileWork {
                            path,
                            language: lang,
                        });
                    }
                }
            }
            info!(
                target: "grafy.pipeline",
                files_seen,
                new = new_count,
                unchanged = unchanged_count,
                modified = modified_count,
                "walk + classify complete"
            );
        }

        // ------------------------------------------------------------------
        // Deleted files: present in previous store but not seen in this walk.
        // ------------------------------------------------------------------
        let deleted_count = prev_records
            .keys()
            .filter(|k| !seen_files.contains(*k))
            .count() as u64;
        let deleted_paths: Vec<String> = prev_records
            .keys()
            .filter(|k| !seen_files.contains(*k))
            .cloned()
            .collect();

        info!(
            target: "grafy.pipeline",
            unchanged = unchanged_count,
            modified = modified_count,
            new = new_count,
            deleted = deleted_count,
            "incremental classification"
        );

        // If nothing changed and nothing deleted, fast-path: just tally and return.
        if work_items.is_empty() && deleted_paths.is_empty() && to_sweep.is_empty() {
            let report = count_nodes_from_store_db(
                store.read_db(),
                languages_seen.clone(),
                unchanged_count,
                0,
                0,
                0,
            )?;
            info!(
                target: "grafy.pipeline",
                unchanged = report.unchanged,
                "fast-path: nothing changed"
            );
            return Ok(report);
        }

        // ------------------------------------------------------------------
        // Channel topology (same as full index, but only changed files flow
        // through pass 1; passes 3 + 4 operate on all definitions).
        // ------------------------------------------------------------------
        let (files_tx, files_rx) = bounded::<FileWork>(CHANNEL_BUFFER);
        // structure_tx and definitions_tx use unbounded channels: the main
        // thread cannot drain definitions_rx until after p1 + p2 join, so a
        // bounded channel here causes a deadlock when the repo emits more
        // definition events than the buffer capacity (plan §8 W6 fix).
        let (structure_tx, structure_rx) = unbounded::<channels::StructureEvent>();
        let (definitions_tx, definitions_rx) = unbounded::<channels::DefinitionEvent>();
        let (write_tx, write_rx) = bounded::<WriteEvent>(CHANNEL_BUFFER);

        // Spawn writer thread.
        let writer_handle = store.writer(write_rx);

        // ------------------------------------------------------------------
        // Send sweep events BEFORE pass 1 starts consuming work.
        // The writer processes them in batch order, so deletions land before
        // fresh inserts for Modified files.
        // ------------------------------------------------------------------
        for rel_path in to_sweep {
            let _ = write_tx.send(WriteEvent::DeleteNodesForFile(rel_path));
        }
        for rel_path in deleted_paths {
            let _ = write_tx.send(WriteEvent::DeleteNodesForFile(rel_path));
        }

        let write_tx_p2 = write_tx.clone();
        let write_tx_p3 = write_tx.clone();
        let write_tx_p4 = write_tx.clone();

        let root_p1 = self.root.clone();
        let root_p2 = self.root.clone();

        // Parse cache: populated by pass 1, consumed by pass 3 + pass 4.
        // The Arc lets us share the map across the scope boundary without copying.
        let parse_cache: Arc<ParseCache> = Arc::new(ParseCache::new());
        let budget = Arc::new(CacheBudget::new());
        let cache_p1 = Arc::clone(&parse_cache);
        let budget_p1 = Arc::clone(&budget);

        // Spawn pass 2.
        let p2_handle = std::thread::spawn(move || {
            pass2::run(&root_p2, structure_rx, definitions_tx, write_tx_p2);
        });

        // Spawn pass 1 — also populates parse_cache.
        let p1_handle = std::thread::spawn(move || {
            pass1::run(
                &root_p1,
                files_rx,
                structure_tx,
                write_tx,
                &cache_p1,
                &budget_p1,
            );
        });

        // Feed work items into pass 1.
        {
            let span = info_span!("pipeline.feed_work");
            let _e = span.enter();
            let total = work_items.len();
            for fw in work_items {
                if files_tx.send(fw).is_err() {
                    warn!(target: "grafy.pipeline", "pass1 receiver dropped early");
                    break;
                }
            }
            info!(target: "grafy.pipeline", sent = total, "work items fed");
            drop(files_tx);
        }

        if let Err(e) = p1_handle.join() {
            warn!(target: "grafy.pipeline", "pass1 thread panicked: {:?}", e);
        }
        if let Err(e) = p2_handle.join() {
            warn!(target: "grafy.pipeline", "pass2 thread panicked: {:?}", e);
        }

        // ------------------------------------------------------------------
        // Barrier: build SymbolTable from definitions, run pass3 + pass4.
        // ------------------------------------------------------------------
        let events: Vec<channels::DefinitionEvent> = definitions_rx.into_iter().collect();
        tracing::info!(
            target: "grafy.pipeline",
            definitions = events.len(),
            "definitions drained — building symbol table"
        );

        let sym = pass3::build_symbol_table(&events, &self.root);
        let files = pass3::unique_files(&events);

        let root_ref = &self.root;
        let files_ref = &files;
        let sym_ref = &sym;
        let cache_ref = &parse_cache;

        std::thread::scope(|s| {
            let p3 = s.spawn(|| {
                pass3::run_with_table(root_ref, files_ref, sym_ref, write_tx_p3, cache_ref);
            });
            let p4 = s.spawn(|| {
                pass4::run_with_table(root_ref, files_ref, sym_ref, write_tx_p4, cache_ref);
            });
            if let Err(e) = p3.join() {
                warn!(target: "grafy.pipeline", "pass3 thread panicked: {:?}", e);
            }
            if let Err(e) = p4.join() {
                warn!(target: "grafy.pipeline", "pass4 thread panicked: {:?}", e);
            }
        });

        // All senders dropped → writer drains and exits.
        let stats = match writer_handle.join() {
            Ok(s) => s,
            Err(e) => {
                warn!(target: "grafy.pipeline", "writer thread panicked: {:?}", e);
                WriterStats::default()
            }
        };

        // ------------------------------------------------------------------
        // SCIP ingest sidecar (plan §4 M2 W2). Runs after the heuristic
        // passes — augments, never replaces. Disabled via GRAFY_SCIP_DISABLE=1.
        // ------------------------------------------------------------------
        if std::env::var("GRAFY_SCIP_DISABLE").is_err() {
            run_scip_ingest(&self.root, &languages_seen);
            // First-run banner: shown once per repo when no SCIP indexer is
            // detected (does not require ingest to have succeeded).
            maybe_show_first_run_banner(&self.root);
        }

        // ------------------------------------------------------------------
        // Re-open store for read-only count (uses the same db path, but redb
        // requires a new read transaction after the writer has committed).
        // Plan §8 fix: we pass the already-opened Store::read_db() handle.
        // ------------------------------------------------------------------
        // Re-open to get a fresh read view after writes committed.
        let store2 = Store::open(&self.root)?;
        let report = count_nodes_from_store_db(
            store2.read_db(),
            languages_seen.clone(),
            unchanged_count,
            modified_count,
            new_count,
            deleted_count,
        )?;

        info!(
            target: "grafy.pipeline",
            files = report.files,
            modules = report.modules,
            functions = report.functions,
            classes = report.classes,
            structs = report.structs,
            enums = report.enums,
            traits = report.traits,
            methods = report.methods,
            calls = report.calls,
            routes = report.routes,
            unchanged = report.unchanged,
            modified = report.modified,
            new_files = report.new_files,
            deleted = report.deleted,
            writer_nodes = stats.nodes_written,
            writer_edges = stats.edges_written,
            writer_nodes_deleted = stats.nodes_deleted,
            writer_edges_deleted = stats.edges_deleted,
            "index complete"
        );

        Ok(report)
    }
}

/// Load all `FileRecord`s from the `files` table of an open store.
fn load_file_records(store: &Store) -> anyhow::Result<HashMap<String, FileRecord>> {
    let db = store.read_db();
    let tx = db.begin_read()?;
    let tbl = tx.open_table(FILES_TABLE)?;
    let mut map = HashMap::new();
    for item in tbl.iter()? {
        let (key, val) = item?;
        if let Ok(rec) = from_bytes::<FileRecord>(val.value()) {
            map.insert(key.value().to_owned(), rec);
        }
    }
    Ok(map)
}

/// Read the `nodes` and `edges` tables from a redb `Database` and tally counts.
/// Takes the `Database` reference directly to avoid a second `Store::open`.
#[allow(clippy::too_many_arguments)]
fn count_nodes_from_store_db(
    db: &redb::Database,
    languages: HashSet<u8>,
    unchanged: u64,
    modified: u64,
    new_files: u64,
    deleted: u64,
) -> anyhow::Result<IndexReport> {
    use crate::store::{NodeRecord, EDGES_TABLE, NODES_TABLE};

    let tx = db.begin_read()?;

    // Node counts by kind.
    let nodes_tbl = tx.open_table(NODES_TABLE)?;
    let mut counts: HashMap<u8, u64> = HashMap::new();
    for item in nodes_tbl.iter()? {
        let (_key, val) = item?;
        if let Ok(rec) = from_bytes::<NodeRecord>(val.value()) {
            *counts.entry(rec.kind as u8).or_insert(0) += 1;
        }
    }

    // Edge counts by kind.
    let edges_tbl = tx.open_table(EDGES_TABLE)?;
    let mut calls: u64 = 0;
    let mut scip_edges: u64 = 0;
    for item in edges_tbl.iter()? {
        let (key, _val) = item?;
        let (_from, _to, kind) = key.value();
        if kind == EdgeKind::Calls as u8 {
            calls += 1;
        } else if kind == EdgeKind::Scip as u8 {
            scip_edges += 1;
        }
    }

    let routes = *counts.get(&(NodeKind::Route as u8)).unwrap_or(&0);

    Ok(IndexReport::from_counts(
        &counts, calls, routes, scip_edges, languages, unchanged, modified, new_files, deleted,
    ))
}

/// Spawn each detected SCIP indexer in sequence, ingest its `.scip` output.
/// Skips indexers for languages absent from this run. Never panics; emits
/// `tracing::warn!` and continues on per-indexer failures.
fn run_scip_ingest(root: &Path, languages_seen: &HashSet<u8>) {
    let span = info_span!("pipeline.scip_ingest");
    let _e = span.enter();

    let indexers = crate::scip::detect::detected_indexers();
    if indexers.is_empty() {
        tracing::debug!(
            target: "grafy.pipeline",
            "no SCIP indexers detected — skipping ingest (heuristic-only)"
        );
        return;
    }

    // Open the store ONCE. We snapshot the node indexes via a short-lived
    // read transaction (released before spawning the writer), then move the
    // Store into the writer thread which takes exclusive write access.
    // redb permits only one process-wide handle; reopening for reads while a
    // writer also holds the file errors with "Database already open".
    let store = match Store::open(root) {
        Ok(s) => s,
        Err(e) => {
            warn!(target: "grafy.pipeline", error = %e, "scip ingest: store reopen failed");
            return;
        }
    };
    let snapshot = match crate::scip::ingest::snapshot_indexes(&store) {
        Ok(s) => s,
        Err(e) => {
            warn!(target: "grafy.pipeline", error = %e, "scip ingest: snapshot failed");
            return;
        }
    };
    let (write_tx, write_rx) = crossbeam_channel::bounded::<WriteEvent>(CHANNEL_BUFFER);
    let writer_handle = store.writer(write_rx);

    for ix in &indexers {
        if !languages_seen.contains(&(ix.language as u8)) {
            continue;
        }
        let scip_path = match crate::scip::run_indexer(ix, root) {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    target: "grafy.scip",
                    indexer = ix.name,
                    language = ix.language.as_str(),
                    error = %e,
                    "SCIP indexer failed — falling back to heuristic for this language."
                );
                continue;
            }
        };
        match crate::scip::ingest::ingest_scip_with_snapshot(
            &scip_path, &snapshot, root, &write_tx, ix.name,
        ) {
            Ok(rep) => info!(
                target: "grafy.scip",
                indexer = ix.name,
                language = ix.language.as_str(),
                occurrences = rep.occurrences_seen,
                edges = rep.edges_emitted,
                "ingested",
            ),
            Err(e) => warn!(
                target: "grafy.scip",
                indexer = ix.name,
                error = %e,
                "ingest failed",
            ),
        }
    }
    drop(write_tx);
    let _ = writer_handle.join();
}

/// Print the first-run banner once per repo when **no** SCIP indexers are
/// detected. Banner is written to stderr (tracing/info go there too); the
/// repo-local marker file `.grafy/.first-run` disables it on subsequent runs.
fn maybe_show_first_run_banner(root: &Path) {
    let marker = root.join(".grafy").join(".first-run");
    if marker.exists() {
        return;
    }
    let detected = crate::scip::detect::detected_indexers();
    if !detected.is_empty() {
        // User has at least one indexer; the banner is for the empty case.
        // Still write the marker so we don't probe on every run.
        let _ = std::fs::write(&marker, b"shown\n");
        return;
    }
    let repo_name = root.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
    eprintln!(
        "grafy: indexed {repo_name} (heuristic resolution).\n  For binding-precise references, install a SCIP indexer:\n    grafy install --with-scip\n  Grafy auto-detects them on the next run.\n  Dismiss permanently: touch {}/.grafy/.first-run",
        root.display()
    );
    let _ = std::fs::write(&marker, b"shown\n");
}

/// Emit a minimal Graphviz `.dot` representation.
pub fn to_dot(report: &IndexReport, root: &Path) -> String {
    let mut out = String::new();
    out.push_str("digraph grafy {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str(&format!("  root [label=\"{}\"];\n", root.display()));
    out.push_str(&format!(
        "  nodes [label=\"files={} modules={} functions={} classes={} structs={} enums={} traits={} methods={}\"];\n",
        report.files,
        report.modules,
        report.functions,
        report.classes,
        report.structs,
        report.enums,
        report.traits,
        report.methods,
    ));
    out.push_str("  root -> nodes;\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pipeline_index_single_rs_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.rs"), "fn foo() {}\nstruct Bar;\n").unwrap();
        let report = Pipeline::new(dir.path()).index().expect("index");
        assert!(report.files >= 1, "files={}", report.files);
        assert!(report.functions >= 1, "functions={}", report.functions);
    }
}
