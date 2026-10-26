//! The editor side of the event system: what the document reaches as `doc.cm`
//! ([`EditorShared`], an [`EditorSink`]), the operation record (`cm.curOp`), and the
//! editor-access adapters used when an operation group ends.
//!
//! See [`cm_core::operation_group`] for the ownership model and the order contract.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cm_core::operation_group::{EdCall, EditorId, EditorSink, Editors, diagnostic};
use cm_core::{ChangeObj, Doc, EditorSemantics};

use crate::editor::{Editor, HeightCache};
use crate::highlight::{self, HighlightState};

/// `cm.curOp`: what one operation collected for its end.
pub(crate) struct OpRecord {
    pub id: u64,
}

/// Work for the editor that could not be delivered when it was due (the editor
/// was not reachable), delivered at its next entry.
pub(crate) enum Parked {
    Call(EdCall),
    End(Box<dyn Any>),
}

/// The part of an editor its document can reach.
pub(crate) struct EditorShared {
    pub id: EditorId,
    pub sem: Cell<EditorSemantics>,
    pub op: RefCell<Option<OpRecord>>,
    pub parked: RefCell<Vec<Parked>>,
    /// Set when the editor is dropped: later work for it is discarded.
    pub destroyed: Cell<bool>,
    pub hs: Rc<RefCell<HighlightState>>,
    pub heights: Rc<RefCell<HeightCache>>,
}

impl EditorShared {
    fn park(&self, item: Parked) {
        if self.destroyed.get() {
            diagnostic(format!(
                "discarded work for editor {}, which was dropped",
                self.id.get()
            ));
            return;
        }
        self.parked.borrow_mut().push(item);
    }
}

/// The editor behind a `dyn Any` handed out by [`Editors`].
pub(crate) fn editor_of(ed: &mut dyn Any) -> &mut Editor {
    ed.downcast_mut::<Editor>()
        .expect("editor calls receive a cm_view::Editor")
}

impl EditorSink for EditorShared {
    fn id(&self) -> EditorId {
        self.id
    }
    fn semantics(&self) -> EditorSemantics {
        self.sem.get()
    }
    fn op_id(&self) -> Option<u64> {
        self.op.borrow().as_ref().map(|o| o.id)
    }
    fn open_op(&self, id: u64) {
        *self.op.borrow_mut() = Some(OpRecord { id });
    }
    fn close_op(&self) -> Box<dyn Any> {
        Box::new(self.op.borrow_mut().take().expect("an open operation"))
    }
    fn end_display(&self, rec: Box<dyn Any>, access: &mut dyn Editors) -> Option<Box<dyn Any>> {
        let reached = access.with_editor(self.id, &mut |ed, _| {
            editor_of(ed).end_operation_display();
        });
        if reached {
            Some(rec)
        } else {
            self.park(Parked::End(rec));
            None
        }
    }
    fn park_call(&self, call: EdCall) {
        self.park(Parked::Call(call));
    }
    fn change_applied(&self, doc: &Doc, c: &Rc<ChangeObj>) {
        // makeChangeSingleDocInEditor: `retreatFrontier(doc, from.line)`.
        highlight::retreat_frontier(doc, &mut self.hs.borrow_mut(), c.from.line);
        let mut h = self.heights.borrow_mut();
        if !h.heights.is_empty() {
            let first = doc.first_line();
            let a = (c.from.line - first).max(0) as usize;
            let b = ((c.to.line - first + 1).max(0) as usize).min(h.heights.len());
            if a <= b {
                h.heights
                    .splice(a..b, std::iter::repeat_n(f64::NAN, c.text.len()));
            } else {
                h.heights.clear();
            }
        }
        h.prefix = None;
        h.max_width = None;
    }
}

/// Access to `me` and to the editors `rest` reaches. While `me` is handed out, it
/// is not reachable through the `rest` it is handed out with.
pub(crate) struct WithSelf<'a> {
    pub me: &'a mut Editor,
    pub rest: &'a mut dyn Editors,
}

impl Editors for WithSelf<'_> {
    fn with_editor(
        &mut self,
        id: EditorId,
        f: &mut dyn FnMut(&mut dyn Any, &mut dyn Editors),
    ) -> bool {
        if id == self.me.id {
            f(&mut *self.me, &mut *self.rest);
            true
        } else {
            self.rest.with_editor(id, f)
        }
    }
}
