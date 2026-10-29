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
//!
//! Private inputs (Ascendance3D/X_ITE_R#168): historical files with no redistribution permission are
//! not tracked. `oracle/private-inputs.tsv` lists each one with its kind, file count, byte count and
//! sha256. [`private_input`] finds and verifies one: `$CM_PRIVATE_INPUTS/<name>` when the variable is
//! set (nothing else is searched), else `oracle/<name>` in this checkout, else in the main worktree.
//! An absent input fails in strict mode; in ci mode it skips only when `CM_EVIDENCE_ALLOW` names the
//! input itself (for example `corpus-items`). A mismatched input always fails.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

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

    /// Whether a ci run may skip for `reason` with `key`. A `private-input-absent` skip needs the
    /// input's own name in the allow list; the reason alone never permits it.
    pub fn allows_key(&self, reason: &str, key: &str) -> bool {
        match reason {
            "private-input-absent" => self.mode == Mode::Ci && self.allow.iter().any(|a| a == key),
            _ => false,
        }
    }
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

/// Fails the test, or (ci mode, allowed) records a named skip and returns. The caller then returns.
#[track_caller]
pub fn skip(reason: &str, detail: impl fmt::Display) {
    record(reason, None, std::panic::Location::caller(), detail)
}

fn record(reason: &str, key: Option<&str>, at: &std::panic::Location, detail: impl fmt::Display) {
    let policy = Policy::from_env();
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("-");
    let allowed = match key {
        Some(k) => policy.allows_key(reason, k),
        None => policy.allows(reason),
    };
    let what = match key {
        Some(k) => format!("{reason} ({k})"),
        None => reason.to_string(),
    };

    if !allowed {
        let how = match policy.mode {
            Mode::Strict => "CM_EVIDENCE is strict".to_string(),
            Mode::Ci => format!("not in CM_EVIDENCE_ALLOW={:?}", policy.allow.join(",")),
        };

        panic!("evidence {what}: {detail} [{how}]");
    }

    let line = format!(
        "{}\t{reason}\t{}\t{}:{}\t{}\n",
        clean(test),
        key.unwrap_or("-"),
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
            "evidence {what}: cannot write {}: {e}",
            policy.log.display()
        );
    }

    eprintln!("evidence skip {what}: {detail}");
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

/// One row of `oracle/private-inputs.tsv`: kind (`file` or `dir`), files, bytes, sha256.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measure {
    pub kind: String,
    pub files: u64,
    pub bytes: u64,
    pub sha256: String,
}

impl fmt::Display for Measure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}, {} files, {} bytes, sha256 {}",
            self.kind, self.files, self.bytes, self.sha256
        )
    }
}

/// The rows of a private-input manifest. A malformed row panics; it is never skipped.
pub fn parse_manifest(text: &str) -> HashMap<String, Measure> {
    let mut rows = HashMap::new();

    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }

        let c: Vec<&str> = line.split('\t').collect();
        let ok = c.len() == 5
            && (c[1] == "file" || c[1] == "dir")
            && c[2].parse::<u64>().is_ok()
            && c[3].parse::<u64>().is_ok()
            && c[4].len() == 64
            && c[4].bytes().all(|b| b.is_ascii_hexdigit());

        assert!(
            ok,
            "evidence: private-inputs.tsv line {}: malformed row {line:?}",
            n + 1
        );
        rows.insert(
            c[0].to_string(),
            Measure {
                kind: c[1].to_string(),
                files: c[2].parse().unwrap(),
                bytes: c[3].parse().unwrap(),
                sha256: c[4].to_string(),
            },
        );
    }

    rows
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(path)?;
    std::io::copy(&mut f, &mut h)?;
    Ok(hex(&h.finalize()))
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Measures a file (sha256 of its bytes) or a directory (sha256 of the `sha256sum` lines of every
/// file, relative paths, bytewise sort), as X_ITE_R's `tools/answer_keys.py` does.
pub fn measure(path: &Path) -> std::io::Result<Measure> {
    let meta = std::fs::metadata(path)?;

    if meta.is_file() {
        return Ok(Measure {
            kind: "file".into(),
            files: 1,
            bytes: meta.len(),
            sha256: sha256_file(path)?,
        });
    }

    fn walk(dir: &Path, base: &Path, out: &mut Vec<(Vec<u8>, PathBuf)>) -> std::io::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let p = e?.path();

            if p.is_dir() {
                walk(&p, base, out)?;
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                out.push((rel.into_bytes(), p));
            }
        }

        Ok(())
    }

    let mut files = vec![];
    walk(path, path, &mut files)?;
    files.sort();
    let (mut listing, mut bytes) = (Sha256::new(), 0);

    for (rel, p) in &files {
        bytes += std::fs::metadata(p)?.len();
        listing.update(sha256_file(p)?.as_bytes());
        listing.update(b"  ");
        listing.update(rel);
        listing.update(b"\n");
    }

    Ok(Measure {
        kind: "dir".into(),
        files: files.len() as u64,
        bytes,
        sha256: hex(&listing.finalize()),
    })
}

/// The main worktree of the checkout at `root`, when Git can name it.
fn main_worktree(root: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;

    text.lines()
        .next()?
        .strip_prefix("worktree ")
        .map(PathBuf::from)
}

/// Where [`private_input`] looks for `name`, in order.
pub fn private_candidates(root: &Path, name: &str, explicit: Option<&Path>) -> Vec<PathBuf> {
    if let Some(dir) = explicit {
        return vec![dir.join(name)];
    }

    let mut out = vec![root.join("oracle").join(name)];

    if let Some(main) = main_worktree(root) {
        // A clone or the main worktree itself names its own root: search it once.
        if main.canonicalize().ok() != root.canonicalize().ok() {
            out.push(main.join("oracle").join(name));
        }
    }

    out
}

/// Why a private input cannot be used.
#[derive(Debug)]
pub enum InputError {
    Absent(String),
    Mismatch(String),
}

/// Finds and verifies `name` below `root` (see the crate documentation). Pure: no policy.
pub fn resolve_private_in(
    root: &Path,
    name: &str,
    explicit: Option<&Path>,
) -> Result<PathBuf, InputError> {
    let manifest = root.join("oracle/private-inputs.tsv");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("evidence: {}: {e}", manifest.display()));
    let rows = parse_manifest(&text);
    let want = rows
        .get(name)
        .unwrap_or_else(|| panic!("evidence: {name} is not a row of {}", manifest.display()));
    let searched = private_candidates(root, name, explicit);

    for p in &searched {
        // Only a path with no entry at all is absent. A broken symlink or an entry that cannot be
        // read is a fault, not a reason to skip.
        match std::fs::symlink_metadata(p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(InputError::Mismatch(format!(
                    "{name}: {}: {e}",
                    p.display()
                )));
            }
            Ok(_) if !p.exists() => {
                return Err(InputError::Mismatch(format!(
                    "{name}: {} is a symlink to a path that does not exist",
                    p.display()
                )));
            }
            Ok(_) => {}
        }

        let got = measure(p)
            .map_err(|e| InputError::Mismatch(format!("{name}: {}: {e}", p.display())))?;

        if &got != want {
            return Err(InputError::Mismatch(format!(
                "{name}: {} is {got}; private-inputs.tsv wants {want}",
                p.display()
            )));
        }

        return Ok(p.clone());
    }

    let list: Vec<String> = searched.iter().map(|p| p.display().to_string()).collect();
    Err(InputError::Absent(format!(
        "{name}: absent; searched {}; restore it from the owner's private backup, or set CM_PRIVATE_INPUTS",
        list.join(", ")
    )))
}

/// The verified path of the private input `name`, or `None` after a permitted, logged skip.
/// Absent: fails in strict mode; skips in ci mode only when `CM_EVIDENCE_ALLOW` names `name`.
/// Mismatched: always fails. Each input is measured once per test process.
#[track_caller]
pub fn private_input(name: &str) -> Option<PathBuf> {
    static SEEN: OnceLock<Mutex<HashMap<String, Result<PathBuf, String>>>> = OnceLock::new();
    let at = std::panic::Location::caller();
    let explicit = std::env::var_os("CM_PRIVATE_INPUTS")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let found = SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(name.to_string())
        .or_insert_with(
            || match resolve_private_in(&workspace_root(), name, explicit.as_deref()) {
                Ok(p) => Ok(p),
                Err(InputError::Absent(d)) => Err(d),
                Err(InputError::Mismatch(d)) => {
                    panic!("evidence private-input-mismatch ({name}): {d}")
                }
            },
        )
        .clone();

    match found {
        Ok(p) => Some(p),
        Err(detail) => {
            record("private-input-absent", Some(name), at, detail);
            None
        }
    }
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

    #[test]
    fn private_input_skip_needs_its_own_name() {
        let p = policy(Mode::Ci, "private-input-absent,tool-absent");

        assert!(!p.allows_key("private-input-absent", "corpus-items"));
        assert!(
            policy(Mode::Ci, "corpus-items").allows_key("private-input-absent", "corpus-items")
        );
        assert!(
            !policy(Mode::Strict, "corpus-items")
                .allows_key("private-input-absent", "corpus-items")
        );
        assert!(
            !policy(Mode::Ci, "corpus-items").allows_key("private-input-mismatch", "corpus-items")
        );
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cm-evidence-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("oracle")).unwrap();
        d
    }

    #[test]
    fn private_input_found_absent_and_mismatched() {
        let root = scratch("resolve");
        let data = root.join("store");
        std::fs::create_dir_all(data.join("set/sub")).unwrap();
        std::fs::write(data.join("set/a.txt"), b"a\n").unwrap();
        std::fs::write(data.join("set/sub/b.txt"), b"bb\n").unwrap();
        let m = measure(&data.join("set")).unwrap();
        assert_eq!((m.kind.as_str(), m.files, m.bytes), ("dir", 2, 5));
        std::fs::write(
            root.join("oracle/private-inputs.tsv"),
            format!(
                "# name\tkind\tfiles\tbytes\tsha256\nset\tdir\t2\t5\t{}\n",
                m.sha256
            ),
        )
        .unwrap();

        // Found through the explicit location.
        assert_eq!(
            resolve_private_in(&root, "set", Some(&data)).unwrap(),
            data.join("set")
        );
        // Absent: the explicit location replaces every other one.
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(matches!(
            resolve_private_in(&root, "set", Some(&empty)),
            Err(InputError::Absent(_))
        ));
        // Mismatched: one extra file.
        std::fs::write(data.join("set/extra.txt"), b"").unwrap();
        assert!(matches!(
            resolve_private_in(&root, "set", Some(&data)),
            Err(InputError::Mismatch(_))
        ));
        std::fs::remove_file(data.join("set/extra.txt")).unwrap();
        // Mismatched: a file where the manifest wants a directory.
        let file_store = root.join("file-store");
        std::fs::create_dir_all(&file_store).unwrap();
        std::fs::write(file_store.join("set"), b"a\n").unwrap();
        assert!(matches!(
            resolve_private_in(&root, "set", Some(&file_store)),
            Err(InputError::Mismatch(_))
        ));
        // Mismatched, not absent: a symlink whose target does not exist.
        #[cfg(unix)]
        {
            let broken = root.join("broken");
            std::fs::create_dir_all(&broken).unwrap();
            std::os::unix::fs::symlink(root.join("no-such-dir"), broken.join("set")).unwrap();
            assert!(matches!(
                resolve_private_in(&root, "set", Some(&broken)),
                Err(InputError::Mismatch(_))
            ));
        }
        // Mismatched: one changed byte.
        std::fs::write(data.join("set/a.txt"), b"A\n").unwrap();
        assert!(matches!(
            resolve_private_in(&root, "set", Some(&data)),
            Err(InputError::Mismatch(_))
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_manifest_parses_and_lists_no_tracked_path() {
        let rows = parse_manifest(
            &std::fs::read_to_string(workspace_root().join("oracle/private-inputs.tsv")).unwrap(),
        );
        assert!(
            rows.contains_key("corpus-items") && rows.contains_key("cm5-cmd-ops-baseline.json")
        );
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(workspace_root())
            .args([
                "ls-files",
                "--",
                "oracle/corpus-items",
                "oracle/cm5-cmd-ops-baseline.json",
            ])
            .output()
            .unwrap();
        assert!(
            out.stdout.is_empty(),
            "tracked private input: {}",
            String::from_utf8_lossy(&out.stdout)
        );
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
