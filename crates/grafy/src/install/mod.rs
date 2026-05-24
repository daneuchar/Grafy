//! `grafy install` — provision SCIP indexers on macOS + Linux. Plan §4 M2 W2.
//!
//! The installer is idempotent (skips indexers already on PATH), never uses
//! `sudo`, and emits actionable errors when prereqs are missing.

pub mod installer;
pub mod prereqs;
pub mod report;

pub use installer::run_with_scip;
pub use prereqs::{probe, PrereqReport};
pub use report::print_report;
