//! Document, line handle and marker events (src/model/document_data.js,
//! selection_updates.js, chunk.js, history.js, mark_text.js). Scenario names refer to
//! the 2026-10-26 CM5 event recordings (`s2-event-system/cm5-events.json`).

use std::cell::RefCell;
use std::rc::Rc;

use cm_core::operation_group::{fire_orphan_delayed, group_open, orphans_pending};
use cm_core::{Doc, EditorSemantics, MarkOptions, NoEditors, Pos};

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

/// s7_orphan (the document part): without an editor no operation is open, so the
/// delayed `change`/`cursorActivity` callbacks wait in the orphan queue until the
/// host delivers it; then they run in queue order. Recorded: `line0.change`,
/// `doc.change(["A"])`, `doc.cursorActivity`, `doc.change(["B"])`.
#[test]
fn s7_bare_document_events_wait_for_the_host() {
    let log: Log = Rc::default();
    let d = Doc::new("one\ntwo");
    let l = log.clone();
    d.on_change(move |_, c| push(&l, format!("doc.change({:?})", c.text)));
    let l = log.clone();
    d.on_cursor_activity(move |_| push(&l, "doc.cursorActivity"));
    let l = log.clone();
    d.get_line_handle(0)
        .unwrap()
        .on_change(move |_, _| push(&l, "line0.change"));
    d.replace_range("A", p(0, 0), None, None);
    push(&log, "--- sync after edit 1");
    d.replace_range("B", p(1, 0), None, None);
    push(&log, "--- sync after edit 2");
    assert!(orphans_pending());
    fire_orphan_delayed(&mut NoEditors);
    assert_eq!(
        take(&log),
        [
            "--- sync after edit 1",
            "--- sync after edit 2",
            "line0.change",
            "doc.change([\"A\"])",
            "doc.cursorActivity",
            "doc.change([\"B\"])"
        ]
    );
}

/// The change object: `{from, to, text, removed, origin}`, `to` in pre-change
/// coordinates (upstream `change_removedText` checks `removed`).
#[test]
fn change_payload_has_removed_text_and_pre_change_to() {
    let log: Log = Rc::default();
    let d = Doc::new("abc\ndef\nghi");
    let l = log.clone();
    d.on_change(move |_, c| {
        push(
            &l,
            format!(
                "{}:{}-{}:{} {:?} {:?} {:?}",
                c.from.line, c.from.ch, c.to.line, c.to.ch, c.text, c.removed, c.origin
            ),
        )
    });
    d.replace_range("X\nY", p(0, 1), Some(p(2, 1)), Some("+input"));
    fire_orphan_delayed(&mut NoEditors);
    assert_eq!(
        take(&log),
        ["0:1-2:1 [\"X\", \"Y\"] [\"bc\", \"def\", \"g\"] Some(\"+input\")"]
    );
}

/// Operations (editor semantics): document events are delivered when the
/// outermost operation ends, in queue order, not before.
#[test]
fn operation_delivers_document_events_at_its_end() {
    let log: Log = Rc::default();
    let d = Doc::new("one\ntwo");
    d.attach_editor_semantics(EditorSemantics::default());
    let l = log.clone();
    d.on_change(move |_, c| push(&l, format!("change {:?}", c.text)));
    let l = log.clone();
    d.on_cursor_activity(move |_| push(&l, "cursorActivity"));
    d.operation(|| {
        d.replace_range("a", p(0, 0), None, None);
        d.operation(|| d.replace_range("b", p(1, 0), None, None));
        d.set_cursor(p(1, 1), None);
        push(&log, "body end");
        assert!(group_open());
    });
    assert!(!orphans_pending(), "nothing was left for the host");
    assert_eq!(
        take(&log),
        [
            "body end",
            "change [\"a\"]",
            "cursorActivity",
            "change [\"b\"]",
            "cursorActivity"
        ]
    );
}

/// chunk.js:38: a removed line's `delete` handlers run (delayed); a line whose
/// text changed gets `change`.
#[test]
fn line_handles_get_change_and_delete() {
    let log: Log = Rc::default();
    let d = Doc::new("a\nb\nc");
    for n in 0..3 {
        let h = d.get_line_handle(n).unwrap();
        let l = log.clone();
        h.on_change(move |h, _| push(&l, format!("line{n}.change {}", h.text())));
        let l = log.clone();
        h.on_delete(move || push(&l, format!("line{n}.delete")));
    }
    d.replace_range("", p(0, 1), Some(p(2, 0)), None);
    assert!(take(&log).is_empty(), "delayed");
    fire_orphan_delayed(&mut NoEditors);
    assert_eq!(
        take(&log),
        ["line0.change ac", "line1.delete", "line2.delete"]
    );
}

/// history.js:102: `historyAdded` fires (immediately) for a change that starts a
/// new history event, not for one merged into the previous event.
#[test]
fn history_added_only_for_new_events() {
    let log: Log = Rc::default();
    let d = Doc::new("");
    let l = log.clone();
    d.on_history_added(move |_| push(&l, "historyAdded"));
    d.replace_range("a", p(0, 0), None, Some("+input"));
    assert_eq!(take(&log), ["historyAdded"], "immediate");
    d.replace_range("b", p(0, 1), None, Some("+input"));
    assert!(take(&log).is_empty(), "merged into the previous event");
    d.replace_range("c", p(0, 2), None, Some("paste"));
    assert_eq!(take(&log), ["historyAdded"]);
}

/// s3 (`m4`): clearing a marker of a document without an editor queues its `clear`
/// handler as an orphan (`setTimeout`), not synchronously.
#[test]
fn s3_editorless_marker_clear_is_an_orphan() {
    let log: Log = Rc::default();
    let d = Doc::new("line 0\nline 1\nline 2");
    let m = d
        .mark_text(p(0, 0), p(0, 2), MarkOptions::default())
        .unwrap()
        .text_marker();
    let l = log.clone();
    m.on_clear(move |_| push(&l, "m4.clear (editorless doc)"));
    m.clear();
    push(&log, "--- after m4.clear sync");
    fire_orphan_delayed(&mut NoEditors);
    assert_eq!(
        take(&log),
        ["--- after m4.clear sync", "m4.clear (editorless doc)"]
    );
}

/// mark_text.js:48-50: `clear` handlers are queued only when the marker is found;
/// a bookmark's `find()` is a position, so its handler gets no range.
#[test]
fn clear_handler_payloads() {
    let log: Log = Rc::default();
    let d = Doc::new("abcdef");
    let bm = d.set_bookmark(p(0, 5), false, false).unwrap().text_marker();
    let l = log.clone();
    bm.on_clear(move |r| push(&l, format!("bookmark {r:?}")));
    let gone = d
        .mark_text(p(0, 1), p(0, 3), MarkOptions::default())
        .unwrap()
        .text_marker();
    let l = log.clone();
    gone.on_clear(move |r| push(&l, format!("gone {r:?}")));
    // Deleting the marked text (clearWhenEmpty) removes the marker's spans.
    d.replace_range("", p(0, 0), Some(p(0, 4)), None);
    bm.clear();
    gone.clear();
    fire_orphan_delayed(&mut NoEditors);
    assert_eq!(
        take(&log),
        ["bookmark None"],
        "a marker that is not found gets no clear"
    );
}

/// `beforeCursorEnter` runs immediately when the cursor would enter the marker; a
/// handler that clears the marker lets the cursor in (clearOnEnter uses the same
/// event).
#[test]
fn before_cursor_enter() {
    let log: Log = Rc::default();
    let d = Doc::new("abcdef");
    d.attach_editor_semantics(EditorSemantics::default());
    let m = d
        .mark_text(
            p(0, 1),
            p(0, 4),
            MarkOptions {
                atomic: true,
                ..Default::default()
            },
        )
        .unwrap()
        .text_marker();
    let (l, m2) = (log.clone(), m.clone());
    m.on_before_cursor_enter(move || {
        push(&l, "beforeCursorEnter");
        m2.clear();
    });
    d.set_cursor(p(0, 2), None);
    assert_eq!(take(&log), ["beforeCursorEnter"]);
    assert_eq!(d.get_cursor(None), p(0, 2), "cleared, so not skipped");
}

/// Handlers removed with `off` are not called; ids from one emitter do not
/// remove another's handlers.
#[test]
fn off_by_id() {
    let log: Log = Rc::default();
    let d = Doc::new("a");
    let l = log.clone();
    let id = d.on_change(move |_, _| push(&l, "change"));
    let h = d.get_line_handle(0).unwrap();
    let l = log.clone();
    let lid = h.on_change(move |_, _| push(&l, "line"));
    assert!(!d.off(lid), "a line handler id is not a document handler");
    assert!(d.off(id) && !d.off(id));
    d.replace_range("x", p(0, 0), None, None);
    fire_orphan_delayed(&mut NoEditors);
    assert_eq!(take(&log), ["line"]);
    assert!(h.off(lid));
}
