//! SCIP ingest sidecar. Plan §4 M2 W2.
//!
//! Grafy auto-detects installed SCIP indexers on `PATH` (one per language),
//! shells out per indexed repo, and ingests the resulting `.scip` protobuf
//! into the redb store as `EdgeKind::Scip` reference edges.
//!
//! M1's heuristic resolver still runs on every index; SCIP edges are
//! **additive** (they augment, not replace, `EdgeKind::Calls`). A language
//! with no installed SCIP indexer simply produces no SCIP edges — the
//! pipeline is otherwise unchanged.
//!
//! Set `GRAFY_SCIP_DISABLE=1` to skip ingest entirely.

pub mod detect;
pub mod ingest;
pub mod runner;

pub use detect::{detected_indexers, IndexerInfo};
pub use ingest::{ingest_scip_file, IngestReport};
pub use runner::run_indexer;
