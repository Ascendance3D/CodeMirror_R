//! The L-B wasm runtime probe as a maintained test: build `cm-web` for
//! `wasm32-unknown-unknown`, generate its JS bindings with `wasm-bindgen`, and
//! run `tests/web/lb_scenarios.mjs` (20 steps in 5 groups, plus the adapter's
//! clock cases) through the adapter's public API in Node.
//!
//! Node is a development host only. The browser runs (Chromium, Firefox, WebKit)
//! need Playwright and run from `tools/web-test.sh`.
//!
//! Tools: `rustup` target `wasm32-unknown-unknown`, `wasm-bindgen` 0.2.129 (the
//! version in Cargo.lock) and `node`. A missing tool is a `tool-absent` evidence
//! skip (fails in strict mode). The wasm build uses its own target directory under
//! this test's, so it never waits on the outer build's lock.
//!
//! The Node run has a deadline kept by this test (`run_with_deadline`, std only), not
//! by an external `timeout` program, which stock macOS lacks and Windows spells
//! differently. At the deadline the child is killed and reaped and the test fails
//! with a timeout message. `node-run.mjs` starts no processes of its own, so killing
//! `node` ends the whole run.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The most the Node run may take (the scenarios take about a second).
const NODE_LIMIT: Duration = Duration::from_secs(120);

/// Runs `cmd args` (with `PATH` replaced when `path` is given); its trimmed stdout if it succeeds.
fn tool(path: Option<&OsStr>, cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    if let Some(p) = path {
        c.env("PATH", p);
    }
    let out = c.args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The wasm-bindgen version Cargo.lock pins.
fn locked_bindgen() -> String {
    let lock = include_str!("../../../Cargo.lock");
    lock.split("[[package]]")
        .find(|p| p.contains("name = \"wasm-bindgen\"\n"))
        .and_then(|p| p.lines().find_map(|l| l.strip_prefix("version = \"")))
        .map(|v| v.trim_end_matches('"').to_string())
        .expect("wasm-bindgen in Cargo.lock")
}

/// The tools the run needs, looked up on `path` (the process `PATH` when `None`). An
/// `Err` names what is missing or unusable; the caller reports it as `tool-absent`.
fn preflight(path: Option<&OsStr>) -> Result<(), String> {
    let want = locked_bindgen();
    match tool(path, "wasm-bindgen", &["--version"]) {
        Some(v) if v.ends_with(&want) => {}
        Some(v) => {
            return Err(format!(
                "wasm-bindgen {want} (Cargo.lock) needed, found {v:?}"
            ));
        }
        None => return Err(format!("wasm-bindgen {want} not found or not runnable")),
    }
    if tool(path, "node", &["--version"]).is_none() {
        return Err("node not found or not runnable".into());
    }
    let targets = tool(path, "rustup", &["target", "list", "--installed"]).unwrap_or_default();
    if !targets.lines().any(|l| l == "wasm32-unknown-unknown") {
        return Err(
            "rustup target wasm32-unknown-unknown not installed (or rustup not found)".into(),
        );
    }
    Ok(())
}

/// How a run under `run_with_deadline` ended.
#[derive(Debug)]
enum Run {
    Exited(Output),
    /// Killed at the deadline and reaped; the output read before the kill.
    TimedOut {
        pid: u32,
        after: Duration,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
}

/// Reads a child pipe to its end on a thread; the bytes arrive on the channel.
fn drain(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

/// Runs `cmd` with its output captured, and kills it if it has not exited by `limit`.
/// A program that cannot start is an `Err` (`NotFound` when it is not installed).
fn run_with_deadline(cmd: &mut Command, limit: Duration) -> std::io::Result<Run> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id();
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Run::Exited(Output {
                status,
                stdout: out.recv().unwrap_or_default(),
                stderr: err.recv().unwrap_or_default(),
            }));
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            child.wait()?;
            // The pipes close with the child; do not wait on one a grandchild still holds.
            let grace = Duration::from_secs(2);
            return Ok(Run::TimedOut {
                pid,
                after: start.elapsed(),
                stdout: out.recv_timeout(grace).unwrap_or_default(),
                stderr: err.recv_timeout(grace).unwrap_or_default(),
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn out_dir() -> PathBuf {
    // CARGO_TARGET_TMPDIR is under the target directory of this build.
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cm-web-wasm")
}

/// Build the wasm module and its bindings; returns the package directory.
fn build_pkg() -> Result<PathBuf, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = out_dir();
    let target = out.join("target");
    let status = Command::new(env!("CARGO"))
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", out.join("build"))
        .args([
            "build",
            "--offline",
            "--locked",
            "--release",
            "-p",
            "cm-web",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build for wasm32 failed: {status}"));
    }
    let wasm = target.join("wasm32-unknown-unknown/release/cm_web.wasm");
    let pkg = out.join("pkg");
    let status = Command::new("wasm-bindgen")
        .args(["--target", "web", "--out-dir"])
        .arg(&pkg)
        .arg(&wasm)
        .status()
        .map_err(|e| format!("wasm-bindgen: {e}"))?;
    if !status.success() {
        return Err(format!("wasm-bindgen failed: {status}"));
    }
    Ok(pkg)
}

#[test]
fn lb_scenarios_through_the_adapter_in_node() {
    if let Err(why) = preflight(None) {
        return cm_evidence::tool_absent(why);
    }

    let pkg = build_pkg().expect("wasm package");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/web/node-run.mjs");
    let out = match run_with_deadline(Command::new("node").arg(&script).arg(&pkg), NODE_LIMIT) {
        Ok(Run::Exited(out)) => out,
        Ok(Run::TimedOut {
            pid,
            after,
            stdout,
            stderr,
        }) => panic!(
            "node run timed out: killed pid {pid} after {after:?} (limit {NODE_LIMIT:?})\n{}\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return cm_evidence::tool_absent(format!("node: {e}"));
        }
        Err(e) => panic!("run node: {e}"),
    };
    let json = String::from_utf8_lossy(&out.stdout);
    std::fs::write(out_dir().join("node-run.json"), json.as_bytes()).unwrap();
    assert!(
        out.status.success(),
        "node run failed ({}):\n{json}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    // The probe's 20 steps all ran and passed, and so did the adapter clock cases.
    assert!(json.contains("\"steps\": 20"), "{json}");
    assert!(json.contains("\"ok\": true"), "{json}");
    assert!(!json.contains("\"ok\": false"), "{json}");
}

// The deadline and the tool checks, with `node -e` children. Without node these are
// `tool-absent` skips, like the run itself.

fn node_e(src: &str) -> Command {
    let mut c = Command::new("node");
    c.args(["-e", src]);
    c
}

fn have_node() -> bool {
    tool(None, "node", &["--version"]).is_some()
}

#[test]
fn deadline_returns_a_child_that_succeeds() {
    if !have_node() {
        return cm_evidence::tool_absent("node not found");
    }
    match run_with_deadline(&mut node_e("console.log('done')"), Duration::from_secs(60)) {
        Ok(Run::Exited(out)) => {
            assert!(out.status.success(), "{:?}", out.status);
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "done");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn deadline_reports_a_child_that_fails() {
    if !have_node() {
        return cm_evidence::tool_absent("node not found");
    }
    let src = "console.error('bad'); process.exit(3)";
    match run_with_deadline(&mut node_e(src), Duration::from_secs(60)) {
        Ok(Run::Exited(out)) => {
            assert_eq!(out.status.code(), Some(3));
            assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "bad");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn deadline_kills_a_child_that_never_completes() {
    if !have_node() {
        return cm_evidence::tool_absent("node not found");
    }
    let limit = Duration::from_secs(2);
    let t0 = Instant::now();
    let src = "console.log('started'); setInterval(() => {}, 1000)";
    match run_with_deadline(&mut node_e(src), limit) {
        Ok(Run::TimedOut {
            pid, after, stdout, ..
        }) => {
            assert!(after >= limit, "{after:?}");
            assert!(
                t0.elapsed() < limit + Duration::from_secs(10),
                "{:?}",
                t0.elapsed()
            );
            assert_eq!(String::from_utf8_lossy(&stdout).trim(), "started");
            // Killed and reaped: no process (not even a zombie) has the pid.
            #[cfg(target_os = "linux")]
            assert!(
                !Path::new(&format!("/proc/{pid}")).exists(),
                "pid {pid} still exists"
            );
            let _ = pid;
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_program_is_not_found() {
    let mut c = Command::new("cm-r-no-such-program-for-wasm-runtime");
    match run_with_deadline(&mut c, Duration::from_secs(5)) {
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
        other => panic!("{other:?}"),
    }
}

#[test]
fn preflight_names_the_missing_tool() {
    // Directories of this process only: concurrent runs of this binary share out_dir().
    let dir = out_dir().join(format!("preflight-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // An empty directory as PATH: nothing is found, and wasm-bindgen is checked first.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let why = preflight(Some(empty.as_os_str())).unwrap_err();
    assert!(
        why.starts_with("wasm-bindgen ") && why.ends_with(" not found or not runnable"),
        "{why}"
    );

    // wasm-bindgen alone: node is the missing tool.
    #[cfg(unix)]
    {
        let Some(found) = std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("wasm-bindgen"))
                .find(|f| f.is_file())
        }) else {
            return cm_evidence::tool_absent("wasm-bindgen not found");
        };
        let only = dir.join("bindgen-only");
        std::fs::create_dir_all(&only).unwrap();
        std::os::unix::fs::symlink(&found, only.join("wasm-bindgen")).unwrap();
        if tool(Some(only.as_os_str()), "wasm-bindgen", &["--version"])
            .is_some_and(|v| v.ends_with(&locked_bindgen()))
        {
            assert_eq!(
                preflight(Some(only.as_os_str())),
                Err("node not found or not runnable".into())
            );
        } else {
            return cm_evidence::tool_absent(format!(
                "wasm-bindgen {} not found",
                locked_bindgen()
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// `cm_view::fileio::StdFs` on wasm32-unknown-unknown (C-HOST-FILE): every call is an
// `Unsupported` error, never a missing file, and `App::open` returns it. cm-web never
// uses `StdFs`, so a small cdylib over cm-view is generated here, built for wasm32 and
// run in Node with plain `WebAssembly.instantiate` (no bindings).

const STDFS_PROBE: &str = r#"
use std::path::Path;
use cm_view::app::{App, AppOptions, MemClipboard};
use cm_view::fileio::{FileHost, StdFs};
/// 0 `Ok(None)`, 1 `Ok(Some)`, 2 `Err(Unsupported)`, 3 another error.
fn code<T>(r: std::io::Result<Option<T>>) -> i32 {
    match r {
        Ok(None) => 0,
        Ok(Some(_)) => 1,
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => 2,
        Err(_) => 3,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn probe_read() -> i32 { code(StdFs.read(Path::new("/etc/hostname"))) }
#[unsafe(no_mangle)]
pub extern "C" fn probe_read_missing() -> i32 { code(StdFs.read(Path::new("/no/such/file.wrl"))) }
#[unsafe(no_mangle)]
pub extern "C" fn probe_write() -> i32 { code(StdFs.write(Path::new("/tmp/x.wrl"), b"x").map(Some)) }
#[unsafe(no_mangle)]
pub extern "C" fn probe_app_open() -> i32 {
    let r = App::open(
        Path::new("/no/such/file.wrl"),
        cm_view::Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    );
    code(r.map(|_| None::<()>))
}
"#;

const STDFS_RUN: &str = r#"
import { readFileSync } from "node:fs";
const m = await WebAssembly.compile(readFileSync(process.argv[2]));
const imports = {};
for (const i of WebAssembly.Module.imports(m)) {
  (imports[i.module] ??= {})[i.name] = () => { throw new Error(`import ${i.module}.${i.name}`); };
}
const { exports: e } = await WebAssembly.instantiate(m, imports);
const names = ["probe_read", "probe_read_missing", "probe_write", "probe_app_open"];
console.log(JSON.stringify(Object.fromEntries(names.map(n => [n, e[n]()]))));
"#;

#[test]
fn std_fs_is_unsupported_on_wasm32_in_node() {
    if let Err(why) = preflight(None) {
        return cm_evidence::tool_absent(why);
    }
    let out = out_dir().join("stdfs-probe");
    let src = out.join("crate");
    std::fs::create_dir_all(src.join("src")).unwrap();
    let cm_view = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cm-view");
    std::fs::write(
        src.join("Cargo.toml"),
        format!(
            "[package]\nname = \"cm-stdfs-probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             [lib]\ncrate-type = [\"cdylib\"]\n\
             [dependencies]\ncm-view = {{ path = {:?}, default-features = false }}\n\
             [workspace]\n",
            cm_view.canonicalize().unwrap()
        ),
    )
    .unwrap();
    std::fs::write(src.join("src/lib.rs"), STDFS_PROBE).unwrap();
    // The workspace's resolved versions, so the offline build needs nothing new.
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"),
        src.join("Cargo.lock"),
    )
    .unwrap();
    let target = out.join("target");
    let status = Command::new(env!("CARGO"))
        .current_dir(&src)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", out.join("build"))
        .args(["build", "--offline", "--target", "wasm32-unknown-unknown"])
        .status()
        .expect("cargo");
    assert!(status.success(), "probe build for wasm32 failed: {status}");
    let wasm = target.join("wasm32-unknown-unknown/debug/cm_stdfs_probe.wasm");
    let script = out.join("run.mjs");
    std::fs::write(&script, STDFS_RUN).unwrap();
    let mut node = Command::new("node");
    node.arg(&script).arg(&wasm);
    let out = match run_with_deadline(&mut node, NODE_LIMIT) {
        Ok(Run::Exited(out)) => out,
        Ok(other) => panic!("node run timed out: {other:?}"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return cm_evidence::tool_absent(format!("node: {e}"));
        }
        Err(e) => panic!("run node: {e}"),
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"{"probe_read":2,"probe_read_missing":2,"probe_write":2,"probe_app_open":2}"#
    );
}
