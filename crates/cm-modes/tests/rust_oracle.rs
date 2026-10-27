//! P10-01 oracle: `mode/rust/rust.js` on the simple-mode engine must reproduce
//! `tests/data/cm5-rust-mode.json` (written by `oracle/cm5-rust-mode.mjs` from a
//! built pinned tree): the rule tables, the mode object's properties, mode and MIME
//! resolution, `electricInput`, and per-line runs (state before and after, indent,
//! tokens from `readToken`, the "failed to advance" error) with and without a
//! `copyState` at each line start, plus `runMode` callbacks.

mod common;

use std::rc::Rc;

use cm_core::util::string_stream::StringStream;
use cm_modes::meta::{ModeInfo, ModeInfoList};
use cm_modes::simple::{RegexSpec, SimpleState, TokenSpec};
use cm_modes::{
    ConfigValue, JsArg, LinesOracle, MimeTarget, Mode, ModeOptions, RegExp, Registry, Spec, State,
    copy_state, read_token, run_mode_utf16, split_lines_runmode, start_state,
};
use common::{enc, spec_from_json, units};
use serde_json::{Map, Value, json};

fn data() -> Value {
    let path = common::root().join("crates/cm-modes/tests/data/cm5-rust-mode.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-rust-mode.json")).unwrap()
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

fn snap(state: &State) -> Value {
    let s = state.get::<SimpleState>();
    json!({
        "state": s.state,
        "indent": s.indent.as_ref().map(|v| v.iter().map(|&x| num(x)).collect::<Vec<_>>()),
        "pending": s.pending.as_ref().map(|p| p.iter().map(|(n, t)| json!([n, t])).collect::<Vec<_>>()),
        "stack": s.stack,
        "local": s.local_mode_name().map(|_| true),
    })
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
            } else {
                panic!("unexpected object meta value")
            }
        }
    }
}

fn flags(re: &RegExp) -> String {
    let f = re.flags();
    let mut s = String::new();
    for (on, c) in [
        (f.dot_all, 's'),
        (f.global, 'g'),
        (f.ignore_case, 'i'),
        (f.multiline, 'm'),
        (f.sticky, 'y'),
        (f.unicode, 'u'),
    ] {
        if on {
            s.push(c);
        }
    }
    // JS orders `flags` as "dgimsuvy".
    let mut v: Vec<char> = s.chars().collect();
    v.sort_by_key(|c| "dgimsuvy".find(*c));
    v.into_iter().collect()
}

#[test]
fn rust_rules_match_cm5() {
    let d = data();
    let states = cm_modes::rust::states();
    let want = d["rules"]["states"].as_array().unwrap();
    assert_eq!(states.states.len(), want.len());
    for ((name, rules), w) in states.states.iter().zip(want) {
        assert_eq!(name, w[0].as_str().unwrap());
        let w = w[1].as_array().unwrap();
        assert_eq!(rules.len(), w.len(), "state {name}");
        for (i, (r, w)) in rules.iter().zip(w).enumerate() {
            let mut got = Map::new();
            got.insert(
                "regex".into(),
                match &r.regex {
                    RegexSpec::Re(re) => {
                        json!({"source": re.source_string(), "flags": flags(re)})
                    }
                    RegexSpec::Str(s) => json!(s),
                    RegexSpec::None => Value::Null,
                },
            );
            got.insert(
                "token".into(),
                match &r.token {
                    TokenSpec::Null => Value::Null,
                    TokenSpec::Str(s) => json!(s),
                    TokenSpec::List(l) => json!(l),
                    TokenSpec::Fn(_) => panic!("rust.js has no function tokens"),
                },
            );
            if let Some(n) = &r.next {
                got.insert("next".into(), json!(n));
            }
            if let Some(n) = &r.push {
                got.insert("push".into(), json!(n));
            }
            for (on, k) in [
                (r.pop, "pop"),
                (r.sol, "sol"),
                (r.indent, "indent"),
                (r.dedent, "dedent"),
            ] {
                if on {
                    got.insert(k.into(), json!(true));
                }
            }
            assert!(r.mode.is_none() && r.dedent_if_line_start.is_none());
            assert_eq!(&Value::Object(got), w, "state {name} rule {i}");
        }
    }
    let meta = states.meta.as_ref().unwrap();
    let want = d["rules"]["meta"].as_object().unwrap();
    assert_eq!(meta.len(), want.len());
    for (k, w) in want {
        assert_eq!(&config_json(&meta[k]), w, "meta {k}");
    }
}

#[test]
fn rust_mode_props_match_cm5() {
    let d = data();
    let p = &d["props"];
    let r = Registry::with_all_modes();
    let options = ModeOptions {
        indent_unit: 4,
        ..ModeOptions::default()
    };
    let mode = r.get_mode(&options, &Spec::from("rust"));
    assert_eq!(mode.name(), p["name"].as_str().unwrap());
    for (k, w) in p["meta"].as_object().unwrap() {
        let v = mode.prop(k).unwrap_or_else(|| panic!("prop {k}"));
        assert_eq!(&config_json(&v), w, "prop {k}");
    }
    // The typed accessors answer the same meta strings.
    assert_eq!(mode.line_comment(), p["meta"]["lineComment"].as_str());
    assert_eq!(
        mode.block_comment_start(),
        p["meta"]["blockCommentStart"].as_str()
    );
    assert_eq!(
        mode.block_comment_end(),
        p["meta"]["blockCommentEnd"].as_str()
    );
    assert_eq!(mode.electric_chars(), p["electricChars"].as_str());
    assert_eq!(p["blankLine"], "undefined");
    assert!(!mode.has_blank_line());
    assert_eq!(mode.helper_type(), p["helperType"].as_str());
    assert!(mode.has_indent());
    let st = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
    assert!(p["innerModeAtStart"].is_null());
    assert!(mode.inner_mode(&st).is_none());
    assert_eq!(snap(&st), p["startState"]);
    let mode2 = r.get_mode(&ModeOptions::default(), &Spec::from("rust"));
    let st2 = start_state(&*mode2, JsArg::Undefined, JsArg::Undefined);
    assert_eq!(snap(&st2), p["startStateIndentUnit2"]);
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
fn rust_resolution_matches_cm5() {
    let d = data();
    let res = &d["resolve"];
    let r = Registry::with_all_modes();
    for c in res["specs"].as_array().unwrap() {
        let spec = spec_of(&c["spec"]);
        let got = r.resolve_mode(&spec);
        let mut obj = Map::new();
        obj.insert("name".into(), json!(got.name));
        for (k, v) in &got.options {
            obj.insert(k.clone(), config_json(v));
        }
        assert_eq!(
            Value::Object(obj),
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
    for (mime, w) in res["mime"].as_object().unwrap() {
        let got = match r.mime_target(mime) {
            None => Value::Null,
            Some(MimeTarget::Name(n)) => json!(n),
            Some(MimeTarget::Spec(_)) => panic!("{mime}: object target"),
        };
        assert_eq!(&got, w, "mimeModes[{mime}]");
    }
    assert_eq!(res["modeDefined"], "function");
    assert!(r.has_mode("rust"));
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
    assert!(l.find_mode_by_name("Rust").unwrap().is_available(&r));
}

#[test]
fn rust_electric_input_matches_cm5() {
    let d = data();
    let r = Registry::with_all_modes();
    let mode = r.get_mode(&ModeOptions::default(), &Spec::from("rust"));
    for c in d["electric"].as_array().unwrap() {
        let s = units(&c[0]);
        assert_eq!(
            mode.electric_input(&s),
            Some(c[1].as_bool().unwrap()),
            "{}",
            c[0]
        );
    }
}

struct Run {
    lines: Vec<Value>,
    error: Value,
}

/// The recorder's `run`: per line, the state before, indent, `readToken` tokens,
/// the state after and `electricInput.test(line)`. `from` restarts at that line with
/// the given state.
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
        if line.is_empty() && mode.has_blank_line() {
            mode.blank_line(&mut state);
        }
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
fn rust_runs_match_cm5() {
    let d = data();
    let r = Registry::with_all_modes();
    let runs = d["runs"].as_array().unwrap();
    let mut bad = Vec::new();
    let (mut tokens, mut restarts) = (0, 0);
    for c in runs {
        let label = format!(
            "{} {} unit {} copy {}",
            c["kind"], c["name"], c["indentUnit"], c["copy"]
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
        // Line restart: tokenizing from a copy of the state saved at line k gives
        // the same lines k.. as the carried run (what the editor's state cache does).
        for (k, saved) in starts.iter().enumerate().skip(1) {
            let (again, _) = run(&mode, &lines, copy, k, copy_state(&*mode, saved));
            restarts += 1;
            if again.lines[..] != want[k..] || again.error != c["error"] {
                bad.push(format!(
                    "{label}: restart at line {k}: {}",
                    first_diff(&again.lines, &want[k..])
                ));
                break;
            }
        }
    }
    for b in &bad {
        println!("{b}");
    }
    println!(
        "rust oracle: {} runs, {tokens} tokens, {restarts} restarts, {} mismatching",
        runs.len(),
        bad.len()
    );
    assert!(bad.is_empty());
}

#[test]
fn rust_runmode_matches_cm5() {
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
