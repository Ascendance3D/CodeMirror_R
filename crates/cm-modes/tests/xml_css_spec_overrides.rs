//! Spec overrides of the xml dictionaries (issue #59: `autoSelfClosers`,
//! `implicitlyClosed`, `contextGrabbers`, `doNotIndent`, mode/xml/xml.js:14-60, merged
//! as `config[prop] = config_[prop]`) and of css `tokenHooks` (mode/css/css.js:19,
//! :42-45, merged by `resolveMode` over a MIME config) against
//! `tests/data/cm5-xml-css-spec-overrides.json`, recorded by
//! `oracle/cm5-xml-css-spec-overrides.mjs` on CodeMirror 5.65.21 in Chromium, Firefox
//! and WebKit (identical output). Tokens (text and style) of every line, and for xml
//! `indent(state, "")` and `indent(state, after)` at each line end. The css hooks port
//! the recording's `HOOKS` one for one.

use std::path::PathBuf;
use std::rc::Rc;

use cm_core::StringStream;
use cm_modes::css::{CssState, HookFn, HookResult, TokenHooks};
use cm_modes::xml::{grabber_dict, tag_dict};
use cm_modes::{
    ConfigValue, JsArg, Mode, ModeOptions, ModeSpec, RegExp, Registry, Spec, State, start_state,
};
use serde_json::{Value, json};

fn data() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/cm5-xml-css-spec-overrides.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-xml-css-spec-overrides.json"))
        .unwrap()
}

fn opts() -> ModeOptions {
    ModeOptions {
        indent_unit: 2,
        ..ModeOptions::default()
    }
}

fn tokens(mode: &Rc<dyn Mode>, state: &mut State, line: &str) -> Value {
    let text: Vec<u16> = line.encode_utf16().collect();
    let mut stream = StringStream::new(&text, Some(4), None);
    let mut toks = Vec::new();
    while !stream.eol() {
        let style = mode.token(&mut stream, state);
        toks.push(json!([stream.current_string(), style]));
        stream.start = stream.pos;
    }
    Value::Array(toks)
}

fn keys(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect()
}

fn xml_spec(v: &Value) -> Spec {
    let mut spec = ModeSpec::named("xml");
    for (k, v) in v.as_object().unwrap() {
        let cv = match (k.as_str(), v) {
            ("contextGrabbers", Value::Array(a)) => {
                let entries: Vec<(&str, Vec<&str>)> = a
                    .iter()
                    .map(|e| (e[0].as_str().unwrap(), keys(&e[1])))
                    .collect();
                let refs: Vec<(&str, &[&str])> =
                    entries.iter().map(|(t, k)| (*t, k.as_slice())).collect();
                grabber_dict(&refs)
            }
            (_, Value::Array(_)) => tag_dict(&keys(v)),
            (_, Value::Bool(b)) => ConfigValue::Bool(*b),
            _ => panic!("xml option {k}: {v}"),
        };
        spec = spec.with(k, cv);
    }
    Spec::Obj(spec)
}

fn indent(mode: &Rc<dyn Mode>, state: &State, after: &str) -> Value {
    let a: Vec<u16> = after.encode_utf16().collect();
    mode.indent(state, &a, &[])
        .map_or(json!("Pass"), |i| json!(i as i64))
}

#[test]
fn xml_dictionary_overrides_match_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["xml"].as_object().unwrap();
    assert!(cases.len() >= 10, "recording lost cases: {}", cases.len());
    let registry = Registry::with_all_modes();
    for (name, case) in cases {
        let mode = registry.get_mode(&opts(), &xml_spec(&case["spec"]));
        let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        let after = case["after"].as_str().unwrap();
        for rec in case["lines"].as_array().unwrap() {
            let line = rec["line"].as_str().unwrap();
            assert_eq!(
                tokens(&mode, &mut state, line),
                rec["toks"],
                "{name} {line:?}"
            );
            assert_eq!(
                indent(&mode, &state, ""),
                rec["indent"],
                "{name} {line:?}: indent"
            );
            assert_eq!(
                indent(&mode, &state, after),
                rec["indentAfter"],
                "{name} {line:?}: indent({after:?})"
            );
        }
    }
}

fn pair(a: &str, b: &str) -> HookResult {
    HookResult::Pair(Some(a.into()), Some(b.into()))
}

/// The recording's `HOOKS`.
fn hook(name: &str) -> HookFn {
    match name {
        "pct" => {
            let re = RegExp::new("[\\w-]", "").unwrap();
            Rc::new(move |s: &mut StringStream<'_>, _: &mut CssState| {
                s.eat_while(|c| re.test(&[c]));
                pair("variable-3", "variable")
            })
        }
        "slash" => Rc::new(|s: &mut StringStream<'_>, _: &mut CssState| {
            if s.eat(b'/' as u16).is_some() {
                s.skip_to_end();
                return pair("comment", "comment");
            }
            HookResult::False
        }),
        "tilde" => Rc::new(|_: &mut StringStream<'_>, _: &mut CssState| {
            HookResult::Style(Some("keyword".into()))
        }),
        "amp" => Rc::new(|_: &mut StringStream<'_>, _: &mut CssState| HookResult::Style(None)),
        "caret" => Rc::new(|_: &mut StringStream<'_>, _: &mut CssState| HookResult::Undefined),
        _ => panic!("hook {name}"),
    }
}

#[test]
fn css_token_hooks_from_spec_match_cm5() {
    let d = data();
    let cases = d["css"].as_object().unwrap();
    assert!(cases.len() >= 4, "recording lost cases: {}", cases.len());
    let registry = Registry::with_all_modes();
    for (name, case) in cases {
        let hooks = TokenHooks(
            case["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| {
                    let ch = h[0].as_str().unwrap().encode_utf16().next().unwrap();
                    (ch, hook(h[1].as_str().unwrap()))
                })
                .collect(),
        );
        let spec = ModeSpec::named(case["spec"]["name"].as_str().unwrap())
            .with("tokenHooks", ConfigValue::Obj(Rc::new(hooks)));
        let mode = registry.get_mode(&opts(), &Spec::Obj(spec));
        let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        for rec in case["lines"].as_array().unwrap() {
            let line = rec["line"].as_str().unwrap();
            let got = tokens(&mode, &mut state, line);
            // The recording writes an undefined style as "undefined"; Rust has `None`.
            let want: Vec<Value> = rec["toks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    if t[1] == "undefined" {
                        json!([t[0], null])
                    } else {
                        t.clone()
                    }
                })
                .collect();
            assert_eq!(got, Value::Array(want), "{name} {line:?}");
        }
    }
}
