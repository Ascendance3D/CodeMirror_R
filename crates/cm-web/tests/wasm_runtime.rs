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

use std::path::{Path, PathBuf};
use std::process::Command;

fn tool(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
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
    let lock = include_str!("../../../Cargo.lock");
    let want = lock
        .split("[[package]]")
        .find(|p| p.contains("name = \"wasm-bindgen\"\n"))
        .and_then(|p| p.lines().find_map(|l| l.strip_prefix("version = \"")))
        .map(|v| v.trim_end_matches('"').to_string())
        .expect("wasm-bindgen in Cargo.lock");
    match tool("wasm-bindgen", &["--version"]) {
        Some(v) if v.ends_with(&want) => {}
        Some(v) => {
            return cm_evidence::tool_absent(format!(
                "wasm-bindgen {want} (Cargo.lock) needed, found {v:?}"
            ));
        }
        None => return cm_evidence::tool_absent(format!("wasm-bindgen {want} not found")),
    }
    if tool("node", &["--version"]).is_none() {
        return cm_evidence::tool_absent("node not found");
    }
    let targets = tool("rustup", &["target", "list", "--installed"]).unwrap_or_default();
    if !targets.lines().any(|l| l == "wasm32-unknown-unknown") {
        return cm_evidence::tool_absent("rustup target wasm32-unknown-unknown not installed");
    }

    let pkg = build_pkg().expect("wasm package");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/web/node-run.mjs");
    let out = Command::new("timeout")
        .arg("120")
        .arg("node")
        .arg(&script)
        .arg(&pkg)
        .output()
        .expect("run node");
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
