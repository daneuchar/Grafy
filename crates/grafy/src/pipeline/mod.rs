//! Indexing pipeline. Four passes (plan §4 M1):
//! 1. structure   — File / Module / Function / Class / Struct / Enum / Trait / Method nodes
//! 2. definitions — FQN resolution + NodeId assignment
//! 3. calls       — M1 heuristic resolver (W3)
//! 4. routes      — HTTP route ↔ call-site linking (W4)
//!
//! M1 W2: passes 1 + 2 implemented. Channel skeleton for 3 + 4 wired.

pub mod channels;
pub mod pass1;
pub mod pass2;
pub mod pass3;
pub mod pass4;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crossbeam_channel::bounded;
use ignore::WalkBuilder;
use redb::ReadableTable;
use tracing::{info, info_span, warn};

use crate::store::EdgeKind;

use grafy_parser::Language;

use crate::store::{NodeKind, Store, WriterStats};

use channels::{FileWork, CHANNEL_BUFFER};

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
}

impl IndexReport {
    fn from_counts(counts: &HashMap<u8, u64>, calls: u64, routes: u64) -> Self {
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
        }
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
    pub fn index(&self) -> anyhow::Result<IndexReport> {
        let span = info_span!("pipeline.index", root = %self.root.display());
        let _e = span.enter();

        // Open (or create) the store.
        let store = Store::open(&self.root)?;

        // ----------------------------------------------------------------
        // Channel topology:
        //
        //   walk ──files──► pass1 ──structure──► pass2 ──definitions──► drain
        //   pass1 ──write_tx──► writer
        //   pass2 ──write_tx──► writer
        //
        //   Barrier: drain thread collects all DefinitionEvents, then the
        //   pipeline builds the SymbolTable and runs pass3 + pass4 in
        //   parallel scoped threads sharing &SymbolTable.
        //
        //   pass3 ──write_tx_p3──► writer
        //   pass4 ──write_tx_p4──► writer
        // ----------------------------------------------------------------

        // Walk → pass 1.
        let (files_tx, files_rx) = bounded::<FileWork>(CHANNEL_BUFFER);

        // Pass 1 → pass 2.
        let (structure_tx, structure_rx) = bounded::<channels::StructureEvent>(CHANNEL_BUFFER);

        // Pass 2 → drain thread. Drain collects all events; its channel closes
        // when pass 2 drops definitions_tx.
        let (definitions_tx, definitions_rx) =
            bounded::<channels::DefinitionEvent>(CHANNEL_BUFFER);

        // Pass 1 + 2 + 3 + 4 → store writer.
        let (write_tx, write_rx) = bounded::<channels::WriteEvent>(CHANNEL_BUFFER);

        // Spawn writer thread first.
        let writer_handle = store.writer(write_rx);

        // Clone write senders for pass 2, pass 3, pass 4 before moving into threads.
        let write_tx_p2 = write_tx.clone();
        let write_tx_p3 = write_tx.clone();
        let write_tx_p4 = write_tx.clone();

        let root_p1 = self.root.clone();
        let root_p2 = self.root.clone();

        // Spawn pass 2.
        // When pass 2 finishes, definitions_tx drops → definitions channel closes.
        let p2_handle = std::thread::spawn(move || {
            pass2::run(&root_p2, structure_rx, definitions_tx, write_tx_p2);
        });

        // Spawn pass 1.
        // When pass 1 finishes it drops structure_tx → pass 2 sees channel close.
        let p1_handle = std::thread::spawn(move || {
            pass1::run(&root_p1, files_rx, structure_tx, write_tx);
        });

        // Walk on the current thread; drop files_tx when done.
        {
            let span = info_span!("pipeline.walk");
            let _e = span.enter();
            let mut files_seen = 0usize;
            let mut files_sent = 0usize;

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
                if files_tx
                    .send(FileWork {
                        path,
                        language: lang,
                    })
                    .is_err()
                {
                    warn!(target: "grafy.pipeline", "pass1 receiver dropped early");
                    break;
                }
                files_sent += 1;
            }
            info!(target: "grafy.pipeline", files_seen, files_sent, "walk complete");
            drop(files_tx);
        }

        // Join pass 1 and 2.
        if let Err(e) = p1_handle.join() {
            warn!(target: "grafy.pipeline", "pass1 thread panicked: {:?}", e);
        }
        if let Err(e) = p2_handle.join() {
            warn!(target: "grafy.pipeline", "pass2 thread panicked: {:?}", e);
        }

        // ----------------------------------------------------------------
        // Option-a barrier: drain the definitions channel (now closed since
        // pass2 dropped definitions_tx), build the shared SymbolTable, then
        // run pass3 and pass4 in parallel sharing &SymbolTable.
        // ----------------------------------------------------------------
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

        // Run pass3 and pass4 in parallel scoped threads sharing &SymbolTable.
        std::thread::scope(|s| {
            let p3 = s.spawn(|| {
                pass3::run_with_table(root_ref, files_ref, sym_ref, write_tx_p3);
            });
            let p4 = s.spawn(|| {
                pass4::run_with_table(root_ref, files_ref, sym_ref, write_tx_p4);
            });
            if let Err(e) = p3.join() {
                warn!(target: "grafy.pipeline", "pass3 thread panicked: {:?}", e);
            }
            if let Err(e) = p4.join() {
                warn!(target: "grafy.pipeline", "pass4 thread panicked: {:?}", e);
            }
        });

        // All senders dropped → writer thread drains and exits.
        let stats = match writer_handle.join() {
            Ok(s) => s,
            Err(e) => {
                warn!(target: "grafy.pipeline", "writer thread panicked: {:?}", e);
                WriterStats::default()
            }
        };

        // Re-open the store (read-only) to tally nodes by kind.
        let report = count_nodes_from_store(&self.root)?;

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
            writer_nodes = stats.nodes_written,
            writer_edges = stats.edges_written,
            "index complete"
        );

        Ok(report)
    }
}

/// Read the `nodes` and `edges` tables from the store and tally counts.
fn count_nodes_from_store(root: &Path) -> anyhow::Result<IndexReport> {
    use crate::store::{NodeRecord, EDGES_TABLE, NODES_TABLE};
    use postcard::from_bytes;

    let store = Store::open(root)?;
    let db = store.read_db();
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
    for item in edges_tbl.iter()? {
        let (key, _val) = item?;
        let (_from, _to, kind) = key.value();
        if kind == EdgeKind::Calls as u8 {
            calls += 1;
        }
    }

    // Route node count (NodeKind::Route = 8).
    let routes = *counts.get(&(NodeKind::Route as u8)).unwrap_or(&0);

    Ok(IndexReport::from_counts(&counts, calls, routes))
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
