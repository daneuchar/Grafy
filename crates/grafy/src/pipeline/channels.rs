//! Pipeline phase channels (M1 W1 skeleton — plan §4).
//!
//! Producer → phase consumers → single redb writer. crossbeam-channel
//! everywhere. Each phase consumes the previous phase's events and
//! emits its own. The writer thread is the single point of redb
//! contact (plan §4 M1 storage discipline).
//!
//! Semantics for each phase land in W2–W4. This module is plumbing.

use std::path::PathBuf;

use crossbeam_channel::{bounded, Receiver, Sender};

/// Channel buffer size between phases. Backpressure is intentional —
/// faster phases must wait for slower ones to drain.
pub const CHANNEL_BUFFER: usize = 1024;

/// One unit of file-level work entering pass 1.
#[derive(Debug, Clone)]
pub struct FileWork {
    pub path: PathBuf,
    pub language: grafy_parser::Language,
}

/// Pass 1 → pass 2 (structure → definitions) hand-off.
#[derive(Debug, Clone)]
pub struct StructureEvent {
    pub file_path: PathBuf,
    pub kind: StructureKind,
    pub name: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    Module,
    Function,
    Class,
    Struct,
    Enum,
    Trait,
    Method,
}

/// Pass 2 → pass 3 (definitions → calls). A flattened symbol record
/// the call resolver consumes.
#[derive(Debug, Clone)]
pub struct DefinitionEvent {
    pub file_path: PathBuf,
    pub fqn: String,
    pub kind: StructureKind,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Pass 3 → pass 4 (calls → routes). Routes need to know about
/// resolved call edges to link a handler function to its route.
#[derive(Debug, Clone)]
pub struct CallEdge {
    pub caller_fqn: String,
    pub callee_fqn: String,
}

/// Pass 4 emits routes. Terminal phase before the writer.
#[derive(Debug, Clone)]
pub struct RouteEdge {
    pub method: String,
    pub pattern: String,
    pub handler_fqn: String,
}

/// Anything that lands in redb. The writer thread is the only consumer.
#[derive(Debug, Clone)]
pub enum WriteEvent {
    Structure(StructureEvent),
    Definition(DefinitionEvent),
    Call(CallEdge),
    Route(RouteEdge),
}

/// Channel set for the full pipeline. Each pair is `(tx, rx)`; consumers
/// move `rx`, producers clone `tx` per worker thread.
pub struct PhaseChannels {
    pub files: (Sender<FileWork>, Receiver<FileWork>),
    pub structure: (Sender<StructureEvent>, Receiver<StructureEvent>),
    pub definitions: (Sender<DefinitionEvent>, Receiver<DefinitionEvent>),
    pub calls: (Sender<CallEdge>, Receiver<CallEdge>),
    pub routes: (Sender<RouteEdge>, Receiver<RouteEdge>),
    /// Drains every phase's output into the single-writer thread.
    pub writes: (Sender<WriteEvent>, Receiver<WriteEvent>),
}

impl PhaseChannels {
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: bounded(CHANNEL_BUFFER),
            structure: bounded(CHANNEL_BUFFER),
            definitions: bounded(CHANNEL_BUFFER),
            calls: bounded(CHANNEL_BUFFER),
            routes: bounded(CHANNEL_BUFFER),
            writes: bounded(CHANNEL_BUFFER),
        }
    }
}

impl Default for PhaseChannels {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_round_trip() {
        let ch = PhaseChannels::new();
        ch.files
            .0
            .send(FileWork {
                path: "x.rs".into(),
                language: grafy_parser::Language::Rust,
            })
            .unwrap();
        let got = ch.files.1.recv().unwrap();
        assert_eq!(got.path.to_string_lossy(), "x.rs");
    }

    #[test]
    fn write_event_carries_all_phases() {
        let ch = PhaseChannels::new();
        ch.writes
            .0
            .send(WriteEvent::Call(CallEdge {
                caller_fqn: "a::b".into(),
                callee_fqn: "c::d".into(),
            }))
            .unwrap();
        match ch.writes.1.recv().unwrap() {
            WriteEvent::Call(e) => {
                assert_eq!(e.caller_fqn, "a::b");
                assert_eq!(e.callee_fqn, "c::d");
            }
            _ => panic!("wrong variant"),
        }
    }
}
