//! The two P1 review fixes, rechecked after the viewport correction: one editor per
//! document (`attachDoc` throws "This document is already in use.") and unwinding
//! (D4: a panic in a handler skips the rest of its group's callbacks, every
//! operation end still runs, and the panic reaches the caller, as
//! `finishOperation`'s `try`/`finally` does in src/util/operation_group.js:32).
//!
//! Each panic test catches the one panic it provokes and checks its payload; any
//! other panic fails the test.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;

use cm_core::operation_group::group_open;
use cm_core::{CmError, Doc, LinkedDocOptions, MarkOptions, Pos};
use cm_modes::{Registry, Spec};
use cm_view::{Editor, EditorOptions, EditorSet, Metrics};

type Log = Rc<RefCell<Vec<String>>>;

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}

fn push(l: &Log, s: impl Into<String>) {
    l.borrow_mut().push(s.into());
}

fn take(l: &Log) -> Vec<String> {
    std::mem::take(&mut *l.borrow_mut())
}

fn text() -> String {
    (0..200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn try_editor(doc: Doc) -> Result<Editor, CmError> {
    let mut cm = Editor::try_with_doc(
        doc,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    )?;
    cm.set_size(600.0, 300.0);
    Ok(cm)
}

fn editor(doc: Doc) -> Editor {
    try_editor(doc).expect("a free document")
}

/// `change`, `changes` and `update` of `cm` go to `log` as `{tag}:{name}`.
fn watch(cm: &Editor, tag: &str, log: &Log) {
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_change(move |_, _| push(&l, format!("{t}:change")));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_changes(move |_, _| push(&l, format!("{t}:changes")));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_update(move |_| push(&l, format!("{t}:update")));
}

/// Run `f`, which must panic with `msg` and nothing else.
fn expect_panic(msg: &'static str, f: impl FnOnce()) {
    let r = catch_unwind(AssertUnwindSafe(f));
    let payload = r.expect_err("the handler panics");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&msg),
        "the provoked panic"
    );
    assert!(!group_open(), "the group closed while unwinding");
}

/// A handler that panics with `msg` the first time only.
fn once(msg: &'static str) -> impl Fn() {
    let fired = Cell::new(false);
    move || {
        if !fired.replace(true) {
            panic_any(msg);
        }
    }
}

/// After a provoked panic the editor edits normally and reports its events.
fn edits_normally(cm: &mut Editor, log: &Log, tag: &str) {
    take(log);
    cm.run_op(|cm| cm.doc.replace_range("ok", p(0, 0), None, None));
    assert!(!group_open());
    assert_eq!(
        take(log),
        [
            format!("{tag}:change"),
            format!("{tag}:changes"),
            format!("{tag}:update")
        ]
    );
}

// ---------------------------------------------------------------------------------
// M1: document ownership

/// A rejected second editor changes nothing: not the document, its owner, its
/// history, or the first editor's events; a later attempt is rejected the same way.
#[test]
fn doc_in_use_leaves_document_and_owner_intact() {
    let doc = Doc::new(&text());
    let mut a = editor(doc.clone());
    a.run_op(|cm| cm.doc.replace_range("A", p(0, 0), None, None));
    let log: Log = Rc::default();
    watch(&a, "A", &log);
    let (value, owner, sel) = (doc.get_value(None), doc.editor_id(), doc.list_selections());
    for _ in 0..2 {
        assert!(matches!(try_editor(doc.clone()), Err(CmError::DocInUse)));
        assert_eq!(doc.get_value(None), value);
        assert_eq!(doc.editor_id(), owner);
        assert_eq!(doc.list_selections(), sel);
    }
    assert_eq!(a.parked_len(), 0);
    a.run_op(|cm| cm.doc.undo());
    assert_eq!(doc.get_line(0).as_deref(), Some("line 0"), "history intact");
    assert_eq!(take(&log), ["A:change", "A:changes", "A:update"]);
}

/// Dropping the owner detaches the document; a new editor then attaches and gets
/// the events, and the old editor's handlers stay silent.
#[test]
fn detach_then_reattach() {
    let doc = Doc::new(&text());
    let log: Log = Rc::default();
    let a = editor(doc.clone());
    watch(&a, "A", &log);
    drop(a);
    assert_eq!(doc.editor_id(), None);
    let mut b = editor(doc.clone());
    assert_eq!(doc.editor_id(), Some(b.id));
    watch(&b, "B", &log);
    edits_normally(&mut b, &log, "B");
    assert!(matches!(try_editor(doc.clone()), Err(CmError::DocInUse)));
}

/// An attempt inside the owner's live operation fails without closing that
/// operation; its change is still reported once at the end.
#[test]
fn doc_in_use_inside_an_active_operation() {
    let mut a = editor(Doc::new(&text()));
    let (doc, id) = (a.doc.clone(), a.id);
    let log: Log = Rc::default();
    watch(&a, "A", &log);
    a.run_op(|a| {
        assert!(matches!(try_editor(doc.clone()), Err(CmError::DocInUse)));
        assert_eq!(doc.editor_id(), Some(id));
        assert!(group_open(), "still inside the operation");
        a.doc.replace_range("x", p(0, 0), None, None);
    });
    assert!(!group_open());
    assert_eq!(take(&log), ["A:change", "A:changes", "A:update"]);
}

/// Linked documents each take one editor; edits reach both editors.
#[test]
fn linked_documents_each_take_one_editor() {
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
    let a = set.insert(editor(d1.clone()));
    let b = set.insert(editor(d2.clone()));
    assert!(matches!(try_editor(d1.clone()), Err(CmError::DocInUse)));
    assert!(matches!(try_editor(d2.clone()), Err(CmError::DocInUse)));
    let log: Log = Rc::default();
    watch(set.get(a).unwrap(), "A", &log);
    watch(set.get(b).unwrap(), "B", &log);
    set.run(a, |cx| cx.doc.replace_range("x", p(0, 0), None, None));
    assert_eq!(
        take(&log),
        [
            "A:change",
            "B:change",
            "A:changes",
            "A:update",
            "B:changes",
            "B:update"
        ]
    );
    assert_eq!(d2.get_line(0).as_deref(), Some("xline 0"));
}

// ---------------------------------------------------------------------------------
// M2: unwinding

/// A delayed `change` handler: the rest of the queue is skipped, the operation end
/// (`changes`, `update`) still runs, then the panic propagates.
#[test]
fn panic_in_a_delayed_callback() {
    let mut cm = editor(Doc::new(&text()));
    let log: Log = Rc::default();
    let f = once("change");
    cm.on_change(move |_, _| f());
    watch(&cm, "A", &log);
    expect_panic("change", || {
        cm.run_op(|cm| cm.doc.replace_range("x", p(0, 0), None, None))
    });
    assert_eq!(take(&log), ["A:changes", "A:update"]);
    edits_normally(&mut cm, &log, "A");
}

/// Parked work: a panic in a parked `update` handler still leaves nothing parked.
#[test]
fn panic_in_parked_work() {
    let doc = Doc::new(&text());
    let mut cm = editor(doc.clone());
    let log: Log = Rc::default();
    watch(&cm, "A", &log);
    let f = once("parked update");
    cm.on_update(move |_| f());
    doc.replace_range("X\n", p(0, 0), None, None);
    assert!(cm.parked_len() > 0);
    expect_panic("parked update", || cm.flush_parked());
    assert_eq!(cm.parked_len(), 0);
    assert_eq!(take(&log), ["A:change", "A:changes", "A:update"]);
    edits_normally(&mut cm, &log, "A");
}

#[test]
fn panic_in_cursor_activity() {
    let mut cm = editor(Doc::new(&text()));
    let log: Log = Rc::default();
    watch(&cm, "A", &log);
    let f = once("cursorActivity");
    cm.on_cursor_activity(move |_| f());
    expect_panic("cursorActivity", || {
        cm.run_op(|cm| cm.doc.set_cursor(p(2, 0), None))
    });
    edits_normally(&mut cm, &log, "A");
}

#[test]
fn panic_in_marker_clear() {
    let mut cm = editor(Doc::new(&text()));
    let log: Log = Rc::default();
    watch(&cm, "A", &log);
    let m = cm.run_op(|cm| {
        cm.doc
            .mark_text(p(0, 0), p(0, 2), MarkOptions::default())
            .unwrap()
            .text_marker()
    });
    let f = once("clear");
    m.on_clear(move |_| f());
    expect_panic("clear", || cm.run_op(|_| m.clear()));
    assert!(m.find().is_none(), "the marker was cleared");
    edits_normally(&mut cm, &log, "A");
}

#[test]
fn panic_in_changes() {
    let mut cm = editor(Doc::new(&text()));
    let log: Log = Rc::default();
    let f = once("changes");
    cm.on_changes(move |_, _| f());
    watch(&cm, "A", &log);
    expect_panic("changes", || {
        cm.run_op(|cm| cm.doc.replace_range("x", p(0, 0), None, None))
    });
    assert_eq!(
        take(&log),
        ["A:change"],
        "the finish phase stops at the panic"
    );
    edits_normally(&mut cm, &log, "A");
}

#[test]
fn panic_in_update() {
    let mut cm = editor(Doc::new(&text()));
    let log: Log = Rc::default();
    let f = once("update");
    cm.on_update(move |_| f());
    watch(&cm, "A", &log);
    expect_panic("update", || {
        cm.run_op(|cm| cm.doc.replace_range("x", p(0, 0), None, None))
    });
    assert_eq!(take(&log), ["A:change", "A:changes"]);
    edits_normally(&mut cm, &log, "A");
}

/// Cross-editor delivery: B's `change` handler panics during an edit made through
/// A. Both operation ends still run, and both editors edit normally afterwards.
#[test]
fn panic_in_cross_editor_delivery() {
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
    let a = set.insert(editor(d1));
    let b = set.insert(editor(d2));
    let log: Log = Rc::default();
    let f = once("B change");
    set.get(b).unwrap().on_change(move |_, _| f());
    watch(set.get(a).unwrap(), "A", &log);
    watch(set.get(b).unwrap(), "B", &log);
    expect_panic("B change", || {
        set.run(a, |cx| cx.doc.replace_range("x", p(0, 0), None, None));
    });
    assert_eq!(
        take(&log),
        ["A:change", "A:changes", "A:update", "B:changes", "B:update"],
        "B's later change handler is skipped; both ends run"
    );
    for id in [a, b] {
        assert_eq!(set.get(id).unwrap().parked_len(), 0);
    }
    set.run(b, |cx| cx.doc.replace_range("y", p(0, 0), None, None));
    assert!(!group_open());
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

/// Reentrant: a `change` handler starts an operation that edits again, and the
/// handler of that nested change panics.
#[test]
fn panic_in_a_reentrant_callback() {
    let mut cm = editor(Doc::new(&text()));
    let log: Log = Rc::default();
    let depth = Rc::new(Cell::new(0));
    let d = depth.clone();
    let armed = Rc::new(Cell::new(true));
    let a2 = armed.clone();
    cm.on_change(move |cx, _| {
        if !a2.get() {
            return;
        }
        d.set(d.get() + 1);
        if d.get() == 1 {
            cx.run_op(|cm, _| cm.doc.replace_range("inner", p(1, 0), None, None));
        } else {
            a2.set(false);
            panic_any("reentrant");
        }
    });
    watch(&cm, "A", &log);
    expect_panic("reentrant", || {
        cm.run_op(|cm| cm.doc.replace_range("outer", p(0, 0), None, None))
    });
    assert_eq!(cm.doc.get_line(1).as_deref(), Some("innerline 1"));
    let got = take(&log);
    assert!(
        got.ends_with(&["A:changes".into(), "A:update".into()]),
        "{got:?}"
    );
    edits_normally(&mut cm, &log, "A");
}
