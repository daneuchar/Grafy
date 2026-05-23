//! Pass 3 — heuristic call resolver (M1 W3 + W6.5).
//!
//! Strategy (spec: `docs/m1-call-resolver.md`):
//!
//! **Phase 1 (index):** Drain the entire `DefinitionEvent` stream into memory,
//! building:
//!   - `fqn_to_id: HashMap<String, NodeId>` keyed by FQN
//!   - `short_name_index: HashMap<String, Vec<NodeId>>` keyed by the last
//!     segment of each FQN (for import resolution)
//!   - `file_defs: HashMap<PathBuf, Vec<(String, u64)>>` per-file (fqn, id) pairs
//!   - `defs_by_range: HashMap<PathBuf, Vec<(u32, u32, u64)>>` per-file sorted
//!     list of `(byte_start, byte_end, node_id)` — used for the W6.5
//!     enclosing-function lookup.
//!
//! **Phase 2 (resolve):** For every file, run `calls.scm` and `imports.scm`
//! using the parse tree from the shared `ParseCache` (populated by pass 1,
//! eliminating the W3 re-read + re-parse). Attribute each call site to its
//! enclosing function/method via a parent-node walk (W6.5 fix for the W3
//! fan-out overshoot).
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
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};
use rayon::prelude::*;
use tracing::{debug, info_span, warn};
use tree_sitter::{Query, QueryCursor};

use grafy_parser::{Language, PER_FILE_TIMEOUT};

use crate::lang::{calls_scm, imports_scm};
use crate::pipeline::cache::ParseCache;
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
    /// File path → sorted list of `(byte_start, byte_end, node_id)` for
    /// the W6.5 enclosing-function lookup (parent-node walk).
    /// Sorted ascending by `byte_start` so we can scan to find the tightest
    /// enclosing definition for any given call-site byte offset.
    pub defs_by_range: HashMap<PathBuf, Vec<(u32, u32, u64)>>,
}

impl SymbolTable {
    fn build(events: &[DefinitionEvent], root: &Path) -> Self {
        let mut fqn_to_id: HashMap<String, u64> = HashMap::with_capacity(events.len());
        let mut short_name_index: HashMap<String, Vec<u64>> = HashMap::with_capacity(events.len());
        let mut file_defs: HashMap<PathBuf, Vec<(String, u64)>> = HashMap::new();
        let mut defs_by_range: HashMap<PathBuf, Vec<(u32, u32, u64)>> = HashMap::new();

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

            defs_by_range
                .entry(ev.file_path.clone())
                .or_default()
                .push((ev.byte_start as u32, ev.byte_end as u32, id));
        }

        // Sort each file's range list by byte_start for deterministic lookup.
        for ranges in defs_by_range.values_mut() {
            ranges.sort_unstable_by_key(|(start, _, _)| *start);
        }

        Self {
            fqn_to_id,
            short_name_index,
            file_defs,
            defs_by_range,
        }
    }

    /// Find the tightest enclosing definition for a call-site byte offset.
    ///
    /// Returns the NodeId of the smallest definition range that contains
    /// `call_byte_start`, i.e. the innermost function/method the call lives in.
    /// Returns `None` if no definition contains the offset (module-level call).
    pub fn enclosing_def(&self, file_path: &Path, call_byte_start: u32) -> Option<u64> {
        let ranges = self.defs_by_range.get(file_path)?;

        // Find the tightest (smallest) range that contains call_byte_start.
        // "Tightest" = smallest (byte_end - byte_start) among all candidates.
        let mut best: Option<(u32, u64)> = None; // (span_size, node_id)
        for &(start, end, id) in ranges {
            if start <= call_byte_start && call_byte_start < end {
                let span = end - start;
                if best.is_none() || span < best.unwrap().0 {
                    best = Some((span, id));
                }
            }
        }
        best.map(|(_, id)| id)
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
// Per-language enclosing-definition node kinds (W6.5)
// ---------------------------------------------------------------------------

/// Node kinds that represent function/method bodies for each language.
///
/// When walking up the tree from a call-site node, the first ancestor whose
/// `kind()` is in this set is the "enclosing definition." If no such ancestor
/// exists, the call is at module level and should be attributed to the
/// enclosing File/Module node.
#[must_use]
pub fn enclosing_def_kinds(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["function_item", "closure_expression"],
        Language::Python => &["function_definition", "lambda"],
        Language::JavaScript | Language::Tsx => &[
            "function_declaration",
            "function_expression",
            "method_definition",
            "arrow_function",
            "function",
        ],
        Language::TypeScript => &[
            "function_declaration",
            "function_expression",
            "method_definition",
            "arrow_function",
            "function",
        ],
        Language::Go => &["function_declaration", "method_declaration", "func_literal"],
        Language::Java => &[
            "method_declaration",
            "constructor_declaration",
            "lambda_expression",
        ],
        Language::Cpp => &[
            "function_definition",
            "function_declarator",
            "lambda_expression",
        ],
        Language::CSharp => &[
            "method_declaration",
            "constructor_declaration",
            "lambda_expression",
        ],
        Language::Php => &[
            "function_definition",
            "method_declaration",
            "anonymous_function_creation_expression",
            "arrow_function",
        ],
        Language::Scala => &["function_definition", "function_declaration"],
        Language::Lua => &["function_declaration", "function_definition"],
    }
}

// ---------------------------------------------------------------------------
// Call-site and import extraction via tree-sitter queries
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct CallSite {
    name: String,
    receiver: Option<String>,
    /// Byte offset of the call-site node. Used for the enclosing-def lookup.
    byte_start: u32,
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
        let mut call_byte_start: u32 = 0;

        for cap in m.captures {
            let cap_name = capture_names[cap.index as usize].as_str();
            if cap_name.starts_with('_') {
                continue; // internal predicate anchors
            }
            let text = &source_str[cap.node.start_byte()..cap.node.end_byte()];
            match cap_name {
                "call.name" => {
                    call_byte_start = cap.node.start_byte() as u32;
                    call_name = Some(text.to_owned());
                }
                "call.receiver" => call_receiver = Some(text.to_owned()),
                _ => {}
            }
        }

        if let Some(name) = call_name {
            sites.push(CallSite {
                name,
                receiver: call_receiver,
                byte_start: call_byte_start,
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
    cache: &ParseCache,
) -> Vec<EdgeWriteEvent> {
    let started = Instant::now();

    // --- Obtain bytes + tree: prefer cache, fall back to fresh read + parse. ---
    let cached = cache.get(file_path).map(|r| Arc::clone(&*r));

    let (bytes_owned, tree_owned);
    let (bytes, tree): (&[u8], &tree_sitter::Tree) = if let Some(ref pf) = cached {
        // Cache hit: use the pre-parsed tree and source bytes.
        (&pf.source, &pf.tree)
    } else {
        // Cache miss (budget exhaustion or file not in this pass's work set).
        // Fall back to read + parse.
        bytes_owned = match std::fs::read(file_path) {
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
        tree_owned = match grafy_parser::parse(&file_path.display().to_string(), lang, &bytes_owned)
        {
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
        (&bytes_owned, &tree_owned)
    };

    let source_str = match std::str::from_utf8(bytes) {
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

    if started.elapsed() > PER_FILE_TIMEOUT {
        warn!(
            target: "grafy.pass3",
            file = %file_path.display(),
            language = lang.as_str(),
            "per-file timeout exceeded before query extraction — skipping call edges. Split the file or open an issue."
        );
        return vec![];
    }

    let call_sites = extract_call_sites(bytes, source_str, tree, lang);
    let imports = extract_imports(bytes, source_str, tree, lang);

    if started.elapsed() > PER_FILE_TIMEOUT {
        warn!(
            target: "grafy.pass3",
            file = %file_path.display(),
            language = lang.as_str(),
            "per-file timeout exceeded during query extraction — skipping call edges. Split the file or open an issue."
        );
        return vec![];
    }

    // W6.5: Fallback caller (module/file node) used when a call site has no
    // enclosing function definition (i.e. it is at module/top level).
    let rel_path = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .into_owned();
    let file_node_id = node_id(&rel_path, &rel_path, NodeKind::File, 0);

    let mut edges: Vec<EdgeWriteEvent> = Vec::new();
    // Dedup to avoid duplicate edges from the same (caller, callee) pair.
    let mut seen: HashSet<(u64, u64)> = HashSet::new();

    for site in &call_sites {
        let callees = resolve_call(site, file_path, lang, &imports, sym);
        if callees.is_empty() {
            continue;
        }

        // W6.5 enclosing-function attribution: find the tightest definition
        // range that contains this call site. Fall back to file node.
        let caller = sym
            .enclosing_def(file_path, site.byte_start)
            .unwrap_or(file_node_id);

        for callee in callees {
            if caller != callee && seen.insert((caller, callee)) {
                edges.push(EdgeWriteEvent {
                    from: caller,
                    to: callee,
                    kind: EdgeKind::Calls as u8,
                });
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
/// `cache` provides pre-parsed trees from pass 1 so pass 3 avoids re-reading
/// and re-parsing each file (W6.5 fix).
pub fn run_with_table(
    root: &Path,
    files: &[(PathBuf, Language)],
    sym: &SymbolTable,
    write_tx: Sender<WriteEvent>,
    cache: &ParseCache,
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
        .flat_map(|(path, lang)| resolve_file(path, root, *lang, sym, cache))
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

    // No cache available in standalone mode — each file will fall back to
    // fresh read + parse (same as the original W3 implementation).
    let empty_cache = ParseCache::new();
    run_with_table(root, &files, &sym, write_tx, &empty_cache);
}
