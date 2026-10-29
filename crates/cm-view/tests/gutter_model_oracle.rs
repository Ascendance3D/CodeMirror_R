//! `setGutterMarker`, `clearGutter` and `lineInfo` against `tests/data/cm5-gutter-model.json`
//! (recorded by `oracle/cm5-gutter-model.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and
//! WebKit, which agree; issue #36). Each step is checked for its return value, its redraw
//! (`update` events of an editor on the document) and every line's `gutterMarkers`.
//!
//! The recording's markers are truthy objects (CM5 stores a DOM node); the falsy values
//! `null`, `undefined`, `0` and `""` are all `None` here. A fractional line number makes
//! `getLine` throw in CM5 and cannot be expressed with an integer line, so it is skipped.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use cm_core::{GutterMarker, LineHandle, LineSel, Pos};
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::{Value, json};

fn data() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-gutter-model.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-gutter-model.json")).unwrap()
}

fn shape(m: Option<Vec<(String, Option<GutterMarker>)>>) -> Value {
    match m {
        None => Value::Null,
        Some(m) => Value::Array(
            m.into_iter()
                .map(|(id, v)| json!([id, v.map(|_| "M")]))
                .collect(),
        ),
    }
}

fn sel(a: &Value, handles: &[LineHandle]) -> LineSel {
    match a.get("h") {
        Some(h) => LineSel::Handle(handles[h.as_u64().unwrap() as usize].clone()),
        None => LineSel::No(a.as_i64().unwrap()),
    }
}

#[test]
fn gutter_model_matches_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_object().unwrap();
    assert!(cases.len() >= 16, "recording lost cases: {}", cases.len());
    for (name, steps) in cases {
        let mut cm = Editor::new(
            "zero\none\ntwo\nthree",
            Spec::Null,
            EditorOptions::default(),
            Metrics::oracle(),
        );
        cm.set_size(600.0, 300.0);
        let updates = Rc::new(Cell::new(0u32));
        let u = updates.clone();
        cm.on_update(move |_| u.set(u.get() + 1));
        let handles: Vec<LineHandle> = (0..4).map(|n| cm.doc.get_line_handle(n).unwrap()).collect();
        for (i, s) in steps.as_array().unwrap().iter().enumerate() {
            let [op, a, b, c] = s["step"].as_array().unwrap().as_slice() else {
                panic!("{name}: bad step");
            };
            let here = format!("{name} step {i} {}", s["step"]);
            if s["err"] == "TypeError" {
                continue; // fractional line number
            }
            let before = updates.get();
            let mut ret = Value::Null;
            let mut info = None;
            match op.as_str().unwrap() {
                "set" => {
                    let value = (c == "N").then(|| GutterMarker::new(()));
                    let r = cm
                        .doc
                        .set_gutter_marker(sel(a, &handles), b.as_str().unwrap(), value);
                    ret = if r.is_some() {
                        json!("line")
                    } else {
                        json!("null")
                    };
                }
                "clear" => cm.doc.clear_gutter(a.as_str().unwrap()),
                "info" => {
                    info = Some(cm.doc.line_info(sel(a, &handles)).map_or(Value::Null, |i| {
                        json!({"line": i.line, "text": i.text, "markers": shape(i.gutter_markers),
                               "sameHandle": cm.doc.get_line_handle(i.line).as_ref() == Some(&i.handle)})
                    }));
                }
                "edit" => match a.as_str().unwrap() {
                    "delete_line1" => {
                        cm.doc
                            .replace_range("", Pos::new(0, 4), Some(Pos::new(1, 3)), None)
                    }
                    "append" => cm.doc.replace_range("x", Pos::new(1, 0), None, None),
                    _ => cm.doc.set_value("a\nb\nc\nd"),
                },
                other => panic!("{here}: op {other}"),
            }
            cm.fire_orphan_delayed();
            if matches!(op.as_str().unwrap(), "set" | "clear") {
                assert_eq!(ret, s["ret"], "{here}: return value");
            }
            assert_eq!(
                updates.get() - before,
                s["redraw"].as_u64().unwrap() as u32,
                "{here}: redraws"
            );
            if let Some(info) = info {
                let want = &s["info"];
                if want.is_null() {
                    assert!(info.is_null(), "{here}: lineInfo {info}");
                } else {
                    for k in ["line", "text", "markers", "sameHandle"] {
                        assert_eq!(info[k], want[k], "{here}: lineInfo.{k}");
                    }
                }
            }
            let lines: Vec<Value> = (0..4)
                .map(|n| match cm.doc.get_line_handle(n) {
                    Some(h) => shape(h.gutter_markers()),
                    None => json!("none"),
                })
                .collect();
            assert_eq!(Value::Array(lines), s["lines"], "{here}: gutterMarkers");
        }
    }
}
