//! Evidence policy for the workspace tests.
//!
//! Extracted from the `x3d-evidence` crate of `Ascendance3D/X_ITE_R` at
//! `ffe080b193f031605095f87ab4cafe443e7b50c5` (X_ITE_R issue #66). Only the policy part moved; the
//! X_ITE answer-key resolver did not. See `PROVENANCE.md`.
//!
//! A test that cannot find its evidence must fail or skip by name; it never passes in silence.
//! [`no_gpu`], [`tool_absent`] and [`golden_unverified`] own that decision:
//!
//! - `CM_EVIDENCE=strict` (the default): every skip fails the test.
//! - `CM_EVIDENCE=ci`: a skip passes only when `CM_EVIDENCE_ALLOW` (comma list) names its reason
//!   (`no-gpu-adapter`, `tool-absent`, `golden-unverified`). Each permitted skip appends one line to `CM_EVIDENCE_LOG`
//!   (default `target/evidence-skips.tsv`): test, reason, key, caller, detail. Cargo hides the stderr
//!   of a passing test, so the log is the record.

use std::fmt;
use std::path::{Path, PathBuf};

/// The checkout that holds this crate.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// How a run treats missing evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Strict,
    Ci,
}

/// The policy from `CM_EVIDENCE` and `CM_EVIDENCE_ALLOW`.
#[derive(Clone, Debug)]
pub struct Policy {
    pub mode: Mode,
    pub allow: Vec<String>,
    pub log: PathBuf,
}

impl Policy {
    pub fn from_env() -> Policy {
        let mode = match std::env::var("CM_EVIDENCE").as_deref() {
            Err(_) | Ok("") | Ok("strict") => Mode::Strict,
            Ok("ci") => Mode::Ci,
            Ok(v) => panic!("CM_EVIDENCE={v:?}: use strict or ci"),
        };
        let allow = std::env::var("CM_EVIDENCE_ALLOW")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let log = std::env::var_os("CM_EVIDENCE_LOG")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace_root().join("target/evidence-skips.tsv"));

        Policy { mode, allow, log }
    }

    /// Whether a ci run may skip for `reason`.
    pub fn allows(&self, reason: &str) -> bool {
        self.mode == Mode::Ci && self.allow.iter().any(|a| a == reason)
    }
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

/// Fails the test, or (ci mode, allowed) records a named skip and returns. The caller then returns.
#[track_caller]
pub fn skip(reason: &str, detail: impl fmt::Display) {
    let policy = Policy::from_env();
    let at = std::panic::Location::caller();
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("-");

    if !policy.allows(reason) {
        let how = match policy.mode {
            Mode::Strict => "CM_EVIDENCE is strict".to_string(),
            Mode::Ci => format!("not in CM_EVIDENCE_ALLOW={:?}", policy.allow.join(",")),
        };

        panic!("evidence {reason}: {detail} [{how}]");
    }

    let line = format!(
        "{}\t{reason}\t-\t{}:{}\t{}\n",
        clean(test),
        at.file(),
        at.line(),
        clean(&detail.to_string())
    );

    if let Some(dir) = policy.log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&policy.log)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));

    if let Err(e) = written {
        panic!(
            "evidence {reason}: cannot write {}: {e}",
            policy.log.display()
        );
    }

    eprintln!("evidence skip {reason}: {detail}");
}

/// No wgpu adapter: fails in strict mode, or records a `no-gpu-adapter` skip.
#[track_caller]
pub fn no_gpu(detail: impl fmt::Display) {
    skip("no-gpu-adapter", detail)
}

/// A required program is absent: fails in strict mode, or records a `tool-absent` skip.
#[track_caller]
pub fn tool_absent(detail: impl fmt::Display) {
    skip("tool-absent", detail)
}

/// The checks ran, but no reviewed reference exists for this environment (for example
/// no golden set for the GPU adapter), so an exact comparison was not made: fails in
/// strict mode, or records a `golden-unverified` skip.
#[track_caller]
pub fn golden_unverified(detail: impl fmt::Display) {
    skip("golden-unverified", detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(mode: Mode, allow: &str) -> Policy {
        Policy {
            mode,
            allow: allow.split(',').map(str::to_string).collect(),
            log: PathBuf::from("/dev/null"),
        }
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();

            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }

    #[test]
    fn strict_allows_no_skip() {
        let p = policy(Mode::Strict, "no-gpu-adapter,tool-absent,golden-unverified");

        assert!(!p.allows("no-gpu-adapter"));
        assert!(!p.allows("tool-absent"));
        assert!(!p.allows("golden-unverified"));
    }

    #[test]
    fn ci_allows_only_named_skips() {
        let p = policy(Mode::Ci, "no-gpu-adapter");

        assert!(p.allows("no-gpu-adapter"));
        assert!(!p.allows("tool-absent"));
        assert!(!p.allows("golden-unverified"));
    }

    /// The old silent forms (`eprintln!` and return) must not come back in a workspace test.
    #[test]
    fn no_silent_skip_remains_in_workspace_tests() {
        let pattern = [
            "eprintln!(\"no adapter",
            "eprintln!(\"skipped: no GPU",
            "eprintln!(\"xvfb-run not found",
        ];
        let mut found = vec![];

        for c in std::fs::read_dir(workspace_root().join("crates")).unwrap() {
            let tests = c.unwrap().path().join("tests");

            if tests.is_dir() {
                let mut files = vec![];
                walk(&tests, &mut files);

                for f in files
                    .iter()
                    .filter(|f| f.extension().is_some_and(|e| e == "rs"))
                {
                    let text = std::fs::read_to_string(f).unwrap();

                    for (n, line) in text.lines().enumerate() {
                        if pattern.iter().any(|p| line.contains(p)) {
                            found.push(format!("{}:{}", f.display(), n + 1));
                        }
                    }
                }
            }
        }

        assert!(found.is_empty(), "silent skips: {found:#?}");
    }
}
