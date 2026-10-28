//! `addLineClass`/`removeLineClass` with an editor: a changed line of the rendered range
//! is redrawn (`regLineChange`), so `update` fires; a no-op or a line outside the range
//! redraws nothing (src/model/changes.js:332). Several calls in one operation share one
//! update (test/test.js `combinedOperations`).

use std::cell::Cell;
use std::rc::Rc;

use cm_core::LineWhere;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};

/// Direct document calls park the editor's part of the work until its next entry
/// (the host's `App::deliver_pending`).
fn deliver(cm: &mut Editor) {
    cm.fire_orphan_delayed();
}

fn editor() -> (Editor, Rc<Cell<u32>>) {
    let text = (0..300)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut cm = Editor::new(
        &text,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
    );
    cm.set_size(600.0, 300.0);
    let updates = Rc::new(Cell::new(0));
    let u = updates.clone();
    cm.on_update(move |_| u.set(u.get() + 1));
    (cm, updates)
}

#[test]
fn class_change_on_a_rendered_line_updates() {
    let (mut cm, updates) = editor();
    cm.doc.add_line_class(2, LineWhere::Wrap, "foo");
    deliver(&mut cm);
    assert_eq!(updates.get(), 1);
    cm.doc.remove_line_class(2, LineWhere::Wrap, Some("foo"));
    deliver(&mut cm);
    assert_eq!(updates.get(), 2);
}

#[test]
fn no_op_and_offscreen_changes_do_not_update() {
    let (mut cm, updates) = editor();
    cm.doc.add_line_class(2, LineWhere::Text, "foo");
    deliver(&mut cm);
    let n = updates.get();
    assert_eq!(n, 1);
    cm.doc.add_line_class(2, LineWhere::Text, "foo");
    cm.doc.remove_line_class(2, LineWhere::Text, Some("absent"));
    deliver(&mut cm);
    assert_eq!(updates.get(), n, "no change, no redraw");
    // Line 250 is far below the rendered range: the class is stored, nothing redraws.
    cm.doc.add_line_class(250, LineWhere::Text, "far");
    deliver(&mut cm);
    assert_eq!(updates.get(), n);
    assert_eq!(
        cm.doc.get_line_handle(250).unwrap().text_class().as_deref(),
        Some("far")
    );
}

#[test]
fn several_calls_in_one_operation_update_once() {
    let (mut cm, updates) = editor();
    cm.run_op(|cm| {
        cm.doc.add_line_class(0, LineWhere::Wrap, "foo");
        cm.doc.add_line_class(1, LineWhere::Background, "bar");
        cm.doc.add_line_class(2, LineWhere::Text, "baz");
    });
    deliver(&mut cm);
    assert_eq!(updates.get(), 1);
}
