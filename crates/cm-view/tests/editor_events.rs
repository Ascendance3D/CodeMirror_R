//! Editor events against the 2026-10-26 CM5 5.65.21 recordings
//! (`.reports/codemirror-r/2026-10-26-p0-completion/s2-event-system/cm5-events.json`,
//! made by `cm5-events.mjs` in Chromium) and the upstream tests that check event
//! order (`test/test.js`: eventOrder, change_removedText, lineChangeEvents,
//! beforeChange, beforeChangeUndo, beforeSelectionChange).
//!
//! Each `sN_*` test replays the recorded scenario of the same name and compares the
//! whole log. `cm.replaceRange(…)` and the other `methodOp` calls are
//! `cm.run_op(|cm| cm.doc.…)` here.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cm_core::operation_group::{fire_orphan_delayed, group_open};
use cm_core::{Doc, MarkOptions, NoEditors, Pos, SelRange};
use cm_modes::Spec;
use cm_view::events::{BeforeChangeHandler, ChangeHandler, EditorHandler};
use cm_view::{Editor, EditorOptions, Metrics};
use common::events::watch;
use common::*;

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn ps(p: Pos) -> String {
    format!("{}:{}", p.line, p.ch)
}

/// `CodeMirror(document.body, {value})` on the recording page: default options,
/// the default 300px-high `.CodeMirror`.
fn editor(value: &str) -> Editor {
    let mut cm = Editor::new(
        value,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
    );
    cm.set_size(600.0, 300.0);
    cm
}

fn rr(cm: &mut Editor, text: &str, at: Pos) {
    cm.run_op(|cm| cm.doc.replace_range(text, at, None, None));
}

/// s1: one `replaceRange` outside any operation. Immediate events first, then the
/// delayed queue (line, document, editor `change`, then the document's
/// `cursorActivity`), the editor's `cursorActivity`, and the finish phase.
#[test]
fn s1_single_change() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(5));
    watch(
        &cm,
        "A",
        &log,
        &[
            "beforeChange",
            "change",
            "changes",
            "cursorActivity",
            "update",
            "viewportChange",
            "beforeSelectionChange",
            "doc.beforeChange",
            "doc.change",
            "doc.cursorActivity",
        ],
    );
    let l = log.clone();
    cm.doc
        .get_line_handle(0)
        .unwrap()
        .on_change(move |_, _| push(&l, "A:line0.change"));
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 2), None));
    push(&log, "--- after setCursor");
    rr(&mut cm, "X\nY", p(0, 1));
    push(&log, "--- after replaceRange");
    assert_eq!(
        take(&log),
        [
            "A:cm.beforeSelectionChange",
            "A:doc.cursorActivity",
            "A:cm.cursorActivity",
            "--- after setCursor",
            "A:doc.beforeChange(0:1)",
            "A:cm.beforeChange(0:1,[\"X\", \"Y\"])",
            "A:cm.beforeSelectionChange",
            "A:line0.change",
            "A:doc.change(0:1)",
            "A:cm.change(0:1,[\"X\", \"Y\"])",
            "A:doc.cursorActivity",
            "A:cm.cursorActivity",
            "A:cm.changes(1)",
            "A:cm.update",
            "A:cm.viewportChange(0,6)",
            "--- after replaceRange"
        ]
    );
}

/// s2: delayed events of nested operations wait for the outermost end.
#[test]
fn s2_nested_ops() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(5));
    watch(
        &cm,
        "A",
        &log,
        &["change", "changes", "cursorActivity", "update"],
    );
    let l = log.clone();
    cm.run_op(|cm| {
        cm.doc.replace_range("a", p(0, 0), None, None);
        push(&l, "body: after edit 1");
        cm.run_op(|cm| {
            cm.doc.replace_range("b", p(1, 0), None, None);
            push(&l, "inner body end");
        });
        push(&l, "body: after inner operation");
        cm.doc.set_cursor(p(2, 1), None);
        push(&l, "body: end");
    });
    push(&log, "--- after operation");
    assert_eq!(
        take(&log),
        [
            "body: after edit 1",
            "inner body end",
            "body: after inner operation",
            "body: end",
            "A:cm.change(0:0,[\"a\"])",
            "A:cm.change(1:0,[\"b\"])",
            "A:cm.cursorActivity",
            "A:cm.changes(2)",
            "A:cm.update",
            "--- after operation"
        ]
    );
}

/// s3: marker `clear` (delayed, with the range) before the editor's
/// `markerCleared`; a collapsed marker reports its lines; an editorless document's
/// marker waits for the host. `m.clear()` outside an operation opens one on the
/// editor (`operation(doc.cm, …)`); here the caller provides it (`run_op`), since a
/// direct call on an attached document parks the editor's part (D6).
#[test]
fn s3_marker_clear() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(5));
    watch(
        &cm,
        "A",
        &log,
        &["change", "changes", "markerCleared", "update"],
    );
    let mark = |cm: &mut Editor, a: Pos, b: Pos, o: MarkOptions| {
        cm.run_op(|cm| cm.doc.mark_text(a, b, o).unwrap().text_marker())
    };
    let m = mark(&mut cm, p(1, 0), p(1, 4), MarkOptions::default());
    let l = log.clone();
    m.on_clear(move |r| {
        let (f, t) = r.unwrap();
        push(&l, format!("m.clear({},{})", ps(f), ps(t.unwrap())))
    });
    cm.run_op(|_| m.clear());
    push(&log, "--- after m.clear outside op");
    let m2 = mark(&mut cm, p(2, 0), p(2, 4), MarkOptions::default());
    let l = log.clone();
    m2.on_clear(move |r| {
        let (f, t) = r.unwrap();
        push(&l, format!("m2.clear({},{})", ps(f), ps(t.unwrap())))
    });
    let l = log.clone();
    cm.run_op(|cm| {
        m2.clear();
        push(&l, "body: after m2.clear");
        cm.doc.replace_range("Z", p(2, 0), None, None);
        push(&l, "body: end");
    });
    push(&log, "--- after op");
    let m3 = mark(
        &mut cm,
        p(1, 1),
        p(3, 1),
        MarkOptions {
            collapsed: true,
            ..Default::default()
        },
    );
    let l = log.clone();
    m3.on_clear(move |_| push(&l, "m3.clear"));
    cm.run_op(|_| m3.clear());
    push(&log, "--- after m3.clear (collapsed)");
    let d = Doc::new(&lines(3));
    let m4 = d
        .mark_text(p(0, 0), p(0, 2), MarkOptions::default())
        .unwrap()
        .text_marker();
    let l = log.clone();
    m4.on_clear(move |_| push(&l, "m4.clear (editorless doc)"));
    m4.clear();
    push(&log, "--- after m4.clear sync");
    fire_orphan_delayed(&mut NoEditors);
    push(&log, "--- after timeout");
    assert_eq!(
        take(&log),
        [
            "m.clear(1:0,1:4)",
            "A:cm.markerCleared(null,null)",
            "A:cm.update",
            "--- after m.clear outside op",
            "body: after m2.clear",
            "body: end",
            "m2.clear(2:0,2:4)",
            "A:cm.markerCleared(null,null)",
            "A:cm.change(2:0,[\"Z\"])",
            "A:cm.changes(1)",
            "A:cm.update",
            "--- after op",
            "A:cm.update",
            "m3.clear",
            "A:cm.markerCleared(1,3)",
            "A:cm.update",
            "--- after m3.clear (collapsed)",
            "--- after m4.clear sync",
            "m4.clear (editorless doc)",
            "--- after timeout"
        ]
    );
}

/// s5a: a handler that removes a later handler and adds a new one: the running
/// dispatch keeps its list (immediate `beforeChange` and delayed `change` alike).
#[test]
fn s5a_remove_add_sync_and_later() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(3));
    let ev = cm.events();
    let l = log.clone();
    let h2: Rc<BeforeChangeHandler> = Rc::new(move |_, _, _| push(&l, "bc:h2"));
    let l = log.clone();
    let h3: Rc<BeforeChangeHandler> =
        Rc::new(move |_, _, _| push(&l, "bc:h3 (added during dispatch)"));
    {
        let (l, ev, h2, h3) = (log.clone(), ev.clone(), h2.clone(), h3.clone());
        cm.on_before_change(move |_, _, _| {
            push(&l, "bc:h1 off(h2) on(h3)");
            ev.with_listeners(|ls| ls.before_change.off_fn(&h2));
            ev.with_listeners(|ls| ls.before_change.on_rc(h3.clone()));
        });
    }
    ev.with_listeners(|ls| ls.before_change.on_rc(h2));
    let l = log.clone();
    let c2: Rc<ChangeHandler> = Rc::new(move |_, _| push(&l, "ch:c2"));
    let l = log.clone();
    let c3: Rc<ChangeHandler> = Rc::new(move |_, _| push(&l, "ch:c3 (added during dispatch)"));
    {
        let (l, c2, c3) = (log.clone(), c2.clone(), c3.clone());
        cm.on_change(move |cx, _| {
            push(&l, "ch:c1 off(c2) on(c3)");
            let ev = cx.events();
            ev.with_listeners(|ls| ls.change.off_fn(&c2));
            ev.with_listeners(|ls| ls.change.on_rc(c3.clone()));
        });
    }
    ev.with_listeners(|ls| ls.change.on_rc(c2));
    rr(&mut cm, "1", p(0, 0));
    push(&log, "=== second change");
    rr(&mut cm, "2", p(0, 0));
    let want = [
        "bc:h1 off(h2) on(h3)",
        "bc:h2",
        "ch:c1 off(c2) on(c3)",
        "ch:c2",
        "=== second change",
        "bc:h1 off(h2) on(h3)",
        "bc:h3 (added during dispatch)",
        "ch:c1 off(c2) on(c3)",
        "ch:c3 (added during dispatch)",
    ];
    // The second dispatch's lists were taken before h1/c1 added another h3/c3.
    let got = take(&log);
    assert_eq!(got, want);
}

/// s5b: a `change` queued before its handler was removed is still delivered; a
/// `cursorActivity` handler collected before removal is called; one added after the
/// selection changed is not, until a later selection change collects it.
#[test]
fn s5b_remove_after_queue() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(3));
    let l = log.clone();
    let k = cm.on_change(move |_, _| push(&l, "change:k"));
    let l = log.clone();
    cm.run_op(|cm| {
        cm.doc.replace_range("x", p(0, 0), None, None);
        cm.off(k);
        push(&l, "body: off(k) after edit");
        cm.doc.replace_range("y", p(0, 0), None, None);
    });
    let l = log.clone();
    let ca1: Rc<EditorHandler> = Rc::new(move |_| push(&l, "ca1"));
    let l = log.clone();
    let ca2: Rc<EditorHandler> =
        Rc::new(move |_| push(&l, "ca2 (added in op after selection change)"));
    let ev = cm.events();
    ev.with_listeners(|ls| ls.cursor_activity.on_rc(ca1.clone()));
    let l = log.clone();
    cm.run_op(|cm| {
        cm.doc.set_cursor(p(1, 1), None);
        ev.with_listeners(|ls| ls.cursor_activity.off_fn(&ca1));
        ev.with_listeners(|ls| ls.cursor_activity.on_rc(ca2.clone()));
        push(&l, "body: off(ca1) on(ca2)");
    });
    push(&log, "=== op with off(ca2) before any selection change");
    cm.run_op(|cm| {
        ev.with_listeners(|ls| ls.cursor_activity.off_fn(&ca2));
        ev.with_listeners(|ls| ls.cursor_activity.on_rc(ca2.clone()));
        cm.doc.set_cursor(p(2, 1), None);
    });
    assert_eq!(
        take(&log),
        [
            "body: off(k) after edit",
            "change:k",
            "body: off(ca1) on(ca2)",
            "ca1",
            "=== op with off(ca2) before any selection change",
            "ca2 (added in op after selection change)"
        ]
    );
}

/// s5c: one function registered twice is signaled twice; `cursorActivity` collects
/// it once (identity); `off` removes one registration.
#[test]
fn s5c_duplicate_function() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(3));
    let ev = cm.events();
    let l = log.clone();
    let dup_change: Rc<ChangeHandler> = Rc::new(move |_, _| push(&l, "dup:change"));
    let l = log.clone();
    let dup_ca: Rc<EditorHandler> = Rc::new(move |_| push(&l, "dup:cursorActivity"));
    ev.with_listeners(|ls| {
        ls.change.on_rc(dup_change.clone());
        ls.change.on_rc(dup_change.clone());
        ls.cursor_activity.on_rc(dup_ca.clone());
        ls.cursor_activity.on_rc(dup_ca);
    });
    rr(&mut cm, "q", p(0, 0));
    ev.with_listeners(|ls| ls.change.off_fn(&dup_change));
    push(&log, "=== after one off(change, dup)");
    rr(&mut cm, "r", p(0, 0));
    assert_eq!(
        take(&log),
        [
            "dup:change",
            "dup:change",
            "dup:cursorActivity",
            "=== after one off(change, dup)",
            "dup:change",
            "dup:cursorActivity"
        ]
    );
}

/// s6a / upstream `eventOrder` (test.js:2394): edits from `change` and
/// `cursorActivity` handlers join the operation that is still open; the
/// `cursorActivity` handler's edit runs after the queue drained, and its `change`
/// comes after it.
#[test]
fn s6a_event_order() {
    let mut cm = cm("");
    let seen: Log = Rc::default();
    let s = seen.clone();
    cm.on_change(move |cx, _| {
        if s.borrow().is_empty() {
            cx.run_op(|cm, _| cm.doc.replace_selection(".", None, None));
        }
        push(&s, "change");
    });
    let s = seen.clone();
    cm.on_cursor_activity(move |cx| {
        cx.run_op(|cm, _| cm.doc.replace_selection("!", None, None));
        push(&s, "activity");
    });
    cm.run_op(|cm| cm.doc.replace_selection("/", None, None));
    assert_eq!(take(&seen).join(","), "change,change,activity,change");
    assert_eq!(cm.get_value(), "/.!", "recorded value");
}

/// s6b: a change made from a `beforeChange` handler is applied first.
#[test]
fn s6b_change_in_before_change() {
    let log: Log = Rc::default();
    let mut cm = editor("abc");
    watch(&cm, "A", &log, &["change", "changes", "update"]);
    let once = Rc::new(Cell::new(false));
    let l = log.clone();
    cm.on_before_change(move |doc, _, c| {
        push(&l, format!("A:cm.beforeChange({:?})", c.text));
        if !once.replace(true) {
            doc.replace_range("<", p(0, 0), None, None);
            push(&l, "beforeChange: back from nested replaceRange");
        }
    });
    rr(&mut cm, ">", p(0, 3));
    push(&log, format!("value={}", cm.get_value()));
    assert_eq!(
        take(&log),
        [
            "A:cm.beforeChange([\">\"])",
            "A:cm.beforeChange([\"<\"])",
            "beforeChange: back from nested replaceRange",
            "A:cm.change(0:0,[\"<\"])",
            "A:cm.change(0:3,[\">\"])",
            "A:cm.changes(2)",
            "A:cm.update",
            "value=<ab>c"
        ]
    );
}

/// s6c: `changes` runs after the group ended, so an edit from it is a new operation
/// that completes (with its own `changes` and `update`) inside the handler.
#[test]
fn s6c_change_in_changes() {
    let log: Log = Rc::default();
    let mut cm = editor("abc");
    let n = Rc::new(Cell::new(0));
    let l = log.clone();
    cm.on_change(move |_, c| push(&l, format!("change({:?})", c.text)));
    let l = log.clone();
    cm.on_changes(move |cx, cs| {
        push(&l, format!("changes({}) enter", cs.len()));
        assert!(!group_open(), "the group has ended");
        if n.replace(n.get() + 1) == 0 {
            cx.run_op(|cm, _| cm.doc.replace_range("+", p(0, 0), None, None));
        }
        push(&l, "changes exit");
    });
    let l = log.clone();
    cm.on_update(move |_| push(&l, "update"));
    let l = log.clone();
    cm.on_cursor_activity(move |_| push(&l, "cursorActivity"));
    rr(&mut cm, "-", p(0, 0));
    push(&log, format!("value={}", cm.get_value()));
    assert_eq!(
        take(&log),
        [
            "change([\"-\"])",
            "cursorActivity",
            "changes(1) enter",
            "change([\"+\"])",
            "cursorActivity",
            "changes(1) enter",
            "changes exit",
            "update",
            "changes exit",
            "update",
            "value=+-abc"
        ]
    );
}

/// s6d: a callback queued from a delayed callback joins the same group's queue.
#[test]
fn s6d_signal_later_inside_callback() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(3));
    let mk = cm.run_op(|cm| {
        cm.doc
            .mark_text(p(2, 0), p(2, 2), MarkOptions::default())
            .unwrap()
            .text_marker()
    });
    let l = log.clone();
    mk.on_clear(move |_| push(&l, "mk.clear"));
    let first = Rc::new(Cell::new(true));
    let l = log.clone();
    cm.on_change(move |_, _| {
        push(&l, "change");
        if first.replace(false) {
            mk.clear();
            push(&l, "change: after mk.clear");
        }
    });
    let l = log.clone();
    cm.on_cursor_activity(move |_| push(&l, "cursorActivity"));
    let l = log.clone();
    cm.on_changes(move |_, _| push(&l, "changes"));
    rr(&mut cm, "k", p(0, 0));
    assert_eq!(
        take(&log),
        [
            "change",
            "change: after mk.clear",
            "mk.clear",
            "cursorActivity",
            "changes"
        ]
    );
}

/// s9 (D4): a panic in a delayed callback skips the rest of the queue, but the
/// operation still ends (`changes`, `update`) before the panic reaches the caller;
/// the editor keeps working.
#[test]
fn s9_panic_in_a_delayed_callback() {
    let log: Log = Rc::default();
    let mut cm = editor("abc");
    let l = log.clone();
    cm.on_change(move |_, _| {
        push(&l, "change h1 throws");
        std::panic::panic_any("boom");
    });
    let l = log.clone();
    cm.on_change(move |_, _| push(&l, "change h2"));
    let l = log.clone();
    cm.on_cursor_activity(move |_| push(&l, "cursorActivity"));
    let l = log.clone();
    cm.on_changes(move |_, _| push(&l, "changes"));
    let l = log.clone();
    cm.on_update(move |_| push(&l, "update"));
    let try_rr = |cm: &mut Editor, t: &str| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rr(cm, t, p(0, 0))))
            .err()
            .and_then(|e| e.downcast_ref::<&str>().map(|s| s.to_string()))
    };
    if let Some(e) = try_rr(&mut cm, "x") {
        push(&log, format!("caught {e}"));
    }
    push(
        &log,
        format!("curOp after: {}", if group_open() { "set" } else { "null" }),
    );
    if let Some(e) = try_rr(&mut cm, "y") {
        push(&log, format!("caught again {e}"));
    }
    push(&log, format!("value={}", cm.get_value()));
    assert_eq!(
        take(&log),
        [
            "change h1 throws",
            "changes",
            "update",
            "caught boom",
            "curOp after: null",
            "change h1 throws",
            "changes",
            "update",
            "caught again boom",
            "value=yxabc"
        ]
    );
}

/// s10: an edit, a scroll and a cursor move in one operation: one `update`, then
/// `viewportChange` with the new rendered range. The recorded range (76,110) comes
/// from the Chromium page's line height; the order and the range's containment of
/// the scrolled-to lines are what carry over to other metrics.
#[test]
fn s10_viewport() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(200));
    watch(
        &cm,
        "A",
        &log,
        &[
            "change",
            "changes",
            "cursorActivity",
            "update",
            "viewportChange",
        ],
    );
    cm.run_op(|cm| {
        cm.doc.replace_range("v", p(0, 0), None, None);
        cm.scroll_to(None, Some(3000.0));
        cm.doc.set_cursor(p(100, 0), None);
    });
    push(&log, "--- after op");
    let got = take(&log);
    let (from, to) = (cm.display.view_from, cm.display.view_to);
    assert_eq!(
        got,
        [
            "A:cm.change(0:0,[\"v\"])".to_string(),
            "A:cm.cursorActivity".into(),
            "A:cm.changes(1)".into(),
            "A:cm.update".into(),
            format!("A:cm.viewportChange({from},{to})"),
            "--- after op".into()
        ]
    );
    assert!(from <= 100 && 100 < to && from > 0, "({from},{to})");
}

/// s10 at the recording page's size (`.CodeMirror` 900x500, the oracle metrics):
/// the recorded rendered range (76,110) itself.
#[test]
fn s10_viewport_at_the_recorded_size() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(200));
    cm.set_size(900.0, 500.0);
    watch(
        &cm,
        "A",
        &log,
        &[
            "change",
            "changes",
            "cursorActivity",
            "update",
            "viewportChange",
        ],
    );
    cm.run_op(|cm| {
        cm.doc.replace_range("v", p(0, 0), None, None);
        cm.scroll_to(None, Some(3000.0));
        cm.doc.set_cursor(p(100, 0), None);
    });
    push(&log, "--- after op");
    assert_eq!(
        take(&log),
        [
            "A:cm.change(0:0,[\"v\"])",
            "A:cm.cursorActivity",
            "A:cm.changes(1)",
            "A:cm.update",
            "A:cm.viewportChange(76,110)",
            "--- after op"
        ]
    );
}

/// An operation that changes nothing reports no `update` and no `viewportChange`
/// (`updateDisplayIfNeeded` bails out).
#[test]
fn quiet_operation_reports_no_update() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(5));
    watch(
        &cm,
        "A",
        &log,
        &["update", "viewportChange", "cursorActivity"],
    );
    cm.run_op(|_| {});
    cm.run_op(|cm| cm.doc.set_cursor(p(1, 1), None));
    assert_eq!(take(&log), ["A:cm.cursorActivity"]);
}

/// Upstream `lineChangeEvents` (test.js:2368).
#[test]
fn line_change_events() {
    let mut cm = cm("");
    add_doc(&mut cm, 3, 5);
    let log: Log = Rc::default();
    for i in 0..5 {
        let h = cm.doc.get_line_handle(i).unwrap();
        let l = log.clone();
        h.on_delete(move || push(&l, format!("del {i}")));
        let l = log.clone();
        h.on_change(move |_, _| push(&l, format!("ch {i}")));
    }
    rr(&mut cm, "x", p(0, 1));
    cm.run_op(|cm| cm.doc.replace_range("xy", p(1, 1), Some(pl(2)), None));
    rr(&mut cm, "foo\nbar", p(0, 1));
    cm.run_op(|cm| {
        let end = pl(cm.doc.line_count() as i64);
        cm.doc.replace_range("", p(0, 0), Some(end), None)
    });
    assert_eq!(
        take(&log),
        [
            "ch 0", "ch 1", "del 2", "ch 0", "ch 0", "del 1", "del 3", "del 4"
        ]
    );
}

/// Upstream `change_removedText` (test.js:2384).
#[test]
fn change_removed_text() {
    let mut cm = cm("");
    cm.set_value("abc\ndef");
    let removed: Rc<RefCell<Vec<String>>> = Rc::default();
    let r = removed.clone();
    cm.on_change(move |_, c| r.borrow_mut().push(c.removed.join("\n")));
    cm.run_op(|cm| {
        cm.doc.replace_range("xyz", p(0, 0), Some(p(1, 1)), None);
        cm.doc.replace_range("123", p(0, 0), None, None);
    });
    assert_eq!(*removed.borrow(), ["abc\nd", ""]);
    removed.borrow_mut().clear();
    cm.run_op(|cm| cm.doc.undo());
    assert_eq!(*removed.borrow(), ["123", "xyz"]);
    removed.borrow_mut().clear();
    cm.run_op(|cm| cm.doc.redo());
    assert_eq!(*removed.borrow(), ["abc\nd", ""]);
}

/// Upstream `beforeChange` (test.js:2416): editor handlers run after document
/// handlers, and a document handler's cancel stops the change.
#[test]
fn before_change() {
    let mut cm = cm("abcdefghijk");
    cm.on_before_change(|_, _, c| {
        let text = c
            .text
            .iter()
            .map(|t| t.replace(char::is_whitespace, "_"))
            .collect();
        c.update(None, None, Some(text), None);
    });
    cm.set_value("hello, i am a\nnew document\n");
    assert_eq!(cm.get_value(), "hello,_i_am_a\nnew_document\n");
    cm.doc.on_before_change(|_, c| {
        if c.from.line == 0 {
            c.cancel();
        }
    });
    cm.set_value("oops");
    assert_eq!(cm.get_value(), "hello,_i_am_a\nnew_document\n");
    cm.run_op(|cm| {
        cm.doc
            .replace_range("hey hey hey", p(1, 0), Some(p(2, 0)), None)
    });
    assert_eq!(cm.get_value(), "hello,_i_am_a\nhey_hey_hey");
}

/// Upstream `beforeChangeUndo` (test.js:2431).
#[test]
fn before_change_undo() {
    let mut cm = cm("one\ntwo");
    cm.run_op(|cm| cm.doc.replace_range("hi", p(0, 0), Some(pl(0)), None));
    cm.run_op(|cm| cm.doc.replace_range("bye", p(0, 0), Some(pl(0)), None));
    assert_eq!(cm.doc.history_size().undo, 2);
    cm.on_before_change(|_, _, c| {
        assert!(!c.can_update());
        c.cancel();
    });
    cm.run_op(|cm| cm.doc.undo());
    assert_eq!(cm.doc.history_size().undo, 0);
    assert_eq!(cm.get_value(), "bye\ntwo");
}

/// Upstream `beforeSelectionChange` (test.js:2443).
#[test]
fn before_selection_change() {
    let mut cm = cm("");
    fn not_at_end(doc: &Doc, pos: Pos) -> Pos {
        let len = doc.get_line(pos.line).unwrap().encode_utf16().count() as i64;
        if len == 0 || pos.ch == len {
            Pos::new(pos.line, pos.ch - 1)
        } else {
            pos
        }
    }
    cm.on_before_selection_change(|doc, _, obj| {
        let r = obj.ranges[0];
        obj.update(vec![SelRange {
            anchor: not_at_end(doc, r.anchor),
            head: not_at_end(doc, r.head),
        }]);
    });
    add_doc(&mut cm, 10, 10);
    cm.exec_command("goLineEnd");
    eq_cursor_pos(cur(&cm), p(0, 9));
    cm.exec_command("selectAll");
    eq_cursor_pos(cm.doc.get_cursor(Some("start")), p(0, 0));
    eq_cursor_pos(cm.doc.get_cursor(Some("end")), p(9, 9));
}

/// The handler ids of one editor are independent of the document's; `off` removes
/// by id, whatever the event.
#[test]
fn editor_off_by_id() {
    let log: Log = Rc::default();
    let mut cm = editor("abc");
    let l = log.clone();
    let id = cm.on_change(move |_, _| push(&l, "change"));
    let l = log.clone();
    let did = cm.doc.on_change(move |_, _| push(&l, "doc.change"));
    assert!(!cm.off(did), "a document handler id");
    assert!(cm.off(id) && !cm.off(id));
    rr(&mut cm, "x", p(0, 0));
    assert_eq!(take(&log), ["doc.change"]);
}

/// `keyHandled` is delayed and carries the binding's name.
#[test]
fn key_handled_is_delayed() {
    let log: Log = Rc::default();
    let mut cm = editor("abc");
    let l = log.clone();
    cm.on_key_handled(move |_, name| push(&l, format!("keyHandled {name}")));
    let l = log.clone();
    cm.on_cursor_activity(move |_| push(&l, "cursorActivity"));
    cm.key_down(&cm_view::KeyEvent::new("End"));
    assert_eq!(take(&log), ["keyHandled End", "cursorActivity"]);
}

/// Marker `hide`/`unhide` (`endOperation_finish`, mark_text.js:134-147): an edit that
/// removes the last of a marker's text hides it at the end of the operation; the
/// undo that brings it back unhides it. A marker that loses a line and gains one in
/// the same operation is neither.
#[test]
fn marker_hide_and_unhide() {
    let log: Log = Rc::default();
    let mut cm = editor("abcdef\nghi");
    let m = cm.run_op(|cm| {
        cm.doc
            .mark_text(p(0, 1), p(0, 3), MarkOptions::default())
            .unwrap()
            .text_marker()
    });
    let l = log.clone();
    m.on_hide(move || push(&l, "hide"));
    let l = log.clone();
    m.on_unhide(move || push(&l, "unhide"));
    let l = log.clone();
    cm.on_changes(move |_, _| push(&l, "changes"));
    cm.run_op(|cm| cm.doc.replace_range("X", p(0, 2), None, None));
    assert_eq!(take(&log), ["changes"], "still has text");
    cm.run_op(|cm| cm.doc.replace_range("", p(0, 0), Some(p(0, 5)), None));
    assert_eq!(take(&log), ["hide", "changes"]);
    cm.run_op(|cm| cm.doc.undo());
    assert_eq!(take(&log), ["unhide", "changes"]);
    assert!(m.find().is_some());
}

/// `update` follows `updateDisplayIfNeeded`'s bail-out: an edit or a styled marker
/// outside the rendered lines dirties nothing (`regChange`/`regLineChange`), and a
/// one-line edit before them does not shift them, so no `update` is reported; an
/// edit inside them is. (Edits that add or remove lines above them:
/// `viewport_matrix.rs`.)
#[test]
fn update_only_for_changes_in_the_rendered_range() {
    let log: Log = Rc::default();
    let mut cm = editor(&lines(2000));
    watch(&cm, "A", &log, &["update", "viewportChange"]);
    let (from, to) = (cm.display.view_from, cm.display.view_to);
    assert!(to < 1800, "({from},{to})");
    rr(&mut cm, "X", p(1900, 0));
    cm.run_op(|cm| {
        cm.doc
            .mark_text(
                p(1800, 0),
                p(1801, 0),
                MarkOptions {
                    class_name: Some("x".into()),
                    ..Default::default()
                },
            )
            .unwrap()
    });
    assert!(take(&log).is_empty(), "outside the rendered range");
    cm.run_op(|cm| cm.scroll_to(None, Some(20_000.0)));
    take(&log);
    let (from, _) = (cm.display.view_from, cm.display.view_to);
    assert!(from > 10, "scrolled ({from})");
    // A one-line edit above the rendered lines shifts nothing.
    rr(&mut cm, "Y", p(0, 0));
    assert!(take(&log).is_empty(), "before the rendered range");
    rr(&mut cm, "Z", p(from, 0));
    assert_eq!(take(&log).first().map(String::as_str), Some("A:cm.update"));
}

#[test]
fn test_find_word_at_javascript() {
    let opts = cm_view::EditorOptions::default();
    let metrics = cm_view::Metrics {
        char_width: 10.0,
        text_height: 15.0,
        padding_h: 0.0,
        padding_top: 0.0,
        wide_width: 20.0,
        astral_width: 20.0,
    };
    let e = cm_view::Editor::new(
        "let a$b = 1;\nlet á = 2;",
        cm_modes::Spec::from("javascript"),
        opts,
        metrics,
    );

    // Test the $ which is a word char in JS
    let (start, end) = e.find_word_at(cm_core::Pos::new(0, 5)); // inside a$b
    assert_eq!(start.ch, 4); // "a$b" starts at 4
    assert_eq!(end.ch, 7); // "a$b" ends at 7

    // Test non-ascii letter which triggers isWordChar fallback
    // pos 1:4 is 'á'
    let (start2, end2) = e.find_word_at(cm_core::Pos::new(1, 4));
    assert_eq!(start2.ch, 4); // "á" starts at 4
    assert_eq!(end2.ch, 5);
}
