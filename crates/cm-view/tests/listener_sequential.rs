//! Listener changes are sequential (P1 F4). In CM5 every `on`/`off` replaces the
//! handler array at once (src/util/event.js:11-34), so changes take effect in the
//! order they are made, whichever way they are made, and a running dispatch keeps
//! the array it started with. `EditorEvents::with_listeners` hands out tables that
//! change the live list on each call, so this holds for changes made through it,
//! through `Editor::on_*`/`off`, `EditorEvents::off`, or a nested `with_listeners`.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;

use cm_core::{Doc, LinkedDocOptions, Pos};
use cm_modes::{Registry, Spec};
use cm_view::events::EditorHandler;
use cm_view::{Editor, EditorOptions, EditorSet, Metrics};

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

/// The independent QA reproduction (2026-10-26-p1-final-qa-5adc052, r1): the same
/// function registered twice; `off(first id)` directly, then `off_fn(f)` through
/// the tables. CM5 (sequential `off`): both entries are gone.
#[test]
fn r1_direct_off_then_copy_off_fn_same_function() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let f = logger(&log, "f");
    let (a, _b) = ev.with_listeners(|ls| (ls.update.on_rc(f.clone()), ls.update.on_rc(f.clone())));
    ev.with_listeners(|ls| {
        assert!(ev.off(a));
        assert!(ls.update.off_fn(&f));
    });
    edit(&mut cm);
    assert!(take(&log).is_empty(), "CM5 removes both registrations");
}

/// The reverse order: `off_fn` first removes the first entry, so a later
/// `off(first id)` finds nothing and says so.
#[test]
fn mixed_id_and_function_removal_in_both_orders() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let f = logger(&log, "f");
    let (a, b) = ev.with_listeners(|ls| (ls.update.on_rc(f.clone()), ls.update.on_rc(f.clone())));
    ev.with_listeners(|ls| {
        assert!(ls.update.off_fn(&f), "removes `a`, the first");
        assert!(!cm.off(a), "already removed");
        assert!(ls.update.contains(b));
    });
    edit(&mut cm);
    assert_eq!(take(&log), ["f"]);
    assert!(ev.off(b));
    assert!(
        !ev.with_listeners(|ls| ls.update.off_fn(&f)),
        "nothing left to remove"
    );
}

/// Registrations made both ways are kept in time order (CM5: a, b, c).
#[test]
fn interleaved_registration_order() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    ev.with_listeners(|ls| {
        ls.update.on_rc(logger(&log, "a"));
        let l = log.clone();
        cm.on_update(move |_| push(&l, "b"));
        ls.update.on_rc(logger(&log, "c"));
    });
    edit(&mut cm);
    assert_eq!(take(&log), ["a", "b", "c"]);
}

/// Three registrations of one function, made three ways; each `off_fn` removes the
/// first remaining one, whichever way it was registered.
#[test]
fn duplicate_function_identities() {
    let log: Log = Rc::default();
    let cm = editor();
    let ev = cm.events();
    let f = logger(&log, "f");
    let a = ev.with_listeners(|ls| ls.update.on_rc(f.clone()));
    let b = ev.with_listeners(|outer| {
        let b = ev.with_listeners(|inner| inner.update.on_rc(f.clone()));
        assert_eq!(
            outer.update.len(),
            2,
            "the outer tables see the inner change"
        );
        b
    });
    let c = ev.with_listeners(|ls| ls.update.on_rc(f.clone()));
    let order: Vec<_> = ev.with_listeners(|ls| ls.update.snapshot().iter().map(|e| e.id).collect());
    assert_eq!(order, [a, b, c]);
    for left in [vec![b, c], vec![c], vec![]] {
        assert!(ev.with_listeners(|ls| ls.update.off_fn(&f)));
        let ids: Vec<_> =
            ev.with_listeners(|ls| ls.update.snapshot().iter().map(|e| e.id).collect());
        assert_eq!(ids, left);
    }
}

/// Nested tables: the outer `off_fn` sees the inner removal and removes the next
/// entry; the outer addition after the inner one comes after it.
#[test]
fn nested_with_listeners_calls() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let f = logger(&log, "f");
    ev.with_listeners(|ls| {
        ls.update.on_rc(f.clone());
        ls.update.on_rc(f.clone());
    });
    ev.with_listeners(|outer| {
        ev.with_listeners(|inner| {
            assert!(inner.update.off_fn(&f));
            inner.update.on_rc(logger(&log, "inner"));
        });
        assert!(outer.update.off_fn(&f), "the second registration");
        assert!(!outer.update.off_fn(&f), "none left");
        outer.update.on_rc(logger(&log, "outer"));
    });
    edit(&mut cm);
    assert_eq!(take(&log), ["inner", "outer"]);
}

/// During a dispatch: a handler adds one and removes another (through the tables
/// and directly). The running dispatch keeps its snapshot; the next one sees both
/// changes.
#[test]
fn registration_and_removal_during_dispatch() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let second = logger(&log, "second");
    let third = logger(&log, "third");
    {
        let (l, ev, second, third) = (log.clone(), ev.clone(), second.clone(), third.clone());
        cm.on_update(move |cx| {
            push(&l, "first");
            ev.with_listeners(|ls| {
                if ls.update.off_fn(&second) {
                    ls.update.on_rc(third.clone());
                }
            });
            let l2 = l.clone();
            cx.on_update(move |_| push(&l2, "direct"));
        });
    }
    ev.with_listeners(|ls| ls.update.on_rc(second));
    edit(&mut cm);
    assert_eq!(take(&log), ["first", "second"]);
    edit(&mut cm);
    assert_eq!(take(&log), ["first", "third", "direct"]);
}

/// A `change` callback queued before its handler is removed still runs once
/// (CM5 `signalLater` captures the handlers); the next change does not call it.
#[test]
fn removal_after_deferred_queueing() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let l = log.clone();
    let f: Rc<cm_view::events::ChangeHandler> = Rc::new(move |_, _| push(&l, "change"));
    ev.with_listeners(|ls| ls.change.on_rc(f.clone()));
    cm.run_op(|cm| {
        cm.doc.replace_range("x", Pos::new(0, 0), None, None);
        assert!(cm.events().with_listeners(|ls| ls.change.off_fn(&f)));
    });
    assert_eq!(take(&log), ["change"]);
    edit(&mut cm);
    assert!(take(&log).is_empty());
}

/// Calls `f` when dropped.
struct OnDrop(Option<Box<dyn FnOnce()>>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

/// The destructor of a handler removed through the tables registers another
/// handler, through the tables and directly: no borrow is held, and both
/// registrations take effect.
#[test]
fn destructor_triggered_registration() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let guard = {
        let (ev, l) = (ev.clone(), log.clone());
        OnDrop(Some(Box::new(move || {
            let l2 = l.clone();
            ev.with_listeners(|ls| ls.update.on_rc(Rc::new(move |_| push(&l2, "from drop"))));
            let l3 = l.clone();
            ev.with_listeners(|ls| {
                let id = ls
                    .update
                    .on_rc(Rc::new(move |_| push(&l3, "second from drop")));
                assert!(ls.update.contains(id));
            });
        })))
    };
    let f: Rc<EditorHandler> = Rc::new(move |_| {
        let _ = &guard;
    });
    ev.with_listeners(|ls| ls.update.on_rc(f.clone()));
    // The tables hold the last reference after this.
    let weak = Rc::downgrade(&f);
    ev.with_listeners(|ls| {
        assert!(ls.update.off_fn(&f));
        drop(f);
        assert!(
            weak.upgrade().is_none(),
            "dropped inside the closure, borrow released"
        );
        assert_eq!(
            ls.update.len(),
            2,
            "the destructor's registrations are visible"
        );
    });
    edit(&mut cm);
    assert_eq!(take(&log), ["from drop", "second from drop"]);
}

/// Cross-editor: A's `update` handler changes B's table. B's finish phase reads its
/// handlers then (CM5 `DisplayUpdate.finish`).
#[test]
fn cross_editor_listener_changes() {
    let d1 = Doc::new("a\nb\nc");
    let d2 = d1
        .linked_doc(LinkedDocOptions {
            shared_hist: false,
            from: None,
            to: None,
            mode: None,
        })
        .unwrap();
    let ed = |d| {
        Editor::try_with_doc(
            d,
            Spec::Null,
            EditorOptions::default(),
            Metrics::oracle(),
            Rc::new(Registry::with_all_modes()),
        )
        .unwrap()
    };
    let mut set = EditorSet::new();
    let a = set.insert(ed(d1));
    let b = set.insert(ed(d2));
    let log: Log = Rc::default();
    let b_old = logger(&log, "B:old");
    let bev = set.get(b).unwrap().events();
    bev.with_listeners(|ls| ls.update.on_rc(b_old.clone()));
    {
        let (l, bev, b_old) = (log.clone(), bev.clone(), b_old.clone());
        set.get(a).unwrap().on_update(move |_| {
            push(&l, "A:update");
            bev.with_listeners(|ls| {
                if ls.update.off_fn(&b_old) {
                    let l2 = l.clone();
                    ls.update.on_rc(Rc::new(move |_| push(&l2, "B:new")));
                }
            });
        });
    }
    set.run(a, |cx| {
        cx.doc.replace_range("x", Pos::new(0, 0), None, None)
    });
    assert_eq!(take(&log), ["A:update", "B:new"]);
    set.run(a, |cx| {
        cx.doc.replace_range("y", Pos::new(0, 0), None, None)
    });
    assert_eq!(take(&log), ["A:update", "B:new"]);
}

/// Conflicting additions and removals of one function, made both ways: the result
/// is what the calls give one after the other.
#[test]
fn conflicting_additions_and_removals() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let f = logger(&log, "f");
    let g = logger(&log, "g");
    ev.with_listeners(|ls| {
        let a = ls.update.on_rc(f.clone());
        ls.update.on_rc(g.clone());
        assert!(cm.off(a));
        assert!(!ls.update.off_fn(&f), "the direct off came first");
        let c = ls.update.on_rc(f.clone());
        assert!(ev.off(c));
        ls.update.on_rc(f.clone());
    });
    edit(&mut cm);
    assert_eq!(take(&log), ["g", "f"]);
}

/// Reentrant execution: a handler removes itself through the tables and runs an
/// operation that dispatches again; the inner dispatch already misses it.
#[test]
fn reentrant_callback_execution() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let me: Rc<RefCell<Option<Rc<cm_view::events::ChangeHandler>>>> = Rc::default();
    let f: Rc<cm_view::events::ChangeHandler> = {
        let (l, ev, me) = (log.clone(), ev.clone(), me.clone());
        Rc::new(move |cx, c| {
            push(&l, format!("self {}", c.text[0]));
            let f = me.borrow_mut().take();
            if let Some(f) = f {
                assert!(ev.with_listeners(|ls| ls.change.off_fn(&f)));
                cx.run_op(|cm, _| cm.doc.replace_range("y", Pos::new(1, 0), None, None));
            }
        })
    };
    *me.borrow_mut() = Some(f.clone());
    ev.with_listeners(|ls| ls.change.on_rc(f.clone()));
    let l = log.clone();
    cm.on_change(move |_, c| push(&l, format!("other {}", c.text[0])));
    drop(f);
    edit(&mut cm);
    // The nested change joins the open group: its callbacks follow those queued.
    assert_eq!(take(&log), ["self x", "other x", "other y"]);
    edit(&mut cm);
    assert_eq!(take(&log), ["other x"]);
}

/// A panic in the closure keeps the changes made before it (JS has no rollback);
/// the tables stay usable.
#[test]
fn closure_panic_keeps_earlier_changes() {
    let log: Log = Rc::default();
    let mut cm = editor();
    let ev = cm.events();
    let r = catch_unwind(AssertUnwindSafe(|| {
        ev.with_listeners(|ls| {
            ls.update.on_rc(logger(&log, "tables"));
            let l = log.clone();
            cm.on_update(move |_| push(&l, "direct"));
            panic_any("wl");
        })
    }));
    assert_eq!(
        r.err().and_then(|e| e.downcast_ref::<&str>().copied()),
        Some("wl")
    );
    edit(&mut cm);
    assert_eq!(take(&log), ["tables", "direct"]);
}
