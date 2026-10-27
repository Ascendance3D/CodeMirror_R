//! Parked operations stay independent when one of them panics (P1 F2).
//!
//! Direct document calls on an attached document park their editor's work (D6):
//! the delayed callbacks of each operation, then its end. CM5 runs each such call
//! as its own operation, so a throw in one never cancels another. The contract
//! (`Editor::flush_parked_with`, D4):
//! * a panic in a queued callback skips the rest of that operation's callbacks
//!   (`fireCallbacksForOps`), but its end still runs;
//! * a panic in an operation's end (`changes`, `update`, …) skips nothing;
//! * later operations are delivered in full, nothing is replayed, and the first
//!   panic resumes with its payload once the parked work is done.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;

use cm_core::operation_group::group_open;
use cm_core::{Doc, LinkedDocOptions, MarkOptions, Pos};
use cm_modes::{Registry, Spec};
use cm_view::{Editor, EditorOptions, EditorSet, Metrics};

type Log = Rc<RefCell<Vec<String>>>;

fn p(l: i64, c: i64) -> Pos {
    Pos::new(l, c)
}
fn push(l: &Log, s: impl Into<String>) {
    l.borrow_mut().push(s.into());
}
fn take(l: &Log) -> Vec<String> {
    std::mem::take(&mut *l.borrow_mut())
}
fn text() -> String {
    (0..50)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}
fn ed(doc: Doc) -> Editor {
    Editor::try_with_doc(
        doc,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    )
    .unwrap()
}
fn edit(cm: &mut Editor) {
    cm.run_op(|cm| cm.doc.replace_range("x", p(0, 0), None, None));
}
fn watch(cm: &Editor, tag: &str, log: &Log) {
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_change(move |_, _| push(&l, format!("{t}:change")));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_changes(move |_, _| push(&l, format!("{t}:changes")));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_update(move |_| push(&l, format!("{t}:update")));
}
/// Run `f`, which must panic with exactly the string payload `msg`, and leave no
/// operation group open.
fn caught(msg: &str, f: impl FnOnce()) {
    let e = catch_unwind(AssertUnwindSafe(f)).expect_err("panics");
    let got = e
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
    assert_eq!(got.as_deref(), Some(msg), "the provoked panic, unchanged");
    assert!(!group_open(), "group closed");
}
/// A guard that is true exactly once, when `pred` first holds.
fn once() -> Rc<Cell<bool>> {
    Rc::new(Cell::new(false))
}

/// The independent QA reproduction (2026-10-26-p1-final-qa-5adc052, unchanged): two
/// parked direct edits; the first parked operation's `update` handler panics.
#[test]
fn p2_two_parked_ops_first_panics() {
    let doc = Doc::new(&text());
    let mut cm = ed(doc.clone());
    let log: Log = Rc::default();
    let fired = Rc::new(Cell::new(false));
    let f = fired.clone();
    cm.on_update(move |_| {
        if !f.replace(true) {
            panic_any("parked");
        }
    });
    watch(&cm, "A", &log);
    doc.replace_range("X\n", p(0, 0), None, None);
    doc.replace_range("Y\n", p(5, 0), None, None);
    let r = catch_unwind(AssertUnwindSafe(|| cm.flush_parked()));
    assert_eq!(
        r.err().and_then(|e| e.downcast_ref::<&str>().copied()),
        Some("parked")
    );
    let after = take(&log);
    edit(&mut cm);
    assert_eq!(take(&log), ["A:change", "A:changes", "A:update"]);
    assert_eq!(cm.parked_len(), 0);
    assert_eq!(
        after,
        ["A:change", "A:changes", "A:change", "A:changes", "A:update"],
        "second parked op lost its change callback"
    );
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Ev {
    Change,
    Changes,
    Update,
}

/// `n` parked direct edits inserting "A", "B", … on lines 0, 1, …; the handler of
/// `ev` panics once, in edit `k`. Returns the log of the flush and checks the
/// payload, that nothing stays parked, and that the next operation is normal.
fn parked_ops(n: usize, ev: Ev, k: usize) -> Vec<String> {
    let names: Vec<String> = (0..n)
        .map(|i| ((b'A' + i as u8) as char).to_string())
        .collect();
    let doc = Doc::new(&text());
    let mut cm = ed(doc.clone());
    let log: Log = Rc::default();
    let target = names[k].clone();
    let msg = format!("boom {ev:?} {target}");
    let fired = once();
    let hit = {
        let (fired, msg) = (fired.clone(), msg.clone());
        move |name: &str| {
            if name == target && !fired.replace(true) {
                panic_any(msg.clone());
            }
        }
    };
    // The panicking handler comes first: the loggers of the same event after it
    // are skipped in that dispatch (`signal` stops at a throw).
    match ev {
        Ev::Change => {
            cm.on_change(move |_, c| hit(&c.text[0]));
        }
        Ev::Changes => {
            cm.on_changes(move |_, cs| hit(&cs[0].text[0]));
        }
        Ev::Update => {
            let last = Rc::new(RefCell::new(String::new()));
            let l = last.clone();
            cm.on_changes(move |_, cs| *l.borrow_mut() = cs[0].text[0].clone());
            cm.on_update(move |_| hit(&last.borrow()));
        }
    }
    let l = log.clone();
    cm.on_change(move |_, c| push(&l, format!("change:{}", c.text[0])));
    let l = log.clone();
    cm.on_changes(move |_, cs| push(&l, format!("changes:{}", cs[0].text[0])));
    let l = log.clone();
    cm.on_update(move |_| push(&l, "update"));
    for (i, name) in names.iter().enumerate() {
        doc.replace_range(name, p(i as i64, 0), None, None);
    }
    assert!(
        log.borrow().is_empty(),
        "parked until the editor's next entry"
    );
    caught(&msg, || cm.flush_parked());
    let got = take(&log);
    assert_eq!(cm.parked_len(), 0, "nothing left parked");
    edit(&mut cm);
    assert_eq!(
        take(&log),
        ["change:x", "changes:x", "update"],
        "the next operation is normal, nothing replayed"
    );
    got
}

/// What CM5 delivers for the same edits as separate operations.
fn expected(n: usize, ev: Ev, k: usize) -> Vec<String> {
    let mut v = Vec::new();
    for i in 0..n {
        let x = ((b'A' + i as u8) as char).to_string();
        let (change, changes, update) = match (i == k, ev) {
            (false, _) => (true, true, true),
            // A throw in a queued callback drops the rest of that operation's
            // queue; its end still runs.
            (true, Ev::Change) => (false, true, true),
            // A throw in `changes` leaves that operation's `update` unsent.
            (true, Ev::Changes) => (true, false, false),
            (true, Ev::Update) => (true, true, false),
        };
        if change {
            v.push(format!("change:{x}"));
        }
        if changes {
            v.push(format!("changes:{x}"));
        }
        if update {
            v.push("update".to_string());
        }
    }
    v
}

#[test]
fn two_and_three_parked_ops_with_a_panic_in_each_position_and_event() {
    for n in [2, 3] {
        for k in 0..n {
            for ev in [Ev::Change, Ev::Changes, Ev::Update] {
                assert_eq!(
                    parked_ops(n, ev, k),
                    expected(n, ev, k),
                    "{n} parked ops, {ev:?} panics in op {k}"
                );
            }
        }
    }
}

/// `cursorActivity` handlers are queued callbacks too: a panic skips the rest of
/// that operation's handlers only.
#[test]
fn cursor_activity_panic_in_a_parked_op() {
    for k in 0..3usize {
        let doc = Doc::new(&text());
        let mut cm = ed(doc.clone());
        let log: Log = Rc::default();
        let n = Rc::new(Cell::new(0usize));
        let (n2, fired) = (n.clone(), once());
        cm.on_cursor_activity(move |_| {
            let i = n2.get();
            n2.set(i + 1);
            if i == k && !fired.replace(true) {
                panic_any("cursor".to_string());
            }
        });
        let l = log.clone();
        let n3 = n.clone();
        cm.on_cursor_activity(move |_| push(&l, format!("ca after {}", n3.get() - 1)));
        for i in 0..3 {
            doc.set_cursor(p(i, 1), None);
        }
        caught("cursor", || cm.flush_parked());
        let want: Vec<String> = (0..3)
            .filter(|&i| i != k)
            .map(|i| format!("ca after {i}"))
            .collect();
        assert_eq!(take(&log), want, "panic in op {k}");
        assert_eq!(cm.parked_len(), 0);
    }
}

/// Marker callbacks: `markerAdded` and `markerCleared` handlers of parked
/// operations (direct `markText` and `clear` on the attached document).
#[test]
fn marker_callback_panics_in_parked_ops() {
    let doc = Doc::new(&text());
    let mut cm = ed(doc.clone());
    let log: Log = Rc::default();
    let line = |m: &cm_core::TextMarker| match m.find() {
        Some(cm_core::MarkerFind::Range { from, .. }) => from.line,
        _ => -1,
    };
    let fired = once();
    cm.on_marker_added(move |_, m| {
        if line(m) == 1 && !fired.replace(true) {
            panic_any("added".to_string());
        }
    });
    let l = log.clone();
    cm.on_marker_added(move |_, m| push(&l, format!("added:{}", line(m))));
    let fired = once();
    let order = Rc::new(Cell::new(0));
    let o = order.clone();
    cm.on_marker_cleared(move |_, _, _, _| {
        o.set(o.get() + 1);
        if o.get() == 2 && !fired.replace(true) {
            panic_any("cleared".to_string());
        }
    });
    let (l, o) = (log.clone(), order.clone());
    cm.on_marker_cleared(move |_, _, _, _| push(&l, format!("cleared #{}", o.get())));
    let opts = || MarkOptions {
        class_name: Some("c".into()),
        ..Default::default()
    };
    let ms: Vec<_> = (0..3)
        .map(|i| {
            doc.mark_text(p(i, 0), p(i, 2), opts())
                .unwrap()
                .text_marker()
        })
        .collect();
    caught("added", || cm.flush_parked());
    assert_eq!(take(&log), ["added:0", "added:2"]);
    for m in &ms {
        m.clear();
    }
    caught("cleared", || cm.flush_parked());
    assert_eq!(take(&log), ["cleared #1", "cleared #3"]);
    assert_eq!(cm.parked_len(), 0);
}

/// Two editors of one `EditorSet` with parked work: a panic in A's parked work
/// does not hold back B's, and the panic resumes after both.
#[test]
fn editor_set_flush_delivers_every_editor_before_resuming() {
    let d1 = Doc::new(&text());
    let d2 = d1
        .linked_doc(LinkedDocOptions {
            shared_hist: false,
            from: None,
            to: None,
            mode: None,
        })
        .unwrap();
    let mut set = EditorSet::new();
    let a = set.insert(ed(d1.clone()));
    let b = set.insert(ed(d2));
    let log: Log = Rc::default();
    let fired = once();
    set.get(a).unwrap().on_update(move |_| {
        if !fired.replace(true) {
            panic_any("A update".to_string());
        }
    });
    watch(set.get(a).unwrap(), "A", &log);
    watch(set.get(b).unwrap(), "B", &log);
    d1.replace_range("X\n", p(0, 0), None, None);
    d1.replace_range("Y\n", p(5, 0), None, None);
    caught("A update", || set.flush_parked());
    let got = take(&log);
    assert_eq!(
        got,
        [
            "A:change",
            "A:changes",
            "A:change",
            "A:changes",
            "A:update", //
            "B:change",
            "B:changes",
            "B:update",
            "B:change",
            "B:changes",
            "B:update",
        ]
    );
    for id in [a, b] {
        assert_eq!(set.get(id).unwrap().parked_len(), 0);
    }
    set.run(b, |cx| cx.doc.replace_range("z", p(0, 0), None, None));
    assert_eq!(
        take(&log),
        [
            "B:change",
            "A:change",
            "B:changes",
            "B:update",
            "A:changes",
            "A:update"
        ]
    );
}

/// A parked callback runs a nested operation whose `update` panics: that is a
/// panic inside a queued callback, so the rest of that operation's callbacks are
/// skipped; its end and the next parked operation are delivered.
#[test]
fn nested_operation_panic_inside_a_parked_callback() {
    let doc = Doc::new(&text());
    let mut cm = ed(doc.clone());
    let log: Log = Rc::default();
    let nested = once();
    cm.on_change(move |cx, c| {
        if c.text[0] == "A" && !nested.replace(true) {
            cx.run_op(|cm, _| cm.doc.replace_range("N", p(10, 0), None, None));
        }
    });
    let fired = once();
    cm.on_update(move |_| {
        if !fired.replace(true) {
            panic_any("nested update".to_string());
        }
    });
    let l = log.clone();
    cm.on_change(move |_, c| push(&l, format!("change:{}", c.text[0])));
    let l = log.clone();
    cm.on_changes(move |_, cs| push(&l, format!("changes:{}", cs[0].text[0])));
    let l = log.clone();
    cm.on_update(move |_| push(&l, "update"));
    doc.replace_range("A", p(0, 0), None, None);
    doc.replace_range("B", p(1, 0), None, None);
    caught("nested update", || cm.flush_parked());
    assert_eq!(
        take(&log),
        [
            // The nested operation, up to its panicking `update`.
            "change:N",
            "changes:N", //
            // The rest of A's callbacks (its `change` logger) are skipped; A's end runs.
            "changes:A",
            "update", //
            // B is untouched.
            "change:B",
            "changes:B",
            "update",
        ]
    );
    assert_eq!(cm.parked_len(), 0);
    edit(&mut cm);
    assert_eq!(take(&log), ["change:x", "changes:x", "update"]);
}

/// A panic that is not caught by the parked delivery itself is not swallowed: a
/// normal operation's handler panic still propagates with its payload, and the
/// operation after it is complete.
#[test]
fn unparked_operation_panics_propagate_unchanged() {
    let mut cm = ed(Doc::new(&text()));
    let log: Log = Rc::default();
    let fired = once();
    cm.on_changes(move |_, _| {
        if !fired.replace(true) {
            panic_any("direct".to_string());
        }
    });
    watch(&cm, "A", &log);
    caught("direct", || edit(&mut cm));
    assert_eq!(take(&log), ["A:change"]);
    edit(&mut cm);
    assert_eq!(take(&log), ["A:change", "A:changes", "A:update"]);
}
