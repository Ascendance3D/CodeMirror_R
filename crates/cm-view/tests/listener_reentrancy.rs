//! Listener tables used again while they are being changed: from the closure of
//! `EditorEvents::with_listeners`, and from the destructor of a handler that `off`
//! drops. In CM5 `on`/`off` just build a new array (src/util/event.js:11-34), so
//! neither can fail; here no table is borrowed while user code runs, and changes
//! made both ways are kept.

use std::cell::RefCell;
use std::rc::Rc;

use cm_core::{Doc, Pos};
use cm_modes::Spec;
use cm_view::events::{EditorEvents, EditorHandler};
use cm_view::{Editor, EditorOptions, Metrics};

type Log = Rc<RefCell<Vec<String>>>;

fn push(l: &Log, s: impl Into<String>) {
    l.borrow_mut().push(s.into());
}

fn take(l: &Log) -> Vec<String> {
    std::mem::take(&mut *l.borrow_mut())
}

fn editor() -> Editor {
    Editor::new(
        "a\nb\nc",
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
    )
}

fn edit(cm: &mut Editor) {
    cm.run_op(|cm| cm.doc.replace_range("x", Pos::new(0, 0), None, None));
}

fn logger(log: &Log, name: &'static str) -> Rc<EditorHandler> {
    let l = log.clone();
    Rc::new(move |_| push(&l, name))
}

/// The closure removes a handler through `EditorEvents::off` and registers one
/// through `Editor::on_update` while it changes the table itself.
#[test]
fn with_listeners_closure_may_use_the_editor_api() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let gone = cm.on_update({
        let l = log.clone();
        move |_| push(&l, "gone")
    });
    let kept = logger(&log, "kept");
    ev.with_listeners(|ls| {
        ls.update.on_rc(kept.clone());
        assert!(ev.off(gone), "removed the other way");
        cm.on_update({
            let l = log.clone();
            move |_| push(&l, "added the other way")
        });
    });
    edit(&mut cm);
    assert_eq!(take(&log), ["kept", "added the other way"]);
    // Both kinds of change persist: removing `kept` by function identity now
    // leaves only the handler added the other way.
    assert!(ev.with_listeners(|ls| ls.update.off_fn(&kept)));
    edit(&mut cm);
    assert_eq!(take(&log), ["added the other way"]);
}

/// A nested `with_listeners` on the same editor.
#[test]
fn with_listeners_nests() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let (a, b) = (logger(&log, "a"), logger(&log, "b"));
    ev.with_listeners(|ls| {
        ls.update.on_rc(a.clone());
        ev.with_listeners(|inner| inner.update.on_rc(b.clone()));
    });
    edit(&mut cm);
    assert_eq!(take(&log), ["a", "b"]);
}

/// Inside a dispatch: a handler removes itself and adds another through
/// `with_listeners` and `off`; the running dispatch keeps its snapshot, the next
/// one sees the change.
#[test]
fn self_removal_and_registration_during_dispatch() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let later = logger(&log, "later");
    let me: Rc<RefCell<Option<cm_core::ListenerId>>> = Rc::default();
    let id = {
        let (l, ev, later, me) = (log.clone(), ev.clone(), later.clone(), me.clone());
        cm.on_update(move |_| {
            push(&l, "self");
            ev.with_listeners(|ls| {
                ls.update.on_rc(later.clone());
                assert!(ev.off(me.borrow().unwrap()));
            });
        })
    };
    *me.borrow_mut() = Some(id);
    let after = logger(&log, "after");
    ev.with_listeners(|ls| ls.update.on_rc(after));
    edit(&mut cm);
    assert_eq!(take(&log), ["self", "after"]);
    edit(&mut cm);
    assert_eq!(take(&log), ["after", "later"]);
}

/// Calls `off(id)` on its emitter when dropped.
struct OffOnDrop(Box<dyn Fn()>);
impl Drop for OffOnDrop {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// `off` drops the last reference to a handler whose destructor uses the same
/// emitter: editor, editor through `EditorEvents`, document.
#[test]
fn a_dropped_handler_may_use_its_emitter() {
    let mut cm = editor();
    let ev: EditorEvents = cm.events();
    let other = cm.on_update(|_| {});
    let guard = {
        let ev = ev.clone();
        OffOnDrop(Box::new(move || {
            ev.off(other);
        }))
    };
    let id = cm.on_update(move |_| {
        let _ = &guard;
    });
    assert!(cm.off(id));
    assert!(!ev.off(other), "the destructor removed it");

    let guard = {
        let ev2 = ev.clone();
        OffOnDrop(Box::new(move || {
            ev2.with_listeners(|ls| ls.update.len());
        }))
    };
    let id = cm.on_change(move |_, _| {
        let _ = &guard;
    });
    assert!(ev.off(id));

    let doc = Doc::new("a");
    let other = doc.on_history_added(|_| {});
    let guard = {
        let d = doc.clone();
        OffOnDrop(Box::new(move || {
            d.off(other);
        }))
    };
    let id = doc.on_change(move |_, _| {
        let _ = &guard;
    });
    assert!(doc.off(id));
    assert!(!doc.off(other), "the destructor removed it");

    let line = doc.get_line_handle(0).unwrap();
    let guard = {
        let h = line.clone();
        OffOnDrop(Box::new(move || {
            h.on_delete(|| {});
        }))
    };
    let id = line.on_change(move |_, _| {
        let _ = &guard;
    });
    assert!(line.off(id));
    edit(&mut cm);
}
