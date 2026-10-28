//! Scroll offsets after the document shrinks. CodeMirror JavaScript never clamps them:
//! `setDocumentHeight` (src/display/update_display.js:263) sets the sizer's `minHeight`
//! and the browser clamps the scroller's `scrollTop`/`scrollLeft`, which
//! `endOperation_finish` then copies to `doc.scrollTop` (src/display/operations.js:163).
//! This happens whether or not a scroll position is queued. The Rust editor has no
//! browser scroller, so `Editor::end_operation_display` clamps the offsets to
//! [`Editor::max_scroll`] before it resolves the operation's queued scrolls.
//!
//! The expected values are `tests/data/cm5-scroll-clamp.json`: CM5 5.65.21 in headless
//! Chromium 151 at [`Metrics::oracle`], `setSize(600, 300)` (CODEMIRROR_R_SCROLL_CLAMP_01
//! recording). The documents are 60 lines of 150 `x` (long) or 60 lines of `ab` (short).

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::Value;

type Sel = Vec<(i64, i64, i64, i64)>;
/// Scroll left and top, selections, UTF-16 length, undo and redo depth.
type State = (f64, f64, Sel, usize, usize, usize);

fn data() -> Value {
    serde_json::from_str(include_str!("data/cm5-scroll-clamp.json")).unwrap()
}

fn doc_text(doc: &str) -> String {
    match doc {
        "long" => vec!["x".repeat(150); 60].join("\n"),
        "short" => vec!["ab"; 60].join("\n"),
        _ => panic!("{doc}"),
    }
}

fn replacement(name: &str) -> String {
    match name {
        "short" => "a\nb".into(),
        "ylong" => vec!["y".repeat(150); 60].join("\n"),
        _ => panic!("{name}"),
    }
}

fn editor(doc: &str, wrap: bool) -> Editor {
    let options = EditorOptions {
        line_wrapping: wrap,
        ..EditorOptions::default()
    };
    let mut cm = Editor::new(&doc_text(doc), Spec::Null, options, Metrics::oracle());
    cm.set_size(600.0, 300.0);
    cm
}

fn pos(v: &[Value], i: usize) -> Pos {
    Pos::new(v[i].as_i64().unwrap(), v[i + 1].as_i64().unwrap())
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// One step of the case table (the vocabulary of `cm5-shrink.mjs`).
fn step(cm: &mut Editor, s: &Value) {
    let s = s.as_array().unwrap();
    let a = &s[1..];
    match s[0].as_str().unwrap() {
        "cursor" => cm.doc.set_cursor(pos(a, 0), None),
        "scrollTo" => cm.scroll_to(Some(num(&a[0])), Some(num(&a[1]))),
        "into" => cm.scroll_into_view(pos(a, 0), pos(a, 0), 0.0),
        "edit" => cm
            .doc
            .replace_range(a[0].as_str().unwrap(), pos(a, 1), None, None),
        "del" | "ddel" => cm.doc.replace_range("", pos(a, 0), Some(pos(a, 2)), None),
        "repl" => cm
            .doc
            .replace_range(a[0].as_str().unwrap(), pos(a, 1), Some(pos(a, 3)), None),
        "set" => cm.set_value(&replacement(a[0].as_str().unwrap())),
        "dset" => cm.doc.set_value(&replacement(a[0].as_str().unwrap())),
        "shrinkcols" => {
            let keep = a[0].as_i64().unwrap();
            for i in 0..cm.doc.line_count() as i64 {
                let n = cm.doc.get_line(i).unwrap().encode_utf16().count() as i64;
                if n > keep {
                    cm.doc
                        .replace_range("", Pos::new(i, keep), Some(Pos::new(i, n)), None);
                }
            }
        }
        "undo" => cm.doc.undo(),
        "redo" => cm.doc.redo(),
        k => panic!("{k}"),
    }
}

fn state(cm: &Editor) -> State {
    let h = cm.doc.history_size();
    let sels = cm
        .doc
        .list_selections()
        .iter()
        .map(|r| (r.anchor.line, r.anchor.ch, r.head.line, r.head.ch))
        .collect();
    (
        cm.display.scroll_left,
        cm.display.scroll_top,
        sels,
        cm.get_value().encode_utf16().count(),
        h.undo,
        h.redo,
    )
}

fn want_state(v: &Value) -> State {
    let v = v.as_array().unwrap();
    let sels = v[2]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let r: Vec<i64> = r
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_i64().unwrap())
                .collect();
            (r[0], r[1], r[2], r[3])
        })
        .collect();
    (
        num(&v[0]),
        num(&v[1]),
        sels,
        v[3].as_u64().unwrap() as usize,
        v[4].as_u64().unwrap() as usize,
        v[5].as_u64().unwrap() as usize,
    )
}

/// Runs a case. The `D` cases call `Doc` methods with no open operation; the editor's
/// part is parked until its next entry (`Editor::flush_parked`).
fn run_case(case: &Value, doc: &str, wrap: bool) -> Vec<State> {
    let direct = case["id"].as_str().unwrap().starts_with('D');
    let mut cm = editor(doc, wrap);
    let mut out = Vec::new();
    let group = |cm: &mut Editor, steps: &Value| {
        if direct && steps.as_array().unwrap().iter().all(|s| s[0] != "scrollTo") {
            steps.as_array().unwrap().iter().for_each(|s| step(cm, s));
            cm.flush_parked();
        } else {
            cm.run_op(|cm| steps.as_array().unwrap().iter().for_each(|s| step(cm, s)));
        }
    };
    for g in case["pres"].as_array().unwrap() {
        group(&mut cm, g);
        out.push(state(&cm));
    }
    group(&mut cm, &case["ops"]);
    out.push(state(&cm));
    for g in case["posts"].as_array().unwrap() {
        group(&mut cm, g);
        out.push(state(&cm));
    }
    out
}

/// Every recorded row. A wrapped long document has estimated line heights in Rust (the
/// known wrapped-line height gap, outside this lane), so its `scrollTop` is not compared
/// with the recording; `all_states_are_inside_the_content` still bounds it.
fn check(only: impl Fn(&str, &str, bool) -> bool) -> usize {
    let d = data();
    let mut bad = Vec::new();
    let mut n = 0;
    for row in d["rows"].as_array().unwrap() {
        let (id, doc, wrap) = (
            row["id"].as_str().unwrap(),
            row["doc"].as_str().unwrap(),
            row["wrap"].as_bool().unwrap(),
        );
        if !only(id, doc, wrap) {
            continue;
        }
        n += 1;
        let case = d["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        let got = run_case(case, doc, wrap);
        let want: Vec<State> = row["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(want_state)
            .collect();
        let relax = wrap && doc == "long";
        // `scrollLeft` of the browser is an integer; the Rust width of the longest line
        // is not (the known right-edge precision gap, under 1 px).
        let same = |g: &State, w: &State| {
            (&g.2, g.3, g.4, g.5) == (&w.2, w.3, w.4, w.5)
                && (g.0 - w.0).abs() < 1.0
                && (relax || g.1 == w.1)
        };
        if got.len() != want.len() || !got.iter().zip(&want).all(|(g, w)| same(g, w)) {
            bad.push(format!(
                "{id} {doc} wrap {wrap}:\n  got  {got:?}\n  want {want:?}"
            ));
        }
    }
    assert!(n > 0);
    assert!(
        bad.is_empty(),
        "{} of {n} rows differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
    n
}

/// All 108 recorded rows (27 cases × 2 documents × 2 wrapping modes).
#[test]
fn all_recorded_rows() {
    assert_eq!(check(|_, _, _| true), 108);
}

/// The original defect (probes D01/D02 of the undo-scroll lane): a shrink with no queued
/// scroll position leaves `(300, 800)` in place instead of `(300, 0)` / `(0, 0)`.
#[test]
fn shrink_without_a_queued_scroll_clamps() {
    let mut cm = editor("long", false);
    cm.scroll_to(Some(300.0), Some(800.0));
    cm.run_op(|cm| {
        cm.doc
            .replace_range("", Pos::new(5, 0), Some(Pos::new(59, 150)), None)
    });
    assert_eq!(
        (cm.display.scroll_left, cm.display.scroll_top),
        (300.0, 0.0)
    );
    let mut cm = editor("long", false);
    cm.scroll_to(Some(300.0), Some(800.0));
    cm.run_op(|cm| cm.doc.set_value("a\nb"));
    assert_eq!((cm.display.scroll_left, cm.display.scroll_top), (0.0, 0.0));
}

/// A valid offset is kept, not reset to zero: the shrink leaves the content taller than
/// the offset needs (S06, S08, S09, S10).
#[test]
fn valid_offsets_are_kept() {
    check(|id, _, _| matches!(id, "S06" | "S08" | "S09" | "S10"));
    let mut cm = editor("long", false);
    cm.scroll_to(Some(300.0), Some(800.0));
    cm.run_op(|cm| {
        cm.doc
            .replace_range("", Pos::new(50, 0), Some(Pos::new(59, 150)), None)
    });
    assert_eq!(
        (cm.display.scroll_left, cm.display.scroll_top),
        (300.0, 800.0)
    );
}

/// Each axis is clamped on its own (S01 vertical, S02 horizontal, S07 the longest line).
#[test]
fn axes_clamp_independently() {
    check(|id, _, _| matches!(id, "S01" | "S02" | "S07"));
}

/// A queued position still decides the final offset (P01-P10): cursor, range, reset
/// and explicit scrolls, before and after the shrink in the same operation.
#[test]
fn pending_requests_keep_their_order() {
    check(|id, _, _| id.starts_with('P'));
}

/// Undo and redo (H01-H03) and direct `Doc` calls (D01, D02) end up where CM5 does.
#[test]
fn history_and_direct_document_operations() {
    check(|id, _, _| id.starts_with('H') || id.starts_with('D'));
}

/// Two editors: shrinking one leaves the other's offsets alone (M01).
#[test]
fn another_editor_is_untouched() {
    let d = data();
    for wrap in [false, true] {
        let mut a = editor("long", wrap);
        let mut b = editor("long", wrap);
        a.scroll_to(Some(300.0), Some(800.0));
        b.scroll_to(Some(300.0), Some(800.0));
        a.run_op(|cm| {
            cm.doc
                .replace_range("", Pos::new(5, 0), Some(Pos::new(59, 150)), None)
        });
        let want = d["m"][if wrap { "M01_w" } else { "M01_n" }]
            .as_array()
            .unwrap();
        let (wa, wb) = (want_state(&want[0]), want_state(&want[1]));
        assert_eq!(a.display.scroll_left, wa.0);
        assert!(wrap || a.display.scroll_top == wa.1);
        assert_eq!((b.display.scroll_left, b.display.scroll_top), (wb.0, wb.1));
    }
}

/// In every recorded state the offsets are inside the content, also where the wrapped
/// height estimate differs from the browser.
#[test]
fn all_states_are_inside_the_content() {
    let d = data();
    for case in d["cases"].as_array().unwrap() {
        for doc in ["long", "short"] {
            for wrap in [false, true] {
                let direct = case["id"].as_str().unwrap().starts_with('D');
                let mut cm = editor(doc, wrap);
                let mut groups: Vec<&Value> = case["pres"].as_array().unwrap().iter().collect();
                groups.push(&case["ops"]);
                groups.extend(case["posts"].as_array().unwrap());
                for g in groups {
                    let steps = g.as_array().unwrap();
                    if direct && steps.iter().all(|s| s[0] != "scrollTo") {
                        steps.iter().for_each(|s| step(&mut cm, s));
                        cm.flush_parked();
                    } else {
                        cm.run_op(|cm| steps.iter().for_each(|s| step(cm, s)));
                    }
                    let (mh, mw) = cm.max_scroll();
                    assert!(
                        (0.0..=mw).contains(&cm.display.scroll_left)
                            && (0.0..=mh).contains(&cm.display.scroll_top),
                        "{} {doc} wrap {wrap}: ({}, {}) outside (0..={mw}, 0..={mh})",
                        case["id"],
                        cm.display.scroll_left,
                        cm.display.scroll_top
                    );
                }
            }
        }
    }
}

/// Out-of-range offsets from the public scroll calls (QA of PR #70). CM5 5.65.21 in
/// Chromium 151 (`scrollTo(v, v)` on 60 lines of 150 `x`, 600x300): NaN, -5 and
/// -Infinity end at (0, 0), +Infinity at the maximum (2297, 1988).
#[test]
fn non_finite_and_negative_offsets() {
    for v in [f64::NAN, -5.0, f64::NEG_INFINITY] {
        let mut cm = editor("long", false);
        cm.scroll_to(Some(v), Some(v));
        assert_eq!(
            (cm.display.scroll_left, cm.display.scroll_top),
            (0.0, 0.0),
            "{v}"
        );
        let mut cm = editor("long", false);
        cm.scroll_to(Some(300.0), Some(800.0));
        cm.set_scroll_top(v);
        cm.set_scroll_left(v);
        assert_eq!(
            (cm.display.scroll_left, cm.display.scroll_top),
            (0.0, 0.0),
            "{v}"
        );
    }
    let mut cm = editor("long", false);
    cm.scroll_to(Some(f64::INFINITY), Some(f64::INFINITY));
    let (mh, mw) = cm.max_scroll();
    assert_eq!((cm.display.scroll_left, cm.display.scroll_top), (mw, mh));
}

/// An offset left negative or NaN in the public `display` is brought inside by the
/// next operation, not only the upper bound (`clamp_scroll`).
#[test]
fn clamp_covers_the_lower_bound() {
    for v in [-100.0, f64::NAN] {
        let mut cm = editor("long", false);
        cm.display.scroll_left = v;
        cm.display.scroll_top = v;
        cm.run_op(|cm| {
            cm.doc
                .replace_range("", Pos::new(5, 0), Some(Pos::new(59, 150)), None)
        });
        assert_eq!(
            (cm.display.scroll_left, cm.display.scroll_top),
            (0.0, 0.0),
            "{v}"
        );
    }
}
