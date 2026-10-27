//! Lifetime of the `ensureCursorVisible` requests cm-core queues for the view
//! (`Doc::take_scroll_requests`).
//!
//! CM5 writes the request into `doc.cm.curOp` (src/display/scrolling.js:119), so a
//! request belongs to the editor the document was attached to when it was made. A
//! document detached from that editor must not carry the request to the next editor
//! it is attached to (review finding L1, 2026-10-26).

use std::rc::Rc;

use cm_core::{Doc, Pos};
use cm_modes::{Registry, Spec};
use cm_view::{Editor, EditorOptions, Metrics};

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}

fn text() -> String {
    (0..200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// An editor over `doc`, sized like the recording page (900x500, oracle metrics).
fn editor_over(doc: Doc) -> Editor {
    let mut cm = Editor::with_doc(
        doc,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    cm.display.window_height = Some(900.0);
    cm.set_size(900.0, 500.0);
    cm
}

#[test]
fn detach_discards_pending_cursor_scroll() {
    let doc = Doc::new(&text());
    let first = editor_over(doc.clone());
    // A direct document call while attached: `setSelectionNoUndo` queues
    // `ensureCursorVisible` for the editor, which has not run an operation yet.
    first.doc.set_cursor(p(150, 0), None);
    doc.detach_editor_semantics();
    drop(first);
    assert!(
        doc.take_scroll_requests().is_empty(),
        "detach left a pending scroll request"
    );
}

#[test]
fn next_editor_does_not_apply_stale_request() {
    let doc = Doc::new(&text());
    let first = editor_over(doc.clone());
    first.doc.set_cursor(p(150, 0), None);
    doc.detach_editor_semantics();
    drop(first);
    let mut second = editor_over(doc.clone());
    second.run_op(|_| {});
    assert_eq!(second.display.scroll_top, 0.0, "stale request scrolled");
    assert_eq!(second.doc.get_cursor(None), p(150, 0));
}

#[test]
fn attached_document_still_scrolls() {
    let doc = Doc::new(&text());
    let first = editor_over(doc.clone());
    doc.detach_editor_semantics();
    drop(first);
    let mut second = editor_over(doc.clone());
    // Same value as the recorded `replaceSelection_*` scenarios (150:0 → 5242).
    second.run_op(|cm| cm.doc.set_cursor(p(150, 0), None));
    assert_eq!(second.display.scroll_top, 5242.0);
    // A direct call while attached is applied by the next operation.
    second.doc.set_cursor(p(0, 0), None);
    second.run_op(|_| {});
    assert_eq!(second.display.scroll_top, 0.0);
}

/// An editor dropped without `detach_editor_semantics` (the `Editor` has no `Drop`):
/// attaching the document to the next editor must not inherit its request either.
/// CM5 cannot reach this state (`attachDoc` throws "This document is already in
/// use."); here attaching starts from an empty queue.
#[test]
fn attach_discards_request_left_by_dropped_editor() {
    let doc = Doc::new(&text());
    let first = editor_over(doc.clone());
    first.doc.set_cursor(p(150, 0), None);
    drop(first);
    let mut second = editor_over(doc.clone());
    second.run_op(|_| {});
    assert_eq!(second.display.scroll_top, 0.0, "stale request scrolled");
    assert_eq!(second.doc.get_cursor(None), p(150, 0));
}
