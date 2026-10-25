//! C1 oracle: compare the util ports with `oracle/cm5-util-baseline.json`, produced
//! by `oracle/cm5-util-dump.mjs` from the CodeMirror 5 sources.

use std::path::PathBuf;

use cm_core::text::to_u16;
use cm_core::util::bidi::{Direction, bidi_ordering, get_bidi_part_at, iterate_bidi_sections};
use cm_core::util::misc::{
    count_column, find_column, find_first, is_extending_char, is_word_char_basic,
    skip_extending_chars,
};
use cm_core::util::string_stream::StringStream;
use serde_json::Value;

fn baseline() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../oracle/cm5-util-baseline.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("util baseline")).unwrap()
}

fn s16(v: &Value) -> Vec<u16> {
    to_u16(v.as_str().unwrap())
}

#[test]
fn is_word_char_basic_all_code_units() {
    let b = baseline();
    let table = b["wordChar"].as_str().unwrap().as_bytes();
    let mut bad = Vec::new();
    for c in 0..0x10000u32 {
        let got = is_word_char_basic(&[c as u16]);
        if got != (table[c as usize] == b'1') {
            bad.push(c);
        }
    }
    assert!(
        bad.is_empty(),
        "{} code units differ, first: {:x?}",
        bad.len(),
        &bad[..bad.len().min(20)]
    );
}

#[test]
fn is_extending_char_all_code_units() {
    let b = baseline();
    let table = b["extending"].as_str().unwrap().as_bytes();
    for c in 0..0x10000u32 {
        assert_eq!(
            is_extending_char(c as u16),
            table[c as usize] == b'1',
            "U+{c:04X}"
        );
    }
}

#[test]
fn count_and_find_column() {
    let b = baseline();
    let cases = b["countColumn"].as_array().unwrap();
    for c in cases {
        let s = s16(&c[0]);
        let end = c[1].as_u64().map(|x| x as usize);
        let got = count_column(
            &s,
            end,
            c[2].as_i64().unwrap(),
            c[3].as_u64().unwrap() as usize,
            c[4].as_i64().unwrap(),
        );
        assert_eq!(got, c[5].as_i64().unwrap(), "countColumn {c}");
    }
    let cases2 = b["findColumn"].as_array().unwrap();
    for c in cases2 {
        let got = find_column(&s16(&c[0]), c[1].as_i64().unwrap(), c[2].as_i64().unwrap());
        assert_eq!(got, c[3].as_i64().unwrap(), "findColumn {c}");
    }
    println!(
        "countColumn {} + findColumn {} cases match",
        cases.len(),
        cases2.len()
    );
}

#[test]
fn bidi_ordering_and_helpers() {
    let b = baseline();
    let cases = b["order"].as_array().unwrap();
    for c in cases {
        let s = s16(&c[0]);
        let dir = if c[1] == "ltr" {
            Direction::Ltr
        } else {
            Direction::Rtl
        };
        let order = bidi_ordering(&s, dir);
        let got: Value = match &order {
            None => Value::Bool(false),
            Some(o) => Value::Array(
                o.iter()
                    .map(|p| serde_json::json!([p.level, p.from, p.to]))
                    .collect(),
            ),
        };
        assert_eq!(got, c[2], "bidi order of {:?} {}", c[0], c[1]);
        if let Some(o) = &order {
            for part in c[3].as_array().unwrap() {
                let ch = part[0].as_u64().unwrap() as usize;
                let (r, other) = get_bidi_part_at(o, ch, part[1] == "before");
                assert_eq!(
                    serde_json::json!([r, other]),
                    serde_json::json!([part[2], part[3]]),
                    "getBidiPartAt {c}"
                );
            }
        }
        let it = &c[4];
        let (from, to) = (
            it[0].as_u64().unwrap() as usize,
            it[1].as_u64().unwrap() as usize,
        );
        let mut secs = Vec::new();
        iterate_bidi_sections(order.as_deref(), from, to, |a, bb, d, i| {
            secs.push(serde_json::json!([
                a,
                bb,
                if d == Direction::Ltr { "ltr" } else { "rtl" },
                i
            ]));
        });
        assert_eq!(Value::Array(secs), it[2], "iterateBidiSections {c}");
    }
    println!("{} bidi cases match", cases.len());
}

#[test]
fn skip_extending_and_find_first() {
    let b = baseline();
    for c in b["skipExtending"].as_array().unwrap() {
        let s: Vec<u16> = c[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u16)
            .collect();
        let got = skip_extending_chars(&s, c[1].as_i64().unwrap(), c[2].as_i64().unwrap());
        assert_eq!(got, c[3].as_i64().unwrap(), "skipExtendingChars {c}");
    }
    for c in b["findFirst"].as_array().unwrap() {
        let (a, bb, t) = (
            c[0].as_i64().unwrap(),
            c[1].as_i64().unwrap(),
            c[2].as_i64().unwrap(),
        );
        let got = if a > bb {
            find_first(|x| x <= t.max(bb), a, bb)
        } else {
            find_first(|x| x >= t.min(bb), a, bb)
        };
        assert_eq!(got, c[3].as_i64().unwrap(), "findFirst {c}");
    }
}

#[test]
fn string_stream_scripts() {
    let b = baseline();
    let cases = b["streams"].as_array().unwrap();
    for c in cases {
        let line = s16(&c[0]);
        let tab = c[1].as_i64();
        let line_start = c[2].as_u64().unwrap() as usize;
        let mut s = StringStream::new(&line, tab, None);
        s.line_start = line_start;
        s.pos = line_start;
        s.start = line_start;
        for step in c[3].as_array().unwrap() {
            let op = step[0].as_str().unwrap();
            let arg = step[1].as_str().unwrap();
            let arg16 = to_u16(arg);
            let ch = |u: Option<u16>| {
                u.map(|u| Value::String(String::from_utf16_lossy(&[u])))
                    .unwrap_or(Value::Null)
            };
            let r: Value = match op {
                "next" => ch(s.next()),
                "peek" => ch(s.peek()),
                "eol" => Value::Bool(s.eol()),
                "sol" => Value::Bool(s.sol()),
                "eatSpace" => Value::Bool(s.eat_space()),
                "skipToEnd" => {
                    s.skip_to_end();
                    Value::Null
                }
                "backUp" => {
                    if s.pos > s.start {
                        s.back_up(1);
                    }
                    Value::Null
                }
                "column" => Value::from(s.column()),
                "indentation" => Value::from(s.indentation()),
                "eat" => ch(s.eat(arg16[0])),
                "eatWhile" => Value::Bool(s.eat_while_char(arg16[0])),
                "skipTo" => {
                    if s.skip_to(&arg16) {
                        Value::Bool(true)
                    } else {
                        Value::Null
                    }
                }
                "matchStr" => {
                    if s.match_str(arg, step[2].as_bool().unwrap(), false) {
                        Value::Bool(true)
                    } else {
                        Value::Null
                    }
                }
                "matchStrCI" => {
                    if s.match_str(arg, true, true) {
                        Value::Bool(true)
                    } else {
                        Value::Null
                    }
                }
                "current" => Value::String(s.current_string()),
                "start" => {
                    s.start = s.pos;
                    Value::Null
                }
                other => panic!("unknown op {other}"),
            };
            assert_eq!(
                serde_json::json!([r, s.pos, s.start]),
                serde_json::json!([step[3], step[4], step[5]]),
                "StringStream {op}({arg:?}) on {:?}",
                c[0]
            );
        }
    }
    println!("{} StringStream scripts match", cases.len());
}
