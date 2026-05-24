//! Provision SCIP indexers. macOS + Linux only.
//!
//! Each `install_<tool>` is idempotent — if the binary is already on PATH it
//! is skipped and reported as "already installed". `--dry-run` prints the
//! command that *would* run without executing.

use std::process::{Command, Stdio};

use crate::install::prereqs::{self, PrereqReport};
use crate::scip::detect::detected_indexers;

/// One row in the post-install summary table.
#[derive(Debug, Clone)]
pub struct InstallEntry {
    pub indexer: &'static str,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub enum Status {
    Installed,
    AlreadyPresent,
    Skipped,
    Failed,
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Installed => "installed",
            Status::AlreadyPresent => "already present",
            Status::Skipped => "skipped",
            Status::Failed => "FAILED",
        }
    }
}

/// `grafy install --with-scip [--dry-run]` entry point. Returns the per-tool
/// summary, exits with the right code via the caller in `main.rs`.
pub fn run_with_scip(dry_run: bool) -> Vec<InstallEntry> {
    if cfg!(target_os = "windows") {
        return vec![InstallEntry {
            indexer: "(all)",
            status: Status::Skipped,
            detail: "Windows install not supported in v1.0; install scip-* tools manually.".into(),
        }];
    }

    let prereqs = prereqs::probe();
    let already: std::collections::HashSet<&'static str> = detected_indexers()
        .iter()
        .map(|i| i.name)
        .collect::<std::collections::HashSet<_>>();

    vec![
        install_one("scip-python", &already, &prereqs, dry_run, install_scip_python),
        install_one("scip-typescript", &already, &prereqs, dry_run, install_scip_typescript),
        install_one("scip-go", &already, &prereqs, dry_run, install_scip_go),
        install_one("scip-java", &already, &prereqs, dry_run, install_scip_java),
        install_one("rust-analyzer", &already, &prereqs, dry_run, install_rust_analyzer),
    ]
}

type Installer = fn(&PrereqReport, bool) -> InstallEntry;

fn install_one(
    name: &'static str,
    already: &std::collections::HashSet<&'static str>,
    prereqs: &PrereqReport,
    dry_run: bool,
    f: Installer,
) -> InstallEntry {
    if already.contains(name) {
        return InstallEntry {
            indexer: name,
            status: Status::AlreadyPresent,
            detail: "on PATH; no action".into(),
        };
    }
    f(prereqs, dry_run)
}

fn install_scip_python(p: &PrereqReport, dry_run: bool) -> InstallEntry {
    if !p.has_npm() {
        return InstallEntry {
            indexer: "scip-python",
            status: Status::Skipped,
            detail: "needs npm — install Node.js (https://nodejs.org/) and retry.".into(),
        };
    }
    run_install(
        "scip-python",
        &["npm", "install", "-g", "@sourcegraph/scip-python"],
        dry_run,
    )
}

fn install_scip_typescript(p: &PrereqReport, dry_run: bool) -> InstallEntry {
    if !p.has_npm() {
        return InstallEntry {
            indexer: "scip-typescript",
            status: Status::Skipped,
            detail: "needs npm — install Node.js (https://nodejs.org/) and retry.".into(),
        };
    }
    run_install(
        "scip-typescript",
        &["npm", "install", "-g", "@sourcegraph/scip-typescript"],
        dry_run,
    )
}

fn install_scip_go(p: &PrereqReport, dry_run: bool) -> InstallEntry {
    if !p.has_go() {
        return InstallEntry {
            indexer: "scip-go",
            status: Status::Skipped,
            detail: "needs `go` (https://go.dev/dl/) and a $GOPATH/bin on PATH.".into(),
        };
    }
    run_install(
        "scip-go",
        &[
            "go",
            "install",
            "github.com/sourcegraph/scip-go/cmd/scip-go@latest",
        ],
        dry_run,
    )
}

fn install_scip_java(p: &PrereqReport, dry_run: bool) -> InstallEntry {
    if p.has_coursier() {
        return run_install("scip-java", &["cs", "install", "scip-java"], dry_run);
    }
    if p.brew.path.is_some() {
        return run_install("scip-java", &["brew", "install", "scip-java"], dry_run);
    }
    InstallEntry {
        indexer: "scip-java",
        status: Status::Skipped,
        detail: "needs coursier (`brew install coursier`) or Homebrew — install one and retry."
            .into(),
    }
}

fn install_rust_analyzer(p: &PrereqReport, dry_run: bool) -> InstallEntry {
    if !p.has_rustup() {
        return InstallEntry {
            indexer: "rust-analyzer",
            status: Status::Skipped,
            detail: "needs rustup (https://rustup.rs/).".into(),
        };
    }
    run_install(
        "rust-analyzer",
        &["rustup", "component", "add", "rust-analyzer"],
        dry_run,
    )
}

fn run_install(name: &'static str, argv: &[&str], dry_run: bool) -> InstallEntry {
    let pretty = argv.join(" ");
    if dry_run {
        return InstallEntry {
            indexer: name,
            status: Status::Skipped,
            detail: format!("dry-run: would execute `{pretty}`"),
        };
    }
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    match cmd.status() {
        Ok(s) if s.success() => InstallEntry {
            indexer: name,
            status: Status::Installed,
            detail: pretty,
        },
        Ok(s) => InstallEntry {
            indexer: name,
            status: Status::Failed,
            detail: format!("`{pretty}` exited non-zero ({s})"),
        },
        Err(e) => InstallEntry {
            indexer: name,
            status: Status::Failed,
            detail: format!("failed to spawn `{pretty}`: {e}"),
        },
    }
}
