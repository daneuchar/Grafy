//! Probe the host for the toolchains required by each SCIP indexer.
//!
//! The probe is non-destructive: `<bin> --version` once per candidate. The
//! caller (`grafy install --with-scip`) uses this report to skip indexers
//! whose prereqs are missing and to print actionable hints.

use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Tool {
    pub name: &'static str,
    /// Version string as returned by the tool, or "not installed".
    pub version: String,
    /// Absolute path on PATH, or None.
    pub path: Option<PathBuf>,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            name: "",
            version: "not installed".into(),
            path: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PrereqReport {
    pub node: Tool,
    pub npm: Tool,
    pub go: Tool,
    pub java: Tool,
    pub javac: Tool,
    pub coursier: Tool,
    pub cargo: Tool,
    pub rustup: Tool,
    pub brew: Tool,
}

impl PrereqReport {
    /// True if `npm` is on PATH (sufficient for the npm-installed SCIP tools).
    #[must_use]
    pub fn has_npm(&self) -> bool {
        self.npm.path.is_some()
    }

    #[must_use]
    pub fn has_go(&self) -> bool {
        self.go.path.is_some()
    }

    /// scip-java install via coursier — we treat coursier as the canonical
    /// route. Homebrew is documented as the fallback.
    #[must_use]
    pub fn has_coursier(&self) -> bool {
        self.coursier.path.is_some()
    }

    #[must_use]
    pub fn has_rustup(&self) -> bool {
        self.rustup.path.is_some()
    }
}

/// Probe every tool we care about and return a snapshot.
#[must_use]
pub fn probe() -> PrereqReport {
    PrereqReport {
        node: tool("node", &["--version"]),
        npm: tool("npm", &["--version"]),
        go: tool("go", &["version"]),
        java: tool("java", &["-version"]), // java prints to stderr (!)
        javac: tool("javac", &["-version"]),
        coursier: tool("coursier", &["--version"]),
        cargo: tool("cargo", &["--version"]),
        rustup: tool("rustup", &["--version"]),
        brew: tool("brew", &["--version"]),
    }
}

fn tool(bin: &'static str, args: &[&str]) -> Tool {
    let Some(path) = which(bin) else {
        return Tool {
            name: bin,
            version: "not installed".into(),
            path: None,
        };
    };
    let out = Command::new(&path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok();
    let version = match out {
        Some(o) => {
            // `java -version` prints to stderr; merge both streams.
            let s = if !o.stdout.is_empty() {
                String::from_utf8_lossy(&o.stdout).into_owned()
            } else {
                String::from_utf8_lossy(&o.stderr).into_owned()
            };
            s.lines().next().unwrap_or("unknown").trim().to_owned()
        }
        None => "unknown".into(),
    };
    Tool {
        name: bin,
        version,
        path: Some(path),
    }
}

fn which(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(bin);
        if is_executable(&candidate) {
            return Some(candidate);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_does_not_panic() {
        let r = probe();
        // Every Tool has a non-empty name.
        assert_eq!(r.node.name, "node");
        assert_eq!(r.cargo.name, "cargo");
    }
}
