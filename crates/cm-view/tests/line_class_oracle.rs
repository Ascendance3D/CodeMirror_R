//! `addLineClass`/`removeLineClass` against `tests/data/cm5-line-class.json` (recorded by
//! `oracle/cm5-line-class.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and WebKit, which
//! agree on every model case; issue #35).
//!
//! CM5 builds `new RegExp("(^|\\s)" + cls + "(?:$|\\s)\\s*")` from the class unescaped, so a
//! class name with regular-expression syntax matches as a pattern, an invalid pattern throws
//! a `SyntaxError` (only when the property is not empty) and an empty class is not a no-op:
//! it is stored on an empty property and appended to a non-empty one. The recording also
//! counts the editor's `update` events per step, so a redraw is checked, not only the value.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use cm_core::LineWhere;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::Value;

fn data() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-line-class.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-line-class.json")).unwrap()
}

/// The recorded property: `"undefined"` and `null` are both an unset property.
fn prop(v: &Value) -> Option<String> {
    v.as_str().filter(|s| *s != "undefined").map(str::to_string)
}

/// Applies one step; `Err` carries the `SyntaxError` of an invalid class pattern.
fn apply(cm: &Editor, op: &str, place: LineWhere, cls: &Value) -> Result<(), String> {
    match op {
        "add" => cm
            .doc
            .add_line_class(0, place, cls.as_str().unwrap())
            .map(|_| ()),
        _ => cm.doc.remove_line_class(0, place, cls.as_str()).map(|_| ()),
    }
    .map_err(|e| e.to_string())
}

#[test]
fn model_cases_match_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["model"].as_object().unwrap();
    assert!(cases.len() >= 29, "recording lost cases: {}", cases.len());
    for (name, steps) in cases {
        let mut cm = Editor::new(
            "one\ntwo",
            Spec::Null,
            EditorOptions::default(),
            Metrics::oracle(),
        );
        cm.set_size(600.0, 300.0);
        let updates = Rc::new(Cell::new(0u32));
        let u = updates.clone();
        cm.on_update(move |_| u.set(u.get() + 1));
        for (i, s) in steps.as_array().unwrap().iter().enumerate() {
            let [op, place, cls] = s["step"].as_array().unwrap().as_slice() else {
                panic!("{name}: bad step");
            };
            let here = format!("{name} step {i} {}", s["step"]);
            let before = updates.get();
            let got = apply(
                &cm,
                op.as_str().unwrap(),
                LineWhere::parse(place.as_str().unwrap()),
                cls,
            );
            cm.fire_orphan_delayed();
            assert_eq!(
                got.is_err(),
                s["err"] == "SyntaxError",
                "{here}: error {got:?}, CM5 {}",
                s["err"]
            );
            assert_eq!(
                updates.get() - before,
                s["redraw"].as_u64().unwrap() as u32,
                "{here}: redraws"
            );
            let h = cm.doc.get_line_handle(0).unwrap();
            assert_eq!(h.text_class(), prop(&s["text"]), "{here}: textClass");
            assert_eq!(h.bg_class(), prop(&s["bg"]), "{here}: bgClass");
            assert_eq!(h.wrap_class(), prop(&s["wrap"]), "{here}: wrapClass");
            assert_eq!(h.gutter_class(), prop(&s["gutter"]), "{here}: gutterClass");
        }
    }
}
