//! Operation groups with real editors: ownership of the group, the delivery contract
//! for direct document calls (D6), editor destruction (D5), unwinding (D4), and
//! thread confinement.

use std::rc::Rc;

use cm_core::operation_group::{group_open, take_diagnostics};
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

/// D6: a document method called directly on an attached document runs in an
/// operation of the editor; the editor's part (its display update and scroll) is
/// parked on the editor and delivered at its next entry, never dropped.
#[test]
fn direct_document_call_parks_editor_work_until_the_next_entry() {
    let doc = Doc::new(&text());
    let mut cm = editor_over(doc.clone());
    assert_eq!(cm.parked_len(), 0);
    doc.set_cursor(p(150, 0), None);
    assert!(!group_open(), "the document call ended its own group");
    assert_eq!(cm.parked_len(), 1, "the operation end is parked");
    assert_eq!(cm.display.scroll_top, 0.0, "nothing ran without the editor");
    cm.run_op(|_| {});
    assert_eq!(cm.parked_len(), 0);
    assert_eq!(
        cm.display.scroll_top, 5242.0,
        "recorded CM5 value for 150:0"
    );
}

/// D6: the host can deliver parked work without an editor operation.
#[test]
fn flush_parked_delivers_without_an_operation() {
    let doc = Doc::new(&text());
    let mut cm = editor_over(doc.clone());
    doc.replace_range("x", p(150, 0), None, None);
    doc.set_cursor(p(150, 0), None);
    assert_eq!(cm.parked_len(), 2);
    cm.flush_parked();
    assert_eq!(cm.parked_len(), 0);
    assert_eq!(cm.display.scroll_top, 5242.0);
}

/// Calls through the editor never park: the editor owns the group it opens.
#[test]
fn editor_operations_do_not_park() {
    let mut cm = editor_over(Doc::new(&text()));
    cm.run_op(|cm| {
        cm.doc.replace_range("x", p(0, 0), None, None);
        cm.run_op(|cm| cm.doc.set_cursor(p(150, 0), None));
    });
    assert_eq!(cm.parked_len(), 0);
    assert_eq!(cm.display.scroll_top, 5242.0);
}

/// D5: dropping an editor detaches its document and discards what was parked on
/// it, with a diagnostic; the document keeps working as a plain document.
#[test]
fn dropping_an_editor_discards_parked_work_and_detaches_the_doc() {
    let doc = Doc::new(&text());
    let cm = editor_over(doc.clone());
    let id = cm.id;
    doc.set_cursor(p(150, 0), None);
    assert_eq!(cm.parked_len(), 1);
    take_diagnostics();
    drop(cm);
    let d = take_diagnostics();
    assert_eq!(d.len(), 1, "{d:?}");
    assert!(d[0].contains(&format!("editor {}", id.get())), "{d:?}");
    assert!(doc.editor_id().is_none(), "the document is detached");
    doc.replace_range("y", p(0, 0), None, None);
    assert_eq!(doc.get_line(0).unwrap(), "yline 0");
    assert!(
        take_diagnostics().is_empty(),
        "a detached document queues nothing"
    );
}

/// D4: a panic in an operation body still ends the operation and the group; the
/// editor is usable afterwards.
#[test]
fn panic_in_an_operation_ends_it() {
    let mut cm = editor_over(Doc::new(&text()));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cm.run_op(|cm| {
            cm.doc.set_cursor(p(150, 0), None);
            panic!("body");
        })
    }));
    assert!(r.is_err());
    assert!(!group_open());
    assert_eq!(cm.display.scroll_top, 5242.0, "the display phase still ran");
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 0), None));
    assert_eq!(cm.display.scroll_top, 0.0);
}

/// Event state is per thread: an editor, its document and its markers cannot be
/// sent to another thread (compile-time check), and another thread does not see
/// this thread's open group.
#[test]
fn event_state_stays_on_its_thread() {
    trait AmbiguousIfSend<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    // Each line compiles only while the type is not `Send`.
    <Editor as AmbiguousIfSend<_>>::check();
    <Doc as AmbiguousIfSend<_>>::check();
    <cm_core::TextMarker as AmbiguousIfSend<_>>::check();

    let mut cm = editor_over(Doc::new(&text()));
    cm.run_op(|_| {
        assert!(group_open());
        let other = std::thread::spawn(group_open).join().unwrap();
        assert!(!other, "another thread has its own (empty) group state");
    });
}

/// A document has at most one editor: a second editor over it fails with
/// `DocInUse` ("This document is already in use.") and the first keeps its
/// events.
#[test]
fn a_document_in_use_cannot_get_a_second_editor() {
    let doc = Doc::new(&text());
    let mut a = editor_over(doc.clone());
    let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let l = log.clone();
    a.on_change(move |_, _| l.borrow_mut().push("A:change".into()));
    let r = Editor::try_with_doc(
        doc.clone(),
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    assert!(matches!(r, Err(cm_core::CmError::DocInUse)));
    assert_eq!(doc.editor_id(), Some(a.id));
    a.run_op(|cm| cm.doc.replace_range("X", p(0, 0), None, None));
    assert_eq!(*log.borrow(), ["A:change"]);
}

/// D4 on the parked path: a panic in a parked callback skips the rest of that
/// operation's callbacks, but the parked operation end (display update, `changes`,
/// `update`) still runs before the panic reaches the caller.
#[test]
fn panic_in_a_parked_callback_still_ends_the_operation() {
    let doc = Doc::new(&text());
    let mut cm = editor_over(doc.clone());
    let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    cm.on_change(|_, _| std::panic::panic_any("boom"));
    let l = log.clone();
    cm.on_change(move |_, _| l.borrow_mut().push("change h2".into()));
    let l = log.clone();
    cm.on_changes(move |_, _| l.borrow_mut().push("changes".into()));
    let l = log.clone();
    cm.on_update(move |_| l.borrow_mut().push("update".into()));
    doc.replace_range("X", p(150, 0), None, None);
    doc.set_cursor(p(150, 0), None);
    assert_eq!(
        cm.parked_len(),
        4,
        "two change calls and an end; the second end"
    );
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cm.flush_parked()));
    assert_eq!(r.unwrap_err().downcast_ref::<&str>(), Some(&"boom"));
    assert_eq!(cm.parked_len(), 0);
    assert_eq!(*log.borrow(), ["changes", "update"]);
    assert_eq!(
        cm.display.scroll_top, 5242.0,
        "the second parked end ran too"
    );
}
