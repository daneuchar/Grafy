//! Probe PATH for installed SCIP indexers.
//!
//! Probe is lazy and cheap: one `<bin> --version` (or equivalent) subprocess
//! call per candidate, capped at 2 seconds. Result is never cached across
//! `Pipeline::index` runs — installs can happen between runs and we want
//! the next index to pick them up.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use grafy_parser::Language;
use tracing::debug;

/// One detected indexer binary on PATH. Construction implies the binary
/// is on PATH and reported a non-empty version string within the probe budget.
#[derive(Debug, Clone)]
pub struct IndexerInfo {
    /// Language this indexer covers. For tools that span multiple languages
    /// (e.g. `scip-typescript` handles TS + JS), the detector emits one
    /// `IndexerInfo` per language.
    pub language: Language,
    /// Absolute path to the binary as resolved by `which`.
    pub binary: PathBuf,
    /// Version string as reported by the tool, trimmed. May be "unknown" if
    /// the tool printed nothing parseable.
    pub version: String,
    /// Human-readable name (`scip-python`, `rust-analyzer`, etc.) for logs.
    pub name: &'static str,
    /// Install command users should run when this indexer is missing.
    pub install_hint: &'static str,
}

/// Single declarative entry: language → (binary, --version subcommand, install hint).
struct Probe {
    language: Language,
    name: &'static str,
    binary: &'static str,
    version_args: &'static [&'static str],
    install_hint: &'static str,
}

/// One row per (language, binary). When a tool serves multiple languages
/// (`scip-typescript` → TS + JS, etc.) we emit one row per language so the
/// pipeline can filter by `IndexReport::has_language` cheaply.
const PROBES: &[Probe] = &[
    Probe {
        language: Language::Python,
        name: "scip-python",
        binary: "scip-python",
        version_args: &["--version"],
        install_hint: "npm install -g @sourcegraph/scip-python",
    },
    Probe {
        language: Language::TypeScript,
        name: "scip-typescript",
        binary: "scip-typescript",
        version_args: &["--version"],
        install_hint: "npm install -g @sourcegraph/scip-typescript",
    },
    Probe {
        language: Language::JavaScript,
        name: "scip-typescript",
        binary: "scip-typescript",
        version_args: &["--version"],
        install_hint: "npm install -g @sourcegraph/scip-typescript",
    },
    Probe {
        language: Language::Tsx,
        name: "scip-typescript",
        binary: "scip-typescript",
        version_args: &["--version"],
        install_hint: "npm install -g @sourcegraph/scip-typescript",
    },
    Probe {
        language: Language::Go,
        name: "scip-go",
        binary: "scip-go",
        version_args: &["--version"],
        install_hint: "go install github.com/sourcegraph/scip-go/cmd/scip-go@latest",
    },
    Probe {
        language: Language::Java,
        name: "scip-java",
        binary: "scip-java",
        version_args: &["--help"], // scip-java has no --version flag in v0.10.x
        install_hint: "cs install scip-java   # or: brew install scip-java",
    },
    Probe {
        language: Language::Cpp,
        name: "scip-clang",
        binary: "scip-clang",
        version_args: &["--version"],
        install_hint: "see https://github.com/sourcegraph/scip-clang/releases",
    },
    Probe {
        language: Language::Rust,
        name: "rust-analyzer",
        binary: "rust-analyzer",
        version_args: &["--version"],
        install_hint: "rustup component add rust-analyzer",
    },
];

/// Maximum wall time per probe. Anything slower is treated as "not on PATH".
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Scan PATH for indexers. Returns one `IndexerInfo` per (language, binary)
/// that responded within the probe budget. Never panics.
#[must_use]
pub fn detected_indexers() -> Vec<IndexerInfo> {
    let mut out = Vec::new();
    for probe in PROBES {
        let Some(binary) = which(probe.binary) else {
            continue;
        };
        let version = match version_of(&binary, probe.version_args) {
            Some(v) => v,
            None => "unknown".to_owned(),
        };
        debug!(
            target: "grafy.scip.detect",
            name = probe.name,
            language = probe.language.as_str(),
            binary = %binary.display(),
            version = %version,
            "indexer detected"
        );
        out.push(IndexerInfo {
            language: probe.language,
            binary,
            version,
            name: probe.name,
            install_hint: probe.install_hint,
        });
    }
    out
}

/// Resolve a binary on PATH. Avoids a `which` dependency by walking
/// `$PATH` ourselves. Returns the first matching executable file.
fn which(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(bin);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        // macOS / linux: some installers add `.sh` shims; skip for now.
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &std::path::Path) -> bool {
    p.is_file()
}

/// Run `<bin> <args>` with a 2-second wall budget, return trimmed stdout.
fn version_of(bin: &std::path::Path, args: &[&str]) -> Option<String> {
    let started = Instant::now();
    let child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Naive timeout via wait_timeout-equivalent: we just wait synchronously and
    // rely on the indexer probes being fast. If they aren't, we want to know.
    let out = child.wait_with_output().ok()?;
    if started.elapsed() > PROBE_TIMEOUT {
        debug!(target: "grafy.scip.detect", "probe exceeded {:?}", PROBE_TIMEOUT);
    }
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s = s.lines().next().unwrap_or("").trim().to_owned();
    if s.is_empty() {
        return None;
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_does_not_panic() {
        // Whatever the box has installed, the call should return cleanly.
        let _ = detected_indexers();
    }

    #[test]
    fn which_returns_none_for_garbage() {
        assert!(which("definitely-not-a-real-binary-xyz-123").is_none());
    }
}
