//! `runMode`'s `addModeClass` prefix and per-mode `flattenSpans` override (issue #60,
//! src/line/highlight.js:208-245) against `tests/data/cm5-mode-class.json`, recorded by
//! `oracle/cm5-mode-class.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and WebKit
//! (identical output). Each line's style array (`line.styles` without the mode
//! generation) and `styleClasses` are compared.
//!
//! The recording sets the mode property with `CodeMirror.extendMode` partway through;
//! the replay applies the same extension to its registry before the cases that follow
//! it in the recording.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use cm_core::Doc;
use cm_modes::{ConfigValue, Registry, Spec};
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::{Value, json};

fn data() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-mode-class.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-mode-class.json")).unwrap()
}

/// Cases recorded after `extendMode("css", {flattenSpans: false})` and
/// `extendMode("xml", {flattenSpans: true})`.
const EXTENDED: &[&str] = &[
    "css_mode_flatten_false",
    "xml_mode_flatten_true_option_off",
    "css_mode_flatten_false_add_mode_class",
];

fn registry(extended: bool) -> Rc<Registry> {
    let mut r = Registry::with_all_modes();
    if extended {
        r.extend_mode(
            "css",
            HashMap::from([("flattenSpans".to_string(), ConfigValue::Bool(false))]),
        );
        r.extend_mode(
            "xml",
            HashMap::from([("flattenSpans".to_string(), ConfigValue::Bool(true))]),
        );
    }
    Rc::new(r)
}

fn styles_of(cm: &Editor, n: i64) -> Value {
    let st = cm.line_styles(n);
    let mut out = Vec::new();
    for (end, style) in &st.spans {
        out.push(json!(end));
        out.push(match style {
            Some(s) => json!(&**s),
            None => Value::Null,
        });
    }
    let classes = match &st.classes {
        None => Value::Null,
        Some((bg, text)) => json!({"bgClass": bg, "textClass": text}),
    };
    json!({"styles": out, "classes": classes})
}

#[test]
fn run_mode_options_match_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_object().unwrap();
    assert!(cases.len() >= 9, "recording lost cases: {}", cases.len());
    for (name, case) in cases {
        let o = &case["options"];
        let options = EditorOptions {
            add_mode_class: o["addModeClass"].as_bool().unwrap_or(false),
            flatten_spans: o["flattenSpans"].as_bool().unwrap_or(true),
            ..EditorOptions::default()
        };
        let mode = Spec::from(o["mode"].as_str().unwrap());
        let doc = Doc::new(case["text"].as_str().unwrap());
        let cm = Editor::with_doc(
            doc,
            mode,
            options,
            Metrics::oracle(),
            registry(EXTENDED.contains(&name.as_str())),
        );
        for (n, want) in case["lines"].as_array().unwrap().iter().enumerate() {
            assert_eq!(&styles_of(&cm, n as i64), want, "{name} line {n}");
        }
    }
}

/// `setOption("addModeClass")` / `setOption("flattenSpans")` reset the mode state
/// (src/edit/options.js:151-152), so cached styles are recomputed with the new value.
#[test]
fn set_option_resets_cached_styles() {
    let d = data();
    let plain = &d["cases"]["xml_flatten_option_on"]["lines"][0];
    let unflat = &d["cases"]["xml_flatten_option_off"]["lines"][0];
    let mut cm = Editor::with_doc(
        Doc::new("<a></a><b/>"),
        Spec::from("xml"),
        EditorOptions::default(),
        Metrics::oracle(),
        registry(false),
    );
    assert_eq!(&styles_of(&cm, 0), plain);
    cm.set_flatten_spans(false);
    assert_eq!(&styles_of(&cm, 0), unflat);
    cm.set_flatten_spans(true);
    cm.set_add_mode_class(true);
    let got = styles_of(&cm, 0);
    assert_eq!(got["styles"][1], "m-xml tag bracket");
    cm.set_add_mode_class(false);
    assert_eq!(&styles_of(&cm, 0), plain);
}
