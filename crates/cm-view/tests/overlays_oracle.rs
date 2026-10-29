//! Highlighting overlays (issue #31: `addOverlay` / `removeOverlay` in
//! src/edit/methods.js:67-87, the overlay pass of `highlightLine` and
//! `Context.baseToken` in src/line/highlight.js:31-102, and `getTokenTypeAt`'s
//! "overlay " cut at methods.js:140) against `tests/data/cm5-overlays.json`, recorded by
//! `oracle/cm5-overlays.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and WebKit
//! (identical output). Each step is checked for its error, its redraws (`update`
//! events), the number of overlays, every line's style array and `styleClasses`, and
//! `getTokenTypeAt` at the recorded positions. The overlay modes below port the JS
//! ones of the recording script one for one.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use cm_core::util::regexp::RegExp;
use cm_core::{Doc, Pos, StringStream};
use cm_modes::{JsArg, Mode, ModeSpec, Registry, Spec, State};
use cm_view::highlight::OverlaySpec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::{Value, json};

fn data() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-overlays.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-overlays.json")).unwrap()
}

/// The recording's `OVERLAYS` table.
#[derive(Clone, Copy)]
enum Kind {
    Foo,
    Digits,
    Word,
    InStr,
    Todo,
    LineOnly,
    Probe,
    Stateful,
}

struct Ov {
    kind: Kind,
    name: &'static str,
}

fn re(src: &str) -> RegExp {
    RegExp::new(src, "").unwrap()
}

impl Mode for Ov {
    fn name(&self) -> &str {
        self.name
    }
    fn start_state(&self, _a1: JsArg, _a2: JsArg) -> Option<State> {
        matches!(self.kind, Kind::Stateful).then_some(State::True)
    }
    fn token(&self, s: &mut StringStream<'_>, _state: &mut State) -> Option<String> {
        match self.kind {
            Kind::Foo => {
                if s.match_str("foo", true, false) {
                    return Some("foo".into());
                }
                s.next();
                None
            }
            Kind::Digits => {
                let b = s.base_token().expect("baseToken");
                let comment = b.0.as_deref().is_some_and(|t| t.contains("comment"));
                if !comment && s.match_regexp(&re("\\d+"), true).is_some() {
                    return Some("x".into());
                }
                s.next();
                None
            }
            Kind::Word => {
                if s.match_regexp(&re("^\\w+"), true).is_some() {
                    return Some("w".into());
                }
                s.next();
                None
            }
            Kind::InStr => {
                if let Some((Some(t), size)) = s.base_token()
                    && t == "string"
                {
                    s.pos = (s.pos as i64 + size) as usize;
                    return Some("in-str".into());
                }
                s.next();
                None
            }
            Kind::Todo => {
                if s.match_str("TODO", true, false) {
                    return Some("todo line-background-hl line-mark".into());
                }
                s.next();
                None
            }
            Kind::LineOnly => {
                s.skip_to_end();
                Some("line-whole".into())
            }
            Kind::Probe => {
                let (ty, size) = s.base_token().expect("baseToken");
                s.next();
                let ty = match ty {
                    Some(t) if !t.is_empty() => t.replace(' ', "_"),
                    _ => "null".into(),
                };
                Some(format!("p-{ty}-{size}"))
            }
            Kind::Stateful => {
                s.skip_to_end();
                None
            }
        }
    }
}

fn overlay(key: &str) -> Rc<dyn Mode> {
    let (kind, name) = match key {
        "foo" => (Kind::Foo, ""),
        "digits" => (Kind::Digits, ""),
        "word" => (Kind::Word, "ovw"),
        "instr" => (Kind::InStr, ""),
        "todo" => (Kind::Todo, ""),
        "lineonly" => (Kind::LineOnly, ""),
        "probe" => (Kind::Probe, ""),
        "stateful" => (Kind::Stateful, ""),
        _ => panic!("unknown overlay {key}"),
    };
    Rc::new(Ov { kind, name })
}

fn registry() -> Rc<Registry> {
    let mut r = Registry::with_all_modes();
    r.define_mode(
        "ovl-digits",
        Rc::new(|_: &Registry, _: &_, _: &_| overlay("digits")),
    );
    r.define_mode(
        "ovl-word",
        Rc::new(|_: &Registry, _: &_, _: &_| overlay("word")),
    );
    Rc::new(r)
}

fn spec_of(v: &Value) -> Spec {
    match v {
        Value::Null => Spec::Null,
        Value::String(s) => Spec::from(s.as_str()),
        Value::Object(o) => Spec::Obj(ModeSpec::named(o["name"].as_str().unwrap())),
        _ => panic!("spec {v}"),
    }
}

fn pos(v: &Value) -> Pos {
    Pos::new(v["line"].as_i64().unwrap(), v["ch"].as_i64().unwrap())
}

fn lines_of(cm: &Editor) -> Value {
    let mut out = Vec::new();
    for n in 0..cm.doc.line_count() as i64 {
        let st = cm.line_styles(n);
        let mut styles = Vec::new();
        for (end, style) in &st.spans {
            styles.push(json!(end));
            styles.push(style.as_deref().map_or(Value::Null, |s| json!(s)));
        }
        let classes = match &st.classes {
            None => Value::Null,
            Some((bg, text)) => json!({"bgClass": bg, "textClass": text}),
        };
        out.push(json!({"styles": styles, "classes": classes}));
    }
    Value::Array(out)
}

#[test]
fn overlays_match_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_object().unwrap();
    assert!(cases.len() >= 16, "recording lost cases: {}", cases.len());
    let mut steps_checked = 0;
    for (name, case) in cases {
        let mut cm = Editor::with_doc(
            Doc::new(case["text"].as_str().unwrap()),
            spec_of(&case["mode"]),
            EditorOptions::default(),
            Metrics::oracle(),
            registry(),
        );
        cm.set_size(800.0, 400.0);
        let updates = Rc::new(Cell::new(0u32));
        let u = updates.clone();
        cm.on_update(move |_| u.set(u.get() + 1));
        let mut objs: HashMap<String, Rc<dyn Mode>> = HashMap::new();
        for rec in case["steps"].as_array().unwrap() {
            let step = &rec["step"];
            let (op, a, b) = (step[0].as_str().unwrap(), &step[1], &step[2]);
            let before = updates.get();
            let mut err: Option<String> = None;
            match op {
                "add" => {
                    let spec = match a {
                        Value::String(k) => OverlaySpec::Mode(
                            objs.entry(k.clone()).or_insert_with(|| overlay(k)).clone(),
                        ),
                        _ => OverlaySpec::Spec(spec_of(&a["spec"])),
                    };
                    let opaque = b["opaque"].as_bool().unwrap_or(false);
                    let priority = b["priority"].as_f64().unwrap_or(0.0);
                    if let Err(e) = cm.add_overlay(spec, opaque, priority) {
                        err = Some(e.to_string());
                    }
                }
                "remove" => {
                    let spec = match a {
                        Value::String(k) => {
                            OverlaySpec::Mode(objs.get(k).cloned().unwrap_or_else(|| overlay(k)))
                        }
                        _ if a.get("spec").is_some() => OverlaySpec::Spec(spec_of(&a["spec"])),
                        _ => OverlaySpec::Spec(Spec::from(a["name"].as_str().unwrap())),
                    };
                    cm.remove_overlay(&spec);
                }
                "replace" => {
                    let (from, to) = (pos(&b[0]), pos(&b[1]));
                    let text = a.as_str().unwrap().to_string();
                    cm.run_op(|cm| cm.doc.replace_range(&text, from, Some(to), None));
                }
                "setMode" => cm.set_mode(spec_of(a)),
                _ => panic!("op {op}"),
            }
            let at = format!("{name} step {step}");
            assert_eq!(
                err.map_or(Value::Null, |e| json!(e)),
                rec["err"],
                "{at}: err"
            );
            assert_eq!(
                cm.hs.borrow().overlays.len() as u64,
                rec["overlays"].as_u64().unwrap(),
                "{at}: overlays"
            );
            assert_eq!(lines_of(&cm), rec["lines"], "{at}: lines");
            let types: Vec<Value> = case["probes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let t = cm.get_token_type_at(Pos::new(
                        p[0].as_i64().unwrap(),
                        p[1].as_i64().unwrap(),
                    ));
                    t.map_or(Value::Null, |t| json!(t))
                })
                .collect();
            assert_eq!(Value::Array(types), rec["types"], "{at}: getTokenTypeAt");
            assert_eq!(
                (updates.get() - before) as u64,
                rec["redraw"].as_u64().unwrap(),
                "{at}: redraw"
            );
            steps_checked += 1;
        }
    }
    assert!(steps_checked >= 30, "{steps_checked} steps");
}
