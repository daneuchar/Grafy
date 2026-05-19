//! Grafy parser pool.
//!
//! Thread-local tree-sitter parsers, one per language per thread.
//! Public API never returns `tree_sitter::Node<'_>` — that lifetime
//! must not cross the crate boundary or thread boundaries.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use thiserror::Error;
use tracing::{debug, warn};
use tree_sitter::{Parser, Tree};

/// Per-file wall-clock cap. Plan §M0 backstop against malicious/slow files.
pub const PER_FILE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("{path}: unsupported language `{lang}`. Add a grammar in grafy-parser/Cargo.toml or open an issue.")]
    UnsupportedLanguage { path: String, lang: String },

    #[error("{path}: tree-sitter parser returned no tree. File may be empty or contain invalid UTF-8 — re-encode to UTF-8 and retry.")]
    NoTree { path: String },

    #[error("{path}: parse exceeded {timeout:?}. File too large or grammar pathology — split the file or open an issue with a minimal repro.")]
    Timeout { path: String, timeout: Duration },
}

/// Language identifiers we accept. Plan §1 lists 12 for v1.0; M0 ships Rust + Python.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
}

impl Language {
    fn ts_language(self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::language(),
            Self::Python => tree_sitter_python::language(),
        }
    }

    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            _ => None,
        }
    }
}

thread_local! {
    static RUST_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Rust));
    static PY_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Python));
}

fn make_parser(lang: Language) -> Parser {
    let mut p = Parser::new();
    p.set_language(&lang.ts_language())
        .expect("static grammar load");
    p
}

/// Parse `source` for `lang`. Owns the resulting `Tree` so no `Node<'a>` leaks.
///
/// Enforces [`PER_FILE_TIMEOUT`] by wall clock around the parse call.
pub fn parse(path: &str, lang: Language, source: &[u8]) -> Result<Tree, ParseError> {
    let started = Instant::now();
    let tree = match lang {
        Language::Rust => RUST_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Python => PY_PARSER.with(|p| p.borrow_mut().parse(source, None)),
    };
    let elapsed = started.elapsed();

    if elapsed > PER_FILE_TIMEOUT {
        warn!(target: "grafy.parser", %path, ?elapsed, "parse exceeded per-file timeout");
        return Err(ParseError::Timeout {
            path: path.to_string(),
            timeout: PER_FILE_TIMEOUT,
        });
    }

    debug!(target: "grafy.parser", %path, ?elapsed, "parsed");
    tree.ok_or(ParseError::NoTree {
        path: path.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_smoke() {
        let src = b"fn main() {}";
        let tree = parse("smoke.rs", Language::Rust, src).expect("parse");
        assert!(tree.root_node().kind() == "source_file");
    }

    #[test]
    fn python_smoke() {
        let src = b"def f():\n    return 1\n";
        let tree = parse("smoke.py", Language::Python, src).expect("parse");
        assert!(tree.root_node().kind() == "module");
    }

    #[test]
    fn ext_routing() {
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("zig"), None);
    }
}
