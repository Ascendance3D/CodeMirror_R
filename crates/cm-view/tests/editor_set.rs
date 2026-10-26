//! Several editors in one [`EditorSet`] (D2): the recorded CM5 cross-editor order
//! (s4 linked documents in two editors, s4b a handler that edits another editor),
//! the `test/doc_test.js` cases whose editors share documents, run with real
//! editors, and editors that cannot be reached (busy, removed).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cm_core::operation_group::{group_open, take_diagnostics};
use cm_core::{Doc, EditorId, LinkedDocOptions, MarkOptions, Marker, MarkerFind, Pos};
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

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn editor_over(doc: Doc) -> Editor {
    let mut cm = Editor::with_doc(
        doc,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    cm.set_size(600.0, 300.0);
    cm
}

/// `CodeMirror(place, {value: other.linkedDoc({sharedHist, from, to})})`.
fn linked(
    set: &mut EditorSet,
    of: EditorId,
    shared_hist: bool,
    range: Option<(i64, i64)>,
) -> EditorId {
    let doc = set
        .get(of)
        .unwrap()
        .doc
        .linked_doc(LinkedDocOptions {
            shared_hist,
            from: range.map(|r| r.0),
            to: range.map(|r| r.1),
            mode: None,
        })
        .unwrap();
    set.insert(editor_over(doc))
}

fn rr(set: &mut EditorSet, id: EditorId, text: &str, from: Pos, to: Option<Pos>) {
    set.run(id, |cx| cx.doc.replace_range(text, from, to, None))
        .expect("reachable");
}

fn value(set: &EditorSet, id: EditorId) -> String {
    set.get(id).unwrap().get_value()
}

fn eq_all(set: &EditorSet, v: &str, ids: &[EditorId]) {
    for &id in ids {
        assert_eq!(value(set, id), v, "editor {}", id.get());
    }
}

/// The recording's `watch(cm, tag, log, which)` for the s4 events.
fn watch(cm: &Editor, tag: &str, log: &Log, before_change: bool) {
    let t = tag.to_string();
    if before_change {
        let (l, t) = (log.clone(), t.clone());
        cm.on_before_change(move |_, _, c| {
            push(
                &l,
                format!(
                    "{t}:cm.beforeChange({}:{},{:?})",
                    c.from.line, c.from.ch, c.text
                ),
            )
        });
    }
    let (l, t2) = (log.clone(), t.clone());
    cm.on_change(move |_, c| {
        push(
            &l,
            format!("{t2}:cm.change({}:{},{:?})", c.from.line, c.from.ch, c.text),
        )
    });
    let (l, t2) = (log.clone(), t.clone());
    cm.on_changes(move |_, cs| push(&l, format!("{t2}:cm.changes({})", cs.len())));
    let (l, t2) = (log.clone(), t.clone());
    cm.on_cursor_activity(move |_| push(&l, format!("{t2}:cm.cursorActivity")));
    let (l, t2) = (log.clone(), t.clone());
    cm.on_update(move |_| push(&l, format!("{t2}:cm.update")));
    let (l, t2) = (log.clone(), t.clone());
    cm.doc.on_change(move |_, c| {
        push(
            &l,
            format!("{t2}:doc.change({}:{})", c.from.line, c.from.ch),
        )
    });
    let (l, t2) = (log.clone(), t);
    cm.doc
        .on_cursor_activity(move |_| push(&l, format!("{t2}:doc.cursorActivity")));
}

/// s4: linked documents in two editors. One group covers every editor an operation
/// reached; each editor's operation ends in the order the operations started (the
/// group owner's first), and the linked document's events follow the originating
/// document's in the delayed queue.
#[test]
fn s4_linked_two_editors() {
    let log: Log = Rc::default();
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new(&lines(5))));
    let b = linked(&mut set, a, true, None);
    for (id, tag) in [(a, "A"), (b, "B")] {
        watch(set.get(id).unwrap(), tag, &log, true);
    }
    set.run(b, |cx| cx.doc.set_cursor(p(0, 3), None));
    set.run(a, |cx| cx.doc.set_cursor(p(0, 3), None));
    take(&log);
    push(&log, "=== a.replaceRange (no op)");
    rr(&mut set, a, "Q", p(0, 0), None);
    push(&log, "=== a.operation{a.replaceRange; b.replaceRange}");
    let l = log.clone();
    set.run(a, |cx| {
        cx.doc.replace_range("R", p(1, 0), None, None);
        assert!(cx.with_editor(b, |cx| {
            cx.run_op(|cm, _| cm.doc.replace_range("S", p(2, 0), None, None))
        }));
        push(&l, "body end");
    });
    push(&log, "=== b.operation{a.replaceRange} (group owned by b)");
    let l = log.clone();
    set.run(b, |cx| {
        assert!(cx.with_editor(a, |cx| {
            cx.run_op(|cm, _| cm.doc.replace_range("T", p(3, 0), None, None))
        }));
        push(&l, "body end");
    });
    push(&log, "=== done");
    assert_eq!(
        take(&log),
        [
            "=== a.replaceRange (no op)",
            "A:cm.beforeChange(0:0,[\"Q\"])",
            "A:doc.change(0:0)",
            "A:cm.change(0:0,[\"Q\"])",
            "A:doc.cursorActivity",
            "B:doc.change(0:0)",
            "B:cm.change(0:0,[\"Q\"])",
            "B:doc.cursorActivity",
            "A:cm.cursorActivity",
            "B:cm.cursorActivity",
            "A:cm.changes(1)",
            "A:cm.update",
            "B:cm.changes(1)",
            "B:cm.update",
            "=== a.operation{a.replaceRange; b.replaceRange}",
            "A:cm.beforeChange(1:0,[\"R\"])",
            "B:cm.beforeChange(2:0,[\"S\"])",
            "body end",
            "A:doc.change(1:0)",
            "A:cm.change(1:0,[\"R\"])",
            "B:doc.change(1:0)",
            "B:cm.change(1:0,[\"R\"])",
            "B:doc.change(2:0)",
            "B:cm.change(2:0,[\"S\"])",
            "A:doc.change(2:0)",
            "A:cm.change(2:0,[\"S\"])",
            "A:cm.changes(2)",
            "A:cm.update",
            "B:cm.changes(2)",
            "B:cm.update",
            "=== b.operation{a.replaceRange} (group owned by b)",
            "A:cm.beforeChange(3:0,[\"T\"])",
            "body end",
            "A:doc.change(3:0)",
            "A:cm.change(3:0,[\"T\"])",
            "B:doc.change(3:0)",
            "B:cm.change(3:0,[\"T\"])",
            "B:cm.changes(1)",
            "B:cm.update",
            "A:cm.changes(1)",
            "A:cm.update",
            "=== done"
        ]
    );
    eq_all(&set, &value(&set, a), &[a, b]);
    assert!(!group_open());
}

/// s4b: a `change` handler of one editor edits another; the other editor's
/// operation joins the open group and ends after the first editor's.
#[test]
fn s4b_cross_editor_handler() {
    let log: Log = Rc::default();
    let mut set = EditorSet::new();
    let c = set.insert(editor_over(Doc::new(&lines(3))));
    let e = set.insert(editor_over(Doc::new(&lines(3))));
    {
        let ed = set.get(e).unwrap();
        let l = log.clone();
        ed.on_change(move |_, ch| {
            push(
                &l,
                format!("E:cm.change({}:{},{:?})", ch.from.line, ch.from.ch, ch.text),
            )
        });
        let l = log.clone();
        ed.on_changes(move |_, cs| push(&l, format!("E:cm.changes({})", cs.len())));
        let l = log.clone();
        ed.on_cursor_activity(move |_| push(&l, "E:cm.cursorActivity"));
        let l = log.clone();
        ed.on_update(move |_| push(&l, "E:cm.update"));
    }
    {
        let ed = set.get(c).unwrap();
        let l = log.clone();
        ed.on_change(move |cx, _| {
            push(&l, "C:cm.change -> e.replaceRange");
            assert!(cx.with_editor(e, |cx| {
                cx.run_op(|cm, _| cm.doc.replace_range("w", p(0, 0), None, None))
            }));
            push(&l, "C: back from e.replaceRange");
        });
        let l = log.clone();
        ed.on_changes(move |_, _| push(&l, "C:cm.changes"));
    }
    rr(&mut set, c, "v", p(0, 0), None);
    push(&log, "=== done");
    assert_eq!(
        take(&log),
        [
            "C:cm.change -> e.replaceRange",
            "C: back from e.replaceRange",
            "E:cm.change(0:0,[\"w\"])",
            "E:cm.cursorActivity",
            "C:cm.changes",
            "E:cm.changes(1)",
            "E:cm.update",
            "=== done"
        ]
    );
}

/// doc_test `basic`: edits through either editor reach both.
#[test]
fn doc_basic_with_editors() {
    for shared_hist in [true, false] {
        let mut set = EditorSet::new();
        let a = set.insert(editor_over(Doc::new("x")));
        let b = linked(&mut set, a, shared_hist, None);
        eq_all(&set, "x", &[a, b]);
        set.run(a, |cx| cx.doc.set_value("hey"));
        eq_all(&set, "hey", &[a, b]);
        set.run(b, |cx| cx.doc.set_value("wow"));
        eq_all(&set, "wow", &[a, b]);
        rr(&mut set, a, "u\nv\nw", p(0, 3), None);
        rr(&mut set, b, "i", p(0, 4), None);
        rr(&mut set, b, "j", p(2, 1), None);
        eq_all(&set, "wowui\nv\nwj", &[a, b]);
        for id in [a, b] {
            assert_eq!(set.get(id).unwrap().parked_len(), 0);
        }
    }
}

/// doc_test `sharedHist`.
#[test]
fn doc_shared_hist_with_editors() {
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new("ab\ncd\nef")));
    let b = linked(&mut set, a, true, None);
    let end = |l: i64| Pos::new(l, 2);
    rr(&mut set, a, "x", end(0), None);
    rr(&mut set, b, "y", end(1), None);
    rr(&mut set, a, "z", end(2), None);
    eq_all(&set, "abx\ncdy\nefz", &[a, b]);
    let undo = |set: &mut EditorSet, id| set.run(id, |cx| cx.doc.undo()).unwrap();
    let redo = |set: &mut EditorSet, id| set.run(id, |cx| cx.doc.redo()).unwrap();
    undo(&mut set, a);
    undo(&mut set, a);
    eq_all(&set, "abx\ncd\nef", &[a, b]);
    redo(&mut set, a);
    eq_all(&set, "abx\ncdy\nef", &[a, b]);
    redo(&mut set, b);
    eq_all(&set, "abx\ncdy\nefz", &[a, b]);
    undo(&mut set, a);
    undo(&mut set, b);
    undo(&mut set, a);
    undo(&mut set, a);
    eq_all(&set, "ab\ncd\nef", &[a, b]);
}

/// doc_test `broadCast`: one document linked into four editors; every editor gets
/// its `change`, in link order, and ends its operation in the same group.
#[test]
fn doc_broadcast_with_editors() {
    let log: Log = Rc::default();
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new("")));
    let others: Vec<EditorId> = (0..4).map(|_| linked(&mut set, a, true, None)).collect();
    for (i, &id) in std::iter::once(&a).chain(&others).enumerate() {
        let l = log.clone();
        set.get(id)
            .unwrap()
            .on_changes(move |_, _| push(&l, format!("changes {i}")));
    }
    set.run(others[0], |cx| cx.doc.set_value("uu"));
    let mut all = vec![a];
    all.extend(&others);
    eq_all(&set, "uu", &all);
    assert_eq!(
        take(&log),
        [
            "changes 1",
            "changes 0",
            "changes 2",
            "changes 3",
            "changes 4"
        ],
        "the owner first, then in the order the change reached the editors"
    );
    rr(&mut set, a, "v", p(0, 1), None);
    eq_all(&set, "uvu", &all);
}

/// doc_test `sharedMarker` with editors: clearing a shared marker inside one
/// editor's operation clears it in every linked editor, and its `clear` handler
/// runs once, when the group ends; each editor reports `markerCleared`.
#[test]
fn doc_shared_marker_with_editors() {
    let log: Log = Rc::default();
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new("ab\ncd\nef\ngh")));
    let b = linked(&mut set, a, true, None);
    let c = linked(&mut set, a, false, Some((1, 2)));
    let mark = set
        .run(b, |cx| {
            cx.doc
                .mark_text(
                    p(0, 1),
                    p(3, 1),
                    MarkOptions {
                        class_name: Some("cm-searching".into()),
                        shared: true,
                        ..Default::default()
                    },
                )
                .unwrap()
        })
        .unwrap();
    let found = set.get(a).unwrap().doc.find_marks_at(p(0, 2));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], mark);
    assert_eq!(set.get(c).unwrap().doc.find_marks_at(p(1, 1)).len(), 1);
    rr(&mut set, b, "x\ny\n", p(0, 0), None);
    assert!(matches!(mark.find(), Some(MarkerFind::Range { from, .. }) if from == p(2, 1)));
    let Marker::Shared(sm) = &mark else {
        panic!("shared")
    };
    let cleared = Rc::new(Cell::new(0));
    let c2 = cleared.clone();
    sm.on_clear(move || c2.set(c2.get() + 1));
    for (id, tag) in [(a, "A"), (b, "B"), (c, "C")] {
        let l = log.clone();
        set.get(id)
            .unwrap()
            .on_marker_cleared(move |_, _, _, _| push(&l, format!("{tag}:markerCleared")));
    }
    set.run(b, |_| mark.clear());
    for id in [a, b, c] {
        assert_eq!(set.get(id).unwrap().doc.find_marks_at(p(3, 1)).len(), 0);
        assert_eq!(set.get(id).unwrap().parked_len(), 0);
    }
    assert!(mark.find().is_none());
    assert_eq!(cleared.get(), 1);
    let mut got = take(&log);
    got.sort();
    assert_eq!(
        got,
        ["A:markerCleared", "B:markerCleared", "C:markerCleared"]
    );
}

/// doc_test `sharedBookmark` with editors.
#[test]
fn doc_shared_bookmark_with_editors() {
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new("ab\ncd\nef\ngh")));
    let b = linked(&mut set, a, true, None);
    let c = linked(&mut set, a, false, Some((1, 2)));
    let mark = set
        .run(b, |cx| cx.doc.set_bookmark(p(1, 1), false, true).unwrap())
        .unwrap();
    assert_eq!(set.get(a).unwrap().doc.find_marks_at(p(1, 1)).len(), 1);
    assert_eq!(set.get(c).unwrap().doc.find_marks_at(p(1, 1)).len(), 1);
    rr(&mut set, b, "x\ny\n", p(0, 0), None);
    assert!(matches!(mark.find(), Some(MarkerFind::Point(q)) if q == p(3, 1)));
    let Marker::Shared(sm) = &mark else {
        panic!("shared")
    };
    let cleared = Rc::new(Cell::new(0));
    let c2 = cleared.clone();
    sm.on_clear(move || c2.set(c2.get() + 1));
    set.run(b, |_| mark.clear());
    for id in [a, b, c] {
        let d = &set.get(id).unwrap().doc;
        assert_eq!(d.find_marks(p(0, 0), Pos::new(5, 0), None).len(), 0);
    }
    assert!(mark.find().is_none());
    assert_eq!(cleared.get(), 1);
}

/// An editor that is busy further up the stack cannot be reached; a change that
/// reaches its document parks the editor's part, which is delivered when the
/// outermost call returns.
#[test]
fn busy_editor_is_unreachable_and_its_work_is_parked() {
    let log: Log = Rc::default();
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new("abc")));
    let b = linked(&mut set, a, true, None);
    let l = log.clone();
    set.get(a)
        .unwrap()
        .on_change(move |_, c| push(&l, format!("A:change {:?}", c.text)));
    let l = log.clone();
    set.get(b).unwrap().on_change(move |cx, c| {
        push(&l, format!("B:change {:?}", c.text));
        if c.text == ["1"] {
            let reached = cx.with_editor(a, |_| {});
            push(&l, format!("B reaches A: {reached}"));
        }
    });
    let l = log.clone();
    set.get(a).unwrap().on_update(move |_| push(&l, "A:update"));
    // A runs; inside, B's handler cannot reach A (A is out of its slot).
    rr(&mut set, a, "1", p(0, 0), None);
    assert_eq!(
        take(&log),
        [
            "A:change [\"1\"]",
            "B:change [\"1\"]",
            "B reaches A: false",
            "A:update"
        ]
    );
    // Reaching a busy editor through the set itself fails the same way.
    set.run(a, |cx| {
        assert!(!cx.others.with_editor(a, &mut |_, _| {}));
        cx.doc.replace_range("2", p(0, 0), None, None);
    });
    assert_eq!(
        take(&log),
        ["A:change [\"2\"]", "B:change [\"2\"]", "A:update"]
    );
    assert!(
        set.run(EditorId::next(), |_| ()).is_none(),
        "unknown editor"
    );
}

/// A direct call on a document whose editor is in the set parks the editor's part;
/// the next `run` (or `flush_parked`) delivers it with the set reachable.
#[test]
fn direct_document_calls_are_delivered_by_the_set() {
    let log: Log = Rc::default();
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new("abc")));
    let b = linked(&mut set, a, true, None);
    for (id, tag) in [(a, "A"), (b, "B")] {
        let l = log.clone();
        set.get(id)
            .unwrap()
            .on_changes(move |_, cs| push(&l, format!("{tag}:changes({})", cs.len())));
    }
    let doc = set.get(a).unwrap().doc.clone();
    doc.replace_range("x", p(0, 0), None, None);
    assert!(take(&log).is_empty(), "parked");
    assert_eq!(set.get(a).unwrap().parked_len(), 1);
    assert_eq!(set.get(b).unwrap().parked_len(), 1);
    set.flush_parked();
    let mut got = take(&log);
    got.sort();
    assert_eq!(got, ["A:changes(1)", "B:changes(1)"]);
}

/// D5 in a set: removing and dropping an editor detaches its document; work queued
/// for it is discarded with a diagnostic; the linked editor keeps working.
#[test]
fn removed_editor_work_is_discarded() {
    let log: Log = Rc::default();
    let mut set = EditorSet::new();
    let a = set.insert(editor_over(Doc::new("abc")));
    let b = linked(&mut set, a, true, None);
    let l = log.clone();
    set.get(b)
        .unwrap()
        .on_change(move |_, _| push(&l, "B:change"));
    let l = log.clone();
    set.get(a)
        .unwrap()
        .on_change(move |_, _| push(&l, "A:change"));
    let doc_a = set.get(a).unwrap().doc.clone();
    doc_a.replace_range("x", p(0, 0), None, None);
    take_diagnostics();
    drop(set.remove(b).expect("not busy"));
    let d = take_diagnostics();
    assert!(
        d.iter().any(|m| m.contains(&format!("editor {}", b.get()))),
        "{d:?}"
    );
    set.flush_parked();
    assert_eq!(take(&log), ["A:change"]);
    rr(&mut set, a, "y", p(0, 0), None);
    assert_eq!(take(&log), ["A:change"]);
    assert_eq!(value(&set, a), "yxabc");
}
