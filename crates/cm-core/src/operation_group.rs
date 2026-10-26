//! `src/util/operation_group.js` and the operation bookkeeping of
//! `src/display/operations.js`: operation groups, delayed (`signalLater`) callbacks,
//! the orphan queue, and the interface a document uses to reach its editor
//! ([`EditorSink`], what `doc.cm` is in the JS).
//!
//! ## Ownership
//! An editor is an owned value used through `&mut`. A document holds only an
//! `Rc<dyn EditorSink>`: the editor's listener tables and its current operation
//! (`cm.curOp`), never the editor itself. Editor-targeted work is queued as data
//! ([`Delayed::Editor`]: the handler snapshot and payload captured when it was
//! queued) and delivered by whoever ends the outermost operation, through
//! [`Editors`], which hands out `&mut` access to the editors it can reach. Work for
//! an editor that cannot be reached (it is borrowed further up the stack, or the
//! caller did not pass it) is parked on that editor ([`EditorSink::park_call`]) and
//! delivered at its next entry.
//!
//! ## Order (the JS contract)
//! * One group per outermost operation; operations of other editors started while
//!   it is open join it (`pushOperation`). The group has one delayed queue, in the
//!   order the callbacks were queued, across editors.
//! * At the end of the outermost operation ([`end_group`]): the delayed queue, then
//!   each operation's collected `cursorActivity` handlers, repeated while handlers
//!   queue more (`fireCallbacksForOps`); then every operation is closed (`curOp =
//!   null`), then the display phase of every operation and the finish phase of
//!   every operation, in the order they joined (`endOperations`).
//! * A panic in a delayed callback drops the rest of the queue, still closes and
//!   finishes every operation, then continues (`try { … } finally { … }`, kept with
//!   `catch_unwind`/`resume_unwind`). This needs `panic = "unwind"`, the default; with
//!   `panic = "abort"` the process ends at the panic and nothing is finished.
//! * A callback queued with no group open goes to the orphan queue
//!   (`orphanDelayedCallbacks`). It is not delivered by other operations; the host
//!   delivers it with [`fire_orphan_delayed`], after a waker
//!   ([`add_orphan_waker`]) told it the queue became non-empty (`setTimeout(…, 0)`).
//!
//! Everything here is per thread (thread-local state; `Rc` handles are not `Send`).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

use crate::model::changes::{BeforeChange, BeforeSelectionChange, ChangeObj};
use crate::model::doc::{Doc, EditorSemantics};
use crate::model::mark_text::TextMarker;

/// The id of an editor, unique on the thread and never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditorId(u32);

thread_local! {
    static NEXT_EDITOR: Cell<u32> = const { Cell::new(1) };
    static NEXT_OP: Cell<u64> = const { Cell::new(1) };
}

impl EditorId {
    pub fn next() -> EditorId {
        NEXT_EDITOR.with(|n| {
            let v = n.get();
            n.set(v + 1);
            EditorId(v)
        })
    }
    pub fn get(self) -> u32 {
        self.0
    }
}

/// An editor-targeted call: receives the editor (as `dyn Any`; the view crate
/// downcasts it to its editor type) and access to the other reachable editors.
pub type EdCall = Box<dyn FnOnce(&mut dyn Any, &mut dyn Editors)>;

/// `&mut` access to editors during delivery.
pub trait Editors {
    /// Run `f` with editor `id` and access to the rest. `false` when `id` cannot
    /// be reached: unknown here, or already borrowed further up the stack.
    fn with_editor(
        &mut self,
        id: EditorId,
        f: &mut dyn FnMut(&mut dyn Any, &mut dyn Editors),
    ) -> bool;
}

/// No editor is reachable.
pub struct NoEditors;

impl Editors for NoEditors {
    fn with_editor(
        &mut self,
        _: EditorId,
        _: &mut dyn FnMut(&mut dyn Any, &mut dyn Editors),
    ) -> bool {
        false
    }
}

/// One queued `signalLater` callback, with its handler and payload captured.
pub enum Delayed {
    /// Document, line and marker handlers: no editor needed.
    Free(Box<dyn FnOnce()>),
    /// Needs editor `target`. When it cannot be reached it is parked on `sink`, or
    /// discarded with a diagnostic when the editor is gone.
    Editor {
        target: EditorId,
        sink: Weak<dyn EditorSink>,
        call: EdCall,
    },
}

/// An editor's `beforeChange` handler bound to its editor.
pub type BeforeChangeFn = dyn Fn(&Doc, &mut BeforeChange);
/// An editor's `beforeSelectionChange` handler bound to its editor.
pub type BeforeSelectionChangeFn = dyn Fn(&Doc, &mut BeforeSelectionChange);

/// What a document sees of its editor (`doc.cm`): the editor's operation state and
/// listener tables. Every method is called without any document borrow held, and
/// implementations do not hold their own borrows across a call into user code.
pub trait EditorSink {
    fn id(&self) -> EditorId;
    /// The model-level options of the editor.
    fn semantics(&self) -> EditorSemantics;
    /// `cm.curOp.id`, `None` when no operation is open.
    fn op_id(&self) -> Option<u64>;
    /// `startOperation`: open an operation record with this id.
    fn open_op(&self, id: u64);
    /// `cm.curOp = null`: close the operation and return its record, which goes to
    /// [`EditorSink::end_display`] and [`EditorSink::end_finish`].
    fn close_op(&self) -> Box<dyn Any>;
    /// The next collected `cursorActivity` handler of the open operation, as a call
    /// (`fireCallbacksForOps`); `None` when all of them were called.
    fn next_cursor_activity(&self) -> Option<EdCall> {
        None
    }
    /// The display phases (`endOperation_R1` … `W2`). `None` when the editor could
    /// not be reached and the record was parked; it is then finished at the editor's
    /// next entry.
    fn end_display(&self, rec: Box<dyn Any>, access: &mut dyn Editors) -> Option<Box<dyn Any>>;
    /// `endOperation_finish`: the events fired after the group is closed.
    fn end_finish(&self, _rec: Box<dyn Any>, _access: &mut dyn Editors) {}
    /// Keep `call` until the editor's next entry.
    fn park_call(&self, call: EdCall);

    /// The editor's `beforeChange` handlers as of now (`signal(doc.cm, …)` runs them
    /// after the document's own handlers).
    fn before_change_handlers(&self) -> Vec<Rc<BeforeChangeFn>> {
        Vec::new()
    }
    /// `hasHandler(doc.cm, "beforeChange")`.
    fn has_before_change(&self) -> bool {
        !self.before_change_handlers().is_empty()
    }
    /// The editor's `beforeSelectionChange` handlers as of now.
    fn before_selection_change_handlers(&self) -> Vec<Rc<BeforeSelectionChangeFn>> {
        Vec::new()
    }
    /// `hasHandler(doc.cm, "beforeSelectionChange")`.
    fn has_before_selection_change(&self) -> bool {
        !self.before_selection_change_handlers().is_empty()
    }
    /// `makeChangeSingleDocInEditor`, after the change is applied to the lines.
    fn change_applied(&self, _doc: &Doc, _change: &Rc<ChangeObj>) {}
    /// `signalCursorActivity(cm)`.
    fn signal_cursor_activity(&self) {}
    /// `setSelectionInner` on a document with this editor: the selection changed.
    fn selection_changed(&self) {
        self.signal_cursor_activity();
    }
    /// `markText` on a document with this editor (`markerAdded`, redraw).
    fn marker_added(&self, _doc: &Doc, _marker: &TextMarker) {}
    /// `detachLine` emptied the marker's lines (`op.maybeHiddenMarkers`).
    fn marker_maybe_hidden(&self, _marker: &TextMarker) {}
    /// `attachLine` gave an empty marker a line (`op.maybeUnhiddenMarkers`).
    fn marker_maybe_unhidden(&self, _marker: &TextMarker) {}
    /// `TextMarker.clear` on a document with this editor: `markerCleared(min, max)`
    /// (both `None` for a marker that is not collapsed); `redraw` when lines of the
    /// marker must be redrawn.
    fn marker_cleared(
        &self,
        _doc: &Doc,
        _marker: &TextMarker,
        _min: Option<i64>,
        _max: Option<i64>,
        _redraw: bool,
    ) {
    }
}

struct Group {
    ops: Vec<Rc<dyn EditorSink>>,
    delayed: VecDeque<Delayed>,
}

type Waker = Rc<dyn Fn()>;

thread_local! {
    static GROUP: RefCell<Option<Group>> = const { RefCell::new(None) };
    static ORPHANS: RefCell<Option<VecDeque<Delayed>>> = const { RefCell::new(None) };
    static WAKERS: RefCell<Vec<(u64, Waker)>> = const { RefCell::new(Vec::new()) };
    static NEXT_WAKER: Cell<u64> = const { Cell::new(1) };
    static DIAGNOSTICS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Whether an operation group is open.
pub fn group_open() -> bool {
    GROUP.with(|g| g.borrow().is_some())
}

/// `startOperation(cm)` when `cm.curOp` is null, then `pushOperation`. `None` when
/// the editor already has an open operation; otherwise whether this operation opened
/// (owns) the group, and so must end it with [`end_group`].
pub fn start_operation(sink: &Rc<dyn EditorSink>) -> Option<bool> {
    if sink.op_id().is_some() {
        return None;
    }
    let id = NEXT_OP.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    sink.open_op(id);
    Some(GROUP.with(|g| {
        let mut g = g.borrow_mut();
        match g.as_mut() {
            Some(gr) => {
                gr.ops.push(sink.clone());
                false
            }
            None => {
                *g = Some(Group {
                    ops: vec![sink.clone()],
                    delayed: VecDeque::new(),
                });
                true
            }
        }
    }))
}

/// `signalLater`: queue `d` in the open group, or in the orphan queue.
pub fn signal_later(d: Delayed) {
    let rest = GROUP.with(|g| match g.borrow_mut().as_mut() {
        Some(gr) => {
            gr.delayed.push_back(d);
            None
        }
        None => Some(d),
    });
    let Some(d) = rest else { return };
    let first = ORPHANS.with(|o| {
        let mut o = o.borrow_mut();
        match o.as_mut() {
            Some(q) => {
                q.push_back(d);
                false
            }
            None => {
                *o = Some(VecDeque::from([d]));
                true
            }
        }
    });
    if first {
        // `setTimeout(fireOrphanDelayed, 0)`: every registered host is told.
        let wakers: Vec<Waker> = WAKERS.with(|w| w.borrow().iter().map(|x| x.1.clone()).collect());
        for w in wakers {
            w();
        }
    }
}

/// Register a host callback, called each time the orphan queue goes from empty to
/// non-empty (the host then calls [`fire_orphan_delayed`] from its event loop). Hosts
/// are independent: one host cannot replace or remove another's waker.
pub fn add_orphan_waker(f: impl Fn() + 'static) -> u64 {
    let id = NEXT_WAKER.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    WAKERS.with(|w| w.borrow_mut().push((id, Rc::new(f))));
    id
}

/// Remove the waker registered as `id`.
pub fn remove_orphan_waker(id: u64) -> bool {
    WAKERS.with(|w| {
        let mut w = w.borrow_mut();
        let n = w.len();
        w.retain(|x| x.0 != id);
        w.len() != n
    })
}

/// Whether orphan callbacks are waiting for [`fire_orphan_delayed`].
pub fn orphans_pending() -> bool {
    ORPHANS.with(|o| o.borrow().is_some())
}

/// `fireOrphanDelayed`: deliver the orphan queue in order. Editor-targeted callbacks
/// go to `access`, or are parked on their editor.
pub fn fire_orphan_delayed(access: &mut dyn Editors) {
    let q = ORPHANS.with(|o| o.borrow_mut().take()).unwrap_or_default();
    for d in q {
        deliver(d, access);
    }
}

/// Messages about discarded work (callbacks for an editor that no longer exists),
/// oldest first; taking them clears the list.
pub fn take_diagnostics() -> Vec<String> {
    DIAGNOSTICS.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

/// Record a diagnostic (see [`take_diagnostics`]).
pub fn diagnostic(msg: String) {
    DIAGNOSTICS.with(|d| d.borrow_mut().push(msg));
}

/// Deliver one callback now.
pub fn deliver(d: Delayed, access: &mut dyn Editors) {
    match d {
        Delayed::Free(f) => f(),
        Delayed::Editor { target, sink, call } => {
            let mut call = Some(call);
            let ok = access.with_editor(target, &mut |ed, rest| {
                (call.take().expect("an editor call runs once"))(ed, rest)
            });
            if ok {
                return;
            }
            let call = call.expect("not delivered");
            match sink.upgrade() {
                Some(s) => s.park_call(call),
                None => diagnostic(format!(
                    "discarded a callback for editor {}, which no longer exists",
                    target.0
                )),
            }
        }
    }
}

fn pop_delayed() -> Option<Delayed> {
    GROUP.with(|g| {
        g.borrow_mut()
            .as_mut()
            .expect("an operation group is open")
            .delayed
            .pop_front()
    })
}

/// `fireCallbacksForOps` (operation_group.js:119-133).
fn fire_callbacks_for_ops(access: &mut dyn Editors) {
    loop {
        while let Some(d) = pop_delayed() {
            deliver(d, access);
        }
        // The live length: editors may join the group while handlers run.
        let mut j = 0;
        while let Some(sink) =
            GROUP.with(|g| g.borrow().as_ref().expect("group").ops.get(j).cloned())
        {
            while let Some(call) = sink.next_cursor_activity() {
                deliver(
                    Delayed::Editor {
                        target: sink.id(),
                        sink: Rc::downgrade(&sink),
                        call,
                    },
                    access,
                );
            }
            j += 1;
        }
        if GROUP.with(|g| g.borrow().as_ref().expect("group").delayed.is_empty()) {
            break;
        }
    }
}

/// `finishOperation` with `endOperation`'s callback, for the operation that owns
/// the group: deliver the queued callbacks, close every operation, then run the
/// display and finish phases of every operation. A panic in a callback is resumed
/// after the operations are finished.
pub fn end_group(access: &mut dyn Editors) {
    let r = catch_unwind(AssertUnwindSafe(|| fire_callbacks_for_ops(access)));
    let group = GROUP
        .with(|g| g.borrow_mut().take())
        .expect("end_group without an open group");
    // `group.ops[i].cm.curOp = null` for every operation first.
    let recs: Vec<(Rc<dyn EditorSink>, Box<dyn Any>)> = group
        .ops
        .into_iter()
        .map(|s| {
            let rec = s.close_op();
            (s, rec)
        })
        .collect();
    // `endOperations(group)`: every display phase, then every finish phase.
    let kept: Vec<_> = recs
        .into_iter()
        .filter_map(|(s, rec)| s.end_display(rec, access).map(|rec| (s, rec)))
        .collect();
    for (s, rec) in kept {
        s.end_finish(rec, access);
    }
    if let Err(p) = r {
        resume_unwind(p);
    }
}

/// `runInOp`/`docMethodOp` for a sink: run `f` inside an operation of `sink` (or the
/// one already open), and end the group if this operation opened it. A panic in `f`
/// still ends the operation, then continues.
pub fn run_in_op<R>(
    sink: &Rc<dyn EditorSink>,
    access: &mut dyn Editors,
    f: impl FnOnce(&mut dyn Editors) -> R,
) -> R {
    let started = start_operation(sink);
    let r = catch_unwind(AssertUnwindSafe(|| f(&mut *access)));
    if started == Some(true) {
        end_group(access);
    }
    r.unwrap_or_else(|p| resume_unwind(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }
    fn log(s: impl Into<String>) {
        LOG.with(|l| l.borrow_mut().push(s.into()));
    }
    fn take_log() -> Vec<String> {
        LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
    }

    /// A sink that logs its phases and queues one cursorActivity call per selection
    /// change; `reachable` decides whether its "editor" can be reached.
    struct TestSink {
        id: EditorId,
        name: &'static str,
        op: Cell<Option<u64>>,
        activity: RefCell<Vec<&'static str>>,
        parked: RefCell<Vec<EdCall>>,
    }

    fn sink(name: &'static str) -> Rc<TestSink> {
        Rc::new(TestSink {
            id: EditorId::next(),
            name,
            op: Cell::new(None),
            activity: RefCell::new(Vec::new()),
            parked: RefCell::new(Vec::new()),
        })
    }

    impl EditorSink for TestSink {
        fn id(&self) -> EditorId {
            self.id
        }
        fn semantics(&self) -> EditorSemantics {
            EditorSemantics::default()
        }
        fn op_id(&self) -> Option<u64> {
            self.op.get()
        }
        fn open_op(&self, id: u64) {
            self.op.set(Some(id));
        }
        fn close_op(&self) -> Box<dyn Any> {
            log(format!("{}:close", self.name));
            self.op.set(None);
            // Collected handlers belong to the operation record.
            self.activity.borrow_mut().clear();
            Box::new(self.name)
        }
        fn next_cursor_activity(&self) -> Option<EdCall> {
            let tag = self.activity.borrow_mut().pop()?;
            Some(Box::new(move |_, _| log(tag)))
        }
        fn end_display(&self, rec: Box<dyn Any>, access: &mut dyn Editors) -> Option<Box<dyn Any>> {
            if access.with_editor(self.id, &mut |_, _| {}) {
                log(format!("{}:display", self.name));
                Some(rec)
            } else {
                log(format!("{}:parked end", self.name));
                None
            }
        }
        fn end_finish(&self, _: Box<dyn Any>, _: &mut dyn Editors) {
            log(format!("{}:finish", self.name));
        }
        fn park_call(&self, call: EdCall) {
            log(format!("{}:parked call", self.name));
            self.parked.borrow_mut().push(call);
        }
    }

    /// Reaches the editors with these ids (a stand-in for an editor arena).
    struct Reach(Vec<EditorId>);
    impl Editors for Reach {
        fn with_editor(
            &mut self,
            id: EditorId,
            f: &mut dyn FnMut(&mut dyn Any, &mut dyn Editors),
        ) -> bool {
            if !self.0.contains(&id) {
                return false;
            }
            f(&mut (), &mut NoEditors);
            true
        }
    }

    fn later(s: &'static str) -> Delayed {
        Delayed::Free(Box::new(move || log(s)))
    }

    fn dyn_sink(s: &Rc<TestSink>) -> Rc<dyn EditorSink> {
        s.clone()
    }

    /// Nested operations of one editor: one group, delivered once at the outer end.
    #[test]
    fn nested_operations_deliver_once_at_the_outermost_end() {
        let a = sink("A");
        let da = dyn_sink(&a);
        let mut reach = Reach(vec![a.id]);
        run_in_op(&da, &mut reach, |acc| {
            signal_later(later("d1"));
            run_in_op(&da, acc, |_| {
                assert!(start_operation(&da).is_none(), "the op is already open");
                signal_later(later("d2"));
                log("inner end");
            });
            a.activity.borrow_mut().push("A:activity");
            log("outer end");
        });
        assert_eq!(
            take_log(),
            [
                "inner end",
                "outer end",
                "d1",
                "d2",
                "A:activity",
                "A:close",
                "A:display",
                "A:finish"
            ]
        );
        assert!(!group_open() && a.op.get().is_none());
    }

    /// Another editor's operation joins the open group: one queue across editors,
    /// then cursor activity op by op, then every display before any finish.
    #[test]
    fn joined_operations_share_one_queue_and_end_in_join_order() {
        let (a, b) = (sink("A"), sink("B"));
        let (da, db) = (dyn_sink(&a), dyn_sink(&b));
        let mut reach = Reach(vec![a.id, b.id]);
        run_in_op(&db, &mut reach, |acc| {
            signal_later(later("B:1"));
            run_in_op(&da, acc, |_| {
                signal_later(later("A:1"));
                a.activity.borrow_mut().push("A:activity");
            });
            assert!(
                a.op.get().is_some(),
                "a joined op stays open until the group ends"
            );
            signal_later(later("B:2"));
            b.activity.borrow_mut().push("B:activity");
        });
        assert_eq!(
            take_log(),
            [
                "B:1",
                "A:1",
                "B:2",
                "B:activity",
                "A:activity",
                "B:close",
                "A:close",
                "B:display",
                "A:display",
                "B:finish",
                "A:finish"
            ]
        );
    }

    /// operation_group.js:122-132: callbacks queued by a delivered callback run in
    /// the same drain, before the cursor activity of the next round.
    #[test]
    fn callbacks_queued_during_delivery_run_in_the_same_group() {
        let a = sink("A");
        let da = dyn_sink(&a);
        let a2 = a.clone();
        run_in_op(&da, &mut Reach(vec![a.id]), |_| {
            a.activity.borrow_mut().push("activity 1");
            signal_later(Delayed::Free(Box::new(move || {
                log("d1");
                signal_later(later("d1 queued d2"));
                a2.activity.borrow_mut().push("activity 2");
            })));
        });
        assert_eq!(
            take_log(),
            [
                "d1",
                "d1 queued d2",
                "activity 2",
                "activity 1",
                "A:close",
                "A:display",
                "A:finish"
            ]
        );
    }

    /// s9: a panic drops the rest of the queue, still closes and finishes the
    /// operations, and then continues; afterwards no group or op is open.
    #[test]
    fn panic_in_a_callback_finishes_the_group_then_continues() {
        let a = sink("A");
        let da = dyn_sink(&a);
        let r = catch_unwind(AssertUnwindSafe(|| {
            run_in_op(&da, &mut Reach(vec![a.id]), |_| {
                signal_later(Delayed::Free(Box::new(|| panic!("boom"))));
                signal_later(later("dropped"));
                a.activity.borrow_mut().push("dropped activity");
            })
        }));
        assert!(r.is_err());
        assert_eq!(take_log(), ["A:close", "A:display", "A:finish"]);
        assert!(!group_open() && a.op.get().is_none());
        // The next operation starts a fresh group.
        run_in_op(&da, &mut Reach(vec![a.id]), |_| signal_later(later("next")));
        assert_eq!(take_log(), ["next", "A:close", "A:display", "A:finish"]);
    }

    /// A panic in the operation body still ends the operation (`finally`).
    #[test]
    fn panic_in_the_body_ends_the_operation() {
        let a = sink("A");
        let da = dyn_sink(&a);
        let r = catch_unwind(AssertUnwindSafe(|| {
            run_in_op(&da, &mut Reach(vec![a.id]), |_| {
                signal_later(later("queued before the panic"));
                panic!("body");
            })
        }));
        assert!(r.is_err());
        assert_eq!(
            take_log(),
            [
                "queued before the panic",
                "A:close",
                "A:display",
                "A:finish"
            ]
        );
        assert!(!group_open());
    }

    /// An unreachable editor gets its calls and its operation end parked; a dropped
    /// editor's calls are discarded with a diagnostic.
    #[test]
    fn unreachable_editors_park_and_dropped_editors_discard() {
        let a = sink("A");
        let da = dyn_sink(&a);
        run_in_op(&da, &mut NoEditors, |_| {
            signal_later(Delayed::Editor {
                target: a.id,
                sink: Rc::downgrade(&da),
                call: Box::new(|_, _| log("A:call")),
            });
        });
        assert_eq!(take_log(), ["A:parked call", "A:close", "A:parked end"]);
        let parked = std::mem::take(&mut *a.parked.borrow_mut());
        for c in parked {
            c(&mut (), &mut NoEditors);
        }
        assert_eq!(take_log(), ["A:call"]);

        let gone = sink("gone");
        let (gid, weak) = (gone.id, Rc::downgrade(&dyn_sink(&gone)));
        drop(gone);
        take_diagnostics();
        deliver(
            Delayed::Editor {
                target: gid,
                sink: weak,
                call: Box::new(|_, _| log("never")),
            },
            &mut NoEditors,
        );
        assert!(take_log().is_empty());
        let d = take_diagnostics();
        assert_eq!(d.len(), 1);
        assert!(d[0].contains("no longer exists"), "{d:?}");
    }

    /// s7: with no group open, callbacks go to the orphan queue; the first one
    /// wakes every host once; an operation does not drain them; the host does.
    #[test]
    fn orphans_wait_for_the_host() {
        let woken = Rc::new(Cell::new(0));
        let (w1, w2) = (woken.clone(), woken.clone());
        let k1 = add_orphan_waker(move || w1.set(w1.get() + 1));
        let k2 = add_orphan_waker(move || w2.set(w2.get() + 10));
        signal_later(later("orphan 1"));
        signal_later(later("orphan 2"));
        assert_eq!(
            woken.get(),
            11,
            "each host woken once, for the first orphan"
        );
        let a = sink("A");
        run_in_op(&dyn_sink(&a), &mut Reach(vec![a.id]), |_| {
            signal_later(later("in op"))
        });
        assert_eq!(take_log(), ["in op", "A:close", "A:display", "A:finish"]);
        assert!(orphans_pending());
        fire_orphan_delayed(&mut NoEditors);
        assert_eq!(take_log(), ["orphan 1", "orphan 2"]);
        assert!(!orphans_pending());
        assert!(remove_orphan_waker(k1));
        signal_later(later("orphan 3"));
        assert_eq!(
            woken.get(),
            21,
            "a removed waker is not called; the other still is"
        );
        assert!(remove_orphan_waker(k2) && !remove_orphan_waker(k2));
        fire_orphan_delayed(&mut NoEditors);
        take_log();
    }
}
