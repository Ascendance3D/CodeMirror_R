//! Runs `oracle/server-utils.test.mjs` (issue #64): the oracle static servers' `safeResolve`
//! rejects `..` traversal, sibling-prefix directories and symlink escapes, and answers a
//! malformed URI with 400. Needs `node`; without it the run records `tool-absent`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn oracle_server_safe_resolve() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = match Command::new("node")
        .arg("oracle/server-utils.test.mjs")
        .current_dir(&root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return cm_evidence::tool_absent(format!("node: {e}")),
    };
    assert!(
        out.status.success(),
        "server-utils.test.mjs failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
