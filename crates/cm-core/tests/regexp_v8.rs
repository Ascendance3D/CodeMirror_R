//! V8 differential for `cm_core::util::regexp` under the `i`, `u` and `iu` flags,
//! and for `StringStream::match_regexp` against CodeMirror 5 `StringStream.match`.
//!
//! Baseline: `oracle/regexp-v8-baseline.json`, produced by
//! `oracle/regexp-v8-dump.mjs` (Node/V8; the runtime is recorded in the file).
//! The exhaustive case-class comparison lives in the `regexp.rs` unit tests.

use std::path::PathBuf;

use cm_core::text::to_u16;
use cm_core::util::regexp::RegExp;
use cm_core::util::string_stream::StringStream;
use serde_json::Value;

fn baseline() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../oracle/regexp-v8-baseline.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("regexp baseline")).unwrap()
}

fn st(v: &Value) -> &str {
    v.as_str().unwrap()
}

/// "-" no match, "E" SyntaxError, else "index:length" (the baseline's tokens).
fn tok(pattern: &str, flags: &str, input: &[u16]) -> String {
    match RegExp::new(pattern, flags) {
        Err(_) => "E".into(),
        Ok(re) => match re.exec(input) {
            Some(m) => format!("{}:{}", m.index(), m.len()),
            None => "-".into(),
        },
    }
}

fn report(bad: &[String], total: usize) {
    assert!(
        bad.is_empty(),
        "{} of {total} differ from V8:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn triage_cases() {
    let b = baseline();
    let cases = b["original"].as_array().unwrap();
    assert_eq!(cases.len(), 30);
    let mut bad = Vec::new();
    for c in cases {
        let (p, f, s, want) = (st(&c[0]), st(&c[1]), st(&c[2]), st(&c[3]));
        let got = tok(p, f, &to_u16(s));
        if got != want {
            bad.push(format!("/{p}/{f} on {s:?}: rust {got}, v8 {want}"));
        }
    }
    report(&bad, cases.len());
}

#[test]
fn flag_matrix() {
    let b = baseline();
    let inputs: Vec<Vec<u16>> = b["matrix"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            st(v)
                .split(' ')
                .map(|h| u16::from_str_radix(h, 16).unwrap())
                .collect()
        })
        .collect();
    let rows = b["matrix"]["rows"].as_array().unwrap();
    let mut bad = Vec::new();
    let mut total = 0;
    for r in rows {
        let (p, f) = (st(&r[0]), st(&r[1]));
        for (input, want) in inputs.iter().zip(st(&r[2]).split(' ')) {
            total += 1;
            let got = tok(p, f, input);
            if got != want {
                bad.push(format!("/{p}/{f} on {:04x?}: rust {got}, v8 {want}", input));
            }
        }
    }
    assert_eq!(total, 280 * 50);
    report(&bad, total);
}

#[test]
fn exec_groups() {
    let b = baseline();
    let cases = b["exec"].as_array().unwrap();
    let mut bad = Vec::new();
    for c in cases {
        let (p, f, s) = (st(&c[0]), st(&c[1]), to_u16(st(&c[2])));
        let from = c[3].as_u64().unwrap() as usize;
        let got = match RegExp::new(p, f) {
            Err(_) => Value::from("E"),
            Ok(re) => match re.exec_from(&s, from) {
                None => Value::Null,
                Some(m) => Value::Array(
                    (0..m.count())
                        .map(|i| match m.group(i) {
                            Some((a, b)) => serde_json::json!([a, b]),
                            None => Value::Null,
                        })
                        .collect(),
                ),
            },
        };
        if got != c[4] {
            bad.push(format!(
                "/{p}/{f} on {:?} from {from}: rust {got}, v8 {}",
                st(&c[2]),
                c[4]
            ));
        }
    }
    report(&bad, cases.len());
}

/// CM5 `StringStream.match(regexp, consume)`. Without `g` the JS result is the
/// exec array; with `g` it is the list of whole matches and has no `index`, and
/// the Rust `Match` is the first match with its capture groups (see
/// `match_regexp`), so only group 0 is compared there.
#[test]
fn string_stream_match() {
    let b = baseline();
    let cases = b["stream"].as_array().unwrap();
    let mut bad = Vec::new();
    for c in cases {
        let line = to_u16(st(&c[0]));
        let pos = c[1].as_u64().unwrap() as usize;
        let (p, f) = (st(&c[2]), st(&c[3]));
        let consume = c[4].as_bool().unwrap();
        let re = RegExp::new(p, f).unwrap();
        let mut s = StringStream::new(&line, Some(4), None);
        s.pos = pos;
        let m = s.match_regexp(&re, consume);
        let rest = &line[pos..];
        let text = |i: usize| -> Value {
            match m.as_ref().and_then(|m| m.text(rest, i)) {
                Some(t) => Value::from(t.to_vec()),
                None => Value::Null,
            }
        };
        let got = match &m {
            None => Value::Null,
            Some(m) if re.flags().global => Value::Array(vec![text(0)]),
            Some(m) => Value::Array((0..m.count()).map(text).collect()),
        };
        let want = match &c[5] {
            Value::Array(a) if re.flags().global => Value::Array(vec![a[0].clone()]),
            v => v.clone(),
        };
        let want_index = if m.is_some() && !re.flags().global {
            Value::from(0)
        } else {
            Value::Null
        };
        let want_pos = c[7].as_u64().unwrap() as usize;
        if got != want || c[6] != want_index || s.pos != want_pos {
            bad.push(format!(
                "{:?} @{pos} /{p}/{f} consume={consume}: rust {got} pos {}, cm5 {} index {} pos {want_pos}",
                st(&c[0]),
                s.pos,
                c[5],
                c[6]
            ));
        }
    }
    report(&bad, cases.len());
}

/// Every pair in a V8 case class matches both ways, as a literal and in a class.
/// (The `regexp.rs` unit test also checks that no other pair matches.)
#[test]
fn case_classes_pairwise() {
    let b = baseline();
    let mut bad = Vec::new();
    let mut total = 0;
    for (flags, unicode) in [("iu", true), ("i", false)] {
        let classes = b["orbits"][if unicode { "u" } else { "i" }]["classes"]
            .as_array()
            .unwrap();
        for class in classes {
            let cps: Vec<u32> = st(class)
                .split(' ')
                .map(|h| u32::from_str_radix(h, 16).unwrap())
                .collect();
            for &a in &cps {
                let ch = char::from_u32(a).unwrap();
                let lit = format!("^\\u{{{a:x}}}$");
                let lit = if unicode {
                    lit
                } else {
                    format!("^\\u{a:04x}$")
                };
                let cls = format!("^[{}]$", &lit[1..lit.len() - 1]);
                let neg = format!("^[^{}]$", &lit[1..lit.len() - 1]);
                let (lit, cls, neg) = (
                    RegExp::new(&lit, flags).unwrap(),
                    RegExp::new(&cls, flags).unwrap(),
                    RegExp::new(&neg, flags).unwrap(),
                );
                for &d in &cps {
                    total += 1;
                    let s = to_u16(&char::from_u32(d).unwrap().to_string());
                    if !lit.test(&s) || !cls.test(&s) || neg.test(&s) {
                        bad.push(format!("/{ch}/{flags} vs U+{d:04X}"));
                    }
                }
            }
        }
    }
    report(&bad, total);
}
