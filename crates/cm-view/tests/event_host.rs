//! The host side of events: the orphan queue (`setTimeout(fireOrphanDelayed, 0)`)
//! delivered by the host, editor work parked by direct document calls, a dropped
//! editor's document (D5), and the input events (`inputRead`, `electricInput`,
//! `keyHandled`, `focus`, `blur`) through the application layer.

mod common;

use std::rc::Rc;

use cm_core::operation_group::{orphans_pending, take_diagnostics};
use cm_core::{Doc, MarkOptions};
use cm_modes::{Registry, Spec};
use cm_view::app::{App, AppOptions, Input, MemClipboard};
use cm_view::{Editor, EditorOptions, KeyEvent, Metrics};
use common::events::on;
use common::*;

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

/// s7_orphan: callbacks queued outside any operation wait for the host; an
/// unrelated editor operation delivers its own events but leaves the orphan queue
/// alone. The host then delivers the queue in order. (The recording's last entry,
/// a user `setTimeout(0)` queued after the orphans, is the browser's task order and
/// has no counterpart here.)
#[test]
fn s7_orphan() {
    let log: Log = Rc::default();
    let d = Doc::new("one\ntwo");
    let l = log.clone();
    let l2 = l.clone();
    d.on_change(move |_, c| push(&l2, format!("doc.change({:?})", c.text)));
    let l2 = l.clone();
    d.on_cursor_activity(move |_| push(&l2, "doc.cursorActivity"));
    let l2 = l.clone();
    d.get_line_handle(0)
        .unwrap()
        .on_change(move |_, _| push(&l2, "line0.change"));
    d.replace_range("A", p(0, 0), None, None);
    push(&log, "--- sync after edit 1");
    d.replace_range("B", p(1, 0), None, None);
    push(&log, "--- sync after edit 2");
    let mut cm = editor_over(Doc::new("x"));
    let l2 = log.clone();
    on(&cm, "change", move |_| push(&l2, "cm.change"));
    cm.run_op(|cm| cm.doc.replace_range("y", p(0, 0), None, None));
    push(&log, "--- after unrelated editor op");
    assert!(orphans_pending());
    cm.fire_orphan_delayed();
    push(&log, "--- after tick");
    assert_eq!(
        take(&log),
        [
            "--- sync after edit 1",
            "--- sync after edit 2",
            "cm.change",
            "--- after unrelated editor op",
            "line0.change",
            "doc.change([\"A\"])",
            "doc.cursorActivity",
            "doc.change([\"B\"])",
            "--- after tick"
        ]
    );
}

/// s8 (D5): CM5 has no `destroy`; the recording detaches the document with
/// `swapDoc`. Here dropping the editor detaches it. The editor's `change` handler
/// no longer runs for the old document; its marker's `clear` is an orphan the host
/// delivers.
#[test]
fn s8_document_after_its_editor_is_gone() {
    let log: Log = Rc::default();
    let mut cm = editor_over(Doc::new("abc"));
    let d = cm.doc.clone();
    let m = cm.run_op(|cm| {
        cm.doc
            .mark_text(p(0, 0), p(0, 1), MarkOptions::default())
            .unwrap()
            .text_marker()
    });
    let l = log.clone();
    m.on_clear(move |_| push(&l, "m.clear delivered"));
    let l = log.clone();
    on(&cm, "change", move |_| push(&l, "cm.change"));
    drop(cm);
    push(&log, "--- after drop; old doc detached");
    m.clear();
    d.replace_range("Z", p(0, 0), None, None);
    push(&log, "--- after edits on detached doc");
    let mut host = editor_over(Doc::new(""));
    host.fire_orphan_delayed();
    push(&log, "--- after tick");
    assert_eq!(
        take(&log),
        [
            "--- after drop; old doc detached",
            "--- after edits on detached doc",
            "m.clear delivered",
            "--- after tick"
        ]
    );
    assert!(
        take_diagnostics().is_empty(),
        "nothing was pending for the editor"
    );
}

fn app(text: &str, mode: &str) -> App {
    cm_core::clock::manual(1_000_000.0);
    App::new(
        text,
        None,
        Metrics::oracle(),
        &AppOptions {
            mode: Some(mode.into()),
            ..Default::default()
        },
        Box::new(MemClipboard::default()),
    )
}

fn key(name: &str, text: Option<&str>) -> Input {
    Input::Key {
        key: KeyEvent::parse(name),
        text: text.map(Into::into),
    }
}

/// Typing through the application: `inputRead` per typed change, `electricInput`
/// when the mode reindents, and `keyHandled` for a bound key, all delayed to the
/// end of the key's operation.
#[test]
fn input_events_through_the_app() {
    let log: Log = Rc::default();
    let mut a = app("{\n  x", "text/javascript");
    for name in ["inputRead", "electricInput", "keyHandled", "change"] {
        let (l, n) = (log.clone(), name.to_string());
        on(&a.editor, name, move |args| push(&l, format!("{n}{args}")));
    }
    a.handle(key("Ctrl-End", None));
    a.handle(key("Enter", None));
    a.handle(Input::Text("}".into()));
    assert_eq!(a.editor.get_value(), "{\n  x\n}");
    assert_eq!(
        take(&log),
        [
            "keyHandled(Ctrl-End)",
            "change(1:3,[\"\", \"\"])",
            "change(2:0,[\"  \"])",
            "keyHandled(Enter)",
            "change(2:2,[\"}\"])",
            "inputRead(2:2,[\"}\"])",
            "change(2:0,[\"\"])",
            "electricInput(2)",
        ]
    );
}

/// `focus` and `blur` run immediately, once per change of state.
#[test]
fn focus_and_blur() {
    let log: Log = Rc::default();
    let mut a = app("x", "text/plain");
    for name in ["focus", "blur"] {
        let (l, n) = (log.clone(), name.to_string());
        on(&a.editor, name, move |_| push(&l, n.clone()));
    }
    a.set_focused(true);
    a.set_focused(true);
    assert!(a.editor.has_focus());
    a.set_focused(false);
    assert_eq!(take(&log), ["focus", "blur"]);
}

/// The application delivers parked editor work and orphans after each input and
/// when idle, so a direct document call never leaves work behind.
#[test]
fn app_delivers_pending_work() {
    let log: Log = Rc::default();
    let mut a = app("abc", "text/plain");
    let l = log.clone();
    on(&a.editor, "changes", move |args| {
        push(&l, format!("changes{args}"))
    });
    let d = a.editor.doc.clone();
    d.replace_range("x", p(0, 0), None, None);
    assert_eq!(a.editor.parked_len(), 1);
    let bare = Doc::new("q");
    let l = log.clone();
    bare.on_change(move |_, _| push(&l, "bare.change"));
    bare.replace_range("r", p(0, 0), None, None);
    a.idle();
    assert_eq!(a.editor.parked_len(), 0);
    assert!(!orphans_pending());
    assert_eq!(take(&log), ["changes(1)", "bare.change"]);
    d.replace_range("y", p(0, 0), None, None);
    a.handle(Input::Wheel { dx: 0.0, dy: 0.0 });
    assert_eq!(take(&log), ["changes(1)"]);
}
