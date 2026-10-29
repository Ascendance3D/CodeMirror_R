//! htmlmixed resolves a tag's inner mode when the tag closes (issue #59, G13 #13:
//! `html` in mode/htmlmixed/htmlmixed.js:88-113 calls `CodeMirror.getMode` at token
//! time), so a MIME type or mode defined after the htmlmixed mode was created applies.
//! Replays `tests/data/cm5-htmlmixed-late-mime.json`, recorded by
//! `oracle/cm5-htmlmixed-late-mime.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and
//! WebKit (identical output): one mode object tokenizes the lines before and after
//! the definitions.

use std::path::PathBuf;
use std::rc::Rc;

use cm_core::StringStream;
use cm_core::util::regexp::RegExp;
use cm_modes::htmlmixed::{TagRule, obj};
use cm_modes::{
    JsArg, MimeTarget, Mode, ModeOptions, ModeSpec, Registry, Spec, State, start_state,
};
use serde_json::{Value, json};

fn data() -> Value {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-htmlmixed-late-mime.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-htmlmixed-late-mime.json"))
        .unwrap()
}

/// The recording's late mode: styles the rest of the line `late-<name>`.
struct Late(String);

impl Mode for Late {
    fn name(&self) -> &str {
        &self.0
    }
    fn token(&self, s: &mut StringStream<'_>, _state: &mut State) -> Option<String> {
        s.skip_to_end();
        Some(format!("late-{}", self.0))
    }
}

fn spec_of(v: &Value) -> Spec {
    let mut tags = Vec::new();
    for (tag, rules) in v["tags"].as_object().unwrap() {
        let rules: Vec<TagRule> = rules
            .as_array()
            .unwrap()
            .iter()
            .map(|r| TagRule {
                attr: r[0].as_str().map(String::from),
                matches: r[1].as_str().map(|src| {
                    let re = RegExp::new(src, "").unwrap();
                    Rc::new(move |s: &[u16]| re.exec(s).is_some()) as _
                }),
                mode: Spec::from(r[2].as_str().unwrap()),
            })
            .collect();
        tags.push((tag.clone(), rules));
    }
    Spec::Obj(ModeSpec::named(v["name"].as_str().unwrap()).with("tags", obj(tags)))
}

fn tokens(mode: &Rc<dyn Mode>, lines: &[Value]) -> Value {
    let mut state = start_state(&**mode, JsArg::Undefined, JsArg::Undefined);
    let mut out = Vec::new();
    for line in lines {
        let u: Vec<u16> = line.as_str().unwrap().encode_utf16().collect();
        let mut stream = StringStream::new(&u, Some(2), None);
        let mut toks = Vec::new();
        while !stream.eol() {
            let style = mode.token(&mut stream, &mut state);
            toks.push(json!([stream.current_string(), style]));
            stream.start = stream.pos;
        }
        out.push(Value::Array(toks));
    }
    Value::Array(out)
}

#[test]
fn htmlmixed_sees_later_definitions() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_object().unwrap();
    assert!(cases.len() >= 4, "recording lost cases: {}", cases.len());
    for (name, case) in cases {
        let mut registry = Registry::with_all_modes();
        let opts = ModeOptions {
            indent_unit: 2,
            ..ModeOptions::default()
        };
        let mode = registry.get_mode(&opts, &spec_of(&case["spec"]));
        let lines = case["lines"].as_array().unwrap();
        assert_eq!(tokens(&mode, lines), case["before"], "{name}: before");
        for def in case["defs"].as_array().unwrap() {
            let n = def[1].as_str().unwrap().to_string();
            match def[0].as_str().unwrap() {
                "mime" => {
                    registry.define_mime(&n, MimeTarget::Name(def[2].as_str().unwrap().into()))
                }
                _ => registry.define_mode(
                    &n.clone(),
                    Rc::new(move |_: &Registry, _: &_, _: &_| {
                        Rc::new(Late(n.clone())) as Rc<dyn Mode>
                    }),
                ),
            }
        }
        assert_eq!(tokens(&mode, lines), case["after"], "{name}: after");
    }
}

/// Clones share the definitions, as every JS caller shares `CodeMirror.mimeModes`;
/// the clone's own tables (`mime_target`) stay as they were.
#[test]
fn current_sees_definitions_through_any_clone() {
    let mut a = Registry::with_all_modes();
    let b = a.clone();
    a.define_mime("text/x-shared", MimeTarget::Name("css".into()));
    assert!(b.mime_target("text/x-shared").is_none());
    assert_eq!(
        b.current().mime_target("text/x-shared"),
        Some(&MimeTarget::Name("css".into()))
    );
    assert_eq!(
        a.current().mime_target("text/x-shared"),
        a.mime_target("text/x-shared")
    );
}
