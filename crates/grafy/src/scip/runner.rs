//! Spawn a SCIP indexer subprocess and capture its `.scip` output.
//!
//! Per-indexer command lines are hard-coded against the maintained Sourcegraph
//! tools as of M2 W2 (2026-05-24). If an upstream CLI gains/breaks a flag, the
//! runner emits a `tracing::warn!` with the verbatim stderr and falls back to
//! the heuristic pipeline (no panic, no failed index).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use grafy_parser::Language;
use tracing::{info, warn};

use crate::scip::detect::IndexerInfo;

/// Per-indexer wall-clock cap. SCIP indexers can be slow on big monorepos
/// (django: ~5 min for scip-python). Five minutes is the equivalent of the
/// per-file 5-second pipeline timeout, scaled to whole-repo work.
const RUNNER_TIMEOUT: Duration = Duration::from_secs(300);

/// Run `indexer` against `repo`, writing the `.scip` artifact under
/// `<repo>/.grafy/scip/<lang>.scip`. Returns the artifact path on success.
///
/// Stderr is captured to `<repo>/.grafy/scip/<lang>.log` for postmortem
/// debugging when the indexer fails. The caller is expected to log + skip
/// on error rather than abort the whole index.
pub fn run_indexer(indexer: &IndexerInfo, repo: &Path) -> Result<PathBuf> {
    let out_dir = repo.join(".grafy").join("scip");
    std::fs::create_dir_all(&out_dir).with_context(|| {
        format!(
            "{}: failed to create .grafy/scip/ — check write permissions on repo root.",
            out_dir.display()
        )
    })?;
    let out_path = out_dir.join(format!("{}.scip", indexer.language.as_str()));
    let log_path = out_dir.join(format!("{}.log", indexer.language.as_str()));

    // Per-tool command + prereq checks. Each returns the Command pre-configured
    // with cwd = repo and the right output flag pointing at out_path.
    let mut cmd = match build_command(indexer, repo, &out_path) {
        Ok(c) => c,
        Err(skip) => {
            warn!(
                target: "grafy.scip.runner",
                indexer = indexer.name,
                language = indexer.language.as_str(),
                reason = %skip,
                "SCIP indexer skipped — prereq missing"
            );
            return Err(skip);
        }
    };

    info!(
        target: "grafy.scip.runner",
        indexer = indexer.name,
        language = indexer.language.as_str(),
        repo = %repo.display(),
        "running SCIP indexer"
    );

    let started = Instant::now();
    let log_file = std::fs::File::create(&log_path).with_context(|| {
        format!(
            "{}: failed to open log file for stderr capture.",
            log_path.display()
        )
    })?;
    let stderr_clone = log_file.try_clone().context("clone stderr log handle")?;
    cmd.stdin(Stdio::null())
        .stdout(stderr_clone)
        .stderr(log_file);

    let mut child = cmd.spawn().with_context(|| {
        format!(
            "{}: failed to spawn {} — is it actually executable? (`{} --version` should print a line)",
            repo.display(),
            indexer.name,
            indexer.binary.display(),
        )
    })?;

    // Synchronous wait — we already capped the indexer to RUNNER_TIMEOUT via a
    // best-effort kill below; on macOS / Linux a 5-minute cap is plenty for
    // any realistic Grafy use case. (Daemon mode in v1.x can stream this.)
    loop {
        match child.try_wait()? {
            Some(status) => {
                let elapsed = started.elapsed();
                if !status.success() {
                    return Err(anyhow!(
                        "{} ({}): exited non-zero — see {} for stderr. ({:?})",
                        indexer.name,
                        indexer.language.as_str(),
                        log_path.display(),
                        status
                    ));
                }
                info!(
                    target: "grafy.scip.runner",
                    indexer = indexer.name,
                    language = indexer.language.as_str(),
                    elapsed_ms = elapsed.as_millis() as u64,
                    "SCIP indexer finished"
                );
                if !out_path.exists() {
                    return Err(anyhow!(
                        "{} reported success but {} does not exist — upstream CLI may have changed.",
                        indexer.name,
                        out_path.display()
                    ));
                }
                return Ok(out_path);
            }
            None => {
                if started.elapsed() > RUNNER_TIMEOUT {
                    let _ = child.kill();
                    return Err(anyhow!(
                        "{} ({}): exceeded {:?} wall budget — kill+skip. Set GRAFY_SCIP_DISABLE=1 to opt out.",
                        indexer.name,
                        indexer.language.as_str(),
                        RUNNER_TIMEOUT
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Construct the per-indexer command. Returns `Err` (skip, not crash) when
/// a prereq is missing — caller logs the reason and moves on.
fn build_command(indexer: &IndexerInfo, repo: &Path, out_path: &Path) -> Result<Command> {
    let out_arg = out_path.to_string_lossy().into_owned();
    let mut cmd = Command::new(&indexer.binary);
    cmd.current_dir(repo);

    match (indexer.name, indexer.language) {
        ("scip-python", Language::Python) => {
            // scip-python index --output <file> --project-name <repo> --project-version 0.0.0 .
            let name = repo
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("grafy-index");
            cmd.arg("index")
                .arg("--output")
                .arg(&out_arg)
                .arg("--project-name")
                .arg(name)
                .arg("--project-version")
                .arg("0.0.0")
                .arg(".");
        }
        ("scip-typescript", _) => {
            // scip-typescript covers TS/JS/TSX from one binary; we run it
            // once even though three IndexerInfo rows exist — the pipeline
            // de-dupes via the cache directory + lang-specific .scip path.
            cmd.arg("index").arg("--output").arg(&out_arg);
            // node_modules required.
            if !repo.join("node_modules").exists() {
                return Err(anyhow!(
                    "scip-typescript needs node_modules. Run `npm install` in {} first.",
                    repo.display()
                ));
            }
        }
        ("scip-go", _) => {
            if !repo.join("go.mod").exists() {
                return Err(anyhow!(
                    "scip-go needs go.mod (Go modules). Run `go mod init` in {} first.",
                    repo.display()
                ));
            }
            cmd.arg("--output").arg(&out_arg).arg(".");
        }
        ("scip-java", _) => {
            let has_mvn = repo.join("pom.xml").exists();
            let has_gradle =
                repo.join("build.gradle").exists() || repo.join("build.gradle.kts").exists();
            if !has_mvn && !has_gradle {
                return Err(anyhow!(
                    "scip-java needs Maven (pom.xml) or Gradle (build.gradle[.kts]). None found in {}.",
                    repo.display()
                ));
            }
            cmd.arg("index");
        }
        ("scip-clang", _) => {
            let cdb = repo.join("compile_commands.json");
            if !cdb.exists() {
                return Err(anyhow!(
                    "scip-clang needs compile_commands.json in {}. Generate via cmake / bear / intercept-build.",
                    repo.display()
                ));
            }
            cmd.arg("--output").arg(&out_arg).arg("--compdb").arg(&cdb);
        }
        ("rust-analyzer", _) => {
            // rust-analyzer scip <path> writes ./index.scip; we rename after.
            cmd.arg("scip").arg(".");
        }
        (name, lang) => {
            return Err(anyhow!(
                "no command template for indexer `{}` on language `{}` — open an issue.",
                name,
                lang.as_str()
            ));
        }
    }
    Ok(cmd)
}
