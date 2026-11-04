//! javascript `parserConfig.wordCharacters` (issue #58: `wordRE` in
//! mode/javascript/javascript.js:21, read by tokenBase at :105, :122-123 and by
//! findFatArrow at :204) against `tests/data/cm5-javascript-word-chars.json`, recorded by
//! `oracle/cm5-javascript-word-chars.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and
//! WebKit (identical output): every token's text and style, and each line's
//! `indent(state, "")`. A string in the recorded spec is a regexp source (`""` is the
//! empty regexp, which matches every character); `false` is falsy, so the default
//! applies.

use std::path::PathBuf;
use std::rc::Rc;

use cm_core::StringStream;
use cm_modes::{ConfigValue, JsArg, ModeOptions, ModeSpec, RegExp, Registry, Spec, start_state};
use serde_json::{Value, json};

fn data() -> Value {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-javascript-word-chars.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-javascript-word-chars.json"))
        .unwrap()
}

fn spec_of(v: &Value) -> Spec {
    let o = v.as_object().unwrap();
    let mut spec = ModeSpec::named(o["name"].as_str().unwrap());
    for (k, v) in o {
        let cv = match (k.as_str(), v) {
            ("name", _) => continue,
            ("wordCharacters", Value::String(s)) => {
                ConfigValue::Obj(Rc::new(RegExp::new(s, "").unwrap()))
            }
            (_, Value::Bool(b)) => ConfigValue::Bool(*b),
            _ => panic!("option {k}: {v}"),
        };
        spec = spec.with(k, cv);
    }
    Spec::Obj(spec)
}

#[test]
fn javascript_word_characters_match_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_object().unwrap();
    assert!(cases.len() >= 9, "recording lost cases: {}", cases.len());
    let registry = Registry::with_all_modes();
    let opts = ModeOptions {
        indent_unit: 2,
        ..ModeOptions::default()
    };
    let mut tokens = 0;
    for (name, case) in cases {
        let mode = registry.get_mode(&opts, &spec_of(&case["spec"]));
        let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        for rec in case["lines"].as_array().unwrap() {
            let line = rec["line"].as_str().unwrap();
            let text: Vec<u16> = line.encode_utf16().collect();
            let mut stream = StringStream::new(&text, Some(4), None);
            let mut toks = Vec::new();
            while !stream.eol() {
                let style = mode.token(&mut stream, &mut state);
                toks.push(json!([stream.current_string(), style]));
                stream.start = stream.pos;
                tokens += 1;
            }
            assert_eq!(Value::Array(toks), rec["toks"], "{name} {line:?}");
            let ind = mode
                .indent(&state, &[], &[])
                .map_or(json!("Pass"), |i| json!(i as i64));
            assert_eq!(ind, rec["indent"], "{name} {line:?}: indent");
        }
    }
    assert!(tokens >= 300, "{tokens} tokens");
}
