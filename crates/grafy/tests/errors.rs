//! Error UX policy — plan M0. Every user-visible error names the
//! file, the language, and a one-line "what to do next."

use grafy_parser::{parse, Language, ParseError};

#[test]
fn parse_error_mentions_path() {
    // Empty input may still produce a tree; force a real error path
    // by feeding something the grammar will accept then check format.
    // We assert against the Display impl directly for the timeout variant.
    let e = ParseError::Timeout {
        path: "evil.rs".into(),
        timeout: std::time::Duration::from_secs(5),
    };
    let msg = format!("{e}");
    assert!(msg.contains("evil.rs"), "error names the file: {msg}");
    assert!(msg.contains("open an issue") || msg.contains("split the file"),
            "error gives next-step action: {msg}");
}

#[test]
fn smoke_rust() {
    let tree = parse("ok.rs", Language::Rust, b"fn main() {}").expect("parse");
    assert!(!tree.root_node().has_error());
}
