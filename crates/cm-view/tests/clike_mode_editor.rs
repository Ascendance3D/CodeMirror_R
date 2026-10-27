//! P10-07: `mode/clike` on the editor. `tests/data/cm5-clike-editor.json` was recorded
//! by `oracle/cm5-clike-editor.mjs` on a real CodeMirror 5 editor (indentUnit 4):
//! `getLineTokens(line, true)`, `indentLine(line, "smart")` per line and over the
//! whole document, `getModeAt`, `getHelpers(pos, "hintWords")`, and typed keys
//! (`triggerElectric` through clike's `electricInput` and `indent`, one key at a time).
//!
//! `indentLine(n, "smart")` starts from a saved state when `findStartLine` finds one
//! within 100 lines, else from the least indented line with a fresh state, so the
//! result depends on which lines the constructor rendered, and on the longest line,
//! which the constructor measures and, outside that range, highlights
//! (`tests/maxline_state.rs`). The recorder writes CM5's
//! rendered range (`view`); [`sized_editor`] picks a line height that renders the
//! same range and checks it.

use std::path::PathBuf;

use cm_core::Pos;
use cm_core::text::from_u16;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Indent, Metrics};
use serde_json::{Value, json};

fn data() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-clike-editor.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-clike-editor.json")).unwrap()
}

fn editor(value: &str, mode: &str) -> Editor {
    Editor::new(
        value,
        Spec::from(mode),
        EditorOptions {
            indent_unit: 4,
            ..EditorOptions::default()
        },
        Metrics::test_grid(),
    )
}

/// The line height at which the constructor renders the lines `0..view_to`, as CM5's
/// did (the text metrics do not reach the mode).
fn line_height_for(value: &str, mode: &str, view_to: i64) -> f64 {
    let mut h = 20.0;
    while h < 200.0 {
        let cm = sized(value, mode, h);
        if cm.display.view_from == 0 && cm.display.view_to == view_to {
            return h;
        }
        h += 0.5;
    }
    panic!("no line height renders lines 0..{view_to}");
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

#[test]
fn clike_editor_documents_match_cm5() {
    let d = data();
    for doc in d["docs"].as_array().unwrap() {
        let name = doc["name"].as_str().unwrap();
        let value = doc["value"].as_str().unwrap();
        let mode = doc["mode"].as_str().unwrap();
        let view_to = doc["view"][1].as_i64().unwrap();
        let h = line_height_for(value, mode, view_to);
        let editor = |v: &str, m: &str| sized(v, m, h);
        let cm = editor(value, mode);
        assert_eq!(cm.mode.name(), doc["modeName"].as_str().unwrap(), "{name}");
        assert_eq!(
            cm.get_mode_at(Pos::new(0, 0)).name(),
            doc["modeAt"].as_str().unwrap(),
            "{name}"
        );
        let hints: Vec<Value> = cm
            .get_helpers(Pos::new(0, 0), "hintWords")
            .iter()
            .map(|h| json!(h.downcast_ref::<Vec<String>>().expect("hintWords list")))
            .collect();
        assert_eq!(&Value::Array(hints), &doc["hintWords"], "{name} hintWords");
        let want = doc["tokens"].as_array().unwrap();
        assert_eq!(cm.doc.line_count(), want.len(), "{name}");
        for (i, w) in want.iter().enumerate() {
            let got: Vec<Value> = cm
                .get_line_tokens(i as i64, true)
                .iter()
                .map(|t| json!([t.start, t.end, t.string, t.ty]))
                .collect();
            assert_eq!(&Value::Array(got), w, "{name} tokens of line {i}");
        }
        for (i, w) in doc["each"].as_array().unwrap().iter().enumerate() {
            let mut cm = editor(value, mode);
            cm.indent_line(i as i64, Some(Indent::Smart), false);
            assert_eq!(
                from_u16(&cm.line_text(i as i64)),
                w.as_str().unwrap(),
                "{name} indentLine({i}, smart)"
            );
        }
        let mut cm = editor(value, mode);
        for i in 0..cm.doc.line_count() {
            cm.indent_line(i as i64, Some(Indent::Smart), false);
        }
        assert_eq!(cm.get_value(), doc["all"].as_str().unwrap(), "{name} all");
    }
}

#[test]
fn clike_electric_input_matches_cm5() {
    let d = data();
    for c in d["electric"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let mut cm = editor(c["value"].as_str().unwrap(), c["mode"].as_str().unwrap());
        let at = &c["at"];
        cm.doc.set_cursor(
            Pos::new(at[0].as_i64().unwrap(), at[1].as_i64().unwrap()),
            None,
        );
        for ch in c["keys"].as_str().unwrap().chars() {
            cm.type_text(&ch.to_string());
        }
        assert_eq!(cm.get_value(), c["value_after"].as_str().unwrap(), "{name}");
        let cur = cm.doc.get_cursor(None);
        assert_eq!(json!([cur.line, cur.ch]), c["cursor"], "{name} cursor");
    }
}
