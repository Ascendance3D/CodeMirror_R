//! P10-01: `mode/rust` on the editor. `tests/data/cm5-rust-editor.json` was recorded
//! by `oracle/cm5-rust-editor.mjs` on a real CodeMirror 5 editor (indentUnit 4):
//! `getLineTokens(line, true)`, `indentLine(line, "smart")` per line and over the
//! whole document, `getModeAt`, and a typed `}` (`triggerElectric` through the
//! simple mode's `electricInput` and `indent`).

use std::path::PathBuf;

use cm_core::Pos;
use cm_core::text::from_u16;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Indent, Metrics};
use serde_json::{Value, json};

fn data() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-rust-editor.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-rust-editor.json")).unwrap()
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

#[test]
fn rust_editor_documents_match_cm5() {
    let d = data();
    for doc in d["docs"].as_array().unwrap() {
        let name = doc["name"].as_str().unwrap();
        let value = doc["value"].as_str().unwrap();
        let mode = doc["mode"].as_str().unwrap();
        let cm = editor(value, mode);
        assert_eq!(cm.mode.name(), doc["modeName"].as_str().unwrap(), "{name}");
        assert_eq!(
            cm.get_mode_at(Pos::new(0, 0)).name(),
            doc["modeAt"].as_str().unwrap(),
            "{name}"
        );
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
fn rust_electric_input_matches_cm5() {
    let d = data();
    for c in d["electric"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let mut cm = editor(c["value"].as_str().unwrap(), "rust");
        let at = &c["at"];
        cm.doc.set_cursor(
            Pos::new(at[0].as_i64().unwrap(), at[1].as_i64().unwrap()),
            None,
        );
        cm.type_text("}");
        assert_eq!(cm.get_value(), c["value_after"].as_str().unwrap(), "{name}");
        let cur = cm.doc.get_cursor(None);
        assert_eq!(json!([cur.line, cur.ch]), c["cursor"], "{name} cursor");
    }
}
