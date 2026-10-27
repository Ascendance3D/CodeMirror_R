//! Regressions for QA finding R1 (an editor on a linked subview panicked at
//! `highlight.rs` "line in range" when its parent changed above the subview) and
//! for event delivery around `shiftDoc` (ledger row 160).
//!
//! The values come from `tests/data/cm5-linked-subview.json`
//! (`oracle/cm5-linked-subview.mjs`, CodeMirror 5.65.21): case `c01_insert_above`
//! and the `throws` scenarios. The rendered-range guard (`Display::invalid_views`)
//! has no CM5 equivalent and is tested on its own.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use cm_core::operation_group::group_open;
use cm_core::{EditorId, LinkedDocOptions, Pos};
use cm_modes::{Registry, Spec};
use cm_view::{Editor, EditorOptions, EditorSet, Metrics};
use serde_json::Value;

type Log = Rc<RefCell<Vec<String>>>;

const RECORDING: &str = include_str!("data/cm5-linked-subview.json");

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}
fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}
fn options(numbers: bool) -> EditorOptions {
    EditorOptions {
        line_numbers: numbers,
        ..Default::default()
    }
}
fn parent(numbers: bool, mode: Spec) -> Editor {
    let mut a = Editor::new(&lines(200), mode, options(numbers), Metrics::oracle());
    a.set_size(600.0, 300.0);
    a
}
/// An editor on `linkedDoc({sharedHist: true, from, to})` of `a`.
fn sub(a: &Editor, from: i64, to: i64, numbers: bool, mode: Spec) -> Editor {
    let d = a
        .doc
        .linked_doc(LinkedDocOptions {
            shared_hist: true,
            from: Some(from),
            to: Some(to),
            mode: None,
        })
        .unwrap();
    let mut b = Editor::with_doc(
        d,
        mode,
        options(numbers),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    b.set_size(600.0, 300.0);
    b
}
fn watch(cm: &Editor, log: &Log, t: &'static str) {
    let l = log.clone();
    cm.on_change(move |_, _| l.borrow_mut().push(format!("{t}change")));
    let l = log.clone();
    cm.on_changes(move |_, _| l.borrow_mut().push(format!("{t}changes")));
    let l = log.clone();
    cm.on_update(move |_| l.borrow_mut().push(format!("{t}update")));
    let l = log.clone();
    cm.on_viewport_change(move |_, a, b| {
        l.borrow_mut().push(format!("{t}viewportChange({a},{b})"))
    });
}
fn view(cm: &Editor) -> (i64, i64) {
    (cm.display.view_from, cm.display.view_to)
}
/// A and B (lines 50..150) in one set, A scrolled to 3000 and B to 400, as in the
/// recording.
fn set_r1(numbers: bool) -> (EditorSet, EditorId, EditorId) {
    let mut set = EditorSet::new();
    let a = parent(numbers, Spec::Null);
    let b = sub(&a, 50, 150, numbers, Spec::Null);
    let (ai, bi) = (set.insert(a), set.insert(b));
    for t in [2000.0, 3000.0] {
        set.run(ai, |cx| cx.scroll_to(None, Some(t)));
    }
    for t in [2000.0, 400.0] {
        set.run(bi, |cx| cx.scroll_to(None, Some(t)));
    }
    (set, ai, bi)
}
fn no_invalid(cm: &Editor) {
    assert_eq!(
        cm.display.invalid_views.get(),
        0,
        "{:?}",
        cm.display.last_invalid_view.get()
    );
}

/// QA R1 in an `EditorSet`: A inserts three lines above B's subview. B moves to
/// first line 53 and view (53, 81) with `update` and `viewportChange`, and A's own
/// `changes` and `update` are delivered (CM5, c01_insert_above).
#[test]
fn r1_parent_insert_above_subview_in_editor_set() {
    for numbers in [false, true] {
        let (mut set, ai, bi) = set_r1(numbers);
        assert_eq!(view(set.get(bi).unwrap()), (50, 78));
        let log: Log = Rc::default();
        watch(set.get(ai).unwrap(), &log, "A:");
        watch(set.get(bi).unwrap(), &log, "B:");
        set.run(ai, |cx| {
            cx.doc.replace_range("P\nQ\nR\n", p(10, 0), None, None)
        });
        let b = set.get(bi).unwrap();
        assert_eq!(b.doc.first_line(), 53);
        assert_eq!(view(b), (53, 81));
        assert_eq!(view(set.get(ai).unwrap()), (68, 99));
        assert_eq!(
            *log.borrow(),
            [
                "A:change",
                "A:changes",
                "A:update",
                "A:viewportChange(68,99)",
                "B:update",
                "B:viewportChange(53,81)"
            ]
        );
        assert!(!group_open());
        no_invalid(set.get(ai).unwrap());
        no_invalid(set.get(bi).unwrap());
        // Both editors stay usable.
        log.borrow_mut().clear();
        set.run(bi, |cx| cx.doc.replace_range("X", p(53, 0), None, None));
        set.run(ai, |cx| cx.doc.replace_range("Y\n", p(60, 0), None, None));
        assert_eq!(set.get(bi).unwrap().doc.get_line(53).unwrap(), "Xline 50");
        assert_eq!(view(set.get(bi).unwrap()), (53, 82));
        assert_eq!(view(set.get(ai).unwrap()), (68, 100));
    }
}

/// The same edit with separate editors (decision D6): B's operation ends at its
/// next entry, then reports the range CM5 reports at once. This is the
/// version-neutral QA probe `qa2_shift_min.rs`.
#[test]
fn r1_parent_insert_above_subview_separate_editors() {
    for numbers in [false, true] {
        let a = parent(numbers, Spec::Null);
        let mut b = sub(&a, 50, 150, numbers, Spec::Null);
        b.scroll_to(None, Some(2000.0));
        b.scroll_to(None, Some(400.0));
        assert_eq!((b.doc.first_line(), view(&b)), (50, (50, 78)));
        let log: Log = Rc::default();
        watch(&b, &log, "B:");
        a.doc.replace_range("P\nQ\nR\n", p(10, 0), None, None);
        b.scroll_to(None, Some(410.0));
        assert_eq!((b.doc.first_line(), view(&b)), (53, (53, 81)));
        assert_eq!(*log.borrow(), ["B:update", "B:viewportChange(53,81)"]);
        no_invalid(&b);
    }
}

/// A deletion above, an insertion at the first line, a deletion across it, and
/// detaching B after a shift: no panic, and B's range stays inside its lines.
#[test]
fn shifts_then_detach_and_reattach() {
    let (mut set, ai, bi) = set_r1(true);
    set.run(ai, |cx| {
        cx.doc.replace_range("", p(10, 0), Some(p(13, 0)), None)
    });
    assert_eq!(view(set.get(bi).unwrap()), (47, 75));
    set.run(ai, |cx| {
        cx.doc.replace_range("", p(45, 0), Some(p(49, 0)), None)
    });
    let b = set.get(bi).unwrap();
    assert_eq!(b.doc.first_line(), 45);
    let (f, t) = view(b);
    assert!(f >= 45 && t <= 45 + b.doc.line_count() as i64);
    no_invalid(b);
    let b = set.remove(bi).unwrap();
    let doc = b.doc.clone();
    drop(b);
    set.run(ai, |cx| cx.doc.replace_range("S\nT\n", p(0, 0), None, None));
    assert_eq!(doc.first_line(), 47);
    let mut b = Editor::with_doc(
        doc,
        Spec::Null,
        options(true),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    b.set_size(600.0, 300.0);
    let bi = set.insert(b);
    set.run(bi, |cx| cx.scroll_to(None, Some(400.0)));
    set.run(ai, |cx| cx.doc.replace_range("U\n", p(0, 0), None, None));
    let b = set.get(bi).unwrap();
    assert_eq!(b.doc.first_line(), 48);
    let (f, t) = view(b);
    assert!(f >= 48 && t <= 48 + b.doc.line_count() as i64);
    no_invalid(b);
}

/// A mode with a start state on the subview: highlighting states saved before the
/// shift, the frontier and the rendered lines stay consistent.
#[test]
fn shift_with_a_stateful_mode() {
    let mut set = EditorSet::new();
    let a = parent(false, Spec::from("javascript"));
    let b = sub(&a, 50, 150, false, Spec::from("javascript"));
    let (ai, bi) = (set.insert(a), set.insert(b));
    set.run(bi, |cx| cx.scroll_to(None, Some(2000.0)));
    set.run(bi, |cx| cx.scroll_to(None, Some(400.0)));
    while set.get(bi).unwrap().highlight_step(50) {}
    set.run(ai, |cx| {
        cx.doc.replace_range("/*\n*/\nx\n", p(10, 0), None, None)
    });
    set.run(ai, |cx| {
        cx.doc.replace_range("", p(0, 0), Some(p(5, 0)), None)
    });
    let b = set.get(bi).unwrap();
    assert_eq!(b.doc.first_line(), 48);
    while b.highlight_step(50) {}
    let (f, t) = view(b);
    for n in f..t {
        b.line_styles(n);
    }
    b.render_view_styles();
    no_invalid(b);
}

/// A range made invalid from outside (the fields are public) that the bail-out of
/// `updateDisplayIfNeeded` would keep (the visible lines are inside it, nothing is
/// dirty) is detected at the end of the next operation, recorded, and computed
/// again; the operation reports a redraw. Nothing panics. (A range the visible
/// lines fall outside of is moved inside by `adjustView`, as in CM5.)
#[test]
fn invalid_range_is_recorded_and_recomputed() {
    let mut b = parent(false, Spec::Null);
    b.scroll_to(None, Some(2000.0));
    b.scroll_to(None, Some(400.0));
    let good = view(&b);
    let log: Log = Rc::default();
    watch(&b, &log, "B:");
    b.display.view_to = 300;
    b.scroll_to(None, Some(400.0));
    assert_eq!(view(&b), good);
    assert_eq!(b.display.invalid_views.get(), 1);
    let bad = b.display.last_invalid_view.get().unwrap();
    assert_eq!(
        (bad.view, bad.doc, bad.at),
        ((good.0, 300), (0, 200), "display")
    );
    // The range ends where it was last reported: `update` only.
    assert_eq!(*log.borrow(), ["B:update"]);
    // Out-of-range lines are also caught when the visible lines are outside.
    b.display.view_from = 500;
    b.display.view_to = 530;
    b.scroll_to(None, Some(400.0));
    assert_eq!(view(&b), good);
    assert_eq!(b.display.invalid_views.get(), 1);
}

/// `render_view_styles` called while B's display phase is still parked (separate
/// editors, D6) draws nothing instead of reading lines before `first`.
#[test]
fn render_while_parked_draws_nothing() {
    let a = parent(false, Spec::Null);
    let mut b = sub(&a, 50, 150, false, Spec::Null);
    b.scroll_to(None, Some(2000.0));
    b.scroll_to(None, Some(400.0));
    a.doc.replace_range("P\nQ\nR\n", p(10, 0), None, None);
    assert_eq!(b.parked_len(), 1);
    b.render_view_styles();
    assert_eq!(b.display.invalid_views.get(), 1);
    assert_eq!(b.display.last_invalid_view.get().unwrap().at, "render");
    b.scroll_to(None, Some(400.0));
    assert_eq!(view(&b), (53, 81));
    assert_eq!(b.display.invalid_views.get(), 1);
}

/// `linkedDoc({from: 50, to: 50})`: CM5 builds a corrupt two-line document
/// (`"undefined\nundefined"`, recorded). Rust must at least not panic when the
/// parent changes around it.
#[test]
fn empty_subview_does_not_panic() {
    let rec: Value = serde_json::from_str(RECORDING).unwrap();
    assert_eq!(rec["empty_subview"]["value"], "undefined\nundefined");
    let mut set = EditorSet::new();
    let a = parent(false, Spec::Null);
    let r = a.doc.linked_doc(LinkedDocOptions {
        shared_hist: true,
        from: Some(50),
        to: Some(50),
        mode: None,
    });
    let ai = set.insert(a);
    if let Ok(d) = r {
        let mut b = Editor::with_doc(
            d,
            Spec::Null,
            EditorOptions::default(),
            Metrics::oracle(),
            Rc::new(Registry::with_all_modes()),
        );
        b.set_size(600.0, 300.0);
        let bi = set.insert(b);
        set.run(ai, |cx| cx.doc.replace_range("P\n", p(10, 0), None, None));
        set.run(ai, |cx| {
            cx.doc.replace_range("", p(0, 0), Some(p(3, 0)), None)
        });
        set.run(bi, |cx| cx.scroll_to(None, Some(10.0)));
        no_invalid(set.get(bi).unwrap());
    }
}

/// B's `update` handler runs a nested operation on B (a scroll) during the
/// operation that shifts it; B's parked work and A's events are delivered once.
#[test]
fn nested_operation_from_handler_during_shift() {
    let (mut set, ai, bi) = set_r1(false);
    let log: Log = Rc::default();
    watch(set.get(ai).unwrap(), &log, "A:");
    watch(set.get(bi).unwrap(), &log, "B:");
    let once = Rc::new(Cell::new(false));
    let o = once.clone();
    set.get(bi).unwrap().on_update(move |cx| {
        if !o.replace(true) {
            cx.scroll_to(None, Some(1200.0));
        }
    });
    set.run(ai, |cx| {
        cx.doc.replace_range("P\nQ\nR\n", p(10, 0), None, None)
    });
    let got = log.borrow().clone();
    assert_eq!(got.iter().filter(|e| *e == "A:changes").count(), 1);
    assert_eq!(got.iter().filter(|e| *e == "A:update").count(), 1);
    assert_eq!(
        got[..4],
        [
            "A:change",
            "A:changes",
            "A:update",
            "A:viewportChange(68,99)"
        ]
    );
    let b = set.get(bi).unwrap();
    assert_eq!(b.display.scroll_top, 1200.0);
    let (f, t) = view(b);
    assert!(f >= 53 && t <= 153);
    assert_eq!(b.parked_len(), 0);
    assert!(!group_open());
    no_invalid(b);
}

/// One handler panics in the operation that shifts B (CM5 `throws`). The events of
/// that operation match CM5, and so does the next operation, except where noted.
///
/// Known difference (not R1): CM5 runs `postUpdateDisplay`, which signals `update`
/// and `viewportChange` and sets `reportedViewFrom/To`, in `endOperation_finish`.
/// A throw in A's finish phase aborts `endOperations` before B's finish, so B's
/// reported range stays old and B's next update reports `viewportChange` again.
/// cm-view records the reported range in the display phase, so B's skipped
/// `viewportChange(53,81)` is not reported later.
#[test]
fn handler_panic_during_shift_matches_cm5_except_rereport() {
    let rec: Value = serde_json::from_str(RECORDING).unwrap();
    let strs = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect()
    };
    for (who, ev, rereport) in [
        ("A", "update", true),
        ("B", "update", false),
        ("A", "changes", true),
        ("B", "viewportChange", false),
    ] {
        let r = &rec["throws"][format!("{who}_{ev}")];
        let (mut set, ai, bi) = set_r1(false);
        let log: Log = Rc::default();
        let thrown = Rc::new(Cell::new(false));
        for (id, tag) in [(ai, "A"), (bi, "B")] {
            let ed = set.get(id).unwrap();
            let hit = move |e: &str, th: &Rc<Cell<bool>>| {
                if !th.get() && tag == who && e == ev {
                    th.set(true);
                    panic!("boom");
                }
            };
            let (l, th) = (log.clone(), thrown.clone());
            ed.on_change(move |_, _| {
                l.borrow_mut().push(format!("{tag}:change"));
                hit("change", &th)
            });
            let (l, th) = (log.clone(), thrown.clone());
            ed.on_changes(move |_, _| {
                l.borrow_mut().push(format!("{tag}:changes"));
                hit("changes", &th)
            });
            let (l, th) = (log.clone(), thrown.clone());
            ed.on_update(move |_| {
                l.borrow_mut().push(format!("{tag}:update"));
                hit("update", &th)
            });
            let (l, th) = (log.clone(), thrown.clone());
            ed.on_viewport_change(move |_, x, y| {
                l.borrow_mut()
                    .push(format!("{tag}:viewportChange({x},{y})"));
                hit("viewportChange", &th)
            });
        }
        let res = catch_unwind(AssertUnwindSafe(|| {
            set.run(ai, |cx| {
                cx.doc.replace_range("P\nQ\nR\n", p(10, 0), None, None)
            })
        }));
        assert!(res.is_err() && r["error"].is_string(), "{who}_{ev}");
        assert_eq!(
            *log.borrow(),
            strs(&r["log"]),
            "{who}_{ev}: first operation"
        );
        let (a, b) = (set.get(ai).unwrap(), set.get(bi).unwrap());
        assert_eq!(view(a), (68, 99));
        assert_eq!(view(b), (53, 81));
        assert_eq!(b.doc.first_line(), r["after"]["bFirst"].as_i64().unwrap());
        assert_eq!(
            (a.parked_len(), b.parked_len(), group_open()),
            (0, 0, false)
        );
        log.borrow_mut().clear();
        set.run(bi, |cx| cx.doc.replace_range("X", p(53, 0), None, None));
        let mut want = strs(&r["next"]);
        if rereport {
            let i = want.iter().position(|e| e == "B:viewportChange(53,81)");
            assert!(i.is_some(), "{who}_{ev}: CM5 re-reports B's range");
            want.remove(i.unwrap());
        }
        assert_eq!(*log.borrow(), want, "{who}_{ev}: next operation");
    }
}
