//! redb-backed graph store. Plan §4 M1.
//!
//! Single-writer thread fed by `crossbeam-channel`.
//! Producers send `WriteEvent`; this module owns the only redb writer.
//!
//! Schema:
//!   `files`  key=String(rel-path)      value=postcard(FileRecord)
//!   `nodes`  key=u64(NodeId)           value=postcard(NodeRecord)
//!   `edges`  key=(u64,u64,u8)          value=[] (set semantics)

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, info_span, warn};

use crate::pipeline::channels::WriteEvent;

// ---------------------------------------------------------------------------
// Table definitions
// ---------------------------------------------------------------------------

pub const FILES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("files");
pub const NODES_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("nodes");
pub const EDGES_TABLE: TableDefinition<(u64, u64, u8), &[u8]> = TableDefinition::new("edges");

// ---------------------------------------------------------------------------
// On-disk record types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub blake3: [u8; 32],
    pub mtime_ns: u64,
    pub lang: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeKind {
    File = 0,
    Module = 1,
    Function = 2,
    Class = 3,
    Struct = 4,
    Enum = 5,
    Trait = 6,
    Method = 7,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub fqn: String,
    pub kind: NodeKind,
    pub file: String,
    pub byte_start: u32,
    pub byte_end: u32,
}

// ---------------------------------------------------------------------------
// NodeId — deterministic, stable across runs
// ---------------------------------------------------------------------------

/// Derive a `NodeId` from `blake3(file + "\0" + fqn + "\0" + kind_byte + byte_start_le)`.
/// First 8 bytes → u64 little-endian. Stable across runs for the same content.
pub fn node_id(file: &str, fqn: &str, kind: NodeKind, byte_start: u32) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(file.as_bytes());
    hasher.update(b"\x00");
    hasher.update(fqn.as_bytes());
    hasher.update(b"\x00");
    hasher.update(&[kind as u8]);
    hasher.update(&byte_start.to_le_bytes());
    let hash = hasher.finalize();
    u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())
}

// ---------------------------------------------------------------------------
// Store handle
// ---------------------------------------------------------------------------

pub struct Store {
    db: Database,
}

impl Store {
    /// Open (or create) the store at `<root>/.grafy/index.redb`.
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        let dir = root.join(".grafy");
        std::fs::create_dir_all(&dir).map_err(|e| {
            anyhow::anyhow!(
                "{}: failed to create .grafy/ directory — check write permissions. ({})",
                dir.display(),
                e
            )
        })?;
        let db_path = dir.join("index.redb");
        let db = Database::create(&db_path).map_err(|e| {
            anyhow::anyhow!(
                "{}: failed to open redb store — delete .grafy/index.redb and retry. ({})",
                db_path.display(),
                e
            )
        })?;

        // Ensure tables exist.
        {
            let tx = db.begin_write()?;
            tx.open_table(FILES_TABLE)?;
            tx.open_table(NODES_TABLE)?;
            tx.open_table(EDGES_TABLE)?;
            tx.commit()?;
        }

        Ok(Self { db })
    }

    /// Spawn the single writer thread. Drains `rx`, batching writes into
    /// redb transactions (256 events or 50 ms, whichever comes first).
    /// Returns the join handle; drop the sender side to signal shutdown.
    pub fn writer(self, rx: Receiver<WriteEvent>) -> std::thread::JoinHandle<WriterStats> {
        std::thread::spawn(move || {
            let span = info_span!("store.write");
            let _e = span.enter();

            let mut stats = WriterStats::default();
            let mut pending: Vec<WriteEvent> = Vec::with_capacity(256);
            let batch_deadline = Duration::from_millis(50);

            loop {
                // Block until the first event (or channel closed).
                let first = match rx.recv_timeout(batch_deadline) {
                    Ok(ev) => ev,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if !pending.is_empty() {
                            flush(&self.db, &mut pending, &mut stats);
                        }
                        continue;
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        // Channel closed — flush remaining and exit.
                        if !pending.is_empty() {
                            flush(&self.db, &mut pending, &mut stats);
                        }
                        break;
                    }
                };

                pending.push(first);

                // Drain as many as are immediately available (up to batch size).
                let batch_start = Instant::now();
                loop {
                    if pending.len() >= 256 || batch_start.elapsed() >= batch_deadline {
                        break;
                    }
                    match rx.try_recv() {
                        Ok(ev) => pending.push(ev),
                        Err(_) => break,
                    }
                }

                if pending.len() >= 256 || batch_start.elapsed() >= batch_deadline {
                    flush(&self.db, &mut pending, &mut stats);
                }
            }

            // Final flush.
            if !pending.is_empty() {
                flush(&self.db, &mut pending, &mut stats);
            }

            info!(
                target: "grafy.store",
                transactions = stats.transactions,
                nodes_written = stats.nodes_written,
                files_written = stats.files_written,
                edges_written = stats.edges_written,
                "writer finished"
            );
            stats
        })
    }

    /// Read-only access for tests and diagnostics. Not used by pipeline.
    pub fn read_db(&self) -> &Database {
        &self.db
    }
}

// ---------------------------------------------------------------------------
// Flush helper — inside the writer thread
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct WriterStats {
    pub transactions: u64,
    pub files_written: u64,
    pub nodes_written: u64,
    pub edges_written: u64,
}

// Track unique NodeId counter across writer thread lifecycle.
static WRITER_NODE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn flush(db: &Database, pending: &mut Vec<WriteEvent>, stats: &mut WriterStats) {
    if pending.is_empty() {
        return;
    }
    let tx = match db.begin_write() {
        Ok(t) => t,
        Err(e) => {
            warn!(target: "grafy.store", error = %e, "begin_write failed — dropping batch");
            pending.clear();
            return;
        }
    };

    {
        let mut files_tbl = match tx.open_table(FILES_TABLE) {
            Ok(t) => t,
            Err(e) => {
                warn!(target: "grafy.store", error = %e, "open files table failed");
                pending.clear();
                return;
            }
        };
        let mut nodes_tbl = match tx.open_table(NODES_TABLE) {
            Ok(t) => t,
            Err(e) => {
                warn!(target: "grafy.store", error = %e, "open nodes table failed");
                pending.clear();
                return;
            }
        };
        let mut edges_tbl = match tx.open_table(EDGES_TABLE) {
            Ok(t) => t,
            Err(e) => {
                warn!(target: "grafy.store", error = %e, "open edges table failed");
                pending.clear();
                return;
            }
        };

        for ev in pending.drain(..) {
            match ev {
                WriteEvent::File(fr) => {
                    let encoded = match postcard::to_allocvec(&fr.record) {
                        Ok(b) => b,
                        Err(e) => {
                            warn!(target: "grafy.store", error = %e, "postcard encode FileRecord");
                            continue;
                        }
                    };
                    if let Err(e) = files_tbl.insert(fr.rel_path.as_str(), encoded.as_slice()) {
                        warn!(target: "grafy.store", error = %e, "insert file record");
                    } else {
                        stats.files_written += 1;
                    }
                }
                WriteEvent::Node(nr) => {
                    let encoded = match postcard::to_allocvec(&nr.record) {
                        Ok(b) => b,
                        Err(e) => {
                            warn!(target: "grafy.store", error = %e, "postcard encode NodeRecord");
                            continue;
                        }
                    };
                    if let Err(e) = nodes_tbl.insert(nr.id, encoded.as_slice()) {
                        warn!(target: "grafy.store", error = %e, "insert node record");
                    } else {
                        stats.nodes_written += 1;
                        WRITER_NODE_COUNTER.fetch_add(1, Ordering::Relaxed);
                        debug!(target: "grafy.store", id = nr.id, fqn = %nr.record.fqn, "node written");
                    }
                }
                WriteEvent::Edge(e) => {
                    if let Err(err) = edges_tbl.insert((e.from, e.to, e.kind), [].as_slice()) {
                        warn!(target: "grafy.store", error = %err, "insert edge");
                    } else {
                        stats.edges_written += 1;
                    }
                }
                // Pass 2 and later phases — not stored this week.
                WriteEvent::Structure(_)
                | WriteEvent::Definition(_)
                | WriteEvent::Call(_)
                | WriteEvent::Route(_) => {}
            }
        }
    }

    if let Err(e) = tx.commit() {
        warn!(target: "grafy.store", error = %e, "commit failed");
        return;
    }
    stats.transactions += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writer_thread_processes_events_and_exits() {
        use crate::pipeline::channels::{FileWriteEvent, NodeWriteEvent};
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).expect("Store::open");

        let (write_tx, write_rx) = crossbeam_channel::bounded(64);

        // Send a file record and a node record, then close the channel.
        write_tx
            .send(WriteEvent::File(FileWriteEvent {
                rel_path: "foo.rs".into(),
                record: FileRecord {
                    blake3: [0u8; 32],
                    mtime_ns: 12345,
                    lang: "rust".into(),
                },
            }))
            .unwrap();
        write_tx
            .send(WriteEvent::Node(NodeWriteEvent {
                id: 42,
                record: NodeRecord {
                    fqn: "foo::bar".into(),
                    kind: NodeKind::Function,
                    file: "foo.rs".into(),
                    byte_start: 0,
                    byte_end: 10,
                },
            }))
            .unwrap();
        drop(write_tx);

        let stats = store.writer(write_rx).join().expect("writer join");
        assert_eq!(stats.files_written, 1);
        assert_eq!(stats.nodes_written, 1);
    }
}
