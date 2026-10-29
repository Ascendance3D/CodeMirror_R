//! `StringStream` before the start of its line (issue #61, G13 #12; `backUp` in
//! `src/util/StringStream.js:47`) against `tests/data/cm5-string-stream.json`, recorded
//! by `oracle/cm5-string-stream.mjs` on CodeMirror 5.65.21 in Chromium, Firefox and
//! WebKit (identical output). Each step is checked for its result and the stream's
//! `pos` and `start` afterwards; a negative JS position is `pos as isize` here.
//!
//! The JS `next()` / `eat("")` return `""` where `charAt` is out of range and
//! `undefined` at the end of the line; Rust returns `None` for both, and the recorded
//! `pos` tells them apart.

use std::path::PathBuf;

use cm_core::text::to_u16;
use cm_core::util::regexp::RegExp;
use cm_core::util::string_stream::StringStream;
use serde_json::{Value, json};

fn data() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-string-stream.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-string-stream.json")).unwrap()
}

fn ch(c: Option<u16>) -> Value {
    match c {
        Some(c) => json!(String::from_utf16_lossy(&[c])),
        None => json!("undefined"),
    }
}

fn undef_or(b: bool, v: Value) -> Value {
    if b { v } else { json!("undefined") }
}

/// Runs one recorded op; returns the result in the recording's normal form.
fn apply(s: &mut StringStream, op: &str) -> Value {
    let mut parts = op.splitn(3, ':');
    let name = parts.next().unwrap();
    let a = parts.next();
    let b = parts.next();
    let consume = b != Some("false");
    match name {
        "backUp" => {
            s.back_up(a.unwrap().parse().unwrap());
            json!("undefined")
        }
        "setStart" => {
            s.start = s.pos;
            json!("undefined")
        }
        "pos" => json!(s.pos as isize),
        "current" => json!(s.current_string()),
        "peek" => ch(s.peek()),
        "next" => ch(s.next()),
        "eol" => json!(s.eol()),
        "sol" => json!(s.sol()),
        "column" => json!(s.column()),
        "indentation" => json!(s.indentation()),
        "match" => undef_or(s.match_str(a.unwrap(), consume, false), json!(true)),
        "matchRe" => {
            let re = RegExp::new(a.unwrap(), "").unwrap();
            let before = s.pos as isize;
            let rest_from = s.string.len() as isize + before.min(0);
            match s.match_regexp(&re, consume) {
                Some(m) => {
                    let from = if before < 0 {
                        rest_from.max(0) as usize
                    } else {
                        before as usize
                    };
                    let (i, l) = (m.index(), m.len());
                    json!(String::from_utf16_lossy(&s.string[from + i..from + i + l]))
                }
                None => json!("null"),
            }
        }
        "eat" => match a {
            Some(c) => ch(s.eat(to_u16(c)[0])),
            None => {
                // `eat("")`: the JS returns the `""` it matched.
                if s.eat_str("") {
                    json!("")
                } else {
                    json!("undefined")
                }
            }
        },
        "eatRe" => {
            let re = RegExp::new(a.unwrap(), "").unwrap();
            ch(s.eat_fn(|c| re.exec(&[c]).is_some()))
        }
        "eatWhile" => {
            let re = RegExp::new(a.unwrap(), "").unwrap();
            json!(s.eat_while(|c| re.exec(&[c]).is_some()))
        }
        "eatSpace" => json!(s.eat_space()),
        "skipTo" => undef_or(s.skip_to(&to_u16(a.unwrap())), json!(true)),
        "skipToEnd" => {
            s.skip_to_end();
            json!("undefined")
        }
        _ => panic!("unknown op {op}"),
    }
}

#[test]
fn string_stream_negative_positions_match_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_object().unwrap();
    assert!(cases.len() >= 11, "recording lost cases: {}", cases.len());
    let mut checked = 0;
    for (name, case) in cases {
        let text = to_u16(case["text"].as_str().unwrap());
        let mut s = StringStream::new(&text, Some(4), None);
        for step in case["steps"].as_array().unwrap() {
            let op = step[0].as_str().unwrap();
            let got = apply(&mut s, op);
            let want = &step[1];
            // `""` (charAt out of range) and `undefined` are both `None` in Rust.
            let same = &got == want || (got == json!("undefined") && want == &json!(""));
            assert!(same, "{name} {op}: got {got}, CM5 {want}");
            assert_eq!(
                (s.pos as isize, s.start as isize),
                (
                    step[2].as_i64().unwrap() as isize,
                    step[3].as_i64().unwrap() as isize
                ),
                "{name} {op}: pos/start"
            );
            checked += 1;
        }
    }
    assert!(checked >= 80, "{checked} steps");
}
