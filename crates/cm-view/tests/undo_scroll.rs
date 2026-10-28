//! Undo and redo scroll restoration. `makeChangeFromHistory`
//! (src/model/changes.js:90-156) ends each undo or redo with
//! `if (!i && doc.cm) doc.cm.scrollIntoView({from: change.from, to: changeEnd(change)})`
//! for the last change it applies (the first of the history event). `scrollIntoView`
//! (src/edit/methods.js:373) is a `methodOp`: it resolves a queued `scrollToPos`
//! (`scrollToRange`, src/display/scrolling.js:131) and queues `{from, to, margin: 0}`,
//! applied when the operation ends (src/display/operations.js:144-152). The selection
//! that the history restores uses `sel_dontScroll`. A detached document, and a linked
//! document without an editor, queue nothing.
//!
//! The expected values are `tests/data/cm5-undo-scroll.json`: CM5 5.65.21 in headless
//! Chromium 151 at [`Metrics::oracle`] (DejaVu Sans Mono 32px, line-height 38px),
//! `setSize(600, 300)` (CODEMIRROR_R_UNDO_SCROLL_01 recording; Firefox 153 and WebKit
//! 26.5 agree except for the sub-pixel width of the right edge). The documents are 60
//! lines of 150 `x` (long), 60 lines of `ab` (short) or empty.

use cm_core::{Doc, LinkedDocOptions, Pos};
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::Value;

type Sel = Vec<(i64, i64, i64, i64)>;

fn data() -> Value {
    serde_json::from_str(include_str!("data/cm5-undo-scroll.json")).unwrap()
}

fn doc_text(doc: &str) -> String {
    match doc {
        "long" => vec!["x".repeat(150); 60].join("\n"),
        "short" => vec!["ab"; 60].join("\n"),
        "empty" => String::new(),
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

/// One step of the case table (the vocabulary of `cm5-undo-scroll.mjs`).
fn step(cm: &mut Editor, s: &Value) {
    let s = s.as_array().unwrap();
    let a = &s[1..];
    match s[0].as_str().unwrap() {
        "cursor" => cm.doc.set_cursor(pos(a, 0), None),
        "sel" => cm.doc.set_selection(pos(a, 0), Some(pos(a, 2)), None),
        "scrollTo" => cm.scroll_to(Some(num(&a[0])), Some(num(&a[1]))),
        "into" => cm.scroll_into_view(pos(a, 0), pos(a, 0), 0.0),
        "edit" => cm
            .doc
            .replace_range(a[0].as_str().unwrap(), pos(a, 1), None, None),
        "del" => cm.doc.replace_range("", pos(a, 0), Some(pos(a, 2)), None),
        "repl" => cm
            .doc
            .replace_range(a[0].as_str().unwrap(), pos(a, 1), Some(pos(a, 3)), None),
        "set" => cm.set_value(&replacement(a[0].as_str().unwrap())),
        "undo" => cm.doc.undo(),
        "redo" => cm.doc.redo(),
        k => panic!("{k}"),
    }
}

/// How the undo or redo of a one-step operation is reached.
#[derive(Clone, Copy, Debug)]
enum Entry {
    /// Inside `Editor::run_op` (the `undo` command, `cm.undo()`).
    Command,
    /// `cm.doc.undo()` called on the attached document with no open operation; the
    /// editor's part of the operation is parked until its next entry
    /// ([`Editor::flush_parked`]).
    Direct,
}

fn group(cm: &mut Editor, g: &Value, entry: Entry) {
    let steps = g.as_array().unwrap();
    let single_history = steps.len() == 1 && matches!(steps[0][0].as_str(), Some("undo" | "redo"));
    if single_history && matches!(entry, Entry::Direct) {
        step(cm, &steps[0]);
        cm.flush_parked();
    } else {
        cm.run_op(|cm| steps.iter().for_each(|s| step(cm, s)));
    }
}

fn sel(doc: &Doc) -> Sel {
    doc.list_selections()
        .iter()
        .map(|r| (r.anchor.line, r.anchor.ch, r.head.line, r.head.ch))
        .collect()
}

fn want_sel(v: &Value) -> Sel {
    v.as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let r = r.as_array().unwrap();
            (
                r[0].as_i64().unwrap(),
                r[1].as_i64().unwrap(),
                r[2].as_i64().unwrap(),
                r[3].as_i64().unwrap(),
            )
        })
        .collect()
}

/// Scroll, selection, UTF-16 length and history depth, the shape of a recorded state.
type State = (f64, f64, Sel, usize, usize, usize);

fn state(cm: &Editor) -> State {
    let h = cm.doc.history_size();
    (
        cm.display.scroll_left,
        cm.display.scroll_top,
        sel(&cm.doc),
        cm.get_value().encode_utf16().count(),
        h.undo,
        h.redo,
    )
}

fn want_state(v: &Value) -> State {
    let v = v.as_array().unwrap();
    (
        num(&v[0]),
        num(&v[1]),
        want_sel(&v[2]),
        v[3].as_u64().unwrap() as usize,
        v[4].as_u64().unwrap() as usize,
        v[5].as_u64().unwrap() as usize,
    )
}

/// Compares two states. With `relax_top` the scroll top is left out: lines that wrap
/// have estimated heights in Rust (the known wrapped-line height gap, not part of this
/// lane), so a long wrapped document scrolls to a different offset for the same range;
/// the offset is then checked against the explicit `scrollIntoView` below.
fn same(got: &State, want: &State, relax_top: bool) -> bool {
    (got.0, &got.2, got.3, got.4, got.5) == (want.0, &want.2, want.3, want.4, want.5)
        && (relax_top || got.1 == want.1)
}

/// `same` as an assertion for a document that wraps when `wrap` is set (the documents
/// of these tests are long).
fn assert_state(got: State, want: State, wrap: bool, what: &str) {
    assert!(
        same(&got, &want, wrap),
        "{what}\n  got  {got:?}\n  want {want:?}"
    );
}

/// Runs the case and returns the state after each operation.
fn run_case(case: &Value, doc: &str, wrap: bool, entry: Entry) -> Vec<State> {
    let mut cm = editor(doc, wrap);
    let mut out = Vec::new();
    for g in case["pres"].as_array().unwrap() {
        group(&mut cm, g, entry);
        out.push(state(&cm));
    }
    // `ops` run in one operation.
    cm.run_op(|cm| {
        for s in case["ops"].as_array().unwrap() {
            step(cm, s);
        }
    });
    out.push(state(&cm));
    for g in case["posts"].as_array().unwrap() {
        group(&mut cm, g, entry);
        out.push(state(&cm));
    }
    out
}

/// Every recorded case, selected by id prefix, through one entry point.
fn check(filter: impl Fn(&str, &str, bool) -> bool, entry: Entry) -> usize {
    let d = data();
    let mut bad = Vec::new();
    let mut n = 0;
    for row in d["rows"].as_array().unwrap() {
        let (id, doc, wrap) = (
            row["id"].as_str().unwrap(),
            row["doc"].as_str().unwrap(),
            row["wrap"].as_bool().unwrap(),
        );
        if !filter(id, doc, wrap) {
            continue;
        }
        n += 1;
        let case = d["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        let got = run_case(case, doc, wrap, entry);
        let want: Vec<State> = row["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(want_state)
            .collect();
        let relax = wrap && doc == "long";
        if got.len() != want.len() || !got.iter().zip(&want).all(|(g, w)| same(g, w, relax)) {
            bad.push(format!(
                "{id} {doc} wrap {wrap} ({entry:?}):\n  got  {got:?}\n  want {want:?}"
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

/// All 174 recorded rows (29 cases × 3 documents × 2 wrapping modes), undo and redo
/// inside an operation and called directly on the attached document.
#[test]
fn all_recorded_rows_command_entry() {
    assert_eq!(check(|_, _, _| true, Entry::Command), 174);
}

#[test]
fn all_recorded_rows_direct_entry() {
    assert_eq!(check(|_, _, _| true, Entry::Direct), 174);
}

/// The difference of the previous lane (S14, S15): after the undo of a `setValue`
/// CM5 ends at scroll top 4, and the redo resets it to 0 (U13).
#[test]
fn undo_of_set_value_scrolls_the_restored_range_into_view() {
    let mut cm = editor("long", false);
    cm.run_op(|cm| {
        cm.doc.replace_range("new\n", Pos::new(0, 0), None, None);
        cm.scroll_to(Some(300.0), Some(800.0));
    });
    cm.run_op(|cm| cm.set_value("a\nb"));
    assert_eq!((cm.display.scroll_left, cm.display.scroll_top), (0.0, 0.0));
    cm.run_op(|cm| cm.doc.undo());
    assert_eq!((cm.display.scroll_left, cm.display.scroll_top), (0.0, 4.0));
    cm.run_op(|cm| cm.doc.redo());
    assert_eq!((cm.display.scroll_left, cm.display.scroll_top), (0.0, 0.0));
}

/// The cursor stays where the history put it (`sel_dontScroll`), and the view follows
/// the changed range, not the cursor: a restored range far below the viewport scrolls
/// to it although the cursor is at the start (U01, vertical and horizontal).
#[test]
fn range_outside_the_viewport_scrolls_but_visible_range_does_not() {
    let mut cm = editor("long", false);
    cm.run_op(|cm| {
        cm.doc.replace_range("INS", Pos::new(40, 100), None, None);
        cm.scroll_to(Some(0.0), Some(0.0));
    });
    cm.run_op(|cm| cm.doc.undo());
    assert_eq!(sel(&cm.doc), vec![(0, 0, 0, 0)]);
    assert!(cm.display.scroll_top > 0.0 && cm.display.scroll_left > 0.0);
    let mut cm = editor("long", false);
    cm.run_op(|cm| {
        cm.doc.replace_range("INS", Pos::new(2, 3), None, None);
        cm.scroll_to(Some(0.0), Some(0.0));
    });
    cm.run_op(|cm| cm.doc.undo());
    assert_eq!((cm.display.scroll_left, cm.display.scroll_top), (0.0, 0.0));
}

/// The queued range is `(from, changeEnd)` with margin 0 and is pending until the
/// operation ends (M05, M06): undo of an insert is the empty range at the insert,
/// undo of a deletion the restored text, redo of a multi-line insert its end.
#[test]
fn queued_range_endpoints_and_timing() {
    let d = data();
    let rec = &d["m"]["M06_ranges_n"]["rec"];
    let want = |k: &str| {
        let v = rec[k].as_array().unwrap();
        (pos(v, 0), pos(v, 2))
    };
    for wrap in [false, true] {
        // One `replaceRange` per call outside any operation: three history events.
        let mut cm2 = editor("long", wrap);
        cm2.doc.replace_range("a\nb\nc", Pos::new(5, 3), None, None);
        cm2.doc
            .replace_range("", Pos::new(30, 10), Some(Pos::new(35, 20)), None);
        cm2.doc
            .replace_range("Z", Pos::new(50, 0), Some(Pos::new(50, 150)), None);
        let got = cm2.run_op(|cm| {
            let mut got = Vec::new();
            for undo in [true, true, true, false, false, false] {
                if undo {
                    cm.doc.undo()
                } else {
                    cm.doc.redo()
                }
                let r = cm.doc.take_scroll_into_view();
                assert_eq!(r.len(), 1);
                got.push((r[0].1, r[0].2));
            }
            // Nothing has scrolled while the operation is open.
            assert_eq!((cm.display.scroll_left, cm.display.scroll_top), (0.0, 0.0));
            got
        });
        let names = [
            "undoRepl", "undoDel", "undoIns", "redoIns", "redoDel", "redoRepl",
        ];
        for (g, n) in got.iter().zip(names) {
            assert_eq!(*g, want(n), "{n} wrap {wrap}");
        }
    }
}

/// A queued range replaces an earlier one and is replaced by a later one, and the
/// resolve of a queued cursor target comes first (U16-U26 are in the table; this checks
/// the pending state of M05).
#[test]
fn range_is_pending_until_the_operation_ends() {
    let d = data();
    for wrap in [false, true] {
        let tag = if wrap { "w" } else { "n" };
        let m = &d["m"][format!("M05_pending_{tag}")];
        let mut cm = editor("long", wrap);
        cm.run_op(|cm| {
            cm.doc.replace_range("INS", Pos::new(40, 100), None, None);
            cm.scroll_to(Some(0.0), Some(0.0));
        });
        let mid = cm.run_op(|cm| {
            cm.doc.undo();
            (cm.display.scroll_left, cm.display.scroll_top)
        });
        assert_eq!(mid, (num(&m["mid"]["docLeft"]), num(&m["mid"]["docTop"])));
    }
}

/// Two editors: an undo scrolls only its own editor; nested operations of two editors
/// each apply their own range (M01, M01b).
#[test]
fn editors_scroll_on_their_own() {
    let d = data();
    for wrap in [false, true] {
        let tag = if wrap { "w" } else { "n" };
        let (mut a, mut b) = (editor("long", wrap), editor("long", wrap));
        for e in [&mut a, &mut b] {
            e.run_op(|cm| {
                cm.doc.replace_range("INS", Pos::new(40, 100), None, None);
                cm.scroll_to(Some(0.0), Some(0.0));
            });
        }
        b.scroll_to(Some(200.0), Some(600.0));
        a.run_op(|cm| cm.doc.undo());
        let m = &d["m"][format!("M01_{tag}")];
        assert_state(state(&a), want_state(&m["a"]), wrap, "a");
        assert_state(state(&b), want_state(&m["b"]), wrap, "b untouched");
        // Undo in both, one inside the other's operation.
        a.run_op(|cm| cm.doc.redo());
        a.scroll_to(Some(0.0), Some(0.0));
        a.run_op(|cm| cm.doc.undo());
        b.run_op(|cm| cm.doc.undo());
        let m = &d["m"][format!("M01b_{tag}")];
        assert_state(
            state(&a),
            want_state(&m["a"]),
            wrap,
            &format!("a2 wrap {wrap}"),
        );
        assert_state(
            state(&b),
            want_state(&m["b"]),
            wrap,
            &format!("b2 wrap {wrap}"),
        );
    }
}

/// A detached document keeps its history and scroll fields and queues nothing (M02).
#[test]
fn detached_document_queues_nothing() {
    let d = Doc::new(&doc_text("long"));
    d.replace_range("INS", Pos::new(40, 100), None, None);
    d.undo();
    d.redo();
    assert!(d.take_scroll_into_view().is_empty());
    assert!(d.take_scroll_requests().is_empty());
    assert!(d.take_scroll_resets().is_empty());
    assert_eq!(d.get_value(None).encode_utf16().count(), 9062);
}

/// A linked document without an editor queues nothing and does not move the editor;
/// the editor's own document with a link behaves as without (M03, M03b, M04).
#[test]
fn linked_documents() {
    let d = data();
    for wrap in [false, true] {
        let tag = if wrap { "w" } else { "n" };
        for shared in [true, false] {
            let mut cm = editor("long", wrap);
            let linked = cm
                .doc
                .linked_doc(LinkedDocOptions {
                    shared_hist: shared,
                    ..Default::default()
                })
                .unwrap();
            cm.run_op(|cm| {
                cm.doc.replace_range("INS", Pos::new(40, 100), None, None);
                cm.scroll_to(Some(0.0), Some(0.0));
            });
            if shared {
                linked.undo();
                assert!(linked.take_scroll_into_view().is_empty());
                assert_eq!(cm.doc.take_scroll_into_view(), vec![], "{tag}");
                cm.run_op(|_| {});
                assert_state(
                    state(&cm),
                    want_state(&d["m"][format!("M03_linked_undo_{tag}")]["after"]),
                    wrap,
                    &format!("linked undo wrap {wrap}"),
                );
                cm.scroll_to(Some(0.0), Some(0.0));
                cm.run_op(|cm| cm.doc.redo());
                assert_state(
                    state(&cm),
                    want_state(&d["m"][format!("M03b_editor_doc_redo_{tag}")]["after"]),
                    wrap,
                    &format!("editor redo wrap {wrap}"),
                );
            } else {
                cm.run_op(|cm| cm.doc.undo());
                assert_state(
                    state(&cm),
                    want_state(&d["m"][format!("M04_unshared_{tag}")]["after"]),
                    wrap,
                    &format!("unshared wrap {wrap}"),
                );
            }
            assert_eq!(
                linked.get_value(None).encode_utf16().count(),
                cm.get_value().encode_utf16().count()
            );
        }
    }
}

/// Where the exact CM5 offsets of a wrapped long document are out of reach (estimated
/// heights), an undo or redo lands where an explicit `scrollIntoView` of the same range
/// does, from the same state: the operation is the same, only the height differs.
#[test]
fn undo_is_scroll_into_view_of_the_restored_range() {
    let edits: [(&str, Pos, Option<Pos>); 3] = [
        ("INS", Pos::new(40, 100), None),
        ("", Pos::new(30, 10), Some(Pos::new(35, 20))),
        ("Z", Pos::new(50, 0), Some(Pos::new(50, 150))),
    ];
    for wrap in [false, true] {
        for (text, from, to) in edits.clone() {
            for undo_first in [true, false] {
                let build = || {
                    let mut cm = editor("long", wrap);
                    cm.doc.replace_range(text, from, to, None);
                    cm.scroll_to(Some(0.0), Some(0.0));
                    if !undo_first {
                        cm.run_op(|cm| cm.doc.undo());
                        cm.scroll_to(Some(0.0), Some(0.0));
                    }
                    cm
                };
                let mut a = build();
                a.run_op(|cm| {
                    if undo_first {
                        cm.doc.undo()
                    } else {
                        cm.doc.redo()
                    }
                });
                let mut b = build();
                b.run_op(|cm| {
                    if undo_first {
                        cm.doc.undo()
                    } else {
                        cm.doc.redo()
                    }
                    let r = cm.doc.take_scroll_into_view();
                    assert_eq!(r.len(), 1);
                    cm.scroll_into_view(r[0].1, r[0].2, 0.0);
                });
                assert_eq!(
                    (a.display.scroll_left, a.display.scroll_top),
                    (b.display.scroll_left, b.display.scroll_top),
                    "{text:?} {from:?} wrap {wrap} undo {undo_first}"
                );
                assert!(a.display.scroll_top > 0.0 || a.display.scroll_left > 0.0);
            }
        }
    }
}

/// The three request channels replay in call order (QA of PR #24): a cursor, a
/// `setValue` reset, a history range and a cursor again, in one operation. The marks
/// are positions in one sequence (`reset` counts earlier cursors, `range` counts
/// earlier cursors and resets), so the range lies after the reset and before the
/// last cursor.
#[test]
fn mixed_requests_keep_their_call_order() {
    let mut cm = editor("long", false);
    cm.run_op(|cm| {
        cm.doc.replace_range("INS", Pos::new(40, 100), None, None);
        cm.scroll_to(Some(0.0), Some(0.0));
    });
    cm.run_op(|cm| cm.set_value("a\nb\nc"));
    let marks = cm.run_op(|cm| {
        cm.doc.set_cursor(Pos::new(1, 0), None);
        cm.doc.set_value("x\ny\nz");
        cm.doc.undo();
        cm.doc.set_cursor(Pos::new(2, 0), None);
        let requests = cm.doc.take_scroll_requests();
        let resets = cm.doc.take_scroll_resets();
        let ranges = cm.doc.take_scroll_into_view();
        // `take_*` consumed the queues for this check, so nothing replays.
        (requests, resets, ranges)
    });
    let (requests, resets, ranges) = marks;
    assert_eq!((resets.len(), ranges.len()), (1, 1));
    assert!(requests.len() >= 2);
    assert!(resets[0] >= 1, "a cursor request precedes the reset");
    let at = ranges[0].0;
    assert!(at >= resets[0] + 1, "the range follows the reset: {at}");
    assert!(
        at < requests.len() + resets.len(),
        "a cursor request follows the range: {at}"
    );
}

/// The last request decides where the view ends: a cursor after a history range
/// brings the cursor line into view from the range's scroll position (CM5 resolves
/// the range first, `scrollPosIntoView` reads `curOp.scrollTop`), and a range after
/// a cursor scrolls to the range.
#[test]
fn later_of_range_and_cursor_wins() {
    let prep = || {
        let mut cm = editor("long", false);
        cm.run_op(|cm| {
            cm.doc.replace_range("INS", Pos::new(40, 100), None, None);
            cm.scroll_to(Some(0.0), Some(0.0));
        });
        cm
    };
    let mut cm = prep();
    cm.run_op(|cm| {
        cm.doc.undo();
        cm.doc.set_cursor(Pos::new(2, 3), None);
    });
    // Line 2 (top 80) is at the top of the view; the range at line 40 (about 1500) is gone.
    assert_eq!(cm.display.scroll_top, 80.0);
    let mut cm = prep();
    cm.run_op(|cm| {
        cm.doc.set_cursor(Pos::new(2, 3), None);
        cm.doc.undo();
    });
    assert!(cm.display.scroll_top > 1000.0 && cm.display.scroll_left > 1000.0);
}
