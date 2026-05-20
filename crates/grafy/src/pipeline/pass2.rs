//! Pass 2 — definitions.
//!
//! Consumes `StructureEvent` from pass 1, resolves FQNs, computes deterministic
//! `NodeId`s, and emits:
//!   - `WriteEvent::Node`       → store writer thread
//!   - `DefinitionEvent`        → pass 3 (W3 call resolver — channel wired now)
//!
//! FQN separator is language-family specific:
//!   Rust / C++              → `::`
//!   Python / Java / C# / Scala / Lua  → `.`
//!   JS / TS / TSX / Go      → `/`
//!   PHP                     → `\\`

use crossbeam_channel::{Receiver, Sender};
use tracing::info_span;

use grafy_parser::Language;

use crate::fqn;
use crate::pipeline::channels::{DefinitionEvent, NodeWriteEvent, StructureEvent, WriteEvent};
use crate::pipeline::pass1::structure_to_node_kind;
use crate::store::{node_id, NodeRecord};

/// Separator between the file-level FQN prefix and the symbol name.
fn sep(lang: Language) -> &'static str {
    match lang {
        Language::Rust | Language::Cpp => "::",
        Language::Python | Language::Java | Language::CSharp | Language::Scala | Language::Lua => {
            "."
        }
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Go => "/",
        Language::Php => "\\",
    }
}

/// Run pass 2 on the structure-event channel.
///
/// `root` is the repository root (used by the FQN module).
/// `structure_rx` is the incoming channel from pass 1.
/// `definitions_tx` feeds pass 3 (W3).
/// `write_tx` feeds the store writer thread.
pub fn run(
    root: &std::path::Path,
    structure_rx: Receiver<StructureEvent>,
    definitions_tx: Sender<DefinitionEvent>,
    write_tx: Sender<WriteEvent>,
) {
    let span = info_span!("pass2.definitions");
    let _e = span.enter();

    for ev in structure_rx {
        process_event(root, ev, &definitions_tx, &write_tx);
    }
}

fn process_event(
    root: &std::path::Path,
    ev: StructureEvent,
    definitions_tx: &Sender<DefinitionEvent>,
    write_tx: &Sender<WriteEvent>,
) {
    // Determine language from the file extension — we trust pass 1 sent
    // only parseable extensions, so silently skip if resolution fails.
    let lang = ev
        .file_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(Language::from_extension);

    let lang = match lang {
        Some(l) => l,
        None => return,
    };

    // Resolve file-level FQN prefix.
    let file_prefix = fqn::for_file(lang, root, &ev.file_path).unwrap_or_default();

    // Build full FQN: prefix + sep + name.
    let fqn = if file_prefix.is_empty() {
        ev.name.clone()
    } else {
        format!("{}{}{}", file_prefix, sep(lang), ev.name)
    };

    // Relative path string (used as `file` field in NodeRecord).
    let rel_path = ev
        .file_path
        .strip_prefix(root)
        .unwrap_or(&ev.file_path)
        .to_string_lossy()
        .into_owned();

    let kind = structure_to_node_kind(ev.kind);
    let id = node_id(&rel_path, &fqn, kind, ev.byte_start as u32);

    // Send to store.
    let _ = write_tx.send(WriteEvent::Node(NodeWriteEvent {
        id,
        record: NodeRecord {
            fqn: fqn.clone(),
            kind,
            file: rel_path,
            byte_start: ev.byte_start as u32,
            byte_end: ev.byte_end as u32,
        },
    }));

    // Forward to pass 3 channel (wired now; W3 will consume).
    let _ = definitions_tx.send(DefinitionEvent {
        file_path: ev.file_path,
        fqn,
        kind: ev.kind,
        byte_start: ev.byte_start,
        byte_end: ev.byte_end,
    });
}
