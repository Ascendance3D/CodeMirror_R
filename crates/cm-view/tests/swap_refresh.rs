//! `swapDoc` (src/edit/methods.js:423), `refresh` (:411) and `onResize`
//! (src/edit/global_events.js:39). The `swapDoc` tests port test/doc_test.js
//! "swapDoc", "docKeepsScroll", "copyDoc" and "docKeepsMode".

use std::cell::RefCell;
use std::rc::Rc;

use cm_core::{CmError, Doc, LinkedDocOptions, Pos};
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};

fn editor_with_doc(doc: Doc, mode: Spec) -> Editor {
    let mut cm = Editor::with_doc(
        doc,
        mode,
        EditorOptions::default(),
        Metrics::test_grid(),
        Rc::new(cm_modes::Registry::with_all_modes()),
    );
    cm.set_size(300.0, 200.0);
    cm
}

/// doc_test.js "swapDoc": `A='a' B*='b' C<A` (C linked to A, B has no editor).
#[test]
fn doc_test_swap_doc() {
    let a_doc = Doc::new("a");
    let mut a = editor_with_doc(a_doc, Spec::Null);
    let b = Doc::new("b");
    let c_doc = a.doc.linked_doc(LinkedDocOptions::default()).unwrap();
    let c = editor_with_doc(c_doc, Spec::Null);
    let d = a.swap_doc(b.clone()).unwrap();
    a.run_op(|_| {});
    d.set_value("x");
    assert_eq!(c.get_value(), "x");
    assert_eq!(d.get_value(None), "x");
    assert_eq!(a.get_value(), "b");
    assert_eq!(b.get_value(None), "b");
    // The editor edits its new document.
    a.run_op(|cm| cm.doc.replace_range("B", Pos::new(0, 0), None, None));
    assert_eq!(b.get_value(None), "Bb");
}

/// doc_test.js "docKeepsScroll".
#[test]
fn doc_test_doc_keeps_scroll() {
    let line = "x".repeat(200);
    let text = vec![line; 200].join("\n");
    let mut a = editor_with_doc(Doc::new(&text), Spec::Null);
    a.scroll_into_view(Pos::new(199, 200), Pos::new(199, 200), 0.0);
    assert!(a.display.scroll_top > 0.0 && a.display.scroll_left > 0.0);
    let c = a.swap_doc(Doc::new("y")).unwrap();
    assert_eq!((a.display.scroll_left, a.display.scroll_top), (0.0, 0.0));
    a.swap_doc(c).unwrap();
    assert!(a.display.scroll_left > 0.0, "not at left");
    assert!(a.display.scroll_top > 0.0, "not at top");
}

/// doc_test.js "copyDoc".
#[test]
fn doc_test_copy_doc() {
    let mut a = editor_with_doc(Doc::new("u"), Spec::Null);
    let copy = a.doc.copy(true);
    a.set_value("foo");
    copy.set_value("bar");
    let old = a.swap_doc(copy).unwrap();
    assert_eq!(a.get_value(), "bar");
    a.run_op(|cm| cm.doc.undo());
    assert_eq!(a.get_value(), "u");
    a.swap_doc(old.clone()).unwrap();
    assert_eq!(a.get_value(), "foo");
    assert_eq!(old.history_size().undo, 1);
    assert_eq!(old.copy(false).history_size().undo, 0);
}

/// doc_test.js "docKeepsMode" (markdown is not ported; text/x-rustsrc stands in).
#[test]
fn doc_test_doc_keeps_mode() {
    let mut a = editor_with_doc(Doc::new("1+1"), Spec::Null);
    let other = Doc::with_options(cm_core::DocOptions {
        text: "hi".into(),
        mode: Some("text/x-rustsrc".into()),
        ..Default::default()
    });
    a.set_mode(Spec::Str("text/javascript".into()));
    let old = a.swap_doc(other).unwrap();
    assert_eq!(a.mode_spec, Spec::Str("text/x-rustsrc".into()));
    assert_eq!(a.mode.name(), "rust");
    a.swap_doc(old).unwrap();
    assert_eq!(a.mode_spec, Spec::Str("text/javascript".into()));
    assert_eq!(a.mode.name(), "javascript");
}

/// `swapDoc` fires delayed with the old document, before the redraw; a document that already has
/// an editor is refused and nothing changes; the view, the longest line and the
/// highlighting follow the new document.
#[test]
fn swap_doc_event_in_use_and_state() {
    let mut a = editor_with_doc(Doc::new("short"), Spec::Str("text/javascript".into()));
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let l = log.clone();
    a.on_swap_doc(move |cx, old| {
        l.borrow_mut().push(format!(
            "swapDoc {} now {}",
            old.get_value(None),
            cx.get_value()
        ))
    });
    let l = log.clone();
    a.on_update(move |_| l.borrow_mut().push("update".into()));
    let other = editor_with_doc(Doc::new("taken"), Spec::Null);
    assert!(matches!(
        a.swap_doc(other.doc.clone()),
        Err(CmError::DocInUse)
    ));
    assert_eq!(a.get_value(), "short");
    assert!(log.borrow().is_empty());
    let long = (0..100)
        .map(|i| format!("var v{i} = {i};"))
        .collect::<Vec<_>>()
        .join("\n");
    let doc = Doc::with_options(cm_core::DocOptions {
        text: long,
        mode: Some("text/javascript".into()),
        ..Default::default()
    });
    a.swap_doc(doc).unwrap();
    {
        let log = log.borrow();
        assert_eq!(log.len(), 2, "{log:?}");
        // finishOperation fires the delayed callbacks (swapDoc) before
        // endOperations draws (update) (src/util/operation_group.js).
        assert!(log[0].starts_with("swapDoc short now var v0 = 0;\nvar v1"));
        assert_eq!(log[1], "update");
    }
    assert_eq!(a.display.view_from, 0);
    assert!(a.display.view_to > 10);
    assert_eq!(a.max_line().map(|(n, _)| n), Some(10));
    // The new document is highlighted with the mode.
    let toks = a.get_line_tokens(0, true);
    assert!(toks.iter().any(|t| t.ty.as_deref() == Some("keyword")));
}

/// `refresh()`: the `refresh` event (immediate, inside the operation) and a
/// redraw (`update`); `set_metrics` runs it with new metrics.
#[test]
fn refresh_event_and_redraw() {
    let mut cm = editor_with_doc(Doc::new("a\nb"), Spec::Null);
    let log = Rc::new(RefCell::new(Vec::<&str>::new()));
    let l = log.clone();
    cm.on_refresh(move |_| l.borrow_mut().push("refresh"));
    let l = log.clone();
    cm.on_update(move |_| l.borrow_mut().push("update"));
    cm.refresh();
    assert_eq!(*log.borrow(), ["refresh", "update"]);
    log.borrow_mut().clear();
    let mut m = Metrics::test_grid();
    m.char_width = 12.0;
    cm.set_metrics(m);
    assert_eq!(*log.borrow(), ["refresh", "update"]);
    assert_eq!(
        cm.char_coords(Pos::new(0, 1), cm_view::CoordSys::Line).left,
        16.0
    );
}

/// `onResize`: new metrics (the host measured again) and the new size; no
/// `refresh` event.
#[test]
fn on_resize_applies_metrics_and_size() {
    let mut cm = editor_with_doc(Doc::new("abc"), Spec::Null);
    let log = Rc::new(RefCell::new(Vec::<&str>::new()));
    let l = log.clone();
    cm.on_refresh(move |_| l.borrow_mut().push("refresh"));
    let mut m = Metrics::test_grid();
    m.char_width = 7.0;
    cm.on_resize(500.0, 100.0, m);
    assert_eq!((cm.display.width, cm.display.height), (500.0, 100.0));
    assert_eq!(cm.metrics.char_width, 7.0);
    assert!(log.borrow().is_empty());
}

/// Line widgets belong to their document: swapping in a document with a widget
/// gives its line the widget's height, and swapping back drops it.
#[test]
fn swap_doc_line_widget_heights() {
    let mut a = editor_with_doc(Doc::new("a\nb\nc"), Spec::Null);
    let th = Metrics::test_grid().text_height;
    let mut b = editor_with_doc(Doc::new("x\ny\nz"), Spec::Null);
    b.add_line_widget(1, Rc::new(()), 30.0, Default::default());
    let bdoc = b.swap_doc(Doc::new("")).unwrap();
    assert_eq!(a.height_at_line(2), 2.0 * th);
    let adoc = a.swap_doc(bdoc).unwrap();
    assert_eq!(a.height_at_line(2), 2.0 * th + 30.0);
    a.swap_doc(adoc).unwrap();
    assert_eq!(a.height_at_line(2), 2.0 * th);
}
