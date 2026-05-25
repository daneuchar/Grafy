//! Pretty-print install summaries and `grafy diagnose` indexer tables.

use std::io::Write;

use crate::install::installer::InstallEntry;
use crate::scip::detect::{detected_indexers, IndexerInfo};

/// Render the post-install summary to `out`.
pub fn print_report<W: Write>(entries: &[InstallEntry], out: &mut W) -> std::io::Result<()> {
    writeln!(out, "grafy install --with-scip — summary")?;
    writeln!(out, "{:-<60}", "")?;
    for e in entries {
        writeln!(
            out,
            "  {:<18}  {:<16}  {}",
            e.indexer,
            e.status.label(),
            e.detail
        )?;
    }
    Ok(())
}

/// Render the `grafy diagnose` indexer block. Shows detected indexers and
/// install commands for the ones still missing.
pub fn print_indexer_status<W: Write>(out: &mut W) -> std::io::Result<()> {
    use grafy_parser::Language;

    let detected: Vec<IndexerInfo> = detected_indexers();
    let by_lang: std::collections::HashMap<Language, &IndexerInfo> =
        detected.iter().map(|i| (i.language, i)).collect();

    writeln!(out, "SCIP indexers detected:")?;

    // Print one row per relevant language.
    let rows: &[(Language, &str, &str)] = &[
        (
            Language::Python,
            "python",
            "npm install -g @sourcegraph/scip-python",
        ),
        (
            Language::TypeScript,
            "ts/js",
            "npm install -g @sourcegraph/scip-typescript",
        ),
        (
            Language::Go,
            "go",
            "go install github.com/sourcegraph/scip-go/cmd/scip-go@latest",
        ),
        (Language::Java, "java", "cs install scip-java"),
        (
            Language::Cpp,
            "c/c++",
            "see https://github.com/sourcegraph/scip-clang/releases",
        ),
        (Language::Rust, "rust", "rustup component add rust-analyzer"),
    ];

    for (lang, label, install_cmd) in rows {
        match by_lang.get(lang) {
            Some(info) => {
                writeln!(
                    out,
                    "  {:<7} OK  {} {}  {}",
                    label,
                    info.name,
                    info.version,
                    info.binary.display()
                )?;
            }
            None => {
                writeln!(out, "  {label:<7} --  install: {install_cmd}")?;
            }
        }
    }
    Ok(())
}
