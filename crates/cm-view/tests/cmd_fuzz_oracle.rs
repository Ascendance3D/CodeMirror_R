//! C5 differential oracle replay: editor commands.
//!
//! `oracle/cm5-cmd-ops.mjs` ran seeded sequences of commands, shifted motion, typing
//! (`applyTextInput`), copy/cut/paste and selection changes on a real CodeMirror 5
//! editor in headless chromium (DejaVu Sans Mono 32px / 38px lines, 900x500, no
//! wrapping) and recorded the editor state after each step. This test replays them
//! on `cm_view::Editor` with `Metrics::oracle()` and compares every step.
//!
//! Baseline: the private input `cm5-cmd-ops-baseline.json` (override with `CM_CMD_BASELINE`).
//! Start documents come from the private input `corpus-items` (read-only). Both are found and
//! verified by `cm_evidence::private_input` (Ascendance3D/X_ITE_R#168).
//! Known, explained differences: `tests/cmd-fuzz-known.tsv` (`seed<TAB>reason`).

use std::path::PathBuf;

use cm_core::{Pos, Sticky, clock};
use cm_modes::Spec;
use cm_view::editor::set_last_copied;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::{Value, json};

/// `hashText` in the oracle.
fn hash_text(s: &[u16]) -> Value {
    let lone = (0..s.len()).any(|i| {
        let c = s[i];
        if (0xd800..0xdc00).contains(&c) {
            !(i + 1 < s.len() && (0xdc00..0xe000).contains(&s[i + 1]))
        } else if (0xdc00..0xe000).contains(&c) {
            !(i > 0 && (0xd800..0xdc00).contains(&s[i - 1]))
        } else {
            false
        }
    });
    if s.len() <= 4000 && !lone {
        return Value::String(String::from_utf16(s).unwrap());
    }
    let mut h1: u32 = 0x811c9dc5;
    let mut h2: u32 = 0x01000193 ^ 0x5bd1e995;
    for &c in s {
        h1 = (h1 ^ c as u32).wrapping_mul(16777619);
        h2 = (h2 ^ c as u32).wrapping_mul(0x01000193 + 2);
    }
    Value::String(format!("#{}:{:08x}{:08x}", s.len(), h1, h2))
}

fn p(v: &Value) -> Pos {
    Pos::new(v[0].as_i64().unwrap(), v[1].as_i64().unwrap())
}

fn pj(p: Pos) -> Value {
    let st = match p.sticky {
        None => Value::Null,
        Some(Sticky::Before) => json!("before"),
        Some(Sticky::After) => json!("after"),
    };
    json!([p.line, p.ch, st])
}

fn snap(cm: &Editor) -> Vec<Value> {
    let sel = cm.doc.selection();
    let hs = cm.doc.history_size();
    vec![
        hash_text(&cm.doc.get_value_utf16(None)),
        Value::Array(
            sel.ranges
                .iter()
                .map(|r| json!([pj(r.anchor), pj(r.head)]))
                .collect(),
        ),
        json!(sel.primary),
        json!(hs.undo),
        json!(hs.redo),
        json!(cm.overwrite),
    ]
}

fn mode_spec(v: &Value) -> Spec {
    match v.as_str() {
        None => Spec::Null,
        Some(s) => Spec::from(s),
    }
}

#[test]
fn cmd_fuzz_oracle() {
    let path = match std::env::var("CM_CMD_BASELINE") {
        Ok(p) => PathBuf::from(p),
        Err(_) => {
            let Some(p) = cm_evidence::private_input("cm5-cmd-ops-baseline.json") else {
                return;
            };
            p
        }
    };
    let data: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("baseline")).unwrap();
    let known: Vec<i64> = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cmd-fuzz-known.tsv"),
    )
    .unwrap_or_default()
    .lines()
    .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    .map(|l| l.split('\t').next().unwrap().trim().parse().unwrap())
    .collect();
    let only: Option<i64> = std::env::var("CM_CMD_SEED")
        .ok()
        .map(|s| s.parse().unwrap());
    let seqs = data["sequences"].as_array().unwrap();
    let corpus = if seqs.iter().any(|q| q["init"]["path"].is_string()) {
        let Some(dir) = cm_evidence::private_input("corpus-items") else {
            return;
        };
        Some(dir)
    } else {
        None
    };
    let (mut nseq, mut nsteps, mut bad, mut known_bad) = (0, 0, Vec::new(), 0);
    for seq in seqs {
        let seed = seq["seed"].as_i64().unwrap();
        if only.is_some_and(|o| o != seed) {
            continue;
        }
        nseq += 1;
        let init = &seq["init"];
        let text = match init["path"].as_str() {
            Some(rel) => {
                let rel = rel
                    .strip_prefix("corpus-items/")
                    .unwrap_or_else(|| panic!("start document outside corpus-items: {rel}"));
                let file = corpus.as_ref().unwrap().join(rel);
                String::from_utf8(std::fs::read(file).unwrap()).unwrap()
            }
            None => init["text"].as_str().unwrap().to_string(),
        };
        let clk = clock::manual(1e12);
        set_last_copied(None);
        let mut cm = Editor::new(
            &text,
            mode_spec(&init["mode"]),
            EditorOptions::default(),
            Metrics::oracle(),
        );
        cm.display.window_height = Some(900.0);
        cm.set_size(900.0, 500.0);
        let mut clipboard = String::new();
        let ops = seq["ops"].as_array().unwrap();
        let outs = seq["out"].as_array().unwrap();
        for (k, o) in ops.iter().enumerate() {
            nsteps += 1;
            cm.catch_up_states(None, true);
            let mut extra: Option<Value> = None;
            match o["op"].as_str().unwrap() {
                "cmd" => {
                    cm.exec_command(o["name"].as_str().unwrap());
                }
                "shift" => {
                    cm.display.shift = true;
                    cm.exec_command(o["name"].as_str().unwrap());
                    cm.display.shift = false;
                }
                "type" => cm.apply_text_input(o["text"].as_str().unwrap(), 0, None),
                "select" => {
                    let head = if o["head"].is_null() {
                        None
                    } else {
                        Some(p(&o["head"]))
                    };
                    cm.doc.set_selection(p(&o["anchor"]), head, None);
                }
                "addSelection" => {
                    let head = if o["head"].is_null() {
                        None
                    } else {
                        Some(p(&o["head"]))
                    };
                    cm.doc.add_selection(p(&o["anchor"]), head, None);
                }
                "setSelections" => {
                    let rs: Vec<(Pos, Option<Pos>)> = o["ranges"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| {
                            (
                                p(&r[0]),
                                Some(if r[1].is_null() { p(&r[0]) } else { p(&r[1]) }),
                            )
                        })
                        .collect();
                    cm.doc
                        .set_selections(&rs, Some(o["primary"].as_u64().unwrap() as usize), None);
                }
                "extending" => cm.doc.set_extending(o["val"].as_bool().unwrap()),
                "tick" => clk.set(clk.get() + o["ms"].as_f64().unwrap()),
                "copy" | "cut" => {
                    let t = if o["op"] == "copy" {
                        cm.copy()
                    } else {
                        cm.cut()
                    };
                    if let Some(t) = &t {
                        clipboard = t.clone();
                    }
                    extra = Some(match t {
                        Some(t) => hash_text(&t.encode_utf16().collect::<Vec<u16>>()),
                        None => Value::Null,
                    });
                }
                "paste" => {
                    let t = match o["text"].as_str() {
                        Some(t) => t.to_string(),
                        None => clipboard.clone(),
                    };
                    cm.paste(&t);
                }
                other => panic!("unknown op {other}"),
            }
            let mut got = snap(&cm);
            if let Some(e) = extra {
                if !e.is_null() {
                    got.push(e);
                }
            }
            let want = outs[k].as_array().unwrap();
            if want.iter().any(|v| v.get("err").is_some()) {
                eprintln!(
                    "seed {seed} step {k}: JS threw {:?}; skipping rest",
                    want.last()
                );
                break;
            }
            if Value::Array(got.clone()) != Value::Array(want.clone()) {
                let short = |v: &Value| {
                    let s = v.to_string();
                    if s.len() > 600 {
                        format!("{}…", &s[..600])
                    } else {
                        s
                    }
                };
                let msg = format!(
                    "seed {seed} ({}) step {k} op {}\n  prev: {}\n  want: {}\n  got:  {}",
                    init["path"].as_str().unwrap_or("random"),
                    o,
                    if k > 0 {
                        short(&Value::Array(outs[k - 1].as_array().unwrap()[1..].to_vec()))
                    } else {
                        String::new()
                    },
                    short(&Value::Array(want.clone())),
                    short(&Value::Array(got))
                );
                if only.is_some() {
                    for l in cm.doc.first_line()..=cm.doc.last_line() {
                        eprintln!("line {l}: {:?}", String::from_utf16_lossy(&cm.line_text(l)));
                    }
                }
                if known.contains(&seed) {
                    known_bad += 1;
                } else {
                    bad.push(msg);
                }
                break;
            }
        }
        clock::set_override(None);
    }
    eprintln!(
        "cmd fuzz: {nseq} sequences, {nsteps} steps, {} mismatching sequences ({known_bad} known)",
        bad.len()
    );
    for m in bad.iter().take(15) {
        eprintln!("{m}");
    }
    assert!(bad.is_empty(), "{} mismatching sequences", bad.len());
}
