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
    #[error(
        "{path}: unsupported language `{lang}`. Add a grammar in grafy-parser/Cargo.toml or open an issue."
    )]
    UnsupportedLanguage { path: String, lang: String },

    #[error(
        "{path}: tree-sitter parser returned no tree. File may be empty or contain invalid UTF-8 — re-encode to UTF-8 and retry."
    )]
    NoTree { path: String },

    #[error(
        "{path}: parse exceeded {timeout:?}. File too large or grammar pathology — split the file or open an issue with a minimal repro."
    )]
    Timeout { path: String, timeout: Duration },
}

/// Languages Grafy can parse. Plan §1 lists 12 for v1.0; all ship in M1 W1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Java,
    Cpp,
    CSharp,
    Php,
    Scala,
    Lua,
}

impl Language {
    fn ts_language(self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Self::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Self::Scala => tree_sitter_scala::LANGUAGE.into(),
            Self::Lua => tree_sitter_lua::LANGUAGE.into(),
        }
    }

    /// Stable short name for logging + diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Go => "go",
            Self::Java => "java",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Php => "php",
            Self::Scala => "scala",
            Self::Lua => "lua",
        }
    }

    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "js" | "mjs" | "cjs" | "jsx" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "cc" | "cpp" | "cxx" | "c++" | "h" | "hpp" | "hxx" | "h++" => Some(Self::Cpp),
            "cs" => Some(Self::CSharp),
            "php" | "phtml" => Some(Self::Php),
            "scala" | "sc" => Some(Self::Scala),
            "lua" => Some(Self::Lua),
            _ => None,
        }
    }
}

thread_local! {
    static RUST_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Rust));
    static PY_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Python));
    static JS_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::JavaScript));
    static TS_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::TypeScript));
    static TSX_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Tsx));
    static GO_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Go));
    static JAVA_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Java));
    static CPP_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Cpp));
    static CSHARP_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::CSharp));
    static PHP_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Php));
    static SCALA_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Scala));
    static LUA_PARSER: RefCell<Parser> = RefCell::new(make_parser(Language::Lua));
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
        Language::JavaScript => JS_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::TypeScript => TS_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Tsx => TSX_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Go => GO_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Java => JAVA_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Cpp => CPP_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::CSharp => CSHARP_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Php => PHP_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Scala => SCALA_PARSER.with(|p| p.borrow_mut().parse(source, None)),
        Language::Lua => LUA_PARSER.with(|p| p.borrow_mut().parse(source, None)),
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

    fn ok(path: &str, lang: Language, src: &[u8]) -> Tree {
        parse(path, lang, src).expect("parse")
    }

    #[test]
    fn rust_smoke() {
        let t = ok("smoke.rs", Language::Rust, b"fn main() {}");
        assert_eq!(t.root_node().kind(), "source_file");
    }

    #[test]
    fn python_smoke() {
        let t = ok("smoke.py", Language::Python, b"def f():\n    return 1\n");
        assert_eq!(t.root_node().kind(), "module");
    }

    #[test]
    fn javascript_smoke() {
        let t = ok("smoke.js", Language::JavaScript, b"function f() { return 1; }");
        assert_eq!(t.root_node().kind(), "program");
    }

    #[test]
    fn typescript_smoke() {
        let t = ok(
            "smoke.ts",
            Language::TypeScript,
            b"function f(): number { return 1; }",
        );
        assert_eq!(t.root_node().kind(), "program");
    }

    #[test]
    fn tsx_smoke() {
        let t = ok(
            "smoke.tsx",
            Language::Tsx,
            b"const X = () => <div>hi</div>;",
        );
        assert_eq!(t.root_node().kind(), "program");
    }

    #[test]
    fn go_smoke() {
        let t = ok(
            "smoke.go",
            Language::Go,
            b"package main\nfunc main() {}\n",
        );
        assert_eq!(t.root_node().kind(), "source_file");
    }

    #[test]
    fn java_smoke() {
        let t = ok(
            "Smoke.java",
            Language::Java,
            b"class Smoke { public static void main(String[] a) {} }",
        );
        assert_eq!(t.root_node().kind(), "program");
    }

    #[test]
    fn cpp_smoke() {
        let t = ok(
            "smoke.cpp",
            Language::Cpp,
            b"int main() { return 0; }",
        );
        assert_eq!(t.root_node().kind(), "translation_unit");
    }

    #[test]
    fn csharp_smoke() {
        let t = ok(
            "Smoke.cs",
            Language::CSharp,
            b"class Smoke { static void Main() {} }",
        );
        assert_eq!(t.root_node().kind(), "compilation_unit");
    }

    #[test]
    fn php_smoke() {
        let t = ok(
            "smoke.php",
            Language::Php,
            b"<?php function f() { return 1; }",
        );
        assert_eq!(t.root_node().kind(), "program");
    }

    #[test]
    fn scala_smoke() {
        let t = ok(
            "Smoke.scala",
            Language::Scala,
            b"object Smoke { def main(args: Array[String]): Unit = () }",
        );
        assert_eq!(t.root_node().kind(), "compilation_unit");
    }

    #[test]
    fn lua_smoke() {
        let t = ok(
            "smoke.lua",
            Language::Lua,
            b"local function f() return 1 end",
        );
        assert_eq!(t.root_node().kind(), "chunk");
    }

    #[test]
    fn ext_routing_full() {
        for (ext, want) in [
            ("rs", Language::Rust),
            ("py", Language::Python),
            ("js", Language::JavaScript),
            ("ts", Language::TypeScript),
            ("tsx", Language::Tsx),
            ("go", Language::Go),
            ("java", Language::Java),
            ("cpp", Language::Cpp),
            ("cs", Language::CSharp),
            ("php", Language::Php),
            ("scala", Language::Scala),
            ("lua", Language::Lua),
        ] {
            assert_eq!(Language::from_extension(ext), Some(want), "ext={ext}");
        }
        assert_eq!(Language::from_extension("zig"), None);
    }
}
