//! L-A: the longest line (`display.maxLine`) and the parser states its measurement
//! leaves. `tests/data/cm5-maxline-state.json` was recorded by
//! `oracle/cm5-maxline-state.mjs` on a real CodeMirror 5 editor (indentUnit 4).
//!
//! Each case builds an editor and runs its steps, one operation each. After
//! construction and after every step the test compares the rendered range,
//! `maxLine` and `maxLineLength`, the lines with a saved state (`stateAfter`), the
//! lines with current styles, and both frontiers. CM5 measures the longest line at
//! the end of an operation that changed it (endOperation_R2), and highlights it
//! there when it is outside the rendered range, so these sets show when that work
//! happens and when it does not. `fold` and `unfold` add and clear a collapsed
//! marker. The editor's line height is chosen so that the
//! constructor renders the range CM5's did.

use std::path::PathBuf;

use cm_core::text::from_u16;
use cm_core::{MarkOptions, Marker, Pos};
use cm_modes::Spec;
use cm_view::highlight::Styles;
use cm_view::{Editor, EditorOptions, Indent, Metrics};
use serde_json::{Value, json};

fn data() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-maxline-state.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-maxline-state.json")).unwrap()
}

fn sized(value: &str, mode: &str, h: f64) -> Editor {
    Editor::new(
        value,
        Spec::from(mode),
        EditorOptions {
            indent_unit: 4,
            ..EditorOptions::default()
        },
        Metrics::monospace(10.0, h),
    )
}

/// The line height at which the constructor renders CM5's range.
fn line_height_for(value: &str, mode: &str, view: (i64, i64)) -> f64 {
    let mut h = 20.0;
    while h < 200.0 {
        let cm = sized(value, mode, h);
        if (cm.display.view_from, cm.display.view_to) == view {
            return h;
        }
        h += 0.5;
    }
    panic!("no line height renders lines {view:?}");
}

/// The recorded state, from the Rust editor.
fn snap(cm: &Editor) -> Value {
    let hs = cm.hs.borrow();
    let (mut states, mut styled) = (Vec::new(), Vec::new());
    let first = cm.first_line();
    let mut n = first;
    cm.doc
        .each_line(first, first + cm.doc.line_count() as i64, |l| {
            if l.state_after().is_some() {
                states.push(n);
            }
            if l.styles()
                .and_then(|s| s.downcast::<Styles>().ok())
                .is_some_and(|s| s.mode_gen == hs.mode_gen)
            {
                styled.push(n);
            }
            n += 1;
            false
        });
    let (max_line, max_len) = cm
        .max_line()
        .map_or((json!(null), json!(0)), |(n, l)| (json!(n), json!(l)));
    json!({
        "view": [cm.display.view_from, cm.display.view_to],
        "maxLine": max_line,
        "maxLineLength": max_len,
        "states": states,
        "styled": styled,
        "highlightFrontier": hs.highlight_frontier,
        "modeFrontier": hs.mode_frontier,
        "lines": cm.doc.line_count(),
    })
}

/// The recorded snapshot without the fields the Rust editor does not report.
fn want(v: &Value) -> Value {
    let mut v = v.clone();
    v.as_object_mut().unwrap().remove("maxLineChanged");
    v
}

fn pos(v: &Value) -> Pos {
    Pos::new(v[0].as_i64().unwrap(), v[1].as_i64().unwrap())
}

fn run(cm: &mut Editor, marks: &mut Vec<Marker>, s: &Value) -> Option<String> {
    match s["op"].as_str().unwrap() {
        "replace" => {
            let (text, from, to) = (s["text"].as_str().unwrap(), pos(&s["from"]), pos(&s["to"]));
            cm.run_op(|cm| cm.doc.replace_range(text, from, Some(to), None));
        }
        // `makeChangeFromHistory` queues its own `scrollIntoView({from, to})`
        // (changes.js:144), applied when the operation ends.
        "undo" | "redo" => {
            let undo = s["op"] == "undo";
            cm.run_op(|cm| {
                if undo {
                    cm.doc.undo();
                } else {
                    cm.doc.redo();
                }
            });
        }
        "setValue" => {
            let text = s["text"].as_str().unwrap();
            cm.run_op(|cm| {
                cm.doc.set_value(text);
                cm.scroll_to(Some(0.0), Some(0.0));
            });
        }
        "option" => match s["name"].as_str().unwrap() {
            "tabSize" => cm.set_tab_size(s["value"].as_i64().unwrap()),
            "mode" => cm.set_mode(Spec::from(s["value"].as_str().unwrap())),
            "lineWrapping" => cm.set_line_wrapping(s["value"].as_bool().unwrap()),
            o => panic!("option {o}"),
        },
        "scroll" => {
            // `scrollTo(null, heightAtLine(line, "local"))`.
            let line = s["line"].as_i64().unwrap();
            let y = cm.height_at_line(line) + cm.metrics.padding_top;
            cm.scroll_to(None, Some(y));
        }
        "indent" => {
            let line = s["line"].as_i64().unwrap();
            cm.indent_line(line, Some(Indent::Smart), false);
            return Some(from_u16(&cm.line_text(line)));
        }
        "fold" => {
            let (from, to) = (pos(&s["from"]), pos(&s["to"]));
            let options = MarkOptions {
                collapsed: true,
                ..Default::default()
            };
            marks.push(cm.run_op(|cm| cm.doc.mark_text(from, to, options).unwrap()));
        }
        "unfold" => {
            let m = marks.pop().unwrap();
            cm.run_op(|_| m.clear());
        }
        op => panic!("step {op}"),
    }
    None
}

#[test]
fn maxline_state_matches_cm5() {
    let d = data();
    let mut diffs = Vec::new();
    let cases = d["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let value = case["value"].as_str().unwrap();
        let mode = case["mode"].as_str().unwrap();
        let v = &case["initial"]["view"];
        let h = line_height_for(
            value,
            mode,
            (v[0].as_i64().unwrap(), v[1].as_i64().unwrap()),
        );
        let mut cm = sized(value, mode, h);
        let mut marks = Vec::new();
        let mut check = |at: String, got: Value, want: Value| {
            if got != want {
                println!("{name} {at}:\n  cm5  {want}\n  rust {got}");
                diffs.push(format!("{name} {at}"));
            }
        };
        check("initial".into(), snap(&cm), want(&case["initial"]));
        for (i, s) in case["steps"].as_array().unwrap().iter().enumerate() {
            let result = run(&mut cm, &mut marks, s);
            let at = format!("step {i} ({})", s["op"].as_str().unwrap());
            if let Some(r) = result {
                check(format!("{at} result"), json!(r), s["result"].clone());
            }
            check(at, snap(&cm), want(&s["after"]));
        }
        // Repeated construction gives the same state.
        check(
            "repeat".into(),
            snap(&sized(value, mode, h)),
            want(&case["repeat"]),
        );
    }
    assert!(diffs.is_empty(), "differences from CM5: {diffs:#?}");
}
