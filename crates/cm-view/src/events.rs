//! Editor events (`src/util/event.js` on a `CodeMirror` instance): the listener
//! tables, the handler context [`Cx`], what the document reaches as `doc.cm`
//! ([`EditorShared`], an [`EditorSink`]), and the operation record (`cm.curOp`) with
//! its display and finish phases.
//!
//! See [`cm_core::operation_group`] for the ownership model and the order contract.
//!
//! ## Handlers
//! * Events fired by the editor (`change`, `changes`, `cursorActivity`, `update`,
//!   `viewportChange`, `markerAdded`, `markerCleared`, `keyHandled`, …) get a
//!   [`Cx`]: the editor (`&mut`, through `Deref`) and the other editors the caller
//!   could reach ([`Cx::others`]). Editor methods called from a handler join the
//!   operation that is still open, or start a new one that the handler's call
//!   completes, as in the JS.
//! * `beforeChange` and `beforeSelectionChange` fire inside the document model,
//!   where no `&mut Editor` exists: they get the document and the editor's
//!   [`EditorId`] (decision D3). The payload is mutable (`update`, `cancel`), as in
//!   the JS; every other payload is immutable (decision D8).
//! * Handlers are registered with `on_*` and removed with [`Editor::off`] by id, or
//!   by function identity through [`EditorEvents::with_listeners`] (`off_fn`).
//!   Every change applies to the live table when it is made, in call order, as
//!   CM5's `on`/`off` do; a dispatch that is running keeps its snapshot.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};

use cm_core::events::{Entry, ListenerId, Listeners, Removed, removed};
use cm_core::operation_group::{
    Batch, BeforeChangeFn, BeforeSelectionChangeFn, Delayed, EdCall, EditorId, EditorSink, Editors,
    diagnostic, signal_later,
};
use cm_core::{
    BeforeChange, BeforeSelectionChange, ChangeObj, Doc, EditorSemantics, MarkerFind, TextMarker,
};

use crate::editor::{Editor, HeightCache};
use crate::highlight::{self, HighlightState};

/// The context of an editor event handler: the editor, and the other editors the
/// code that ended the operation could reach.
pub struct Cx<'a> {
    pub editor: &'a mut Editor,
    pub others: &'a mut dyn Editors,
}

impl Deref for Cx<'_> {
    type Target = Editor;
    fn deref(&self) -> &Editor {
        self.editor
    }
}

impl DerefMut for Cx<'_> {
    fn deref_mut(&mut self) -> &mut Editor {
        self.editor
    }
}

impl Cx<'_> {
    /// `cm.operation(f)` with the other editors reachable, so an edit of a linked
    /// document's editor is delivered in this group.
    pub fn run_op<R>(&mut self, f: impl FnOnce(&mut Editor, &mut dyn Editors) -> R) -> R {
        let others = &mut *self.others;
        self.editor.run_op_with(others, f)
    }
    /// Run `f` with editor `id` (another editor reached by the caller). `false`
    /// when it cannot be reached (unknown, or busy further up the stack).
    pub fn with_editor(&mut self, id: EditorId, mut f: impl FnMut(&mut Cx)) -> bool {
        self.others.with_editor(id, &mut |ed, rest| {
            f(&mut Cx {
                editor: editor_of(ed),
                others: rest,
            })
        })
    }
}

pub type ChangeHandler = dyn Fn(&mut Cx<'_>, &ChangeObj);
pub type ChangesHandler = dyn Fn(&mut Cx<'_>, &[Rc<ChangeObj>]);
pub type EditorHandler = dyn Fn(&mut Cx<'_>);
pub type ViewportHandler = dyn Fn(&mut Cx<'_>, i64, i64);
pub type MarkerHandler = dyn Fn(&mut Cx<'_>, &TextMarker);
pub type MarkerClearedHandler = dyn Fn(&mut Cx<'_>, &TextMarker, Option<i64>, Option<i64>);
pub type NameHandler = dyn Fn(&mut Cx<'_>, &str);
pub type LineHandler = dyn Fn(&mut Cx<'_>, i64);
pub type InputReadHandler = dyn Fn(&mut Cx<'_>, &InputChange);
pub type SwapDocHandler = dyn Fn(&mut Cx<'_>, &Doc);

/// The `inputRead` payload: the change typed or pasted text asked for (before
/// `beforeChange` handlers saw it).
#[derive(Clone, Debug, PartialEq)]
pub struct InputChange {
    pub from: cm_core::Pos,
    pub to: cm_core::Pos,
    pub text: Vec<String>,
    pub origin: String,
}
pub type BeforeChangeHandler = dyn Fn(&Doc, EditorId, &mut BeforeChange);
pub type BeforeSelHandler = dyn Fn(&Doc, EditorId, &mut BeforeSelectionChange);

/// The handlers of one editor, by event.
#[derive(Clone, Default)]
pub struct EditorListeners {
    /// Immediate, inside the model, after the document's own handlers.
    pub before_change: Listeners<BeforeChangeHandler>,
    pub before_selection_change: Listeners<BeforeSelHandler>,
    /// Delayed, once per change.
    pub change: Listeners<ChangeHandler>,
    /// At the end of the operation, with every change of the operation.
    pub changes: Listeners<ChangesHandler>,
    /// Collected when the selection changes; called after the delayed queue.
    pub cursor_activity: Listeners<EditorHandler>,
    /// At the end of an operation that redrew the display.
    pub update: Listeners<EditorHandler>,
    /// After `update`, when the rendered line range changed.
    pub viewport_change: Listeners<ViewportHandler>,
    /// Delayed.
    pub marker_added: Listeners<MarkerHandler>,
    pub marker_cleared: Listeners<MarkerClearedHandler>,
    /// Delayed: a key binding handled a key (the binding's name).
    pub key_handled: Listeners<NameHandler>,
    /// Delayed: typed or pasted text was applied.
    pub input_read: Listeners<InputReadHandler>,
    /// Delayed: an electric character reindented this line.
    pub electric_input: Listeners<LineHandler>,
    /// Immediate: the editor gained or lost focus.
    pub focus: Listeners<EditorHandler>,
    pub blur: Listeners<EditorHandler>,
    /// Immediate: `refresh()` ran.
    pub refresh: Listeners<EditorHandler>,
    /// Delayed: `swapDoc` replaced the document (the old one).
    pub swap_doc: Listeners<SwapDocHandler>,
}

impl EditorListeners {
    /// Remove the handler registered as `id`, whatever its event.
    pub fn off(&mut self, id: ListenerId) -> bool {
        self.remove(id).is_some()
    }
    /// [`EditorListeners::off`], returning the entry to drop once the tables are
    /// released.
    pub fn remove(&mut self, id: ListenerId) -> Removed {
        removed(self.before_change.remove(id))
            .or_else(|| removed(self.before_selection_change.remove(id)))
            .or_else(|| removed(self.change.remove(id)))
            .or_else(|| removed(self.changes.remove(id)))
            .or_else(|| removed(self.cursor_activity.remove(id)))
            .or_else(|| removed(self.update.remove(id)))
            .or_else(|| removed(self.viewport_change.remove(id)))
            .or_else(|| removed(self.marker_added.remove(id)))
            .or_else(|| removed(self.marker_cleared.remove(id)))
            .or_else(|| removed(self.key_handled.remove(id)))
            .or_else(|| removed(self.input_read.remove(id)))
            .or_else(|| removed(self.electric_input.remove(id)))
            .or_else(|| removed(self.focus.remove(id)))
            .or_else(|| removed(self.blur.remove(id)))
            .or_else(|| removed(self.refresh.remove(id)))
            .or_else(|| removed(self.swap_doc.remove(id)))
    }
}

/// What the display phase decided to report at the finish phase
/// (`DisplayUpdate.signal`: recorded only when a handler exists at that point).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DisplayEvent {
    Update,
    ViewportChange(i64, i64),
}

/// One call that invalidates part of the display (src/display/view_tracking.js).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewChange {
    /// `regChange(cm, from, to, lendiff)`: lines `from..to` (before the change)
    /// were replaced by `to - from + lendiff` lines. It may cut, shift or reset the
    /// rendered range.
    Range { from: i64, to: i64, lendiff: i64 },
    /// `regLineChange(cm, line, type)`: line `line` must be redrawn. It marks that
    /// line of the rendered range dirty and never moves the range.
    Line(i64),
    /// `for (l = display.viewFrom; l < display.viewTo; l++) regLineChange(cm, l,
    /// "gutter")` (shiftDoc): every line of the rendered range, as it is at this
    /// point of the replay, must be redrawn.
    ViewLines,
}

/// `cm.curOp`: what one operation collected for its end.
pub(crate) struct OpRecord {
    pub id: u64,
    /// `op.changeObjs`: the changes made while a `changes` handler existed.
    pub change_objs: Vec<Rc<ChangeObj>>,
    /// `op.cursorActivityHandlers` and `op.cursorActivityCalled`.
    pub cursor_activity: Vec<Entry<EditorHandler>>,
    pub ca_called: usize,
    /// The `regChange`/`regLineChange` calls, in order; the display phase replays
    /// them against the rendered range.
    pub regs: Vec<ViewChange>,
    pub selection_changed: bool,
    pub maybe_hidden: Vec<TextMarker>,
    pub maybe_unhidden: Vec<TextMarker>,
    pub display: Vec<DisplayEvent>,
    /// `op.updateMaxLine`: the longest line must be found again (endOperation_R1).
    pub update_max_line: bool,
}

impl OpRecord {
    fn new(id: u64) -> OpRecord {
        OpRecord {
            id,
            change_objs: Vec::new(),
            cursor_activity: Vec::new(),
            ca_called: 0,
            regs: Vec::new(),
            selection_changed: false,
            maybe_hidden: Vec::new(),
            maybe_unhidden: Vec::new(),
            display: Vec::new(),
            update_max_line: false,
        }
    }
}

/// Work for the editor that could not be delivered when it was due (the editor
/// was not reachable), delivered at its next entry.
pub(crate) enum Parked {
    /// A queued callback (`signalLater`, `cursorActivity`) of a delivery batch.
    Call(Batch, EdCall),
    /// An operation's display and finish phases.
    End(Box<OpRecord>),
    /// An operation's finish phase, when the editor was reached for its display
    /// phase but not for its finish.
    Finish(EdCall),
}

/// The part of an editor its document can reach (`doc.cm`). Its cells are never
/// borrowed while a handler runs.
pub(crate) struct EditorShared {
    pub id: EditorId,
    pub me: Weak<EditorShared>,
    pub sem: Cell<EditorSemantics>,
    pub ls: RefCell<EditorListeners>,
    pub op: RefCell<Option<OpRecord>>,
    pub parked: RefCell<Vec<Parked>>,
    /// Set when the editor is dropped: later work for it is discarded.
    pub destroyed: Cell<bool>,
    pub hs: Rc<RefCell<HighlightState>>,
    pub heights: Rc<RefCell<HeightCache>>,
}

impl EditorShared {
    pub(crate) fn new(
        sem: EditorSemantics,
        hs: Rc<RefCell<HighlightState>>,
        heights: Rc<RefCell<HeightCache>>,
    ) -> Rc<EditorShared> {
        Rc::new_cyclic(|me| EditorShared {
            id: EditorId::next(),
            me: me.clone(),
            sem: Cell::new(sem),
            ls: RefCell::new(EditorListeners::default()),
            op: RefCell::new(None),
            parked: RefCell::new(Vec::new()),
            destroyed: Cell::new(false),
            hs,
            heights,
        })
    }

    pub(crate) fn park(&self, item: Parked) {
        if self.destroyed.get() {
            diagnostic(format!(
                "discarded work for editor {}, which was dropped",
                self.id.get()
            ));
            return;
        }
        self.parked.borrow_mut().push(item);
    }

    /// `signalLater(cm, …)`: queue `call` for this editor.
    pub(crate) fn later(&self, call: impl FnOnce(&mut Cx) + 'static) {
        let sink: Weak<dyn EditorSink> = self.me.clone();
        signal_later(Delayed::Editor {
            target: self.id,
            sink,
            call: Box::new(move |ed, rest| {
                call(&mut Cx {
                    editor: editor_of(ed),
                    others: rest,
                })
            }),
        });
    }

    /// `isWholeLineUpdate(doc, change)` (src/model/document_data.js:16), with this
    /// editor's `wholeLineUpdateBefore`.
    fn whole_line_update(&self, c: &ChangeObj) -> bool {
        c.from.ch == 0
            && c.to.ch == 0
            && c.text.last().is_some_and(|t| t.is_empty())
            && self.sem.get().whole_line_update_before
    }

    /// `regLineChange(cm, line, "text")` for each of `lines`, in the open operation.
    pub(crate) fn reg_line_changes(&self, lines: &[i64]) {
        self.with_op(|op| op.regs.extend(lines.iter().copied().map(ViewChange::Line)));
    }

    fn with_op(&self, f: impl FnOnce(&mut OpRecord)) {
        if let Some(op) = self.op.borrow_mut().as_mut() {
            f(op);
        }
    }

    /// `regChange(cm)` over the whole document.
    pub(crate) fn reg_change_all(&self, doc: &Doc) {
        let (from, to) = (doc.first_line(), doc.first_line() + doc.line_count() as i64);
        self.with_op(|op| {
            op.regs.push(ViewChange::Range {
                from,
                to,
                lendiff: 0,
            })
        });
    }

    /// The `display.maxLine` bookkeeping of makeChangeSingleDocInEditor
    /// (changes.js:214-241), after the change. CM5 checks before the change whether
    /// the max line is among the lines `checkWidthStart..=to.line`; those lines
    /// become `checkWidthStart..from.line + text.length`, so the same holds exactly
    /// when the max line is now among those or was removed. A line longer than
    /// `maxLineLength` becomes the max line; otherwise a touched max line is found
    /// again at the end of the operation.
    fn track_max_line(&self, doc: &Doc, c: &ChangeObj) {
        let mut h = self.heights.borrow_mut();
        let ml = &mut h.max_line;
        if ml.wrapping {
            return;
        }
        let start = doc.visual_line_no(c.from.line);
        let end = c.from.line + c.text.len() as i64;
        let mut recompute = match ml.line.as_ref().map(|l| doc.get_line_number(l)) {
            Some(Some(n)) => n >= start && n < end,
            Some(None) => true,
            None => false,
        };
        doc.each_line(start, end, |l| {
            let len = l.line_length();
            if len > ml.length {
                ml.line = Some(l.clone());
                ml.length = len;
                ml.changed = true;
                recompute = false;
            }
            false
        });
        drop(h);
        if recompute {
            self.with_op(|op| op.update_max_line = true);
        }
    }
}

/// The editor behind a `dyn Any` handed out by [`Editors`].
pub(crate) fn editor_of(ed: &mut dyn Any) -> &mut Editor {
    ed.downcast_mut::<Editor>()
        .expect("editor calls receive a cm_view::Editor")
}

/// `endOperation_finish`: marker `hide`/`unhide`, then `changes`, then the display
/// events, with the handlers registered at this point.
pub(crate) fn finish(cx: &mut Cx, rec: OpRecord) {
    for m in &rec.maybe_hidden {
        if m.line_count() == 0 {
            m.signal_hidden(true);
        }
    }
    for m in &rec.maybe_unhidden {
        if m.line_count() > 0 {
            m.signal_hidden(false);
        }
    }
    if !rec.change_objs.is_empty() {
        let snap = cx.editor.shared.ls.borrow().changes.snapshot();
        for e in snap.iter() {
            (e.f)(cx, &rec.change_objs);
        }
    }
    for ev in rec.display {
        match ev {
            DisplayEvent::Update => {
                let snap = cx.editor.shared.ls.borrow().update.snapshot();
                for e in snap.iter() {
                    (e.f)(cx);
                }
            }
            DisplayEvent::ViewportChange(from, to) => {
                let snap = cx.editor.shared.ls.borrow().viewport_change.snapshot();
                for e in snap.iter() {
                    (e.f)(cx, from, to);
                }
            }
        }
    }
}

fn record(rec: Box<dyn Any>) -> Box<OpRecord> {
    rec.downcast::<OpRecord>()
        .expect("an editor operation record")
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
        *self.op.borrow_mut() = Some(OpRecord::new(id));
    }
    fn close_op(&self) -> Box<dyn Any> {
        Box::new(self.op.borrow_mut().take().expect("an open operation"))
    }
    fn next_cursor_activity(&self) -> Option<EdCall> {
        let h = {
            let mut op = self.op.borrow_mut();
            let op = op.as_mut()?;
            let h = op.cursor_activity.get(op.ca_called)?.f.clone();
            op.ca_called += 1;
            h
        };
        Some(Box::new(move |ed, rest| {
            h(&mut Cx {
                editor: editor_of(ed),
                others: rest,
            })
        }))
    }
    fn end_display(&self, rec: Box<dyn Any>, access: &mut dyn Editors) -> Option<Box<dyn Any>> {
        let mut rec = record(rec);
        let reached = access.with_editor(self.id, &mut |ed, _| {
            editor_of(ed).end_operation_display(&mut rec);
        });
        if reached {
            Some(rec)
        } else {
            self.park(Parked::End(rec));
            None
        }
    }
    fn end_finish(&self, rec: Box<dyn Any>, access: &mut dyn Editors) {
        let mut rec = Some(*record(rec));
        let reached = access.with_editor(self.id, &mut |ed, rest| {
            finish(
                &mut Cx {
                    editor: editor_of(ed),
                    others: rest,
                },
                rec.take().expect("finished once"),
            )
        });
        if !reached && let Some(rec) = rec {
            self.park(Parked::Finish(Box::new(move |ed, rest| {
                finish(
                    &mut Cx {
                        editor: editor_of(ed),
                        others: rest,
                    },
                    rec,
                )
            })));
        }
    }
    fn park_call(&self, batch: Batch, call: EdCall) {
        self.park(Parked::Call(batch, call));
    }

    fn before_change_handlers(&self) -> Vec<Rc<BeforeChangeFn>> {
        let id = self.id;
        let snap = self.ls.borrow().before_change.snapshot();
        snap.iter()
            .map(|e| {
                let f = e.f.clone();
                Rc::new(move |d: &Doc, c: &mut BeforeChange| f(d, id, c)) as Rc<BeforeChangeFn>
            })
            .collect()
    }
    fn before_selection_change_handlers(&self) -> Vec<Rc<BeforeSelectionChangeFn>> {
        let id = self.id;
        let snap = self.ls.borrow().before_selection_change.snapshot();
        snap.iter()
            .map(|e| {
                let f = e.f.clone();
                Rc::new(move |d: &Doc, s: &mut BeforeSelectionChange| f(d, id, s))
                    as Rc<BeforeSelectionChangeFn>
            })
            .collect()
    }
    fn change_applied(&self, doc: &Doc, c: &Rc<ChangeObj>, full: bool) {
        // makeChangeSingleDocInEditor: `retreatFrontier(doc, from.line)`.
        highlight::retreat_frontier(doc, &mut self.hs.borrow_mut(), c.from.line);
        {
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
        self.track_max_line(doc, c);
        // `regChange`, then `signalLater(cm, "change", cm, obj)` and `changeObjs`.
        let (snap, has_changes) = {
            let ls = self.ls.borrow();
            (ls.change.snapshot(), !ls.changes.is_empty())
        };
        // changes.js:249-254: `regChange(cm)` for a full change, `regLineChange` for
        // one that stays on one line and is not a whole-line update, otherwise
        // `regChange(cm, from.line, to.line + 1, lendiff)`.
        let lendiff = c.text.len() as i64 - (c.to.line - c.from.line + 1);
        let reg = if full {
            ViewChange::Range {
                from: doc.first_line(),
                to: doc.first_line() + doc.line_count() as i64,
                lendiff: 0,
            }
        } else if c.from.line == c.to.line && c.text.len() == 1 && !self.whole_line_update(c) {
            ViewChange::Line(c.from.line)
        } else {
            ViewChange::Range {
                from: c.from.line,
                to: c.to.line + 1,
                lendiff,
            }
        };
        self.with_op(|op| {
            op.regs.push(reg);
            if has_changes {
                op.change_objs.push(c.clone());
            }
        });
        for e in snap.iter() {
            let (f, c) = (e.f.clone(), c.clone());
            self.later(move |cx| f(cx, &c));
        }
    }
    fn doc_shifted(&self, doc: &Doc, distance: i64) {
        // changes.js:168-170: `regChange(doc.cm, doc.first, doc.first - distance,
        // distance)` with the new `first`, then a gutter `regLineChange` for each
        // line of the range that regChange left.
        let first = doc.first_line();
        self.with_op(|op| {
            op.regs.push(ViewChange::Range {
                from: first,
                to: first - distance,
                lendiff: distance,
            });
            op.regs.push(ViewChange::ViewLines);
        });
    }
    fn signal_cursor_activity(&self) {
        // event.js:58-64: add each handler not yet collected (function identity).
        let snap = self.ls.borrow().cursor_activity.snapshot();
        self.with_op(|op| {
            for e in snap.iter() {
                if !op.cursor_activity.iter().any(|x| x.same_fn(&e.f)) {
                    op.cursor_activity.push(e.clone());
                }
            }
        });
    }
    fn selection_changed(&self) {
        self.with_op(|op| op.selection_changed = true);
        self.signal_cursor_activity();
    }
    fn marker_added(&self, doc: &Doc, marker: &TextMarker) {
        let o = marker.options();
        // markText: `regChange` for a collapsed marker, `regLineChange` for each line
        // of one with a class or style.
        let redraw = marker.is_collapsed()
            || o.class_name.is_some()
            || o.start_style.is_some()
            || o.end_style.is_some()
            || o.css.is_some()
            || o.title.is_some();
        if redraw {
            let lines = match marker.find() {
                Some(MarkerFind::Range { from, to }) => Some((from.line, to.unwrap_or(from).line)),
                Some(MarkerFind::Point(p)) => Some((p.line, p.line)),
                None => None,
            };
            if let Some((a, b)) = lines {
                // markText (mark_text.js:183-211): a collapsed marker over the max
                // line's visual line makes the end of the operation find it again.
                // The marker is already in place, so this asks whether the max line
                // is among the lines from `from.line`'s visual line to `to.line`.
                let touches_max = marker.is_collapsed() && {
                    let h = self.heights.borrow();
                    let ml = &h.max_line;
                    !ml.wrapping
                        && ml
                            .line
                            .as_ref()
                            .and_then(|l| doc.get_line_number(l))
                            .is_some_and(|n| n >= doc.visual_line_no(a) && n <= b)
                };
                self.with_op(|op| {
                    op.update_max_line |= touches_max;
                    if marker.is_collapsed() {
                        op.regs.push(ViewChange::Range {
                            from: a,
                            to: b + 1,
                            lendiff: 0,
                        });
                    } else {
                        op.regs.extend((a..=b).map(ViewChange::Line));
                    }
                });
            }
        }
        let snap = self.ls.borrow().marker_added.snapshot();
        for e in snap.iter() {
            let (f, m) = (e.f.clone(), marker.clone());
            self.later(move |cx| f(cx, &m));
        }
    }
    fn marker_maybe_hidden(&self, marker: &TextMarker) {
        self.with_op(|op| op.maybe_hidden.push(marker.clone()));
    }
    fn marker_maybe_unhidden(&self, marker: &TextMarker) {
        self.with_op(|op| {
            if !op.maybe_hidden.contains(marker) {
                op.maybe_unhidden.push(marker.clone());
            }
        });
    }
    fn line_changed(&self, _doc: &Doc, line: i64, _kind: cm_core::LineChange) {
        // `regLineChange(cm, line, "class" | "gutter")`: the rendered line is redrawn.
        self.with_op(|op| op.regs.push(ViewChange::Line(line)));
    }

    fn marker_cleared(
        &self,
        doc: &Doc,
        marker: &TextMarker,
        min: Option<i64>,
        max: Option<i64>,
        redraw: &[(i64, i64)],
    ) {
        // mark_text.js:56 and :74: `regLineChange` for each line of a marker that is
        // not collapsed, `regChange(cm, min, max + 1)` for a collapsed one.
        let collapsed = marker.is_collapsed();
        // mark_text.js:63-71: each line of a cleared collapsed marker may make its
        // visual line the longest.
        if let (true, Some(a), Some(b)) = (collapsed, min, max) {
            let mut h = self.heights.borrow_mut();
            let ml = &mut h.max_line;
            if !ml.wrapping {
                for n in a..=b {
                    let Some(visual) = doc.get_line_handle(doc.visual_line_no(n)) else {
                        continue;
                    };
                    let len = visual.line_length();
                    if len > ml.length {
                        ml.line = Some(visual);
                        ml.length = len;
                        ml.changed = true;
                    }
                }
            }
        }
        self.with_op(|op| {
            op.regs.extend(redraw.iter().map(|&(a, b)| {
                if collapsed {
                    ViewChange::Range {
                        from: a,
                        to: b,
                        lendiff: 0,
                    }
                } else {
                    ViewChange::Line(a)
                }
            }))
        });
        let snap = self.ls.borrow().marker_cleared.snapshot();
        for e in snap.iter() {
            let (f, m) = (e.f.clone(), marker.clone());
            self.later(move |cx| f(cx, &m, min, max));
        }
    }
}

/// One of an editor's listener tables, changed in place ([`EditorEvents::with_listeners`]).
pub struct Table<'a, H: ?Sized> {
    shared: &'a EditorShared,
    pick: fn(&mut EditorListeners) -> &mut Listeners<H>,
}

impl<H: ?Sized> Table<'_, H> {
    /// `on(cm, type, f)`: append `f`.
    pub fn on_rc(&self, f: Rc<H>) -> ListenerId {
        (self.pick)(&mut self.shared.ls.borrow_mut()).on_rc(f)
    }
    /// Remove the handler registered as `id`. `false` when it is not in this table.
    pub fn off(&self, id: ListenerId) -> bool {
        // Dropped after the borrow ends.
        let gone = (self.pick)(&mut self.shared.ls.borrow_mut()).remove(id);
        gone.is_some()
    }
    /// `off(cm, type, f)`: remove the first entry with the same function.
    pub fn off_fn(&self, f: &Rc<H>) -> bool {
        let gone = (self.pick)(&mut self.shared.ls.borrow_mut()).remove_fn(f);
        gone.is_some()
    }
    /// The handlers as of now (`getHandlers`).
    pub fn snapshot(&self) -> Rc<[Entry<H>]> {
        (self.pick)(&mut self.shared.ls.borrow_mut()).snapshot()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn len(&self) -> usize {
        (self.pick)(&mut self.shared.ls.borrow_mut()).len()
    }
    pub fn contains(&self, id: ListenerId) -> bool {
        (self.pick)(&mut self.shared.ls.borrow_mut()).contains(id)
    }
}

macro_rules! listener_tables {
    ($($name:ident: $h:ty),* $(,)?) => {
        /// An editor's listener tables, by event ([`EditorListeners`]), each changed in
        /// place.
        pub struct ListenerTables<'a> {
            $(pub $name: Table<'a, $h>,)*
        }

        impl<'a> ListenerTables<'a> {
            fn new(shared: &'a EditorShared) -> ListenerTables<'a> {
                $(
                    let $name = {
                        fn pick(l: &mut EditorListeners) -> &mut Listeners<$h> {
                            &mut l.$name
                        }
                        Table { shared, pick }
                    };
                )*
                ListenerTables { $($name,)* }
            }
        }
    };
}

listener_tables! {
    before_change: BeforeChangeHandler,
    before_selection_change: BeforeSelHandler,
    change: ChangeHandler,
    changes: ChangesHandler,
    cursor_activity: EditorHandler,
    update: EditorHandler,
    viewport_change: ViewportHandler,
    marker_added: MarkerHandler,
    marker_cleared: MarkerClearedHandler,
    key_handled: NameHandler,
    input_read: InputReadHandler,
    electric_input: LineHandler,
    focus: EditorHandler,
    blur: EditorHandler,
}

/// A handle on an editor's listener tables, usable without the editor (for example
/// from a handler that removes itself).
#[derive(Clone)]
pub struct EditorEvents(pub(crate) Rc<EditorShared>);

impl EditorEvents {
    pub fn id(&self) -> EditorId {
        self.0.id
    }
    /// Run `f` with the listener tables, for `on_rc`/`off_fn` (function identity).
    ///
    /// Each call on a table (`ls.update.on_rc(f)`, `ls.change.off_fn(&f)`, …)
    /// changes the live table at once, like CM5's `on`/`off`: changes made through
    /// `f`, through `Editor::on_*`/`off`, or in a nested `with_listeners` apply in
    /// the order they are made, so every `off_fn` sees the removals made before it.
    /// No table is borrowed while user code runs: each call borrows the table only
    /// for its own change, and a removed handler is dropped after the borrow ends
    /// (its destructor may use the tables). A dispatch that is running keeps its
    /// snapshot. If `f` panics, the changes it made before the panic stay, as in
    /// the JS.
    pub fn with_listeners<R>(&self, f: impl FnOnce(&mut ListenerTables) -> R) -> R {
        f(&mut ListenerTables::new(&self.0))
    }
    /// Remove the handler registered as `id`, whatever its event.
    pub fn off(&self, id: ListenerId) -> bool {
        let gone = self.0.ls.borrow_mut().remove(id);
        gone.is_some()
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
