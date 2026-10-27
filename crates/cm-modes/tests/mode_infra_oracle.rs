//! P10-00 differential: the mode infrastructure against pinned CodeMirror 5.
//!
//! Replays `tests/data/cm5-mode-infra.json`, written by `oracle/cm5-mode-infra.mjs`
//! from a disposable build of the pinned tree, over the cases in
//! `tests/data/mode-infra-cases.json`:
//! - JS `RegExp.exec` results (every group span) and `SyntaxError`s;
//! - every rule regexp of the six upstream simple modes (dockerfile, factor,
//!   handlebars, nsis, rust, wast), compiled as `toRegex` does and matched at every
//!   position of the modes' demo text (SHA-256 of the records);
//! - `modeInfo` and the four `findModeBy*` lookups;
//! - `resolveMode` / `getMode` names;
//! - token runs (text, style, line, start, a state probe after each token),
//!   `blankLine` results and `indent()` at each line start, for a simple mode, an
//!   overlay mode and two multiplexing modes whose JS definitions are in the
//!   recorder; the Rust twins are below.

mod common;

use std::collections::HashMap;
use std::rc::Rc;

use cm_core::text::is_js_space;
use cm_core::util::string_stream::StringStream;
use cm_modes::meta::ModeInfoList;
use cm_modes::multiplex::{Delimiter, InnerMode, multiplexing_mode};
use cm_modes::overlay::overlay_mode;
use cm_modes::simple::{
    LocalModeSpec, RegexSpec, Rule, SimpleState, SimpleStates, TokenSpec, TokenValue,
    define_simple_mode,
};
use cm_modes::{
    ConfigValue, JsArg, LinesOracle, MimeTarget, Mode, ModeOptions, RegExp, Registry, Spec, State,
    copy_state, css, htmlmixed, inner_mode, javascript, split_lines_runmode, start_state, xml,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use common::{enc, indent_value, root, spec_from_json, units};

fn load(name: &str) -> Value {
    let p = root().join("crates/cm-modes/tests/data").join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

fn re(source: &str, flags: &str) -> RegexSpec {
    RegexSpec::re(source, flags).unwrap()
}

fn tok(s: &str) -> TokenSpec {
    TokenSpec::Str(s.into())
}

fn list(v: &[Option<&str>]) -> TokenSpec {
    TokenSpec::List(v.iter().map(|x| x.map(str::to_string)).collect())
}

fn rule(regex: RegexSpec, token: TokenSpec) -> Rule {
    Rule::new(regex, token)
}

fn local(spec: &str, end: &str) -> LocalModeSpec {
    LocalModeSpec {
        spec: Some(Spec::from(spec)),
        end: re(end, ""),
        ..Default::default()
    }
}

/// The recorder's `p10simple`.
fn p10simple() -> SimpleStates {
    let start = vec![
        rule(re(r#""(?:[^\\]|\\.)*?(?:"|$)"#, ""), tok("string")),
        rule(
            re(r"(function)(\s+)([a-z$][\w$]*)", ""),
            list(&[Some("keyword"), None, Some("variable-2")]),
        ),
        rule(
            re(r"(?:function|var|return|if|for|while|else|do|this)\b", ""),
            tok("keyword"),
        ),
        rule(re("true|false|null|undefined", ""), tok("atom")),
        rule(
            re(r"0x[a-f\d]+|[-+]?(?:\.\d+|\d+\.?\d*)(?:e[-+]?\d+)?", "i"),
            tok("number"),
        ),
        rule(re(r"\/\/.*", ""), tok("comment")),
        Rule {
            next: Some("comment".into()),
            ..rule(re(r"\/\*", ""), tok("comment"))
        },
        Rule {
            push: Some("heredoc".into()),
            ..rule(re("<<", ""), tok("meta"))
        },
        Rule {
            mode: Some(local("javascript", r"<\/script>")),
            ..rule(re("<script>", ""), tok("tag"))
        },
        Rule {
            mode: Some(LocalModeSpec {
                persistent: true,
                ..local("css", r"<\/style>")
            }),
            ..rule(re("<style>", ""), list(&[Some("tag"), Some("tag.end")]))
        },
        Rule {
            mode: Some(LocalModeSpec {
                force_end: Some(false),
                ..local("javascript", r"<\/raw>")
            }),
            ..rule(re("<raw>", ""), tok("tag"))
        },
        rule(re(r"[-+\/*=<>!]+", ""), tok("operator")),
        Rule {
            indent: true,
            ..rule(re(r"[\{\[\(]", ""), TokenSpec::Null)
        },
        Rule {
            dedent: true,
            ..rule(re(r"[\}\]\)]", ""), TokenSpec::Null)
        },
        rule(
            re(r"@(\w+)(\.)(\w+)", ""),
            list(&[Some("tag.name"), Some("punct"), Some("attr.x")]),
        ),
        rule(
            re(r"(\[\[)(\w+)(\]\])", ""),
            list(&[Some("bracket"), Some("attribute"), Some("bracket")]),
        ),
        rule(
            re(r"%(\w+)(-?)(\w*)", ""),
            list(&[Some("a"), Some("b"), Some("c")]),
        ),
        rule(
            re(r"&(\w*)(\w*)", ""),
            list(&[Some("amp"), Some("x"), Some("y")]),
        ),
        rule(RegexSpec::Str("#[A-Z]+".into()), tok("hash.tag")),
        rule(
            re(r"\^(\w+)(!)", ""),
            TokenSpec::Fn(Rc::new(|m| {
                if m[1].as_deref() == Some(&[b'u' as u16, b'p' as u16][..]) {
                    TokenValue::Str("up".into())
                } else {
                    TokenValue::List(vec![Some("fn-a".into()), Some("fn-b".into())])
                }
            })),
        ),
        Rule {
            sol: true,
            ..rule(re("~~", ""), tok("solmark"))
        },
        rule(re(r"é+|\u{1F600}", "u"), tok("uni")),
        rule(re("[A-Z]{2,}", "i"), tok("caps")),
        rule(re(r"[a-z$][\w$]*", ""), tok("variable")),
    ];
    let comment = vec![
        Rule {
            next: Some("start".into()),
            ..rule(re(r".*?\*\/", ""), tok("comment"))
        },
        rule(re(".*", ""), tok("comment")),
    ];
    let heredoc = vec![
        Rule {
            pop: true,
            ..rule(re(">>", ""), tok("meta"))
        },
        rule(re("[^>]+|>", ""), tok("string-2")),
    ];
    let mut meta = HashMap::new();
    meta.insert(
        "dontIndentStates".to_string(),
        ConfigValue::Obj(Rc::new(vec!["comment".to_string()])),
    );
    meta.insert("lineComment".to_string(), ConfigValue::Str("//".into()));
    meta.insert(
        "electricInput".to_string(),
        ConfigValue::Obj(Rc::new(RegExp::new(r"^\s*\}$", "").unwrap())),
    );
    SimpleStates {
        states: vec![
            ("start".into(), start),
            ("comment".into(), comment),
            ("heredoc".into(), heredoc),
        ],
        meta: Some(meta),
    }
}

/// The recorder's `todoOverlay` / `todoCombine`.
struct Todo {
    combine2: bool,
}

#[derive(Clone, Debug)]
struct TodoState;

impl Mode for Todo {
    fn name(&self) -> &str {
        ""
    }
    fn start_state(&self, _: JsArg, _: JsArg) -> Option<State> {
        self.combine2.then(|| State::new(TodoState))
    }
    fn token(&self, stream: &mut StringStream<'_>, _state: &mut State) -> Option<String> {
        if stream.match_str("TODO", true, false) {
            return Some("todo".into());
        }
        while stream.next().is_some() && !stream.match_str("TODO", false, false) {}
        None
    }
    fn combine_tokens(&self, state: &State) -> Option<bool> {
        matches!(state, State::Value(_)).then_some(true)
    }
    fn has_blank_line(&self) -> bool {
        self.combine2
    }
    fn blank_line(&self, _state: &mut State) -> Option<String> {
        Some("line-blank".into())
    }
}

/// The recorder's registry: xml, javascript, css, htmlmixed, then the test modes.
fn registry() -> Registry {
    let mut r = Registry::new();
    xml::register(&mut r);
    javascript::register(&mut r);
    css::register(&mut r);
    htmlmixed::register(&mut r);
    define_simple_mode(&mut r, "p10simple", p10simple()).unwrap();
    r.define_mime("text/x-p10simple", MimeTarget::Name("p10simple".into()));
    r.define_mode(
        "p10overlay",
        Rc::new(|reg, opts, spec| {
            let backdrop = spec
                .get("backdrop")
                .filter(|v| v.truthy())
                .and_then(ConfigValue::as_str)
                .unwrap_or("javascript");
            overlay_mode(
                reg.get_mode(opts, &Spec::from(backdrop)),
                Rc::new(Todo {
                    combine2: spec.truthy("combine2"),
                }),
                spec.truthy("combine"),
            )
        }),
    );
    r.define_mode(
        "p10multi",
        Rc::new(|reg, opts, _| {
            let get = |s: &str| reg.get_mode(opts, &Spec::from(s));
            multiplexing_mode(
                get("text/html"),
                vec![
                    InnerMode {
                        delim_style: Some("delimit".into()),
                        ..InnerMode::new(
                            Delimiter::str("{{"),
                            Some(Delimiter::str("}}")),
                            get("javascript"),
                        )
                    },
                    InnerMode {
                        inner_style: Some("inner".into()),
                        ..InnerMode::new(
                            Delimiter::re(RegExp::new("<%=?", "").unwrap()),
                            Some(Delimiter::str("%>")),
                            get("text/css"),
                        )
                    },
                    InnerMode {
                        parse_delimiters: true,
                        ..InnerMode::new(
                            Delimiter::str("[["),
                            Some(Delimiter::re(RegExp::new(r"\]\]", "").unwrap())),
                            get("p10simple"),
                        )
                    },
                    InnerMode {
                        delim_style: Some("bang".into()),
                        ..InnerMode::new(Delimiter::str("!!"), None, get("javascript"))
                    },
                ],
            )
        }),
    );
    r.define_mode(
        "p10multinl",
        Rc::new(|reg, opts, _| {
            multiplexing_mode(
                reg.get_mode(opts, &Spec::from("javascript")),
                vec![InnerMode {
                    delim_style: Some("nl".into()),
                    ..InnerMode::new(
                        Delimiter::str("\n"),
                        Some(Delimiter::str("\n")),
                        reg.get_mode(opts, &Spec::from("text/css")),
                    )
                }],
            )
        }),
    );
    r
}

fn num(x: f64) -> Value {
    if x.fract() == 0.0 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

/// The recorder's `probe(mode, state)`.
fn probe(mode: &Rc<dyn Mode>, state: &State) -> String {
    let inner = inner_mode(mode.clone(), state.clone()).0;
    let mut p = match inner.name() {
        "" => "?".to_string(),
        n => n.to_string(),
    };
    if mode.name() == "p10simple" {
        let s = state.get::<SimpleState>();
        let indent = match &s.indent {
            None => Value::Null,
            Some(v) => Value::Array(v.iter().map(|&x| num(x)).collect()),
        };
        let stack = json!(s.stack.clone().unwrap_or_default());
        p += &format!(
            "|{}|{}|{}|{}|{}",
            s.state,
            serde_json::to_string(&indent).unwrap(),
            serde_json::to_string(&stack).unwrap(),
            s.local_mode_name().unwrap_or(""),
            s.persistent_count()
        );
    }
    p
}

/// The recorder's `run(c)`.
fn run(r: &Registry, spec: &Spec, text: &[u16], copy_each_line: bool, indent_unit: i64) -> Value {
    let config = ModeOptions {
        indent_unit,
        ..ModeOptions::default()
    };
    let mode = r.get_mode(&config, spec);
    let lines = split_lines_runmode(text);
    let mut tokens = Vec::new();
    let mut indents = Vec::new();
    let mut blanks = Vec::new();
    let mut error = Value::Null;
    let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
    let mut guard = 0;
    'lines: for (i, line) in lines.iter().enumerate() {
        if copy_each_line {
            state = copy_state(&*mode, &state);
        }
        if mode.has_indent() {
            let ws = line.iter().take_while(|&&c| is_js_space(c)).count();
            indents.push(json!(indent_value(mode.indent(&state, &line[ws..], line))));
        }
        let oracle = LinesOracle { lines: &lines, i };
        let mut stream = StringStream::new(line, None, Some(&oracle));
        if stream.string.is_empty() && mode.has_blank_line() {
            let b = mode.blank_line(&mut state);
            blanks.push(json!([i, b]));
        }
        while !stream.eol() {
            let style = mode.token(&mut stream, &mut state);
            tokens.push(json!([
                enc(stream.current()),
                style,
                i,
                stream.start,
                probe(&mode, &state)
            ]));
            if stream.pos == stream.start {
                guard += 1;
                if guard > 1000 {
                    error = json!("runMode spins: no progress");
                    break 'lines;
                }
            }
            stream.start = stream.pos;
        }
    }
    json!({
        "name": mode.name(),
        "tokens": tokens,
        "indents": if mode.has_indent() { json!(indents) } else { Value::Null },
        "blanks": blanks,
        "error": error,
    })
}

#[test]
fn recorded_against_pinned_cm5() {
    let rec = load("cm5-mode-infra.json");
    assert_eq!(rec["cm5"], "5.65.21");
}

#[test]
fn regexp_exec_matches_cm5() {
    let rec = load("cm5-mode-infra.json");
    // Classified differences: each must still differ from CM5.
    let classified: &[(&str, &str)] = &[(
        r"\p{L}",
        "Unicode property escapes are not implemented (no upstream mode uses them)",
    )];
    let mut bad = Vec::new();
    let mut seen = Vec::new();
    let cases = rec["regexp"].as_array().unwrap();
    for case in cases {
        let c = &case[0];
        let source = units(&c[0]);
        let flags = c[1].as_str().unwrap();
        let input = units(&c[2]);
        let from = c[3].as_u64().unwrap() as usize;
        let got = match RegExp::from_utf16(&source, flags) {
            Err(_) => json!({"error": true}),
            Ok(r) => match r.exec_from(&input, from) {
                None => json!({"match": null}),
                Some(m) => json!({"match": (0..m.count())
                    .map(|i| m.group(i).map_or(Value::Null, |(a, b)| json!([a, b])))
                    .collect::<Vec<_>>()}),
            },
        };
        let src = String::from_utf16_lossy(&source);
        if let Some((_, why)) = classified.iter().find(|(s, _)| *s == src) {
            assert_ne!(got, case[1], "classified case now matches: {src} ({why})");
            seen.push(src);
            continue;
        }
        if got != case[1] {
            bad.push(format!("{c}: cm5 {} rust {got}", case[1]));
        }
    }
    assert_eq!(
        seen.len(),
        classified.len(),
        "every classified case is recorded"
    );
    assert!(
        bad.is_empty(),
        "{} of {} differ:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

#[test]
fn harvested_simple_mode_regexps_match_cm5() {
    let rec = load("cm5-mode-infra.json");
    let h = &rec["harvest"];
    let lines: Vec<Vec<u16>> = h["lines"].as_array().unwrap().iter().map(units).collect();
    let mut sha = Sha256::new();
    let (mut records, mut matched) = (0u64, 0u64);
    for r in h["regexps"].as_array().unwrap() {
        let re = RegExp::from_utf16(&units(&r[0]), r[1].as_str().unwrap())
            .unwrap_or_else(|e| panic!("{r}: {e}"));
        for line in &lines {
            for pos in 0..=line.len() {
                let rec = match re.exec(&line[pos..]) {
                    None => "-".to_string(),
                    Some(m) => {
                        matched += 1;
                        (0..m.count())
                            .map(|i| m.group(i).map_or("u".into(), |(a, b)| format!("{a}-{b}")))
                            .collect::<Vec<_>>()
                            .join(",")
                    }
                };
                for u in (rec + "\n").encode_utf16() {
                    sha.update(u.to_le_bytes());
                }
                records += 1;
            }
        }
    }
    assert_eq!(records, h["records"].as_u64().unwrap());
    assert_eq!(matched, h["matched"].as_u64().unwrap());
    assert_eq!(
        format!("{:x}", sha.finalize()),
        h["digest"].as_str().unwrap()
    );
}

#[test]
fn mode_info_matches_cm5() {
    let rec = load("cm5-mode-infra.json");
    let m = &rec["meta"];
    let l = ModeInfoList::upstream();
    let infos: Vec<Value> = l
        .infos
        .iter()
        .map(|i| {
            json!({
                "name": i.name, "mime": i.mime, "mimes": i.mimes, "mode": i.mode,
                "ext": i.ext, "alias": i.alias,
                "file": i.file.as_ref().map(|f| json!([f.source_string(), f.flags().as_string()])),
            })
        })
        .collect();
    assert_eq!(Value::Array(infos), m["infos"]);
    let name = |i: Option<&cm_modes::meta::ModeInfo>| i.map_or(Value::Null, |i| json!(i.name));
    let check = |key: &str, f: &dyn Fn(&str) -> Value| {
        for q in m[key].as_array().unwrap() {
            let arg = q[0].as_str().unwrap();
            assert_eq!(f(arg), q[1], "{key}({arg:?})");
        }
    };
    check("byMime", &|x| name(l.find_mode_by_mime(x)));
    check("byExtension", &|x| name(l.find_mode_by_extension(x)));
    check("byFileName", &|x| name(l.find_mode_by_file_name(x)));
    check("byName", &|x| name(l.find_mode_by_name(x)));
}

#[test]
fn resolve_mode_matches_cm5() {
    let rec = load("cm5-mode-infra.json");
    let r = registry();
    for c in rec["resolve"].as_array().unwrap() {
        let spec = spec_from_json(&c[0], &|s| panic!("{s}"));
        let res = r.resolve_mode(&spec);
        let mut opts = serde_json::Map::new();
        for (k, v) in &res.options {
            opts.insert(
                k.clone(),
                match v {
                    ConfigValue::Null => Value::Null,
                    ConfigValue::Bool(b) => json!(b),
                    ConfigValue::Num(n) => num(*n),
                    ConfigValue::Str(s) => json!(s),
                    ConfigValue::Obj(_) => json!("<obj>"),
                },
            );
        }
        if let Some(h) = &res.helper_type {
            opts.insert("helperType".into(), json!(h));
        }
        assert_eq!(json!(res.name), c[1], "resolveMode({})", c[0]);
        // Primitive options exactly; object-valued options (keyword tables) by key.
        // Classified: the css port (P0) keeps the keyword tables of its MIME specs
        // in the mode, so `text/x-scss` names only `propertyKeywords`.
        let want = c[2].as_object().unwrap();
        let prim = |m: &serde_json::Map<String, Value>| -> Vec<(String, Value)> {
            m.iter()
                .filter(|(_, v)| *v != "<obj>")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        let objs = |m: &serde_json::Map<String, Value>| -> Vec<String> {
            m.iter()
                .filter(|(_, v)| *v == "<obj>")
                .map(|(k, _)| k.clone())
                .collect()
        };
        assert_eq!(prim(&opts), prim(want), "resolveMode({}) options", c[0]);
        if c[0] == "text/x-scss" {
            assert_ne!(
                objs(&opts),
                objs(want),
                "classified css MIME difference now matches"
            );
        } else {
            assert_eq!(
                objs(&opts),
                objs(want),
                "resolveMode({}) object options",
                c[0]
            );
        }
        let got = r.get_mode(&ModeOptions::default(), &spec);
        assert_eq!(json!(got.name()), c[3], "getMode({}).name", c[0]);
    }
}

#[test]
fn mode_runs_match_cm5() {
    let rec = load("cm5-mode-infra.json");
    let cases = load("mode-infra-cases.json");
    let r = registry();
    let runs: HashMap<String, &Value> = rec["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["id"].as_str().unwrap().to_string(), x))
        .collect();
    let mut compared = 0;
    let mut tokens = 0;
    let mut bad = Vec::new();
    for c in cases["runs"].as_array().unwrap() {
        let spec = spec_from_json(&c["spec"], &|s| panic!("{s}"));
        let units_list = c
            .get("indentUnits")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| x.as_i64().unwrap()).collect::<Vec<_>>())
            .unwrap_or(vec![2]);
        for t in c["texts"].as_array().unwrap() {
            let t = t.as_str().unwrap();
            let text = units(&cases["texts"][t]);
            for copy in [false, true] {
                for &unit in &units_list {
                    let id = format!(
                        "{}/{t}/{}/{unit}",
                        c["id"].as_str().unwrap(),
                        if copy { "copy" } else { "keep" }
                    );
                    let want = runs[&id];
                    let got = run(&r, &spec, &text, copy, unit);
                    compared += 1;
                    tokens += want["tokens"].as_array().unwrap().len();
                    for key in ["name", "indents", "blanks", "error"] {
                        if got[key] != want[key] {
                            bad.push(format!("{id} {key}: cm5 {} rust {}", want[key], got[key]));
                        }
                    }
                    let (g, w) = (
                        got["tokens"].as_array().unwrap(),
                        want["tokens"].as_array().unwrap(),
                    );
                    if let Some(i) = (0..g.len().max(w.len())).find(|&i| g.get(i) != w.get(i)) {
                        bad.push(format!(
                            "{id} token {i}: cm5 {:?} rust {:?}",
                            w.get(i),
                            g.get(i)
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(compared, runs.len(), "every recorded run is compared");
    assert!(
        bad.is_empty(),
        "{} differences in {compared} runs ({tokens} tokens):\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The `meta` object reaches the mode's properties and typed accessors.
#[test]
fn simple_mode_meta_and_errors() {
    let r = registry();
    let m = r.get_mode(&ModeOptions::default(), &Spec::from("p10simple"));
    assert_eq!(m.name(), "p10simple");
    assert_eq!(m.line_comment(), Some("//"));
    assert_eq!(m.electric_input(&cm_core::text::to_u16("  }")), Some(true));
    assert_eq!(
        m.electric_input(&cm_core::text::to_u16("  } x")),
        Some(false)
    );
    assert!(m.prop("dontIndentStates").is_some());
    assert!(m.has_inner_mode());
    let mut bad = SimpleStates::default();
    let mut r2 = Registry::new();
    assert!(define_simple_mode(&mut r2, "x", bad.clone()).is_err());
    bad.states.push((
        "start".into(),
        vec![Rule {
            next: Some("nope".into()),
            ..Rule::default()
        }],
    ));
    assert_eq!(
        define_simple_mode(&mut r2, "x", bad)
            .unwrap_err()
            .to_string(),
        "Undefined state nope in simple mode"
    );
}

/// The helpers the ported modes register (`hintWords` for css, `wordChars` for
/// javascript) equal the values CM5 registers.
#[test]
fn ported_mode_helpers_match_cm5() {
    let rec = load("cm5-mode-infra.json");
    let h = &rec["helpers"];
    let r = registry();
    let words = r.helpers.get("hintWords", "css").unwrap();
    let words = words.downcast_ref::<Vec<String>>().unwrap();
    assert_eq!(json!(words), h["hintWordsCss"]);
    let wc = r.helpers.get("wordChars", "javascript").unwrap();
    let wc = wc.downcast_ref::<RegExp>().unwrap();
    assert_eq!(
        json!([wc.source_string(), wc.flags().as_string()]),
        h["wordCharsJavascript"]
    );
    let js = r.get_mode(&ModeOptions::default(), &Spec::from("text/javascript"));
    assert_eq!(r.helpers.for_mode(&*js, "wordChars", &()).len(), 1);
}
