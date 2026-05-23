//! Pass 3 — heuristic call resolver (M1 W3).
//!
//! Strategy (spec: `docs/m1-call-resolver.md`):
//!
//! **Phase 1 (index):** Drain the entire `DefinitionEvent` stream into memory,
//! building:
//!   - `fqn_to_id: HashMap<String, NodeId>` keyed by FQN
//!   - `short_name_index: HashMap<String, Vec<NodeId>>` keyed by the last
//!     segment of each FQN (for import resolution)
//!   - `file_defs: HashMap<PathBuf, Vec<(String, u64)>>` per-file (fqn, id) pairs
//!
//! **Phase 2 (resolve):** For every file, reparse it (W3 cost; W6 will cache),
//! run `calls.scm` and `imports.scm`, resolve each call site, emit edges.
//!
//! **Orchestration choice (option a):** Drain `definitions_rx` to a `Vec`
//! first (channel closes when pass 2 drops `definitions_tx`), build the
//! symbol table, then resolve in parallel via rayon, finally flush edges
//! through `write_tx`. The barrier is the channel-close detection in
//! `Receiver::into_iter()`.
//!
//! **Language families (spec §per-language-family-strategy):**
//! - Family A (dynamic/lexical): Python, TypeScript, JavaScript, TSX, PHP, Lua.
//!   Resolution order: module scope → import scope. First match wins.
//! - Family B (lexical with explicit types): Rust, Go, Scala, Java, C#, C++.
//!   Resolution order: module scope → import/use scope → receiver type heuristic
//!   → trait/interface dispatch (documented overshoot).
//!
//! **Unresolved calls** → no edge, logged via `tracing::debug!` (common case).
//! **Timeout** → if per-file resolution exceeds `PER_FILE_TIMEOUT`, abort that
//! file's edges and log `tracing::warn!` with file + lang + next-step action.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};
use rayon::prelude::*;
use tracing::{debug, info_span, warn};
use tree_sitter::{Query, QueryCursor};

use grafy_parser::{Language, PER_FILE_TIMEOUT};

use crate::lang::{calls_scm, imports_scm};
use crate::pipeline::channels::{DefinitionEvent, EdgeWriteEvent, WriteEvent};
use crate::store::{node_id, EdgeKind, NodeKind};

// ---------------------------------------------------------------------------
// Language family classification
// ---------------------------------------------------------------------------

/// Two-family classification for the heuristic resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangFamily {
    /// Dynamic / lexical languages: Python, TypeScript, JavaScript, TSX, PHP, Lua.
    A,
    /// Lexical with explicit types: Rust, Go, Scala, Java, C#, C++.
    B,
}

/// Classify `lang` into a resolution family.
#[must_use]
pub fn lang_family(lang: Language) -> LangFamily {
    match lang {
        Language::Python
        | Language::TypeScript
        | Language::JavaScript
        | Language::Tsx
        | Language::Php
        | Language::Lua => LangFamily::A,
        Language::Rust
        | Language::Go
        | Language::Scala
        | Language::Java
        | Language::CSharp
        | Language::Cpp => LangFamily::B,
    }
}

// ---------------------------------------------------------------------------
// In-memory symbol table
// ---------------------------------------------------------------------------

/// Built once from the full `DefinitionEvent` stream before resolution begins.
/// Made `pub` so pass4 can borrow a shared reference via the pipeline.
pub struct SymbolTable {
    /// FQN → NodeId.
    pub fqn_to_id: HashMap<String, u64>,
    /// Short name (last segment) → list of NodeIds. Multiple FQNs may share a
    /// short name; all are stored for trait/interface dispatch (Family B).
    pub short_name_index: HashMap<String, Vec<u64>>,
    /// File path → list of (fqn, node_id) definitions in that file.
    pub file_defs: HashMap<PathBuf, Vec<(String, u64)>>,
}

impl SymbolTable {
    fn build(events: &[DefinitionEvent], root: &Path) -> Self {
        let mut fqn_to_id: HashMap<String, u64> = HashMap::with_capacity(events.len());
        let mut short_name_index: HashMap<String, Vec<u64>> = HashMap::with_capacity(events.len());
        let mut file_defs: HashMap<PathBuf, Vec<(String, u64)>> = HashMap::new();

        for ev in events {
            let rel_path = ev
                .file_path
                .strip_prefix(root)
                .unwrap_or(&ev.file_path)
                .to_string_lossy()
                .into_owned();

            let kind = structure_kind_to_node_kind(ev.kind);
            let id = node_id(&rel_path, &ev.fqn, kind, ev.byte_start as u32);

            fqn_to_id.insert(ev.fqn.clone(), id);

            let short = last_segment(&ev.fqn).to_owned();
            short_name_index.entry(short).or_default().push(id);

            file_defs
                .entry(ev.file_path.clone())
                .or_default()
                .push((ev.fqn.clone(), id));
        }

        Self {
            fqn_to_id,
            short_name_index,
            file_defs,
        }
    }

    /// Resolve by short name: same-file definitions first, then any in the index.
    pub fn resolve_short(&self, short_name: &str, file_path: &Path) -> Option<u64> {
        // Module scope: same file.
        if let Some(defs) = self.file_defs.get(file_path) {
            for (fqn, id) in defs {
                if last_segment(fqn) == short_name {
                    return Some(*id);
                }
            }
        }
        // Short-name index (any file), first match wins.
        self.short_name_index
            .get(short_name)
            .and_then(|v| v.first().copied())
    }

    /// Resolve a fully-qualified name.
    pub fn resolve_fqn(&self, fqn: &str) -> Option<u64> {
        self.fqn_to_id.get(fqn).copied()
    }

    /// All NodeIds with this short name (for trait/interface dispatch).
    fn all_by_short_name(&self, short_name: &str) -> &[u64] {
        self.short_name_index
            .get(short_name)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the last segment of a FQN (supports `::`, `.`, `/`, `\` separators).
pub fn last_segment(fqn: &str) -> &str {
    fqn.rsplit([':', '.', '/', '\\']).next().unwrap_or(fqn)
}

pub fn structure_kind_to_node_kind(k: crate::pipeline::channels::StructureKind) -> NodeKind {
    use crate::pipeline::channels::StructureKind;
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

pub fn ts_language_for(lang: Language) -> tree_sitter::Language {
    match lang {
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Language::Scala => tree_sitter_scala::LANGUAGE.into(),
        Language::Lua => tree_sitter_lua::LANGUAGE.into(),
    }
}

pub fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
        || (s.starts_with('`') && s.ends_with('`'))
    {
        s[1..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Call-site and import extraction via tree-sitter queries
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct CallSite {
    name: String,
    receiver: Option<String>,
}

#[derive(Debug)]
struct ImportBinding {
    /// The local name as it appears in the code.
    name: String,
    /// The source module path (quotes stripped).
    module: String,
}

fn extract_call_sites(
    bytes: &[u8],
    source_str: &str,
    tree: &tree_sitter::Tree,
    lang: Language,
) -> Vec<CallSite> {
    let ts_lang = ts_language_for(lang);
    let query = match Query::new(&ts_lang, calls_scm(lang)) {
        Ok(q) => q,
        Err(e) => {
            debug!(
                target: "grafy.pass3",
                language = lang.as_str(),
                "calls.scm compile error: {e}"
            );
            return vec![];
        }
    };

    let capture_names: Vec<String> = query
        .capture_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut cursor = QueryCursor::new();
    let mut sites = Vec::new();

    for m in cursor.matches(&query, tree.root_node(), bytes) {
        let mut call_name: Option<String> = None;
        let mut call_receiver: Option<String> = None;

        for cap in m.captures {
            let cap_name = capture_names[cap.index as usize].as_str();
            if cap_name.starts_with('_') {
                continue; // internal predicate anchors
            }
            let text = &source_str[cap.node.start_byte()..cap.node.end_byte()];
            match cap_name {
                "call.name" => call_name = Some(text.to_owned()),
                "call.receiver" => call_receiver = Some(text.to_owned()),
                _ => {}
            }
        }

        if let Some(name) = call_name {
            sites.push(CallSite {
                name,
                receiver: call_receiver,
            });
        }
    }

    sites
}

fn extract_imports(
    bytes: &[u8],
    source_str: &str,
    tree: &tree_sitter::Tree,
    lang: Language,
) -> Vec<ImportBinding> {
    let ts_lang = ts_language_for(lang);
    let query = match Query::new(&ts_lang, imports_scm(lang)) {
        Ok(q) => q,
        Err(e) => {
            debug!(
                target: "grafy.pass3",
                language = lang.as_str(),
                "imports.scm compile error: {e}"
            );
            return vec![];
        }
    };

    let capture_names: Vec<String> = query
        .capture_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut cursor = QueryCursor::new();
    let mut bindings = Vec::new();

    for m in cursor.matches(&query, tree.root_node(), bytes) {
        let mut import_name: Option<String> = None;
        let mut import_module: Option<String> = None;

        for cap in m.captures {
            let cap_name = capture_names[cap.index as usize].as_str();
            if cap_name.starts_with('_') {
                continue;
            }
            let text = &source_str[cap.node.start_byte()..cap.node.end_byte()];
            match cap_name {
                "import.name" => import_name = Some(text.to_owned()),
                "import.module" => import_module = Some(text.to_owned()),
                _ => {}
            }
        }

        if let Some(module) = import_module {
            let module = strip_quotes(&module);
            let name = import_name.unwrap_or_else(|| last_segment(&module).to_owned());
            bindings.push(ImportBinding { name, module });
        }
    }

    bindings
}

// ---------------------------------------------------------------------------
// Resolution strategies
// ---------------------------------------------------------------------------

fn resolve_call(
    site: &CallSite,
    file_path: &Path,
    lang: Language,
    imports: &[ImportBinding],
    sym: &SymbolTable,
) -> Vec<u64> {
    match lang_family(lang) {
        LangFamily::A => resolve_family_a(site, file_path, imports, sym),
        LangFamily::B => resolve_family_b(site, file_path, imports, sym),
    }
}

/// Family A: Python, TypeScript, JavaScript, TSX, PHP, Lua.
/// Resolution order: module scope → import scope. First match wins.
fn resolve_family_a(
    site: &CallSite,
    file_path: &Path,
    imports: &[ImportBinding],
    sym: &SymbolTable,
) -> Vec<u64> {
    // Steps 1 + 2: module scope (same-file first, then index-wide short name).
    if let Some(id) = sym.resolve_short(&site.name, file_path) {
        return vec![id];
    }

    // Step 3: import scope.
    for binding in imports {
        if binding.name == site.name {
            // Attempt various FQN constructions.
            for sep in &[".", "/", "::", "\\"] {
                let candidate = format!("{}{}{}", binding.module, sep, site.name);
                if let Some(id) = sym.resolve_fqn(&candidate) {
                    return vec![id];
                }
            }
            // Try module short name + sep + call name.
            let mod_short = last_segment(&binding.module);
            for sep in &[".", "/"] {
                let candidate = format!("{}{}{}", mod_short, sep, site.name);
                if let Some(id) = sym.resolve_fqn(&candidate) {
                    return vec![id];
                }
            }
            // The import may have brought the symbol directly into scope.
            if let Some(id) = sym.resolve_fqn(&site.name) {
                return vec![id];
            }
        }
    }

    debug!(
        target: "grafy.pass3",
        file = %file_path.display(),
        call = %site.name,
        "unresolved call (family A)"
    );
    vec![]
}

/// Family B: Rust, Go, Scala, Java, C#, C++.
/// Resolution order: module scope → import/use scope → receiver heuristic
/// → trait/interface dispatch (documented overshoot).
fn resolve_family_b(
    site: &CallSite,
    file_path: &Path,
    imports: &[ImportBinding],
    sym: &SymbolTable,
) -> Vec<u64> {
    // Step 1: module scope.
    if let Some(id) = sym.resolve_short(&site.name, file_path) {
        return vec![id];
    }

    // Step 2: import/use/using scope.
    for binding in imports {
        if binding.name == site.name {
            for sep in &["::", ".", "/", "\\"] {
                let candidate = format!("{}{}{}", binding.module, sep, site.name);
                if let Some(id) = sym.resolve_fqn(&candidate) {
                    return vec![id];
                }
            }
            if let Some(id) = sym.resolve_fqn(&site.name) {
                return vec![id];
            }
        }
    }

    // Step 3: method receiver type heuristic.
    if let Some(receiver) = &site.receiver {
        let receiver_short = last_segment(receiver);
        for sep in &["::", ".", "/"] {
            let candidate = format!("{}{}{}", receiver_short, sep, site.name);
            if let Some(id) = sym.resolve_fqn(&candidate) {
                return vec![id];
            }
        }
    }

    // Step 4: trait/interface dispatch — all impls/satisfiers in the index
    // that expose this short name. Documented to overshoot (plan §4 W3).
    let all = sym.all_by_short_name(&site.name);
    if !all.is_empty() {
        return all.to_vec();
    }

    debug!(
        target: "grafy.pass3",
        file = %file_path.display(),
        call = %site.name,
        "unresolved call (family B)"
    );
    vec![]
}

// ---------------------------------------------------------------------------
// Per-file resolution driver
// ---------------------------------------------------------------------------

fn resolve_file(
    file_path: &Path,
    root: &Path,
    lang: Language,
    sym: &SymbolTable,
) -> Vec<EdgeWriteEvent> {
    let started = Instant::now();

    let bytes = match std::fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            warn!(
                target: "grafy.pass3",
                file = %file_path.display(),
                language = lang.as_str(),
                "read failed — check file permissions. ({})", e
            );
            return vec![];
        }
    };

    let source_str = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => {
            warn!(
                target: "grafy.pass3",
                file = %file_path.display(),
                language = lang.as_str(),
                "non-UTF-8 source — re-encode to UTF-8 and retry."
            );
            return vec![];
        }
    };

    // Reparse (W3 cost; W6 will cache parse trees).
    let tree = match grafy_parser::parse(&file_path.display().to_string(), lang, &bytes) {
        Ok(t) => t,
        Err(e) => {
            warn!(
                target: "grafy.pass3",
                file = %file_path.display(),
                language = lang.as_str(),
                "reparse failed — verify valid {} source. ({})", lang.as_str(), e
            );
            return vec![];
        }
    };

    if started.elapsed() > PER_FILE_TIMEOUT {
        warn!(
            target: "grafy.pass3",
            file = %file_path.display(),
            language = lang.as_str(),
            "per-file timeout exceeded during reparse — skipping call edges. Split the file or open an issue."
        );
        return vec![];
    }

    let call_sites = extract_call_sites(&bytes, source_str, &tree, lang);
    let imports = extract_imports(&bytes, source_str, &tree, lang);

    if started.elapsed() > PER_FILE_TIMEOUT {
        warn!(
            target: "grafy.pass3",
            file = %file_path.display(),
            language = lang.as_str(),
            "per-file timeout exceeded during query extraction — skipping call edges. Split the file or open an issue."
        );
        return vec![];
    }

    // Caller set: all definitions in this file. W3 heuristic: attribute every
    // call site to every function/method definition in the file. This overshoots
    // when a file has many top-level functions — a known W3 limitation. W6+
    // will use the enclosing-node tree walk to narrow attribution.
    let callers: Vec<u64> = sym
        .file_defs
        .get(file_path)
        .map(|defs| defs.iter().map(|(_, id)| *id).collect())
        .unwrap_or_else(|| {
            let rel_path = file_path
                .strip_prefix(root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .into_owned();
            vec![node_id(&rel_path, &rel_path, NodeKind::File, 0)]
        });

    let mut edges: Vec<EdgeWriteEvent> = Vec::new();
    // Dedup to avoid duplicate edges from the same (caller, callee) pair.
    let mut seen: HashSet<(u64, u64)> = HashSet::new();

    for site in &call_sites {
        let callees = resolve_call(site, file_path, lang, &imports, sym);
        for callee in callees {
            for &caller in &callers {
                if caller != callee && seen.insert((caller, callee)) {
                    edges.push(EdgeWriteEvent {
                        from: caller,
                        to: callee,
                        kind: EdgeKind::Calls as u8,
                    });
                }
            }
        }
    }

    edges
}

// ---------------------------------------------------------------------------
// Public helpers (reused by pass4 via pipeline orchestration)
// ---------------------------------------------------------------------------

/// Build the in-memory symbol table from a pre-drained slice of definition events.
/// Extracted so both pass3 and the pipeline orchestrator can call it without
/// duplicating the drain logic.
pub fn build_symbol_table(events: &[DefinitionEvent], root: &Path) -> SymbolTable {
    SymbolTable::build(events, root)
}

/// Extract the unique `(file, lang)` pairs from a set of definition events.
/// Used by both pass3 and the pipeline orchestrator.
pub fn unique_files(events: &[DefinitionEvent]) -> Vec<(PathBuf, Language)> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    events
        .iter()
        .filter_map(|ev| {
            if seen.insert(ev.file_path.clone()) {
                let lang = ev
                    .file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .and_then(Language::from_extension)?;
                Some((ev.file_path.clone(), lang))
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Run pass 3 using a pre-built symbol table and pre-computed file list.
///
/// Called from the pipeline after draining `definitions_rx` to a `Vec` and
/// building the shared symbol table once (which pass4 also uses).
pub fn run_with_table(
    root: &Path,
    files: &[(PathBuf, Language)],
    sym: &SymbolTable,
    write_tx: Sender<WriteEvent>,
) {
    let span = info_span!("pass3.calls");
    let _e = span.enter();

    tracing::info!(
        target: "grafy.pass3",
        files = files.len(),
        "resolving call edges"
    );

    // Parallel resolution via rayon.
    let all_edges: Vec<EdgeWriteEvent> = files
        .par_iter()
        .flat_map(|(path, lang)| resolve_file(path, root, *lang, sym))
        .collect();

    let edge_count = all_edges.len();

    for ev in all_edges {
        let _ = write_tx.send(WriteEvent::Edge(ev));
    }

    tracing::info!(
        target: "grafy.pass3",
        edges_emitted = edge_count,
        "pass3 complete"
    );
}

/// Run pass 3. Consumes `definitions_rx`, builds the in-memory symbol table
/// (option-a barrier: drain full stream, then resolve in parallel), emits
/// `WriteEvent::Edge` entries via `write_tx`.
///
/// Kept for use in unit tests and contexts where the pipeline doesn't need to
/// share the symbol table with pass4. The pipeline orchestrator uses
/// `run_with_table` directly.
pub fn run(root: &Path, definitions_rx: Receiver<DefinitionEvent>, write_tx: Sender<WriteEvent>) {
    let span = info_span!("pass3.calls.standalone");
    let _e = span.enter();

    let events: Vec<DefinitionEvent> = definitions_rx.into_iter().collect();
    tracing::info!(
        target: "grafy.pass3",
        definitions = events.len(),
        "symbol table built"
    );

    let sym = build_symbol_table(&events, root);
    let files = unique_files(&events);

    run_with_table(root, &files, &sym, write_tx);
}
