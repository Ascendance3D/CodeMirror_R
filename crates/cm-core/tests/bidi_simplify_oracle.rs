//! `bidiSimplify` (issue #32: src/edit/mouse_events.js:337) against
//! `tests/data/cm5-bidi-simplify.json`, recorded by `oracle/cm5-bidi-simplify.mjs`,
//! which runs the pinned function text with the real `getOrder`, `getBidiPartAt`,
//! `cmp`, `Pos` and `Range` in node. Every anchor (each ch, both stickies) of seven
//! mixed-direction lines is combined with every head on its line and with heads on the
//! lines around it, in LTR and RTL documents, with the order `bidiSimplify` computes
//! itself (`bidiOrdering(text, undefined)`) and with an RTL order cached on the line.

use std::path::PathBuf;

use cm_core::text::to_u16;
use cm_core::util::bidi::{BidiSpan, Direction, bidi_ordering_js, bidi_simplify};
use cm_core::{Pos, SelRange, Sticky};
use serde_json::{Value, json};

fn data() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-bidi-simplify.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("cm5-bidi-simplify.json")).unwrap()
}

fn sticky(v: &Value) -> Option<Sticky> {
    match v.as_str() {
        Some("before") => Some(Sticky::Before),
        Some("after") => Some(Sticky::After),
        _ => None,
    }
}

fn sticky_name(s: Option<Sticky>) -> Value {
    match s {
        Some(Sticky::Before) => json!("before"),
        Some(Sticky::After) => json!("after"),
        None => Value::Null,
    }
}

#[test]
fn bidi_simplify_matches_cm5() {
    let d = data();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 21);
    let (mut checked, mut changed) = (0, 0);
    for case in cases {
        let text = to_u16(case["text"].as_str().unwrap());
        let doc_dir = if case["direction"] == "rtl" {
            Direction::Rtl
        } else {
            Direction::Ltr
        };
        // `getOrder(line)` with no direction, or the RTL order cached by rendering.
        let para = (case["cached"] == true).then_some(Direction::Rtl);
        let order = bidi_ordering_js(&text, para);
        let order_json = order.as_ref().map_or(Value::Null, |o| {
            Value::Array(
                o.iter()
                    .map(|p: &BidiSpan| json!([p.level, p.from, p.to]))
                    .collect(),
            )
        });
        assert_eq!(order_json, case["order"], "{case:?} order");
        for r in case["results"].as_array().unwrap() {
            let anchor = Pos {
                line: 1,
                ch: r[0].as_i64().unwrap(),
                sticky: sticky(&r[1]),
            };
            let head = Pos {
                line: r[2].as_i64().unwrap(),
                ch: r[3].as_i64().unwrap(),
                sticky: sticky(&r[4]),
            };
            let range = SelRange::new(anchor, head);
            let got = bidi_simplify(range, order.as_deref(), doc_dir);
            assert_eq!(got.head, head);
            let got_json = if got == range {
                Value::Null
            } else {
                assert_eq!(got.anchor.line, 1);
                changed += 1;
                json!([got.anchor.ch, sticky_name(got.anchor.sticky)])
            };
            assert_eq!(
                got_json, r[5],
                "{:?} {:?} {r}",
                case["text"], case["direction"]
            );
            checked += 1;
        }
    }
    assert!(
        checked > 5000 && changed > 1000,
        "{checked} checked, {changed} changed"
    );
}
