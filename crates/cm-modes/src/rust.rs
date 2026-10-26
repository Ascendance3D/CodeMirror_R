//! Port of `mode/rust/rust.js` (CodeMirror 5.65.21): the `rust` simple mode and its
//! MIME types `text/x-rustsrc` and `text/rust`.
//!
//! The mode is a [`crate::simple`] table, as upstream's `defineSimpleMode` call is.
//! Rule order, regexp sources, token names, state names and `meta` are copied from
//! the upstream file; `crates/cm-modes/tests/rust_oracle.rs` checks them against
//! the pinned tree.

use std::collections::HashMap;
use std::rc::Rc;

use crate::simple::{RegexSpec, Rule, SimpleStates, TokenSpec, define_simple_mode};
use crate::{ConfigValue, MimeTarget, RegExp, Registry};

fn re(source: &str) -> RegexSpec {
    RegexSpec::re(source, "").expect("rust.js regexps compile")
}

fn tok(style: &str) -> TokenSpec {
    TokenSpec::Str(style.to_string())
}

fn styles(list: &[Option<&str>]) -> TokenSpec {
    TokenSpec::List(list.iter().map(|s| s.map(str::to_string)).collect())
}

fn rule(regex: &str, token: TokenSpec) -> Rule {
    Rule::new(re(regex), token)
}

fn goto(regex: &str, token: TokenSpec, next: &str) -> Rule {
    Rule {
        next: Some(next.to_string()),
        ..rule(regex, token)
    }
}

/// The `states` argument of upstream's `defineSimpleMode("rust", ...)`.
pub fn states() -> SimpleStates {
    let start = vec![
        // string and byte string
        goto(r#"b?""#, tok("string"), "string"),
        // raw string and raw byte string
        goto(r#"b?r""#, tok("string"), "string_raw"),
        goto(r#"b?r#+""#, tok("string"), "string_raw_hash"),
        // character
        rule(
            r#"'(?:[^'\\]|\\(?:[nrt0'"]|x[\da-fA-F]{2}|u\{[\da-fA-F]{6}\}))'"#,
            tok("string-2"),
        ),
        // byte
        rule(
            r#"b'(?:[^']|\\(?:['\\nrt0]|x[\da-fA-F]{2}))'"#,
            tok("string-2"),
        ),
        rule(
            r"(?:(?:[0-9][0-9_]*)(?:(?:[Ee][+-]?[0-9_]+)|\.[0-9_]+(?:[Ee][+-]?[0-9_]+)?)(?:f32|f64)?)|(?:0(?:b[01_]+|(?:o[0-7_]+)|(?:x[0-9a-fA-F_]+))|(?:[0-9][0-9_]*))(?:u8|u16|u32|u64|i8|i16|i32|i64|isize|usize)?",
            tok("number"),
        ),
        rule(
            r"(let(?:\s+mut)?|fn|enum|mod|struct|type|union)(\s+)([a-zA-Z_][a-zA-Z0-9_]*)",
            styles(&[Some("keyword"), None, Some("def")]),
        ),
        rule(
            r"(?:abstract|alignof|as|async|await|box|break|continue|const|crate|do|dyn|else|enum|extern|fn|for|final|if|impl|in|loop|macro|match|mod|move|offsetof|override|priv|proc|pub|pure|ref|return|self|sizeof|static|struct|super|trait|type|typeof|union|unsafe|unsized|use|virtual|where|while|yield)\b",
            tok("keyword"),
        ),
        rule(
            r"\b(?:Self|isize|usize|char|bool|u8|u16|u32|u64|f16|f32|f64|i8|i16|i32|i64|str|Option)\b",
            tok("atom"),
        ),
        rule(r"\b(?:true|false|Some|None|Ok|Err)\b", tok("builtin")),
        rule(
            r"\b(fn)(\s+)([a-zA-Z_][a-zA-Z0-9_]*)",
            styles(&[Some("keyword"), None, Some("def")]),
        ),
        rule(r"#!?\[.*\]", tok("meta")),
        rule(r"\/\/.*", tok("comment")),
        goto(r"\/\*", tok("comment"), "comment"),
        rule(r"[-+\/*=<>!]+", tok("operator")),
        rule(r"[a-zA-Z_]\w*!", tok("variable-3")),
        rule(r"[a-zA-Z_]\w*", tok("variable")),
        Rule {
            indent: true,
            ..rule(r"[\{\[\(]", TokenSpec::Null)
        },
        Rule {
            dedent: true,
            ..rule(r"[\}\]\)]", TokenSpec::Null)
        },
    ];
    let string = vec![
        goto(r#"""#, tok("string"), "start"),
        rule(r#"(?:[^\\"]|\\(?:.|$))*"#, tok("string")),
    ];
    let string_raw = vec![
        goto(r#"""#, tok("string"), "start"),
        rule(r#"[^"]*"#, tok("string")),
    ];
    let string_raw_hash = vec![
        goto(r##""#+"##, tok("string"), "start"),
        rule(r##"(?:[^"]|"(?!#))*"##, tok("string")),
    ];
    let comment = vec![
        goto(r".*?\*\/", tok("comment"), "start"),
        rule(r".*", tok("comment")),
    ];

    let mut meta = HashMap::new();
    meta.insert(
        "dontIndentStates".to_string(),
        ConfigValue::Obj(Rc::new(vec!["comment".to_string()])),
    );
    meta.insert(
        "electricInput".to_string(),
        ConfigValue::Obj(Rc::new(RegExp::new(r"^\s*\}$", "").expect("electricInput"))),
    );
    meta.insert(
        "blockCommentStart".to_string(),
        ConfigValue::Str("/*".into()),
    );
    meta.insert("blockCommentEnd".to_string(), ConfigValue::Str("*/".into()));
    meta.insert("lineComment".to_string(), ConfigValue::Str("//".into()));
    meta.insert("fold".to_string(), ConfigValue::Str("brace".into()));

    SimpleStates {
        states: vec![
            ("start".into(), start),
            ("string".into(), string),
            ("string_raw".into(), string_raw),
            ("string_raw_hash".into(), string_raw_hash),
            ("comment".into(), comment),
        ],
        meta: Some(meta),
    }
}

/// Runs `mode/rust/rust.js`: `defineSimpleMode("rust", ...)` and its two
/// `defineMIME` calls.
pub fn register(r: &mut Registry) {
    define_simple_mode(r, "rust", states()).expect("the rust tables are valid");
    r.define_mime("text/x-rustsrc", MimeTarget::Name("rust".into()));
    r.define_mime("text/rust", MimeTarget::Name("rust".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simple::SimpleState;
    use crate::{JsArg, ModeOptions, Spec, State, copy_state, start_state};
    use cm_core::text::to_u16;
    use cm_core::util::string_stream::StringStream;

    fn tokens(
        mode: &dyn crate::Mode,
        state: &mut State,
        line: &str,
    ) -> Vec<(String, Option<String>)> {
        let text = to_u16(line);
        let mut stream = StringStream::new(&text, Some(4), None);
        let mut out = Vec::new();
        while !stream.eol() {
            let style = mode.token(&mut stream, state);
            out.push((stream.current_string(), style));
            stream.start = stream.pos;
        }
        out
    }

    #[test]
    fn rust_is_a_simple_mode() {
        let mut r = Registry::new();
        register(&mut r);
        assert!(r.has_mode("rust"));
        let mode = r.get_mode(&ModeOptions::default(), &Spec::from("text/x-rustsrc"));
        assert_eq!(mode.name(), "rust");
        let st = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        let s = st.get::<SimpleState>();
        assert_eq!(
            (s.state.as_str(), s.indent.as_deref()),
            ("start", Some(&[][..]))
        );
        assert_eq!(mode.prop("fold").unwrap().as_str(), Some("brace"));
    }

    #[test]
    fn copied_state_is_independent() {
        let mut r = Registry::new();
        register(&mut r);
        let options = ModeOptions {
            indent_unit: 4,
            ..ModeOptions::default()
        };
        let mode = r.get_mode(&options, &Spec::from("rust"));
        let mut a = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        tokens(&*mode, &mut a, "fn f() { /* open");
        let b = copy_state(&*mode, &a);
        tokens(&*mode, &mut a, "*/ }");
        let (sa, sb) = (a.get::<SimpleState>(), b.get::<SimpleState>());
        assert_eq!(
            (sa.state.as_str(), sa.indent.as_deref()),
            ("start", Some(&[][..]))
        );
        assert_eq!(
            (sb.state.as_str(), sb.indent.as_deref()),
            ("comment", Some(&[4.0][..]))
        );
    }

    /// `stream.match` runs on `string.slice(pos)`, so `\b` sees no character before
    /// the stream position: `f32` after `1` is an atom, as in CM5.
    #[test]
    fn word_boundary_is_relative_to_the_stream_position() {
        let mut r = Registry::new();
        register(&mut r);
        let mode = r.get_mode(&ModeOptions::default(), &Spec::from("rust"));
        let mut st = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        let got = tokens(&*mode, &mut st, "1f32 let mut x");
        let want: Vec<(String, Option<String>)> = [
            ("1", Some("number")),
            ("f32", Some("atom")),
            (" ", None),
            ("let mut", Some("keyword")),
            (" ", None),
            ("x", Some("def")),
        ]
        .iter()
        .map(|(t, s)| (t.to_string(), s.map(str::to_string)))
        .collect();
        assert_eq!(got, want);
    }
}
