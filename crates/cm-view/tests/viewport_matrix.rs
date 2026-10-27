//! `update` and `viewportChange` for edits and scrolls around a scrolled viewport,
//! against the CM5 5.65.21 recording `tests/data/cm5-viewport-matrix.json` (made by
//! `oracle/cm5-viewport-matrix.mjs` in Chromium).
//!
//! The recording page measures like [`Metrics::oracle`] (DejaVu Sans Mono 32px,
//! line-height 38px, 4px padding) and sizes the editor 600x300, so rendered ranges,
//! scroll positions and visible lines are compared as exact numbers. Two values are
//! compared only by geometry: CM5 estimates the height of a wrapped line it has not
//! drawn (`estimateHeight`) and reports `getScrollInfo().height` from the DOM, which
//! an operation that does not redraw leaves stale.
//!
//! Each case is a fresh editor over 200 lines (`line N`), scrolled to `top` (3000,
//! unless the case says otherwise), then one action with the log on. CM5's `scroll`
//! event is not ported (src/edit/CodeMirror.js:189), so it is left out of the
//! recorded logs before they are compared.

use std::cell::RefCell;
use std::rc::Rc;

use cm_core::{LinkedDocOptions, Pos};
use cm_modes::{Registry, Spec};
use cm_view::app::{App, AppOptions, Input, MemClipboard};
use cm_view::{Editor, EditorOptions, EditorSet, Metrics};
use serde_json::Value;

type Log = Rc<RefCell<Vec<String>>>;

const RECORDING: &str = include_str!("data/cm5-viewport-matrix.json");

fn recording() -> Value {
    let v: Value = serde_json::from_str(RECORDING).expect("recording JSON");
    assert_eq!(v["version"], "5.65.21");
    v
}

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn ps(p: Pos) -> String {
    format!("{}:{}", p.line, p.ch)
}

/// The recording's `watch(cm, log, tag)`, without `scroll`.
fn watch(cm: &Editor, log: &Log, tag: &str) {
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_change(move |_, c| {
        let text = serde_json::to_string(&c.text).unwrap();
        l.borrow_mut()
            .push(format!("{t}change({},{},{text})", ps(c.from), ps(c.to)));
    });
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_changes(move |_, cs| l.borrow_mut().push(format!("{t}changes({})", cs.len())));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_update(move |_| l.borrow_mut().push(format!("{t}update")));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_viewport_change(move |_, a, b| {
        l.borrow_mut().push(format!("{t}viewportChange({a},{b})"))
    });
}

/// What the recording's `state(cm)` reports that carries over exactly.
#[derive(Debug, PartialEq)]
struct State {
    vp: (i64, i64),
    top: f64,
    left: f64,
    lines: i64,
    visible: (i64, i64),
    first_text: String,
}

fn state(cm: &Editor) -> State {
    // `lineAtHeight(top, "local")`: local coordinates include the top padding.
    let top = cm.display.scroll_top - cm.metrics.padding_top;
    let first = cm.line_at_height(top);
    State {
        vp: (cm.display.view_from, cm.display.view_to),
        top: cm.display.scroll_top,
        left: cm.display.scroll_left,
        lines: cm.doc.line_count() as i64,
        visible: (first, cm.line_at_height(top + cm.display.height)),
        first_text: cm.doc.get_line(first).unwrap_or_default(),
    }
}

fn recorded_state(v: &Value) -> State {
    let pair = |a: &Value| (a[0].as_i64().unwrap(), a[1].as_i64().unwrap());
    State {
        vp: pair(&v["vp"]),
        top: v["top"].as_f64().unwrap(),
        left: v["left"].as_f64().unwrap(),
        lines: v["lines"].as_i64().unwrap(),
        visible: pair(&v["visible"]),
        first_text: v["firstText"].as_str().unwrap().to_string(),
    }
}

fn recorded_log(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .filter(|e| !e.ends_with("scroll"))
        .collect()
}

fn editor(wrap: bool) -> Editor {
    let mut cm = Editor::new(
        &lines(200),
        Spec::Null,
        EditorOptions {
            line_wrapping: wrap,
            ..Default::default()
        },
        Metrics::oracle(),
    );
    cm.set_size(600.0, 300.0);
    cm
}

fn rr(cm: &mut Editor, text: &str, from: Pos, to: Option<Pos>) {
    cm.run_op(|cm| cm.doc.replace_range(text, from, to, None));
}

struct Case {
    name: &'static str,
    wrap: bool,
    top: f64,
    left: Option<f64>,
    setup: fn(&mut Editor),
    act: fn(&mut Editor),
    /// Wrapped-line heights: compare the visible lines by geometry only.
    geometric: bool,
}

fn none(_: &mut Editor) {}
fn long_line_85(cm: &mut Editor) {
    rr(cm, &"h".repeat(120), p(85, 0), None);
}
fn wrap_line_0(cm: &mut Editor) {
    rr(cm, &"w".repeat(70), p(0, 0), None);
}

const fn case(name: &'static str, act: fn(&mut Editor)) -> Case {
    Case {
        name,
        wrap: false,
        top: 3000.0,
        left: None,
        setup: none,
        act,
        geometric: false,
    }
}

fn cases() -> Vec<Case> {
    vec![
        case("m01_above_insert_line", |cm| rr(cm, "X\n", p(0, 0), None)),
        case("m02_below_insert_line", |cm| rr(cm, "X\n", p(190, 0), None)),
        case("m03_inside_char", |cm| rr(cm, "X", p(80, 0), None)),
        case("m03b_inside_insert_line", |cm| {
            rr(cm, "X\n", p(80, 0), None)
        }),
        case("m04a_top_boundary_before", |cm| {
            let f = cm.display.view_from;
            rr(cm, "X\n", p(f - 1, 0), None)
        }),
        case("m04b_top_boundary_first_line", |cm| {
            let f = cm.display.view_from;
            rr(cm, "X\n", p(f, 0), None)
        }),
        case("m04c_top_boundary_char_first_line", |cm| {
            let f = cm.display.view_from;
            rr(cm, "X", p(f, 0), None)
        }),
        case("m04d_top_boundary_char_before", |cm| {
            let f = cm.display.view_from;
            rr(cm, "X", p(f - 1, 0), None)
        }),
        case("m05a_bottom_boundary_last_line", |cm| {
            let t = cm.display.view_to;
            rr(cm, "X\n", p(t - 1, 0), None)
        }),
        case("m05b_bottom_boundary_after", |cm| {
            let t = cm.display.view_to;
            rr(cm, "X\n", p(t, 0), None)
        }),
        case("m05c_bottom_boundary_char_after", |cm| {
            let t = cm.display.view_to;
            rr(cm, "X", p(t, 0), None)
        }),
        case("m06a_cross_top_delete", |cm| {
            let f = cm.display.view_from;
            rr(cm, "", p(f - 2, 0), Some(p(f + 2, 0)))
        }),
        case("m06b_cross_bottom_delete", |cm| {
            let t = cm.display.view_to;
            rr(cm, "", p(t - 2, 0), Some(p(t + 2, 0)))
        }),
        case("m06c_cross_whole_view", |cm| {
            let (f, t) = (cm.display.view_from, cm.display.view_to);
            rr(cm, "Z", p(f - 1, 0), Some(p(t + 1, 0)))
        }),
        case("m07a_above_insert_five", |cm| {
            rr(cm, "a\nb\nc\nd\ne\n", p(3, 0), None)
        }),
        case("m07b_inside_insert_five", |cm| {
            rr(cm, "a\nb\nc\nd\ne\n", p(82, 0), None)
        }),
        case("m08a_above_delete_line", |cm| {
            rr(cm, "", p(0, 0), Some(p(1, 0)))
        }),
        case("m08b_above_delete_three", |cm| {
            rr(cm, "", p(2, 0), Some(p(5, 0)))
        }),
        case("m08c_inside_delete_line", |cm| {
            rr(cm, "", p(81, 0), Some(p(82, 0)))
        }),
        case("m08d_below_delete_line", |cm| {
            rr(cm, "", p(190, 0), Some(p(191, 0)))
        }),
        case("m09a_above_replace_one_line", |cm| {
            rr(cm, "QQQQ", p(0, 0), Some(p(0, 4)))
        }),
        case("m09b_above_replace_multi_same_count", |cm| {
            rr(cm, "a\nb\nc", p(1, 0), Some(p(3, 6)))
        }),
        case("m09c_inside_replace_multi_same_count", |cm| {
            rr(cm, "a\nb\nc", p(81, 0), Some(p(83, 6)))
        }),
        Case {
            wrap: true,
            geometric: true,
            ..case("m10a_wrap_above_grows", wrap_line_0)
        },
        Case {
            wrap: true,
            geometric: true,
            setup: wrap_line_0,
            ..case("m10b_wrap_above_shrinks", |cm| {
                rr(cm, "", p(0, 0), Some(p(0, 70)))
            })
        },
        Case {
            wrap: true,
            geometric: true,
            ..case("m11a_wrap_inside_grows", |cm| {
                let top = cm.display.scroll_top - cm.metrics.padding_top;
                let f = cm.line_at_height(top);
                rr(cm, &"w".repeat(70), p(f + 2, 0), None)
            })
        },
        Case {
            wrap: true,
            geometric: true,
            ..case("m11b_wrap_below_grows", |cm| {
                rr(cm, &"w".repeat(70), p(190, 0), None)
            })
        },
        Case {
            left: Some(200.0),
            setup: long_line_85,
            ..case("m12a_hscroll_above_insert_line", |cm| {
                rr(cm, "X\n", p(0, 0), None)
            })
        },
        Case {
            left: Some(200.0),
            setup: long_line_85,
            ..case("m12b_hscroll_above_char", |cm| rr(cm, "X", p(0, 0), None))
        },
        case("x01_nested_above_and_below", |cm| {
            cm.run_op(|cm| {
                cm.run_op(|cm| cm.doc.replace_range("X\n", p(0, 0), None, None));
                cm.doc.replace_range("Y\n", p(190, 0), None, None);
            })
        }),
        case("x02_multi_insert_then_delete_above", |cm| {
            cm.run_op(|cm| {
                cm.doc.replace_range("X\n", p(0, 0), None, None);
                cm.doc.replace_range("", p(0, 0), Some(p(1, 0)), None);
            })
        }),
        case("x03_multi_two_above", |cm| {
            cm.run_op(|cm| {
                cm.doc.replace_range("X\n", p(0, 0), None, None);
                cm.doc.replace_range("Y\n", p(5, 0), None, None);
            })
        }),
        Case {
            top: 0.0,
            ..case("s01_scroll_op_from_top", |cm| {
                cm.scroll_to(None, Some(3000.0))
            })
        },
        case("s02_scroll_op_same", |cm| cm.scroll_to(None, Some(3000.0))),
        case("s03_scroll_op_small", |cm| {
            cm.scroll_to(None, Some(3000.0 + 2.0 * 38.0))
        }),
        case("s06_empty_op", |cm| cm.run_op(|_| {})),
    ]
}

fn start(c: &Case) -> (Editor, Log) {
    let mut cm = editor(c.wrap);
    (c.setup)(&mut cm);
    cm.scroll_to(c.left, Some(c.top));
    let log: Log = Rc::default();
    watch(&cm, &log, "");
    (cm, log)
}

/// Every edit and scroll case: the same events, in order with the same payloads,
/// the same rendered range before and after, and the same scroll position and
/// visible lines.
#[test]
fn viewport_matrix_matches_cm5() {
    let rec = recording();
    let mut failures = Vec::new();
    for c in cases() {
        let r = &rec["cases"][c.name];
        assert!(r.is_object(), "{} is recorded", c.name);
        let (mut cm, log) = start(&c);
        let before = state(&cm);
        (c.act)(&mut cm);
        let after = state(&cm);
        let got = log.borrow().clone();
        let (want_before, want_after) = (recorded_state(&r["before"]), recorded_state(&r["after"]));
        let want_log = recorded_log(&r["log"]);
        // s01 starts at the top: the rendered range there keeps the lines the
        // editor drew at its default 600px height before `set_size` (CM5's page
        // starts at its CSS height, 300px), so only that range is not compared.
        let want_before = if c.name == "s01_scroll_op_from_top" {
            State {
                vp: before.vp,
                ..want_before
            }
        } else {
            want_before
        };
        let ok = if c.geometric {
            before.vp == want_before.vp
                && after.vp == want_after.vp
                && after.top == want_after.top
                && after.visible.0 >= after.vp.0
                && after.visible.1 <= after.vp.1
                && (before.visible.0 - after.visible.0).signum()
                    == (want_before.visible.0 - want_after.visible.0).signum()
        } else {
            before == want_before && after == want_after
        };
        println!(
            "{:40} vp {:?}->{:?} vis {:?}->{:?} h {} log {:?}",
            c.name,
            before.vp,
            after.vp,
            before.visible,
            after.visible,
            cm.doc_height(),
            got
        );
        if !ok || got != want_log {
            failures.push(format!(
                "{}:\n  got  {before:?}\n    -> {after:?}\n    {got:?}\n  want {want_before:?}\n    -> {want_after:?}\n    {want_log:?}",
                c.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// x04, x05: `doc.replaceRange` on the editor's document outside an operation. CM5
/// runs it as an operation of `doc.cm` (`docMethodOp`); here the editor's part is
/// parked until the host delivers it (D6), then reports the same events.
#[test]
fn direct_document_edit_reports_when_delivered() {
    let rec = recording();
    for name in ["x04_direct_doc_above", "x05_direct_doc_char_above"] {
        let r = &rec["cases"][name];
        let (mut cm, log) = start(&case(name, none));
        assert_eq!(state(&cm), recorded_state(&r["before"]), "{name}");
        let (text, at) = if name == "x04_direct_doc_above" {
            ("X\n", p(0, 0))
        } else {
            ("X", p(0, 0))
        };
        cm.doc.replace_range(text, at, None, None);
        assert!(log.borrow().is_empty(), "{name}: parked until delivered");
        assert!(cm.parked_len() > 0, "{name}");
        cm.flush_parked();
        assert_eq!(*log.borrow(), recorded_log(&r["log"]), "{name}");
        assert_eq!(state(&cm), recorded_state(&r["after"]), "{name}");
    }
}

/// s04, s05: a native scroll (the scroller's `scroll` event, `updateScrollTop` ->
/// `updateDisplaySimple`): far reports `update` and `viewportChange`; a small one
/// inside the rendered range reports nothing.
#[test]
fn native_scroll_through_the_app() {
    let rec = recording();
    for (name, from, dy) in [
        ("s04_native_scroll_far", 0.0, 3000.0),
        ("s05_native_scroll_small", 3000.0, 2.0 * 38.0),
    ] {
        let r = &rec["cases"][name];
        cm_core::clock::manual(1_000_000.0);
        let mut app = App::new(
            &lines(200),
            None,
            Metrics::oracle(),
            &AppOptions {
                no_line_numbers: true,
                mode: Some("text/plain".into()),
                ..Default::default()
            },
            Box::new(MemClipboard::default()),
        );
        app.handle(Input::Resize {
            width: 600.0,
            height: 300.0,
        });
        app.editor.scroll_to(None, Some(from));
        let log: Log = Rc::default();
        watch(&app.editor, &log, "");
        let before = state(&app.editor);
        // As in s01: from the top, the range drawn at the default height stays.
        let vp = if from == 0.0 {
            before.vp
        } else {
            recorded_state(&r["before"]).vp
        };
        assert_eq!(
            before,
            State {
                vp,
                ..recorded_state(&r["before"])
            },
            "{name}"
        );
        app.handle(Input::Wheel { dx: 0.0, dy });
        assert_eq!(*log.borrow(), recorded_log(&r["log"]), "{name}");
        assert_eq!(state(&app.editor), recorded_state(&r["after"]), "{name}");
    }
}

/// s04 through `Editor::set_scroll_top` (`updateScrollTop`) outside an operation:
/// the same events as the scroller's `scroll` event.
#[test]
fn set_scroll_top_reports_like_a_native_scroll() {
    let rec = recording();
    let r = &rec["cases"]["s04_native_scroll_far"];
    let (mut cm, log) = start(&Case {
        top: 0.0,
        ..case("s04_native_scroll_far", none)
    });
    cm.set_scroll_top(3000.0);
    assert_eq!(*log.borrow(), recorded_log(&r["log"]));
    assert_eq!(state(&cm), recorded_state(&r["after"]));
}

/// l01: two editors over linked documents, both scrolled. A line inserted above
/// the view in A reports `update` and `viewportChange` in both, in CM5's order; a
/// one-line edit above in B then reports only the changes.
#[test]
fn linked_documents_in_an_editor_set() {
    let rec = recording();
    let r = &rec["cases"]["l01_linked_above"];
    let mut set = EditorSet::new();
    let a = set.insert(editor(false));
    let doc_b = set
        .get(a)
        .unwrap()
        .doc
        .linked_doc(LinkedDocOptions {
            shared_hist: true,
            from: None,
            to: None,
            mode: None,
        })
        .unwrap();
    let mut eb = Editor::with_doc(
        doc_b,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    eb.set_size(600.0, 300.0);
    let b = set.insert(eb);
    for id in [a, b] {
        set.run(id, |cx| cx.scroll_to(None, Some(3000.0)));
    }
    let log: Log = Rc::default();
    watch(set.get(a).unwrap(), &log, "A:");
    watch(set.get(b).unwrap(), &log, "B:");
    assert_eq!(
        state(set.get(a).unwrap()),
        recorded_state(&r["before"]["a"])
    );
    assert_eq!(
        state(set.get(b).unwrap()),
        recorded_state(&r["before"]["b"])
    );
    set.run(a, |cx| cx.doc.replace_range("X\n", p(0, 0), None, None));
    assert_eq!(
        std::mem::take(&mut *log.borrow_mut()),
        recorded_log(&r["log"])
    );
    assert_eq!(state(set.get(a).unwrap()), recorded_state(&r["after"]["a"]));
    assert_eq!(state(set.get(b).unwrap()), recorded_state(&r["after"]["b"]));
    set.run(b, |cx| cx.doc.replace_range("X", p(0, 0), None, None));
    assert_eq!(*log.borrow(), recorded_log(&r["log2"]));
    assert_eq!(
        state(set.get(a).unwrap()),
        recorded_state(&r["after2"]["a"])
    );
    assert_eq!(
        state(set.get(b).unwrap()),
        recorded_state(&r["after2"]["b"])
    );
}

/// The QA reproduction of M3 (2026-10-26-p1-final-independent-qa,
/// `evidence/qa_edges.rs`), unchanged: a line inserted above the rendered range
/// at a fixed scroll position reports `update`.
#[test]
fn viewport_insertion_above_should_update() {
    let text = (0..200)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = Editor::with_doc(
        cm_core::Doc::new(&text),
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    e.set_size(600., 300.);
    e.run_op(|e| e.scroll_to(None, Some(3000.)));
    let updates = Rc::new(std::cell::Cell::new(0));
    let u = updates.clone();
    e.on_update(move |_| u.set(u.get() + 1));
    e.run_op(|e| e.doc.replace_range("X\n", p(0, 0), None, None));
    assert!(
        updates.get() > 0,
        "CM5 emits update for insertion above viewport at fixed scrollTop"
    );
}
