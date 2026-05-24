//! Pass 1 — structure.
//!
//! Consumes `FileWork` from the walk phase, parses each file with the
//! language-specific definitions query, and emits:
//!   - `WriteEvent::File`      → store (file metadata)
//!   - `StructureEvent`        → pass 2 (one per captured definition)
//!
//! As a side-effect, inserts `Arc<ParsedFile>` into the shared `ParseCache`
//! so passes 3 and 4 can reuse parse trees without re-reading from disk.
//!
//! All parsing runs on a rayon thread pool. No `Node<'_>` leaves this module.
//! Errors are logged with file + language + next-step action; never panicked.

use std::sync::Arc;
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender};
use rayon::prelude::*;
use tracing::{info_span, warn};
use tree_sitter::QueryCursor;

use grafy_parser::{parse, Language};

use crate::pipeline::cache::{CacheBudget, ParseCache, ParsedFile};
use crate::pipeline::channels::{
    FileWork, FileWriteEvent, NodeWriteEvent, StructureEvent, StructureKind, WriteEvent,
};
use crate::store::{node_id, FileRecord, NodeKind, NodeRecord};

/// Map a tree-sitter capture name (the `@foo.def` part) to `StructureKind`.
/// Returns `None` for name-only captures (e.g. `@function.name`) and unknown
/// capture kinds — callers must ignore those.
fn capture_to_kind(name: &str) -> Option<StructureKind> {
    match name {
        "function.def" | "arrow.def" => Some(StructureKind::Function),
        "class.def" | "interface.def" | "object.def" => Some(StructureKind::Class),
        "struct.def" => Some(StructureKind::Struct),
        "enum.def" => Some(StructureKind::Enum),
        "trait.def" => Some(StructureKind::Trait),
        "method.def" | "ctor.def" => Some(StructureKind::Method),
        "mod.def" | "ns.def" => Some(StructureKind::Module),
        "impl.def" => Some(StructureKind::Struct), // impl blocks treated as Struct linkage
        "decorated.def" => Some(StructureKind::Function),
        "type.def" | "signature.def" | "local.def" => Some(StructureKind::Function),
        _ => None,
    }
}

/// Map `StructureKind` → `NodeKind` for storage.
pub fn structure_to_node_kind(k: StructureKind) -> NodeKind {
    match k {
        StructureKind::Module => NodeKind::Module,
        StructureKind::Function => NodeKind::Function,
        StructureKind::Class => NodeKind::Class,
        StructureKind::Struct => NodeKind::Struct,
        StructureKind::Enum => NodeKind::Enum,
        StructureKind::Trait => NodeKind::Trait,
        StructureKind::Method => NodeKind::Method,
    }
}

/// Run pass 1 on the file-work channel.
///
/// `root` is the repository root (used for relative path computation).
/// `files_rx` is the incoming work channel.
/// `structure_tx` feeds pass 2.
/// `write_tx` feeds the store writer thread.
/// `cache` + `budget` are populated here and consumed by pass 3 / pass 4.
pub fn run(
    root: &std::path::Path,
    files_rx: Receiver<FileWork>,
    structure_tx: Sender<StructureEvent>,
    write_tx: Sender<WriteEvent>,
    cache: &ParseCache,
    budget: &CacheBudget,
) {
    let span = info_span!("pass1.structure");
    let _e = span.enter();

    // Drain the channel into a Vec so rayon can parallelise the work.
    let work: Vec<FileWork> = files_rx.into_iter().collect();

    work.into_par_iter().for_each(|fw| {
        process_file(
            root,
            &fw.path,
            fw.language,
            &structure_tx,
            &write_tx,
            cache,
            budget,
        );
    });
}

fn process_file(
    root: &std::path::Path,
    path: &std::path::Path,
    lang: Language,
    structure_tx: &Sender<StructureEvent>,
    write_tx: &Sender<WriteEvent>,
    cache: &ParseCache,
    budget: &CacheBudget,
) {
    // Read bytes.
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            warn!(
                target: "grafy.pass1",
                file = %path.display(),
                language = lang.as_str(),
                "read failed — check file permissions and encoding. ({})",
                e
            );
            return;
        }
    };

    // Wrap bytes in Arc early so we can share ownership with the parse cache
    // without copying.
    let bytes: Arc<[u8]> = bytes.into();

    // Hash + mtime for file record.
    let blake3_hash = {
        let mut h = blake3::Hasher::new();
        h.update(&bytes);
        *h.finalize().as_bytes()
    };
    let mtime_ns = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    // Relative path (key in files table).
    let rel_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();

    // Emit file record to store.
    let _ = write_tx.send(WriteEvent::File(FileWriteEvent {
        rel_path: rel_path.clone(),
        record: FileRecord {
            blake3: blake3_hash,
            mtime_ns,
            lang: lang.as_str().to_owned(),
        },
    }));

    // Also emit a File node for the file itself.
    let file_node_id = node_id(&rel_path, &rel_path, NodeKind::File, 0);
    let _ = write_tx.send(WriteEvent::Node(NodeWriteEvent {
        id: file_node_id,
        record: NodeRecord {
            fqn: rel_path.clone(),
            kind: NodeKind::File,
            file: rel_path.clone(),
            byte_start: 0,
            byte_end: bytes.len() as u32,
        },
    }));

    // Parse.
    let tree = match parse(&path.display().to_string(), lang, &bytes) {
        Ok(t) => t,
        Err(e) => {
            warn!(
                target: "grafy.pass1",
                file = %path.display(),
                language = lang.as_str(),
                "parse failed — verify the file is valid {lang} source. ({e})",
                lang = lang.as_str(),
                e = e,
            );
            return;
        }
    };

    // Insert into parse cache if the budget allows. We clone the tree
    // (a tree_sitter::Tree clone is cheap — it clones an Arc<[Node]>
    // internally) and share the source bytes via Arc<[u8]>.
    if budget.try_reserve(bytes.len() as u64) {
        cache.insert(
            path.to_path_buf(),
            Arc::new(ParsedFile {
                tree: tree.clone(),
                source: Arc::clone(&bytes),
                lang,
            }),
        );
    }

    // Build query (cached per language across the run).
    let query_arc = match crate::pipeline::queries::get(
        lang,
        crate::pipeline::queries::QueryKind::Definitions,
    ) {
        Ok(q) => q,
        Err(e) => {
            warn!(
                target: "grafy.pass1",
                file = %path.display(),
                language = lang.as_str(),
                "query compile failed — report this as a bug in the definitions.scm for {lang}. ({e})",
                lang = lang.as_str(),
                e = e,
            );
            return;
        }
    };
    let query = &*query_arc;

    // Walk matches — extract the `.def` capture for structure kind and the
    // `.name` capture for the symbol text. We rely on the pattern that every
    // `*.def` capture has a sibling `*.name` capture within the same match.
    let capture_names: Vec<String> = query
        .capture_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut cursor = QueryCursor::new();
    let root_node = tree.root_node();

    for m in cursor.matches(query, root_node, &bytes[..]) {
        // Find the .def capture and the .name capture in this match.
        let mut def_kind: Option<StructureKind> = None;
        let mut def_byte_start: usize = 0;
        let mut def_byte_end: usize = 0;
        let mut name_text: Option<String> = None;

        for cap in m.captures {
            let cap_name = capture_names[cap.index as usize].as_str();
            let node = cap.node;

            if cap_name.ends_with(".def") {
                if let Some(kind) = capture_to_kind(cap_name) {
                    def_kind = Some(kind);
                    def_byte_start = node.start_byte();
                    def_byte_end = node.end_byte();
                }
            } else if cap_name.ends_with(".name") || cap_name.ends_with(".table") {
                // Use the first name capture; skip table prefix captures.
                // Lazy: only decode the captured byte range (Tier 2 lever 4).
                if name_text.is_none() && !cap_name.ends_with(".table") {
                    if let Ok(text) =
                        std::str::from_utf8(&bytes[node.start_byte()..node.end_byte()])
                    {
                        name_text = Some(text.to_owned());
                    }
                }
            }
        }

        if let (Some(kind), Some(name)) = (def_kind, name_text) {
            let ev = StructureEvent {
                file_path: path.to_path_buf(),
                kind,
                name,
                byte_start: def_byte_start,
                byte_end: def_byte_end,
            };
            let _ = structure_tx.send(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;
    use tempfile::tempdir;

    #[test]
    fn run_with_thread_channels() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("foo.rs");
        std::fs::write(&path, "fn foo() {}\nstruct Bar;\n").unwrap();

        let (files_tx, files_rx) = bounded(16);
        let (structure_tx, structure_rx) = bounded(1024);
        let (write_tx, _write_rx) = bounded(1024);

        files_tx
            .send(FileWork {
                path: path.clone(),
                language: Language::Rust,
            })
            .unwrap();
        drop(files_tx);

        let root = dir.path().to_path_buf();
        let cache = Arc::new(ParseCache::new());
        let budget = Arc::new(CacheBudget::new());
        let handle = std::thread::spawn(move || {
            run(&root, files_rx, structure_tx, write_tx, &cache, &budget);
        });

        let events: Vec<_> = structure_rx.iter().collect();
        handle.join().expect("pass1 thread panicked");
        assert!(!events.is_empty(), "no structure events from threaded run");
    }

    #[test]
    fn single_rust_file_emits_structure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("foo.rs");
        std::fs::write(&path, "fn foo() {}\nstruct Bar;\n").unwrap();

        let (structure_tx, structure_rx) = bounded(1024);
        let (write_tx, _write_rx) = bounded(1024);
        let cache = ParseCache::new();
        let budget = CacheBudget::new();

        process_file(
            dir.path(),
            &path,
            Language::Rust,
            &structure_tx,
            &write_tx,
            &cache,
            &budget,
        );
        drop(structure_tx);

        let events: Vec<_> = structure_rx.iter().collect();
        assert!(
            !events.is_empty(),
            "expected structure events from Rust source"
        );
        let has_fn = events
            .iter()
            .any(|e| e.kind == StructureKind::Function && e.name == "foo");
        assert!(has_fn, "expected function 'foo', got: {events:?}");
    }

    #[test]
    fn single_python_file_emits_structure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bar.py");
        std::fs::write(
            &path,
            "class Dog:\n    def bark(self): pass\n\ndef free(): pass\n",
        )
        .unwrap();

        let (structure_tx, structure_rx) = bounded(1024);
        let (write_tx, _write_rx) = bounded(1024);
        let cache = ParseCache::new();
        let budget = CacheBudget::new();

        process_file(
            dir.path(),
            &path,
            Language::Python,
            &structure_tx,
            &write_tx,
            &cache,
            &budget,
        );
        drop(structure_tx);

        let events: Vec<_> = structure_rx.iter().collect();
        assert!(
            !events.is_empty(),
            "expected structure events from Python source"
        );
        let has_class = events.iter().any(|e| e.kind == StructureKind::Class);
        assert!(has_class, "expected class node, got: {events:?}");
    }
}
