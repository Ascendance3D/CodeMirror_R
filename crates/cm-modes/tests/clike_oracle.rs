//! P10-07 oracle: `mode/clike/clike.js` must reproduce `tests/data/cm5-clike-mode.json`
//! (written by `oracle/cm5-clike-mode.mjs` from a built pinned tree): every `def(...)`
//! configuration and its `hintWords`, the mode object's properties per MIME, mode and
//! MIME resolution, `electricInput`, per-line runs (state before and after, indent,
//! tokens from `readToken`, the "failed to advance" error) with and without a
//! `copyState` at each line start, restarts from saved state copies, and `runMode`.
//!
//! The recorder loads clike.js afresh for the runs and for the runMode inputs (Ceylon's
//! `stringTokenizer` lives in its module scope); each of those tests uses one fresh
//! [`Registry`] and replays the recorded order.

mod common;

use std::collections::HashMap;
use std::rc::Rc;

use cm_core::util::string_stream::StringStream;
use cm_modes::clike::{ClikeState, CtxType, Hooks, Info, Predicate, Tokenize, Words};
use cm_modes::meta::{ModeInfo, ModeInfoList};
use cm_modes::{
    ConfigValue, JsArg, LinesOracle, Mode, ModeOptions, RegExp, Registry, Spec, State, copy_state,
    read_token, run_mode_utf16, split_lines_runmode, start_state,
};
use common::{enc, spec_from_json, units};
use serde_json::{Map, Value, json};

fn data() -> Value {
    let path = common::root().join("crates/cm-modes/tests/data/cm5-clike-mode.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-clike-mode.json")).unwrap()
}

fn spec_of(v: &Value) -> Spec {
    spec_from_json(v, &|s| panic!("no special spec {s}"))
}

/// A JS number as JSON: integral values print without a fraction, as in JS.
fn num(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

fn flags(re: &RegExp) -> String {
    let f = re.flags();
    let mut s = String::new();
    // JS orders `flags` as "dgimsuvy".
    for (on, c) in [
        (f.global, 'g'),
        (f.ignore_case, 'i'),
        (f.multiline, 'm'),
        (f.dot_all, 's'),
        (f.unicode, 'u'),
        (f.sticky, 'y'),
    ] {
        if on {
            s.push(c);
        }
    }
    s
}

fn config_json(v: &ConfigValue) -> Value {
    match v {
        ConfigValue::Null => Value::Null,
        ConfigValue::Bool(b) => json!(b),
        ConfigValue::Num(n) => num(*n),
        ConfigValue::Str(s) => json!(s),
        ConfigValue::Obj(o) => {
            if let Some(l) = o.downcast_ref::<Vec<String>>() {
                json!(l)
            } else if let Some(re) = o.downcast_ref::<RegExp>() {
                json!({"regexp": re.source_string(), "flags": flags(re)})
            } else if let Some(w) = o.downcast_ref::<Words>() {
                match w {
                    Words::Set { keys, .. } => json!({"keys": keys}),
                    Words::Fn(p) => json!({"fn": p.name}),
                }
            } else if let Some(p) = o.downcast_ref::<Predicate>() {
                json!({"fn": p.name})
            } else if let Some(h) = o.downcast_ref::<Hooks>() {
                let mut m = Map::new();
                for (k, n) in h.names() {
                    m.insert(k, json!(n));
                }
                Value::Object(m)
            } else if let Some(m) = o.downcast_ref::<HashMap<String, ConfigValue>>() {
                let mut out = Map::new();
                for (k, v) in m {
                    out.insert(k.clone(), config_json(v));
                }
                json!({"object": out})
            } else {
                panic!("unexpected object value")
            }
        }
    }
}

fn snap(state: &State) -> Value {
    let s = state.get::<ClikeState>();
    let tokenize = match &s.tokenize {
        None => Value::Null,
        Some(Tokenize::String(q)) => json!(["tokenString", q]),
        Some(Tokenize::Comment) => json!(["tokenComment"]),
        Some(Tokenize::CppHook) => json!(["cppHook"]),
        Some(Tokenize::RawString) => json!(["tokenRawString"]),
        Some(Tokenize::AtString) => json!(["tokenAtString"]),
        Some(Tokenize::TripleString) => json!(["tokenTripleString"]),
        Some(Tokenize::NestedComment(d)) => json!(["tokenNestedComment", d]),
        Some(Tokenize::KotlinString(t)) => json!(["tokenKotlinString", t]),
        Some(Tokenize::CeylonString(t)) => json!([
            "tokenCeylonString",
            match t {
                cm_modes::clike::CeylonType::Single => "single",
                cm_modes::clike::CeylonType::Triple => "triple",
            }
        ]),
    };
    let mut context = Vec::new();
    let mut c = Some(s.context.clone());
    while let Some(x) = c {
        let info = match &x.info {
            Info::Undefined => json!(["undefined"]),
            Info::Null => Value::Null,
            Info::Str(t) => enc(t),
        };
        context.push(json!([
            num(x.indented),
            num(x.column),
            x.ty.as_str(),
            info,
            x.align.get()
        ]));
        c = x.prev.clone();
    }
    json!({
        "tokenize": tokenize,
        "context": context,
        "indented": num(s.indented),
        "startOfLine": s.start_of_line,
        "prevToken": s.prev_token,
        "typeAtEndOfLine": if s.type_at_end_of_line { json!(true) } else { Value::Null },
        "delim": s.cpp11_raw_string_delim.as_deref().map(enc),
    })
}

#[test]
fn clike_configs_match_cm5() {
    let d = data();
    let want = d["config"].as_array().unwrap();
    let langs = cm_modes::clike::languages();
    assert_eq!(langs.len(), want.len());
    let r = Registry::with_all_modes();
    for ((mimes, spec), w) in langs.iter().zip(want) {
        let label = &w["mimes"];
        assert_eq!(&json!(mimes), label);
        assert_eq!(spec.name, w["name"].as_str().unwrap(), "{label}");
        let mut options = Map::new();
        for (k, v) in &spec.options {
            options.insert(k.clone(), config_json(v));
        }
        let wo = w["options"].as_object().unwrap();
        for (k, v) in wo {
            assert_eq!(options.get(k), Some(v), "{label} option {k}");
        }
        assert_eq!(options.len(), wo.len(), "{label} option keys");
        let props = spec.mode_props.as_ref().map(|p| {
            let mut m = Map::new();
            for (k, v) in p {
                m.insert(k.clone(), config_json(v));
            }
            json!({"object": m})
        });
        assert_eq!(
            props.unwrap_or(Value::Null),
            w["modeProps"],
            "{label} modeProps"
        );
        // `def` sets helperType and registers hintWords for mimes[0].
        for mime in mimes {
            let resolved = r.resolve_mode(&Spec::from(*mime));
            assert_eq!(
                json!(resolved.helper_type),
                w["helperType"],
                "{mime} helperType"
            );
        }
        let hint = r
            .helpers
            .get("hintWords", mimes[0])
            .map(|h| json!(h.downcast_ref::<Vec<String>>().unwrap()));
        assert_eq!(
            hint.unwrap_or(Value::Null),
            w["hintWords"],
            "{label} hintWords"
        );
        for mime in &mimes[1..] {
            assert!(
                r.helpers.get("hintWords", mime).is_none(),
                "{mime} hintWords"
            );
        }
    }
}

#[test]
fn clike_mode_props_match_cm5() {
    let d = data();
    let r = Registry::with_all_modes();
    let options = ModeOptions {
        indent_unit: 4,
        ..ModeOptions::default()
    };
    for p in d["props"].as_array().unwrap() {
        let mime = p["mime"].as_str().unwrap();
        let mode = r.get_mode(&options, &Spec::from(mime));
        let resolved = r.resolve_mode(&Spec::from(mime));
        // The mode object's own keys, then helperType and modeProps keys.
        let mut keys: Vec<String> = [
            "blockCommentContinue",
            "blockCommentEnd",
            "blockCommentStart",
            "electricInput",
            "fold",
            "indent",
            "lineComment",
            "name",
            "startState",
            "token",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if resolved.helper_type.is_some() {
            keys.push("helperType".into());
        }
        for k in resolved.mode_props.iter().flat_map(|m| m.keys()) {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        keys.sort();
        assert_eq!(json!(keys), p["keys"], "{mime} keys");
        for (k, w) in p["values"].as_object().unwrap() {
            let got = match k.as_str() {
                "name" => json!(mode.name()),
                "helperType" => json!(mode.helper_type()),
                _ => config_json(&mode.prop(k).unwrap_or_else(|| panic!("{mime} prop {k}"))),
            };
            assert_eq!(&got, w, "{mime} prop {k}");
        }
        assert_eq!(mode.line_comment(), Some("//"));
        assert_eq!(mode.block_comment_start(), Some("/*"));
        assert_eq!(mode.block_comment_end(), Some("*/"));
        assert_eq!(mode.electric_chars(), None);
        assert!(!mode.has_blank_line() && !mode.has_inner_mode() && mode.has_indent());
        let st = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        assert_eq!(snap(&st), p["startState"], "{mime} startState");
        let st = start_state(&*mode, JsArg::Num(6.0), JsArg::Undefined);
        assert_eq!(snap(&st), p["startStateBase6"], "{mime} startState(6)");
        let mode2 = r.get_mode(&ModeOptions::default(), &Spec::from(mime));
        let st2 = start_state(&*mode2, JsArg::Undefined, JsArg::Undefined);
        assert_eq!(snap(&st2), p["startStateIndentUnit2"], "{mime}");
    }
}

fn info_json(i: Option<&ModeInfo>) -> Value {
    match i {
        None => Value::Null,
        Some(i) => json!([i.name, i.mime, i.mode, i.ext]),
    }
}

fn js_info(v: &Value) -> Value {
    if v.is_null() {
        return Value::Null;
    }
    json!([v["name"], v["mime"], v["mode"], v["ext"]])
}

#[test]
fn clike_resolution_matches_cm5() {
    let d = data();
    let res = &d["resolve"];
    let r = Registry::with_all_modes();
    for c in res["specs"].as_array().unwrap() {
        let spec = spec_of(&c["spec"]);
        let got = r.resolve_mode(&spec);
        let mut keys: Vec<String> = got.options.keys().cloned().collect();
        keys.push("name".into());
        if got.helper_type.is_some() {
            keys.push("helperType".into());
        }
        if got.mode_props.is_some() {
            keys.push("modeProps".into());
        }
        keys.sort();
        assert_eq!(
            json!({"name": got.name, "helperType": got.helper_type, "keys": keys}),
            c["resolved"],
            "resolveMode {}",
            c["spec"]
        );
        assert_eq!(
            r.get_mode(&ModeOptions::default(), &spec).name(),
            c["modeName"].as_str().unwrap(),
            "getMode {}",
            c["spec"]
        );
    }
    assert_eq!(res["modeDefined"], "function");
    assert!(r.has_mode("clike"));
    let l = ModeInfoList::upstream();
    for (k, w) in res["findModeByMIME"].as_object().unwrap() {
        assert_eq!(info_json(l.find_mode_by_mime(k)), js_info(w), "byMIME {k}");
    }
    for (k, w) in res["findModeByExtension"].as_object().unwrap() {
        assert_eq!(
            info_json(l.find_mode_by_extension(k)),
            js_info(w),
            "byExt {k}"
        );
    }
    for (k, w) in res["findModeByName"].as_object().unwrap() {
        assert_eq!(info_json(l.find_mode_by_name(k)), js_info(w), "byName {k}");
    }
    for (k, w) in res["findModeByFileName"].as_object().unwrap() {
        assert_eq!(
            info_json(l.find_mode_by_file_name(k)),
            js_info(w),
            "byFile {k}"
        );
    }
    for name in [
        "C",
        "C++",
        "C#",
        "Java",
        "Kotlin",
        "Objective-C",
        "Objective-C++",
        "Scala",
        "Squirrel",
    ] {
        assert!(
            l.find_mode_by_name(name).unwrap().is_available(&r),
            "{name}"
        );
    }
}

#[test]
fn clike_electric_input_matches_cm5() {
    let d = data();
    let r = Registry::with_all_modes();
    for e in d["electric"].as_array().unwrap() {
        let mime = e["mime"].as_str().unwrap();
        let mode = r.get_mode(&ModeOptions::default(), &Spec::from(mime));
        for c in e["cases"].as_array().unwrap() {
            assert_eq!(
                mode.electric_input(&units(&c[0])),
                Some(c[1].as_bool().unwrap()),
                "{mime} {}",
                c[0]
            );
        }
    }
}

struct Run {
    lines: Vec<Value>,
    error: Value,
}

/// The recorder's `runLines`: per line, the state before, indent, `readToken`
/// tokens, the state after and `electricInput.test(line)`, from line `from` with
/// `state`. Returns the state copies saved at each line start.
fn run(
    mode: &Rc<dyn Mode>,
    lines: &[Vec<u16>],
    copy: bool,
    from: usize,
    state: State,
) -> (Run, Vec<State>) {
    let mut state = state;
    let mut out = Vec::new();
    let mut starts = Vec::new();
    let mut error = Value::Null;
    for i in from..lines.len() {
        let line = &lines[i];
        if copy {
            state = copy_state(&**mode, &state);
        }
        starts.push(copy_state(&**mode, &state));
        let before = snap(&state);
        let ws = line
            .iter()
            .take_while(|&&c| cm_core::text::is_js_space(c))
            .count();
        let indent = match mode.indent(&state, &line[ws..], line) {
            None => json!("Pass"),
            Some(x) => num(x),
        };
        let oracle = LinesOracle { lines, i };
        let mut stream = StringStream::new(line, Some(4), Some(&oracle));
        let mut tokens = Vec::new();
        while !stream.eol() {
            match read_token(mode, &mut stream, &mut state, None) {
                Ok(style) => {
                    tokens.push(json!([enc(stream.current()), style, stream.start]));
                    stream.start = stream.pos;
                }
                Err(e) => {
                    error = json!({"line": i, "pos": stream.pos, "message": e.to_string()});
                    break;
                }
            }
        }
        let electric = mode.electric_input(line).unwrap();
        out.push(json!({
            "before": before,
            "indent": indent,
            "tokens": tokens,
            "after": snap(&state),
            "electric": electric,
        }));
        if !error.is_null() {
            break;
        }
    }
    (Run { lines: out, error }, starts)
}

fn first_diff(got: &[Value], want: &[Value]) -> String {
    let k = got
        .iter()
        .zip(want)
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(want.len()));
    format!("line {k}: js {:?}\n  rust {:?}", want.get(k), got.get(k))
}

#[test]
fn clike_runs_match_cm5() {
    let d = data();
    let r = Registry::with_all_modes();
    let runs = d["runs"].as_array().unwrap();
    let mut bad = Vec::new();
    let (mut tokens, mut restarts, mut shared) = (0, 0, 0);
    for c in runs {
        let label = format!(
            "{} {} {} unit {} copy {}",
            c["kind"], c["name"], c["spec"], c["indentUnit"], c["copy"]
        );
        let options = ModeOptions {
            indent_unit: c["indentUnit"].as_i64().unwrap(),
            ..ModeOptions::default()
        };
        let mode = r.get_mode(&options, &spec_of(&c["spec"]));
        assert_eq!(mode.name(), c["mode"].as_str().unwrap(), "{label}");
        let lines = split_lines_runmode(&units(&c["text"]));
        let copy = c["copy"].as_bool().unwrap();
        let st = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        let (got, starts) = run(&mode, &lines, copy, 0, st);
        let want = c["lines"].as_array().unwrap();
        tokens += got
            .lines
            .iter()
            .map(|l| l["tokens"].as_array().unwrap().len())
            .sum::<usize>();
        if got.lines != *want || got.error != c["error"] {
            bad.push(format!(
                "{label}: {} / error js {} rust {}",
                first_diff(&got.lines, want),
                c["error"],
                got.error
            ));
            continue;
        }
        // Restarts from the copy saved at each line start, in the recorder's order. The
        // JS lists the restarts that differ from the carried run (lines whose saved
        // contexts were changed in place later) with the differing lines.
        let recorded: HashMap<usize, &Value> = c["restarts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| (x["at"].as_u64().unwrap() as usize, x))
            .collect();
        for (k, saved) in starts.iter().enumerate().skip(1) {
            let (again, _) = run(&mode, &lines, copy, k, copy_state(&*mode, saved));
            restarts += 1;
            let mut expect: Vec<Value> = want[k..].to_vec();
            let mut error = c["error"].clone();
            if let Some(x) = recorded.get(&k) {
                shared += 1;
                expect.truncate(x["count"].as_u64().unwrap() as usize);
                for (idx, l) in x["lines"].as_object().unwrap() {
                    let i: usize = idx.parse().unwrap();
                    expect[i - k] = l.clone();
                }
                error = x["error"].clone();
            }
            if again.lines != expect || again.error != error {
                bad.push(format!(
                    "{label}: restart at line {k}: {}",
                    first_diff(&again.lines, &expect)
                ));
                break;
            }
        }
    }
    for b in &bad {
        println!("{b}");
    }
    println!(
        "clike oracle: {} runs, {tokens} tokens, {restarts} restarts ({shared} with shared-context changes), {} mismatching",
        runs.len(),
        bad.len()
    );
    assert!(bad.is_empty());
}

#[test]
fn clike_runmode_matches_cm5() {
    let d = data();
    let r = Registry::with_all_modes();
    let cases = d["runmode"].as_array().unwrap();
    let mut bad = Vec::new();
    for c in cases {
        let mut got = Vec::new();
        run_mode_utf16(
            &r,
            &units(&c["text"]),
            &spec_of(&c["spec"]),
            |t| {
                got.push(match t.line {
                    None => json!(["\n"]),
                    Some(line) => json!([enc(&t.text), t.style, line, t.start]),
                })
            },
            None,
        );
        let want = c["tokens"].as_array().unwrap();
        if &got != want {
            bad.push(format!(
                "{} {}: {}",
                c["spec"],
                c["name"],
                first_diff(&got, want)
            ));
        }
    }
    for b in &bad {
        println!("{b}");
    }
    assert!(
        bad.is_empty(),
        "{} of {} runMode inputs differ",
        bad.len(),
        cases.len()
    );
}

/// `CtxType` names are the JS `type` strings.
#[test]
fn context_type_names() {
    assert_eq!(
        [
            CtxType::Top,
            CtxType::Statement,
            CtxType::Brace,
            CtxType::Bracket,
            CtxType::Paren
        ]
        .map(CtxType::as_str),
        ["top", "statement", "}", "]", ")"]
    );
}
