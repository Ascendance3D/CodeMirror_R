//! MIME aliases registered by `CodeMirror.defineMIME` in
//! `upstream/codemirror5/mode/javascript/javascript.js:948-958` and
//! `upstream/codemirror5/mode/xml/xml.js:412-413`.
//!
//! Each alias must resolve (`resolveMode`, `src/modes.js`) to exactly the spec the JS
//! registers, and the tokens prove the spec's options reach the mode. The expected
//! specs and tokens were read off the JS and confirmed with CM5's own `resolveMode`
//! and `addon/runmode/runmode.js` run in node against `upstream/codemirror5`.

use cm_modes::{ConfigValue, ModeOptions, ModeSpec, Registry, Spec, run_mode_collect};

type Tokens = Vec<(&'static str, Option<&'static str>)>;

fn tokens(r: &Registry, src: &str, spec: &Spec) -> Vec<(String, Option<String>)> {
    run_mode_collect(r, src, spec)
}

fn expect(t: Tokens) -> Vec<(String, Option<String>)> {
    t.into_iter()
        .map(|(a, b)| (a.to_string(), b.map(str::to_string)))
        .collect()
}

/// `{"a": 1}` under `text/javascript` (statement context: a block, then a string
/// expression statement).
fn js_object_tokens() -> Tokens {
    vec![
        ("{", None),
        ("\"a\"", Some("string")),
        (":", None),
        (" ", None),
        ("1", Some("number")),
        ("}", None),
    ]
}

/// `{"a": 1}` with `json: true`: `jsonMode` starts in `expression`
/// (javascript.js:254), so `"a"` is an object key and gets
/// `cx.style + " property"` (javascript.js:535).
fn json_object_tokens() -> Tokens {
    vec![
        ("{", None),
        ("\"a\"", Some("string property")),
        (":", None),
        (" ", None),
        ("1", Some("number")),
        ("}", None),
    ]
}

/// javascript.js:949-952: `defineMIME(<mime>, "javascript")`, i.e. `{name: "javascript"}`
/// with no options; tokens equal plain `text/javascript` (javascript.js:948).
#[test]
fn javascript_mime_aliases_resolve_to_plain_javascript() {
    let r = Registry::with_all_modes();
    for mime in [
        "text/ecmascript",
        "application/javascript",
        "application/x-javascript",
        "application/ecmascript",
    ] {
        let spec = Spec::from(mime);
        assert_eq!(
            r.resolve_mode(&spec),
            ModeSpec::named("javascript"),
            "{mime}"
        );
        assert_eq!(
            r.get_mode(&ModeOptions::default(), &spec).name(),
            "javascript",
            "{mime}"
        );
        assert_eq!(
            tokens(&r, "{\"a\": 1}", &spec),
            expect(js_object_tokens()),
            "{mime}"
        );
        assert_eq!(
            tokens(&r, "{\"a\": 1}", &spec),
            tokens(&r, "{\"a\": 1}", &Spec::from("text/javascript")),
            "{mime}"
        );
    }
}

/// javascript.js:954-955: `defineMIME(<mime>, { name: "javascript", json: true })`.
#[test]
fn json_mime_aliases_resolve_to_javascript_json() {
    let r = Registry::with_all_modes();
    let want = ModeSpec::named("javascript").with("json", ConfigValue::Bool(true));
    for mime in ["application/x-json", "application/manifest+json"] {
        let spec = Spec::from(mime);
        let got = r.resolve_mode(&spec);
        assert_eq!(got, want, "{mime}");
        // Not jsonld (javascript.js:956 is a separate MIME).
        assert_eq!(got.get("jsonld"), None, "{mime}");
        assert_eq!(
            r.get_mode(&ModeOptions::default(), &spec).name(),
            "javascript",
            "{mime}"
        );
        assert_eq!(
            tokens(&r, "{\"a\": 1}", &spec),
            expect(json_object_tokens()),
            "{mime}"
        );
        assert_eq!(
            tokens(&r, "{\"a\": 1}", &spec),
            tokens(&r, "{\"a\": 1}", &Spec::from("application/json")),
            "{mime}"
        );
    }
}

/// `application/manifest+json` is defined explicitly (javascript.js:955), so it is
/// found in `mimeModes` before the `+json` suffix rule of `resolveMode` applies. Both
/// paths give `{name: "javascript", json: true}`; the defined entry is the one used.
#[test]
fn manifest_json_is_a_defined_mime() {
    let r = Registry::with_all_modes();
    assert!(r.has_mime("application/manifest+json"));
    assert!(r.has_mime("application/x-json"));
}

/// xml.js:412: `defineMIME("text/xml", "xml")`: `{name: "xml"}`, no `htmlMode`. In
/// XML `br` is an ordinary tag, so `</br>` closes `<br>`; with `htmlMode: true` `br` is
/// an auto self-closer (xml.js:14-17) and `</br>` is a stray close tag
/// (`"tag error"`, then `"tag bracket error"`).
#[test]
fn text_xml_resolves_to_xml_without_html_mode() {
    let r = Registry::with_all_modes();
    let spec = Spec::from("text/xml");
    let got = r.resolve_mode(&spec);
    assert_eq!(got, ModeSpec::named("xml"));
    assert_eq!(got, r.resolve_mode(&Spec::from("application/xml")));
    assert_eq!(got.get("htmlMode"), None);
    assert_eq!(r.get_mode(&ModeOptions::default(), &spec).name(), "xml");
    assert_eq!(
        tokens(&r, "<br></br>", &spec),
        expect(vec![
            ("<", Some("tag bracket")),
            ("br", Some("tag")),
            (">", Some("tag bracket")),
            ("</", Some("tag bracket")),
            ("br", Some("tag")),
            (">", Some("tag bracket")),
        ])
    );
    let html = Spec::Obj(ModeSpec::named("xml").with("htmlMode", ConfigValue::Bool(true)));
    assert_eq!(
        tokens(&r, "<br></br>", &html),
        expect(vec![
            ("<", Some("tag bracket")),
            ("br", Some("tag")),
            (">", Some("tag bracket")),
            ("</", Some("tag bracket")),
            ("br", Some("tag error")),
            (">", Some("tag bracket error")),
        ])
    );
}
