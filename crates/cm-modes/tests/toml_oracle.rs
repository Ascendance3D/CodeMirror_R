//! C3 oracle: `runMode` + `mode/toml` must reproduce every callback of
//! `oracle/cm5-toml-baseline.json` (from `oracle/cm5-toml-tokens.mjs`, which runs
//! the original `addon/runmode/runmode.node.js`) exactly: text, style, line, start.

use std::path::PathBuf;

use cm_core::text::to_u16;
use cm_modes::{Registry, Spec, run_mode_utf16};
use serde_json::{Value, json};

/// A JSON string, or an array of UTF-16 code units for ill-formed text.
fn units(v: &Value) -> Vec<u16> {
    match v {
        Value::String(s) => to_u16(s),
        Value::Array(a) => a.iter().map(|x| x.as_u64().unwrap() as u16).collect(),
        other => panic!("bad text {other}"),
    }
}

fn enc(u: &[u16]) -> Value {
    match String::from_utf16(u) {
        Ok(s) => Value::String(s),
        Err(_) => Value::Array(u.iter().map(|&c| json!(c)).collect()),
    }
}

#[test]
fn toml_tokens_match_cm5_runmode() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../oracle/cm5-toml-baseline.json");
    let base: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("toml baseline")).unwrap();
    let registry = Registry::with_all_modes();
    let cases = base["cases"].as_array().unwrap();
    let mut callbacks = 0;
    let mut bad = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let text = units(&c["text"]);
        let spec = Spec::from(c["spec"].as_str().unwrap());
        let mut got = Vec::new();
        run_mode_utf16(
            &registry,
            &text,
            &spec,
            |t| {
                got.push(match t.line {
                    None => json!(["\n"]),
                    Some(line) => json!([enc(&t.text), t.style, line, t.start]),
                })
            },
            None,
        );
        callbacks += got.len();
        let want = c["tokens"].as_array().unwrap();
        if &got != want {
            let k = got
                .iter()
                .zip(want)
                .position(|(a, b)| a != b)
                .unwrap_or(got.len().min(want.len()));
            bad.push(format!(
                "case {i} ({}): first difference at callback {k}: js {:?} rust {:?}",
                c["kind"],
                want.get(k),
                got.get(k)
            ));
        }
    }
    for b in &bad {
        println!("{b}");
    }
    println!(
        "toml oracle: {} inputs, {callbacks} callbacks, {} mismatching inputs",
        cases.len(),
        bad.len()
    );
    assert!(bad.is_empty());
}
