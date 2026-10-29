//! javascript mode parser options and mode-object functions (issue #58:
//! mode/javascript/javascript.js:279 `register` with `parserConfig.globalVars`, :858
//! `expressionAllowed`, :867 `startState` with `parserConfig.localVars` / `globalVars`,
//! :940 `skipExpression`) against `tests/data/cm5-javascript-config.json`, recorded by
//! `oracle/cm5-javascript-config.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and
//! WebKit (identical output).
//!
//! Each case tokenizes its lines with a fresh mode and state. Every token is checked
//! for its text, style and `expressionAllowed(stream, state)` with and without a
//! backUp of 1; every line end for `state.localVars`, `state.globalVars`,
//! `state.context` and `indent(state, "")`. A line "@skip" is `skipExpression(state)`.

use std::path::PathBuf;

use cm_core::StringStream;
use cm_modes::javascript::{JsState, var_list};
use cm_modes::{ConfigValue, JsArg, ModeOptions, ModeSpec, Registry, Spec, start_state};
use serde_json::{Value, json};

fn data() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-javascript-config.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-javascript-config.json")).unwrap()
}

fn spec_of(v: &Value) -> Spec {
    let o = v.as_object().unwrap();
    let mut spec = ModeSpec::named(o["name"].as_str().unwrap());
    for (k, v) in o {
        if k == "name" {
            continue;
        }
        let cv = match v {
            Value::Bool(b) => ConfigValue::Bool(*b),
            Value::Array(a) => {
                let names: Vec<&str> = a.iter().map(|n| n.as_str().unwrap()).collect();
                var_list(&names)
            }
            _ => panic!("option {k}: {v}"),
        };
        spec = spec.with(k, cv);
    }
    Spec::Obj(spec)
}

fn names(v: Vec<Option<String>>) -> Value {
    Value::Array(
        v.into_iter()
            .map(|n| n.map_or(Value::Null, |n| json!(n)))
            .collect(),
    )
}

#[test]
fn javascript_parser_config_matches_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_object().unwrap();
    assert!(cases.len() >= 11, "recording lost cases: {}", cases.len());
    let registry = Registry::with_all_modes();
    let opts = ModeOptions {
        indent_unit: 2,
        ..ModeOptions::default()
    };
    let mut tokens_checked = 0;
    for (name, case) in cases {
        let mode = registry.get_mode(&opts, &spec_of(&case["spec"]));
        let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        for rec in case["lines"].as_array().unwrap() {
            let line = rec["line"].as_str().unwrap();
            let mut toks = Vec::new();
            if line == "@skip" {
                assert!(mode.skip_expression(&mut state), "{name}: skipExpression");
            } else {
                let text: Vec<u16> = line.encode_utf16().collect();
                let mut stream = StringStream::new(&text, Some(4), None);
                while !stream.eol() {
                    let style = mode.token(&mut stream, &mut state);
                    let ea = mode.expression_allowed(&stream, &state, 0).unwrap();
                    let ea1 = mode.expression_allowed(&stream, &state, 1).unwrap();
                    toks.push(json!([stream.current_string(), style, ea, ea1]));
                    stream.start = stream.pos;
                    tokens_checked += 1;
                }
            }
            assert_eq!(Value::Array(toks), rec["toks"], "{name} {line:?}: tokens");
            let st = state.get::<JsState>();
            assert_eq!(
                names(st.local_var_names()),
                rec["localVars"],
                "{name} {line:?}: localVars"
            );
            assert_eq!(
                st.global_var_names().map_or(Value::Null, names),
                rec["globalVars"],
                "{name} {line:?}: globalVars"
            );
            assert_eq!(
                json!(st.has_context()),
                rec["context"],
                "{name} {line:?}: context"
            );
            let ind = mode
                .indent(&state, &[], &[])
                .map_or(json!("Pass"), |i| json!(i));
            assert_eq!(
                ind.as_f64().map(|f| json!(f as i64)).unwrap_or(ind),
                rec["indent"],
                "{name} {line:?}: indent"
            );
        }
    }
    assert!(tokens_checked >= 150, "{tokens_checked} tokens");
}

/// A mode without the properties: `expressionAllowed` is absent (`None`) and
/// `skipExpression` does nothing.
#[test]
fn other_modes_lack_js_functions() {
    let registry = Registry::with_all_modes();
    let mode = registry.get_mode(&ModeOptions::default(), &Spec::from("css"));
    let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
    let text: Vec<u16> = "a".encode_utf16().collect();
    let stream = StringStream::new(&text, Some(4), None);
    assert_eq!(mode.expression_allowed(&stream, &state, 0), None);
    assert!(!mode.skip_expression(&mut state));
}
