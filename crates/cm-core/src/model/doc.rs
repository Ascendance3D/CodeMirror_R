//! `src/model/Doc.js`: the public document API.
//!
//! A [`Doc`] is a cheap, clonable handle (`Rc`). Linked documents hold strong
//! references to each other, like the JS objects do; call [`Doc::unlink_doc`] to
//! break a link (otherwise a linked group lives until all of it is dropped, which
//! for a reference cycle means never — the JS relies on a tracing GC here).
//!
//! ## Editor semantics
//! A bare JS `Doc` and one attached to a `CodeMirror` editor behave differently in
//! the model: editor calls run inside *operations* (changes and selection updates
//! in the same operation merge into one history event), `historyEventDelay` is 1250
//! instead of 500, `setExtending` only works with an editor, and atomic markers
//! re-check the selection. [`Doc::attach_editor_semantics`] switches a document to
//! the editor behavior without any view, and [`Doc::operation`] groups calls.

use std::any::Any;
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::change_measurement::compute_replaced_sel;
use super::changes::{
    BeforeChange, BeforeSelectionChange, Change, ChangeObj, make_change, make_change_from_history,
    replace_range,
};
use super::history::{
    HistRef, History, HistoryData, HistorySize, copy_history_array, data_to_events, events_to_data,
};
use super::line::{LineHandle, LineRef, new_line};
use super::mark_text::{
    MarkOptions, Marker, MarkerType, TextMarker, copy_shared_markers, detach_shared_markers,
    find_shared_markers, mark_text,
};
use super::selection::{Range, Sel, SelRange, Selection, normalize_selection, simple_selection};
use super::selection_updates::{
    SelOptions, extend_selection, extend_selections, select_all, set_selection,
    set_selection_replace_history, set_simple_selection,
};
use super::spans::{
    height_at_line, line_is_hidden, line_length, visual_line, visual_line_end_no, visual_line_no,
};
use super::tree::Tree;
use crate::CmError;
use crate::events::{ListenerId, Listeners, removed};
use crate::operation_group::{EditorId, EditorSink, Editors, NoEditors, run_in_op};
use crate::pos::{CH_END, Pos, PosRef};
use crate::text::{Text, from_u16, join, split_by, split_lines_auto, to_u16};
use crate::util::bidi::Direction;

static NEXT_DOC_ID: AtomicU32 = AtomicU32::new(0);

/// Model-level behavior of a document attached to an editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorSemantics {
    /// `historyEventDelay` option (default 1250).
    pub history_event_delay: f64,
    /// `wholeLineUpdateBefore` option (default true).
    pub whole_line_update_before: bool,
    /// `selectionsMayTouch` option (default false).
    pub selections_may_touch: bool,
    /// `undoDepth` option (default 200), applied to the history when attaching.
    pub undo_depth: f64,
}

impl Default for EditorSemantics {
    fn default() -> Self {
        EditorSemantics {
            history_event_delay: 1250.0,
            whole_line_update_before: true,
            selections_may_touch: false,
            undo_depth: 200.0,
        }
    }
}

/// The editor of a document with [`Doc::attach_editor_semantics`]: operations and
/// options, no view and no editor events.
struct HeadlessSink {
    id: EditorId,
    sem: EditorSemantics,
    op: Cell<Option<u64>>,
}

impl EditorSink for HeadlessSink {
    fn id(&self) -> EditorId {
        self.id
    }
    fn semantics(&self) -> EditorSemantics {
        self.sem
    }
    fn op_id(&self) -> Option<u64> {
        self.op.get()
    }
    fn open_op(&self, id: u64) {
        self.op.set(Some(id));
    }
    fn close_op(&self) -> Box<dyn Any> {
        self.op.set(None);
        Box::new(())
    }
    fn end_display(&self, rec: Box<dyn Any>, _: &mut dyn Editors) -> Option<Box<dyn Any>> {
        Some(rec)
    }
    fn park_call(&self, _: crate::operation_group::Batch, _: crate::operation_group::EdCall) {
        crate::operation_group::diagnostic(format!(
            "discarded a callback for headless editor {}",
            self.id.get()
        ));
    }
}

pub(crate) struct Link {
    pub doc: Doc,
    pub is_parent: bool,
    pub shared_hist: bool,
}

pub(crate) type BeforeChangeHandler = dyn Fn(&Doc, &mut BeforeChange);
pub(crate) type BeforeSelHandler = dyn Fn(&Doc, &mut BeforeSelectionChange);
pub(crate) type ChangeHandler = dyn Fn(&Doc, &ChangeObj);

pub(crate) struct DocInner {
    pub id: u32,
    pub first: i64,
    pub tree: Tree,
    pub sel: Sel,
    pub history: HistRef,
    pub linked: Vec<Link>,
    pub line_sep: Option<Text>,
    pub cant_edit: bool,
    pub clean_generation: u64,
    pub extend: bool,
    pub direction: Direction,
    pub mode_option: Option<String>,
    /// `doc.cm`: the editor this document is attached to.
    pub sink: Option<Rc<dyn EditorSink>>,
    /// `op.markedSpans`: span arrays created by `addMarkedSpan` in the operation
    /// with this id, which later additions in the same operation mutate in place.
    pub op_span_arrays: (u64, Vec<super::spans::SpanArr>),
    pub before_change: Listeners<BeforeChangeHandler>,
    pub before_selection_change: Listeners<BeforeSelHandler>,
    /// `change` (delayed).
    pub change: Listeners<ChangeHandler>,
    /// `cursorActivity` (delayed).
    pub cursor_activity: Listeners<dyn Fn(&Doc)>,
    /// `historyAdded` (immediate).
    pub history_added: Listeners<dyn Fn(&Doc)>,
    /// `cm.state.suppressEdits`: set while a key handler runs in a read-only editor.
    pub suppress_edits: bool,
    /// The cursor at each `ensureCursorVisible(doc.cm)` call of `setSelectionNoUndo`
    /// and `replaceSelections`, in order; taken by the view
    /// ([`Doc::take_scroll_requests`]).
    pub scroll_requests: Vec<Pos>,
    /// `cm.display.shift` (see [`Doc::set_display_shift`]).
    pub display_shift: bool,
}

#[derive(Clone)]
pub struct Doc(pub(crate) Rc<RefCell<DocInner>>);

/// Constructor arguments (`new Doc(text, mode, firstLine, lineSep, direction)`).
#[derive(Clone, Debug, Default)]
pub struct DocOptions {
    /// Split with the document's line splitting rules.
    pub text: String,
    /// When set, used as the lines directly (the JS accepts an array).
    pub lines: Option<Vec<String>>,
    pub mode: Option<String>,
    pub first_line: i64,
    pub line_sep: Option<String>,
    pub rtl: bool,
}

/// `linkedDoc` options.
#[derive(Clone, Debug, Default)]
pub struct LinkedDocOptions {
    pub shared_hist: bool,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub mode: Option<String>,
}

impl std::fmt::Debug for Doc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Doc#{}", self.id())
    }
}

impl PartialEq for Doc {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl Doc {
    // ----------------------------------------------------------------------------
    // Construction

    pub fn new(text: &str) -> Doc {
        Doc::with_options(DocOptions {
            text: text.to_string(),
            ..Default::default()
        })
    }

    pub fn with_options(o: DocOptions) -> Doc {
        let lines = o.lines.map(|ls| ls.iter().map(|l| to_u16(l)).collect());
        Doc::construct(
            lines,
            &o.text,
            o.mode,
            o.first_line,
            o.line_sep.map(|s| to_u16(&s)),
            if o.rtl {
                Direction::Rtl
            } else {
                Direction::Ltr
            },
        )
    }

    fn construct(
        lines: Option<Vec<Text>>,
        text: &str,
        mode: Option<String>,
        first_line: i64,
        line_sep: Option<Text>,
        direction: Direction,
    ) -> Doc {
        let start = PosRef::lc(first_line, 0);
        let inner = DocInner {
            id: NEXT_DOC_ID.fetch_add(1, Ordering::Relaxed) + 1,
            first: first_line,
            tree: Tree::new(vec![new_line(Vec::new(), None)]),
            sel: simple_selection(start.clone(), None),
            history: Rc::new(RefCell::new(History::new(None))),
            linked: Vec::new(),
            line_sep,
            cant_edit: false,
            clean_generation: 1,
            extend: false,
            direction,
            mode_option: mode,
            sink: None,
            op_span_arrays: (0, Vec::new()),
            before_change: Listeners::default(),
            suppress_edits: false,
            display_shift: false,
            scroll_requests: Vec::new(),
            before_selection_change: Listeners::default(),
            change: Listeners::default(),
            cursor_activity: Listeners::default(),
            history_added: Listeners::default(),
        };
        let doc = Doc(Rc::new(RefCell::new(inner)));
        let text = match lines {
            Some(l) => l,
            None => doc.split_lines_u16(&to_u16(text)),
        };
        let p = Pos::new(first_line, 0);
        super::document_data::update_doc(
            &doc,
            &Change {
                from: p,
                to: p,
                text: Some(text),
                origin: None,
                full: false,
            },
            None,
            None,
        );
        set_selection(
            &doc,
            simple_selection(PosRef::lc(first_line, 0), None),
            Some(&dont_scroll()),
        );
        doc
    }

    // ----------------------------------------------------------------------------
    // Crate-internal plumbing

    pub(crate) fn inner(&self) -> Ref<'_, DocInner> {
        self.0.borrow()
    }
    pub(crate) fn inner_mut(&self) -> RefMut<'_, DocInner> {
        self.0.borrow_mut()
    }
    pub fn ptr_eq(&self, other: &Doc) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    /// The document id (`doc.id`), unique per process.
    pub fn id(&self) -> u32 {
        self.inner().id
    }
    pub(crate) fn size(&self) -> usize {
        self.inner().tree.size()
    }
    pub(crate) fn history_rc(&self) -> HistRef {
        self.inner().history.clone()
    }
    pub(crate) fn has_editor(&self) -> bool {
        self.inner().sink.is_some()
    }
    /// `hasHandler(doc, "beforeChange") || doc.cm && hasHandler(doc.cm, "beforeChange")`.
    pub(crate) fn has_before_change(&self) -> bool {
        !self.inner().before_change.is_empty() || self.sink().is_some_and(|s| s.has_before_change())
    }
    /// The same for `beforeSelectionChange`.
    pub(crate) fn has_before_selection_change(&self) -> bool {
        !self.inner().before_selection_change.is_empty()
            || self.sink().is_some_and(|s| s.has_before_selection_change())
    }
    /// `doc.cm`, cloned so no document borrow is held while it is used.
    pub fn sink(&self) -> Option<Rc<dyn EditorSink>> {
        self.inner().sink.clone()
    }
    /// The id of the editor this document is attached to.
    pub fn editor_id(&self) -> Option<EditorId> {
        self.sink().map(|s| s.id())
    }
    fn semantics(&self) -> Option<EditorSemantics> {
        self.sink().map(|s| s.semantics())
    }
    pub(crate) fn history_event_delay(&self) -> f64 {
        self.semantics()
            .map(|e| e.history_event_delay)
            .unwrap_or(500.0)
    }
    pub(crate) fn whole_line_update_before(&self) -> bool {
        self.semantics().is_none_or(|e| e.whole_line_update_before)
    }
    pub(crate) fn selections_may_touch(&self) -> bool {
        self.semantics().is_some_and(|e| e.selections_may_touch)
    }
    /// `doc.cm && (doc.cm.display.shift || doc.extend)`: without an editor this is
    /// always false, even after `setExtending(true)`.
    pub(crate) fn extend_flag(&self) -> bool {
        let i = self.inner();
        i.sink.is_some() && (i.display_shift || i.extend)
    }
    /// `cm.display.shift`, mirrored by a view so `extendSelection(s)` sees it.
    pub fn set_display_shift(&self, v: bool) {
        self.inner_mut().display_shift = v;
    }
    /// `doc.cm ? doc.cm.curOp.id : NaN`.
    pub(crate) fn op_id(&self) -> Option<u64> {
        self.sink().and_then(|s| s.op_id())
    }
    /// `docMethodOp`: run `f` inside an operation of the attached editor, opening
    /// one when none is open. A document cannot reach its editor, so when this
    /// operation opens the group, the editor's part of the work is parked on the
    /// editor until its next entry (see [`Doc::attach_editor`]).
    pub(crate) fn in_op<R>(&self, f: impl FnOnce() -> R) -> R {
        match self.sink() {
            Some(sink) => run_in_op(&sink, &mut NoEditors, |_| f()),
            None => f(),
        }
    }
    pub(crate) fn is_line(&self, l: i64) -> bool {
        let i = self.inner();
        l >= i.first && l < i.first + i.tree.size() as i64
    }
    /// `getLine(doc, n)`. Panics with the JS error message for missing lines.
    pub(crate) fn get_line_ref(&self, n: i64) -> LineRef {
        self.try_line_ref(n).unwrap_or_else(|e| panic!("{e}"))
    }
    pub(crate) fn try_line_ref(&self, n: i64) -> Result<LineRef, CmError> {
        let i = self.inner();
        let rel = n - i.first;
        if rel < 0 || rel >= i.tree.size() as i64 {
            return Err(CmError::NoSuchLine(n));
        }
        Ok(i.tree.get(rel as usize))
    }
    /// `lineNo(line)`.
    pub(crate) fn line_no(&self, line: &LineRef) -> Option<i64> {
        let i = self.inner();
        i.tree.index_of_line(line).map(|n| n as i64 + i.first)
    }
    /// The lines of `doc.iter(from, to)`: `iterN(from - first, to - from)`. Lines past
    /// the end are silently skipped; a start before the first line makes the JS call
    /// the callback with `undefined`, which every caller dereferences (TypeError).
    pub(crate) fn iter_lines(&self, from: i64, to: i64) -> Vec<LineRef> {
        let i = self.inner();
        let at = from - i.first;
        let n = to - from;
        if n <= 0 {
            return Vec::new();
        }
        if at < 0 {
            panic!("JS TypeError: Cannot read properties of undefined (reading 'text')");
        }
        if at as usize >= i.tree.size() {
            return Vec::new();
        }
        i.tree.lines_range(at as usize, n as usize)
    }
    /// `doc.insert(at, lines)` (absolute line number; may be before `first`).
    pub(crate) fn insert_lines(&self, at: i64, lines: Vec<LineRef>) {
        let height = lines.iter().map(|l| l.borrow().height).sum();
        let mut i = self.inner_mut();
        let rel = at - i.first;
        i.tree.insert_inner(super::tree::ROOT, rel, lines, height);
    }
    /// `doc.remove(at, n)`.
    pub(crate) fn remove_lines(&self, at: i64, n: usize) {
        let deleted = {
            let mut i = self.inner_mut();
            let rel = at - i.first;
            i.tree.remove_inner(super::tree::ROOT, rel, n);
            std::mem::take(&mut i.tree.deleted_with_handlers)
        };
        super::mark_text::flush_marker_line_hooks();
        // chunk.js removeInner: `signalLater(line, "delete")` for each removed line.
        for line in deleted {
            super::line::signal_line_delete(&line);
        }
    }
    /// `getBetween(doc, start, end)`.
    pub(crate) fn get_between(&self, start: Pos, end: Pos) -> Vec<Text> {
        let mut out = Vec::new();
        let mut n = start.line;
        for line in self.iter_lines(start.line, end.line + 1) {
            let l = line.borrow();
            let mut text: &[u16] = &l.text;
            if n == end.line {
                text = crate::text::js_slice(text, 0, Some(end.ch));
            }
            if n == start.line {
                text = crate::text::js_slice(text, start.ch, None);
            }
            out.push(text.to_vec());
            n += 1;
        }
        out
    }
    /// `linkedDocs(doc, f, sharedHistOnly)` as a list of `(doc, sharedHist)` in the
    /// order the JS visits them.
    pub(crate) fn linked_docs(&self, shared_hist_only: bool) -> Vec<(Doc, bool)> {
        fn propagate(
            doc: &Doc,
            skip: Option<&Doc>,
            shared_hist: bool,
            only: bool,
            out: &mut Vec<(Doc, bool)>,
        ) {
            let links: Vec<(Doc, bool)> = doc
                .inner()
                .linked
                .iter()
                .map(|l| (l.doc.clone(), l.shared_hist))
                .collect();
            for (d, rel_shared) in links {
                if skip.is_some_and(|s| s.ptr_eq(&d)) {
                    continue;
                }
                let shared = shared_hist && rel_shared;
                if only && !shared {
                    continue;
                }
                out.push((d.clone(), shared));
                propagate(&d, Some(doc), shared, only, out);
            }
        }
        let mut out = Vec::new();
        propagate(self, None, true, shared_hist_only, &mut out);
        out
    }

    /// The single-argument `doc.iter(op)`: `this.iterN(this.first, this.first +
    /// this.size, op)`. `iterN` takes a *relative* start, so for a document whose
    /// first line is not 0 this skips the first `first` lines. CM5 bug, kept: it
    /// affects `posFromIndex` and `getAllMarks`.
    pub(crate) fn iter_all_js(&self) -> Vec<LineRef> {
        let i = self.inner();
        let size = i.tree.size() as i64;
        let at = i.first.max(0);
        if at >= size {
            return Vec::new();
        }
        i.tree.lines_range(at as usize, (size - at) as usize)
    }

    pub(crate) fn split_lines_u16(&self, s: &[u16]) -> Vec<Text> {
        match &self.inner().line_sep {
            Some(sep) => split_by(s, sep),
            None => split_lines_auto(s),
        }
    }

    pub(crate) fn find_marks_filtered(
        &self,
        from: Pos,
        to: Pos,
        filter: Option<&dyn Fn(&TextMarker) -> bool>,
    ) -> Vec<Marker> {
        let from = self.clip_pos(from);
        let to = self.clip_pos(to);
        let mut found = Vec::new();
        let mut line_no = from.line;
        for line in self.iter_lines(from.line, to.line + 1) {
            let spans = line.borrow().marked_spans.as_ref().map(|a| a.get());
            if let Some(spans) = spans {
                for span in &spans {
                    let excluded = span
                        .to
                        .is_some_and(|t| line_no == from.line && from.ch >= t)
                        || span.from.is_none() && line_no != from.line
                        || span.from.is_some_and(|f| line_no == to.line && f >= to.ch);
                    if !excluded && filter.is_none_or(|f| f(&span.marker)) {
                        found.push(Marker::outer(&span.marker));
                    }
                }
            }
            line_no += 1;
        }
        found
    }

    fn pos_arg(&self, p: Pos) -> PosRef {
        self.clip_pos_ref(&PosRef::new(p))
    }

    /// `clipPos(doc, pos)` keeping object identity when the position is valid.
    pub(crate) fn clip_pos_ref(&self, pos: &PosRef) -> PosRef {
        let p = pos.get();
        let c = self.clip_pos(p);
        if c == p { pos.clone() } else { PosRef::new(c) }
    }

    // ----------------------------------------------------------------------------
    // Editor semantics, operations, events

    /// Make the model behave as if the document were attached to an editor, with no
    /// view: operations, editor options, no editor events.
    ///
    /// # Panics
    /// When the document already has an editor ([`CmError::DocInUse`]).
    pub fn attach_editor_semantics(&self, sem: EditorSemantics) {
        if let Err(e) = self.attach_editor(Rc::new(HeadlessSink {
            id: EditorId::next(),
            sem,
            op: Cell::new(None),
        })) {
            panic!("{e}");
        }
    }
    /// `attachDoc(cm, doc)`: attach the document to an editor (`doc.cm = cm`).
    ///
    /// Delivery contract for document methods called directly on an attached
    /// document (not through the editor): the call runs in an operation of the
    /// editor, like `docMethodOp`, and its document, line and marker events are
    /// delivered when it returns. The editor's own events and its display update
    /// cannot run without the editor, so they are parked on it and delivered at
    /// its next entry (any editor method that runs an operation), or when the host
    /// flushes it; nothing is dropped while the editor exists. Parked work runs in
    /// the order it was parked, but it runs after the document changes that came
    /// after it: a handler of parked work sees the document as it is when the work
    /// is delivered, and an editor call it makes delivers the rest of the parked
    /// work first.
    ///
    /// A document has at most one editor: attaching a second one fails with
    /// [`CmError::DocInUse`] (`attachDoc` throws "This document is already in use.")
    /// and leaves the first attached.
    pub fn attach_editor(&self, sink: Rc<dyn EditorSink>) -> Result<(), CmError> {
        let undo_depth = sink.semantics().undo_depth;
        let mut i = self.inner_mut();
        if i.sink.is_some() {
            return Err(CmError::DocInUse);
        }
        i.sink = Some(sink);
        // A request belongs to the editor it was made for (see `detach_editor`); a
        // new editor starts with none, even when the previous one was dropped
        // without detaching.
        i.scroll_requests.clear();
        drop(i);
        // The `undoDepth` option handler runs when the editor is created.
        self.set_undo_depth(undo_depth);
        Ok(())
    }
    /// `detachDoc`: the document has no editor afterwards.
    pub fn detach_editor(&self) {
        let mut i = self.inner_mut();
        i.sink = None;
        // CM5 writes each request into the old editor's `curOp`; none outlives it.
        i.scroll_requests.clear();
    }
    /// The same as [`Doc::detach_editor`].
    pub fn detach_editor_semantics(&self) {
        self.detach_editor();
    }
    /// `cm.operation(f)`: group calls into one operation (only meaningful with
    /// editor semantics).
    pub fn operation<R>(&self, f: impl FnOnce() -> R) -> R {
        self.in_op(f)
    }
    /// `on("beforeChange", f)`.
    pub fn on_before_change<F: Fn(&Doc, &mut BeforeChange) + 'static>(&self, f: F) -> ListenerId {
        self.inner_mut().before_change.on_rc(Rc::new(f))
    }
    /// Remove the document handler registered as `id`, whatever its event.
    /// `false` when no document handler has that id.
    pub fn off(&self, id: ListenerId) -> bool {
        let gone = {
            let mut i = self.inner_mut();
            removed(i.before_change.remove(id))
                .or_else(|| removed(i.before_selection_change.remove(id)))
                .or_else(|| removed(i.change.remove(id)))
                .or_else(|| removed(i.cursor_activity.remove(id)))
                .or_else(|| removed(i.history_added.remove(id)))
        };
        gone.is_some()
    }
    /// `on("change", f)`: called after each change to this document, with the
    /// change as applied here. Delayed (`signalLater`): it runs when the outermost
    /// operation ends, or, with no operation open (no editor), when the host
    /// delivers the orphan queue ([`crate::operation_group::fire_orphan_delayed`]).
    pub fn on_change<F: Fn(&Doc, &ChangeObj) + 'static>(&self, f: F) -> ListenerId {
        self.inner_mut().change.on_rc(Rc::new(f))
    }
    /// `on("cursorActivity", f)`: the selection changed. Delayed like `change`.
    pub fn on_cursor_activity<F: Fn(&Doc) + 'static>(&self, f: F) -> ListenerId {
        self.inner_mut().cursor_activity.on_rc(Rc::new(f))
    }
    /// `on("historyAdded", f)`: a change started a new history event (it was not
    /// merged into the previous one). Immediate.
    pub fn on_history_added<F: Fn(&Doc) + 'static>(&self, f: F) -> ListenerId {
        self.inner_mut().history_added.on_rc(Rc::new(f))
    }
    /// `cm.state.suppressEdits` (src/edit/key_events.js:doHandleBinding sets it while a
    /// key handler runs in a read-only editor). Only honored with editor semantics,
    /// like the JS (`doc.cm && doc.cm.state.suppressEdits`).
    pub fn set_suppress_edits(&self, v: bool) {
        self.inner_mut().suppress_edits = v;
    }
    /// The cursor positions that asked to be scrolled into view since the last call,
    /// oldest first (`setSelectionNoUndo`: `if (!(options && options.scroll === false)
    /// && doc.cm) ensureCursorVisible(doc.cm)`; `replaceSelections` without a replaced
    /// selection). `ensureCursorVisible` reads the cursor when it is called, so each
    /// request keeps its own position. Only with editor semantics.
    pub fn take_scroll_requests(&self) -> Vec<Pos> {
        std::mem::take(&mut self.inner_mut().scroll_requests)
    }
    /// `ensureCursorVisible(doc.cm)`: record the cursor for the view.
    pub(crate) fn request_cursor_visible(&self) {
        let cur = self.get_cursor(None);
        self.inner_mut().scroll_requests.push(cur);
    }
    pub fn suppress_edits(&self) -> bool {
        let i = self.inner();
        i.sink.is_some() && i.suppress_edits
    }
    /// `goalColumn` of each current selection range (`moveV`, src/edit/methods.js).
    pub fn goal_columns(&self) -> Vec<Option<f64>> {
        self.inner()
            .sel
            .ranges
            .iter()
            .map(|r| r.goal_column.get())
            .collect()
    }
    /// `doc.sel.ranges[i].goalColumn = v`.
    pub fn set_goal_column(&self, i: usize, v: Option<f64>) {
        if let Some(r) = self.inner().sel.ranges.get(i) {
            r.goal_column.set(v);
        }
    }
    /// `doc.history.lastSelOrigin = null` (the end of a mouse selection,
    /// src/edit/mouse_events.js:leftButtonSelect `done`).
    pub fn clear_last_sel_origin(&self) {
        self.history_rc().borrow_mut().last_sel_origin = None;
    }
    /// `skipAtomic(doc, pos, oldPos, bias, mayClear)` (src/model/selection_updates.js).
    pub fn skip_atomic(&self, pos: Pos, old_pos: Option<Pos>, bias: i64, may_clear: bool) -> Pos {
        let p = PosRef::new(pos);
        let o = old_pos.map(PosRef::new);
        super::selection_updates::skip_atomic(self, &p, o.as_ref(), Some(bias), may_clear).get()
    }
    /// `replaceOneSelection(doc, i, range, options)` (src/model/selection_updates.js).
    pub fn replace_one_selection(
        &self,
        i: usize,
        anchor: Pos,
        head: Pos,
        options: Option<SelOptions>,
    ) {
        self.in_op(|| {
            let range = if anchor == head {
                let p = PosRef::new(anchor);
                Range::new(p.clone(), p)
            } else {
                Range::new(PosRef::new(anchor), PosRef::new(head))
            };
            super::selection_updates::replace_one_selection(self, i, range, options.as_ref());
        })
    }
    /// `makeChange(doc, {from, to, text, origin})` (src/model/changes.js) with the
    /// positions passed through unclipped, as `applyTextInput` does.
    pub fn make_change_lines(&self, from: Pos, to: Pos, lines: Vec<String>, origin: Option<&str>) {
        let text: Vec<Text> = lines.iter().map(|l| to_u16(l)).collect();
        make_change(
            self,
            Change {
                from,
                to,
                text: Some(text),
                origin: origin.map(|s| s.to_string()),
                full: false,
            },
            false,
        );
    }

    /// `on("beforeSelectionChange", f)`.
    pub fn on_before_selection_change<F: Fn(&Doc, &mut BeforeSelectionChange) + 'static>(
        &self,
        f: F,
    ) -> ListenerId {
        self.inner_mut().before_selection_change.on_rc(Rc::new(f))
    }

    // ----------------------------------------------------------------------------
    // Text

    /// `getValue(lineSep)`.
    pub fn get_value(&self, line_sep: Option<&str>) -> String {
        from_u16(&self.get_value_utf16(line_sep))
    }
    pub fn get_value_utf16(&self, line_sep: Option<&str>) -> Text {
        let lines: Vec<Text> = self.get_value_lines_utf16();
        let sep = match line_sep {
            Some(s) if !s.is_empty() => to_u16(s),
            _ => self.line_separator_u16(),
        };
        join(&lines, &sep)
    }
    /// `getValue(false)`.
    pub fn get_value_lines(&self) -> Vec<String> {
        self.get_value_lines_utf16()
            .iter()
            .map(|l| from_u16(l))
            .collect()
    }
    fn get_value_lines_utf16(&self) -> Vec<Text> {
        let (first, size) = (self.first_line(), self.size() as i64);
        self.iter_lines(first, first + size)
            .iter()
            .map(|l| l.borrow().text.clone())
            .collect()
    }

    /// `setValue(code)`.
    pub fn set_value(&self, code: &str) {
        self.in_op(|| {
            let first = self.first_line();
            let last = self.last_line();
            let top = Pos::new(first, 0);
            let end = Pos::new(last, self.get_line_ref(last).borrow().text.len() as i64);
            let text = self.split_lines_u16(&to_u16(code));
            make_change(
                self,
                Change {
                    from: top,
                    to: end,
                    text: Some(text),
                    origin: Some("setValue".into()),
                    full: true,
                },
                true,
            );
            set_selection(
                self,
                simple_selection(PosRef::new(top), None),
                Some(&dont_scroll()),
            );
        })
    }

    /// `replaceRange(code, from, to, origin)`.
    pub fn replace_range(&self, code: &str, from: Pos, to: Option<Pos>, origin: Option<&str>) {
        let lines = self.split_lines_u16(&to_u16(code));
        self.replace_range_lines_u16(lines, from, to, origin);
    }
    /// `replaceRange(arrayOfLines, from, to, origin)`.
    pub fn replace_range_lines(
        &self,
        lines: &[&str],
        from: Pos,
        to: Option<Pos>,
        origin: Option<&str>,
    ) {
        self.replace_range_lines_u16(lines.iter().map(|l| to_u16(l)).collect(), from, to, origin);
    }
    /// `replaceRange(code, from, to, origin)` with UTF-16 text (keeps lone surrogates).
    pub fn replace_range_utf16(
        &self,
        code: &[u16],
        from: Pos,
        to: Option<Pos>,
        origin: Option<&str>,
    ) {
        let lines = self.split_lines_u16(code);
        self.replace_range_lines_u16(lines, from, to, origin);
    }
    fn replace_range_lines_u16(
        &self,
        lines: Vec<Text>,
        from: Pos,
        to: Option<Pos>,
        origin: Option<&str>,
    ) {
        let from = self.clip_pos(from);
        let to = match to {
            Some(t) => self.clip_pos(t),
            None => from,
        };
        replace_range(self, lines, from, to, origin.map(str::to_string));
    }

    /// `getRange(from, to, lineSep)`. `Some("")` joins without separator.
    pub fn get_range(&self, from: Pos, to: Pos, line_sep: Option<&str>) -> String {
        from_u16(&self.get_range_utf16(from, to, line_sep))
    }
    pub fn get_range_utf16(&self, from: Pos, to: Pos, line_sep: Option<&str>) -> Text {
        let lines = self.get_between(self.clip_pos(from), self.clip_pos(to));
        let sep = match line_sep {
            Some("") => Vec::new(),
            Some(s) => to_u16(s),
            None => self.line_separator_u16(),
        };
        join(&lines, &sep)
    }
    /// `getRange(from, to, false)`.
    pub fn get_range_lines(&self, from: Pos, to: Pos) -> Vec<String> {
        self.get_between(self.clip_pos(from), self.clip_pos(to))
            .iter()
            .map(|l| from_u16(l))
            .collect()
    }

    /// `getLine(n)`: `None` (JS undefined) outside the document.
    pub fn get_line(&self, line: i64) -> Option<String> {
        self.get_line_utf16(line).map(|t| from_u16(&t))
    }
    pub fn get_line_utf16(&self, line: i64) -> Option<Text> {
        self.get_line_handle(line).map(|h| h.text_utf16())
    }
    pub fn get_line_handle(&self, line: i64) -> Option<LineHandle> {
        if self.is_line(line) {
            Some(LineHandle(self.get_line_ref(line)))
        } else {
            None
        }
    }
    /// `getLineNumber(handle)`: `None` once the line has been removed.
    pub fn get_line_number(&self, handle: &LineHandle) -> Option<i64> {
        self.line_no(&handle.0)
    }
    /// `getLineHandleVisualStart(line)`.
    pub fn get_line_handle_visual_start(&self, line: i64) -> Result<LineHandle, CmError> {
        Ok(LineHandle(visual_line(self.try_line_ref(line)?)))
    }
    /// `eachLine(from, to, f)` / `iter`: stops early when `f` returns true.
    pub fn each_line<F: FnMut(&LineHandle) -> bool>(&self, from: i64, to: i64, mut f: F) {
        for l in self.iter_lines(from, to) {
            if f(&LineHandle(l)) {
                break;
            }
        }
    }
    pub fn line_count(&self) -> usize {
        self.size()
    }
    pub fn first_line(&self) -> i64 {
        self.inner().first
    }
    pub fn last_line(&self) -> i64 {
        let i = self.inner();
        i.first + i.tree.size() as i64 - 1
    }

    /// `clipPos(pos)`.
    pub fn clip_pos(&self, pos: Pos) -> Pos {
        let (first, size) = {
            let i = self.inner();
            (i.first, i.tree.size() as i64)
        };
        if pos.line < first {
            return Pos::new(first, 0);
        }
        let last = first + size - 1;
        if pos.line > last {
            return Pos::new(last, self.get_line_ref(last).borrow().text.len() as i64);
        }
        let len = self.get_line_ref(pos.line).borrow().text.len() as i64;
        if pos.ch == CH_END || pos.ch > len {
            Pos::new(pos.line, len)
        } else if pos.ch < 0 {
            Pos::new(pos.line, 0)
        } else {
            pos
        }
    }

    // ----------------------------------------------------------------------------
    // Selection

    /// `getCursor(start)`: `None`/`"head"` → head, `"anchor"`, `"end"`/`"to"` → end
    /// (also what `false` means in the JS), anything else → start.
    pub fn get_cursor(&self, start: Option<&str>) -> Pos {
        let sel = self.inner().sel.clone();
        let range = sel.primary();
        match start {
            None | Some("head") => range.head.get(),
            Some("anchor") => range.anchor.get(),
            Some("end") | Some("to") => range.to().get(),
            _ => range.from().get(),
        }
    }
    pub fn list_selections(&self) -> Vec<SelRange> {
        self.selection().ranges
    }
    /// The full selection, including the primary index.
    pub fn selection(&self) -> Selection {
        self.inner().sel.snapshot()
    }
    pub fn something_selected(&self) -> bool {
        self.inner().sel.something_selected()
    }

    /// `setCursor(pos, options)`.
    pub fn set_cursor(&self, pos: Pos, options: Option<SelOptions>) {
        self.in_op(|| {
            let p = self.pos_arg(pos);
            set_simple_selection(self, p, None, options.as_ref());
        })
    }
    /// `setSelection(anchor, head, options)`; `head == None` is a cursor.
    pub fn set_selection(&self, anchor: Pos, head: Option<Pos>, options: Option<SelOptions>) {
        self.in_op(|| {
            let a = PosRef::new(anchor);
            let h = head.map(PosRef::new).unwrap_or_else(|| a.clone());
            let (a, h) = (self.clip_pos_ref(&a), self.clip_pos_ref(&h));
            set_simple_selection(self, a, Some(h), options.as_ref());
        })
    }
    /// `extendSelection(head, other, options)`.
    pub fn extend_selection(&self, head: Pos, other: Option<Pos>, options: Option<SelOptions>) {
        self.in_op(|| {
            let h = self.pos_arg(head);
            let o = other.map(|o| self.pos_arg(o));
            extend_selection(self, h, o, options.as_ref(), None);
        })
    }
    /// `extendSelections(heads, options)`.
    pub fn extend_selections(&self, heads: &[Pos], options: Option<SelOptions>) {
        self.in_op(|| {
            let hs = heads.iter().map(|&h| self.pos_arg(h)).collect();
            extend_selections(self, hs, options.as_ref());
        })
    }
    /// `extendSelectionsBy(f, options)`.
    pub fn extend_selections_by<F: Fn(&SelRange) -> Pos>(&self, f: F, options: Option<SelOptions>) {
        let heads: Vec<Pos> = self.list_selections().iter().map(f).collect();
        self.extend_selections(&heads, options);
    }
    /// `setSelections(ranges, primary, options)`; each range is `(anchor, head)` with
    /// `head == None` meaning a cursor. An empty slice does nothing.
    pub fn set_selections(
        &self,
        ranges: &[(Pos, Option<Pos>)],
        primary: Option<usize>,
        options: Option<SelOptions>,
    ) {
        if ranges.is_empty() {
            return;
        }
        self.in_op(|| {
            let out: Vec<Range> = ranges
                .iter()
                .map(|&(a, h)| {
                    let a = PosRef::new(a);
                    let h = h.map(PosRef::new).unwrap_or_else(|| a.clone());
                    Range::new(self.clip_pos_ref(&a), self.clip_pos_ref(&h))
                })
                .collect();
            let primary =
                primary.unwrap_or_else(|| (ranges.len() - 1).min(self.inner().sel.prim_index));
            let sel = normalize_selection(self.selections_may_touch(), out, primary);
            set_selection(self, sel, options.as_ref());
        })
    }
    /// `addSelection(anchor, head, options)`.
    pub fn add_selection(&self, anchor: Pos, head: Option<Pos>, options: Option<SelOptions>) {
        self.in_op(|| {
            let mut ranges = self.inner().sel.ranges.clone();
            let a = PosRef::new(anchor);
            let h = head.map(PosRef::new).unwrap_or_else(|| a.clone());
            ranges.push(Range::new(self.clip_pos_ref(&a), self.clip_pos_ref(&h)));
            let n = ranges.len();
            let sel = normalize_selection(self.selections_may_touch(), ranges, n - 1);
            set_selection(self, sel, options.as_ref());
        })
    }
    /// The `selectAll` command.
    pub fn select_all(&self) {
        self.in_op(|| select_all(self))
    }

    /// `getSelection(lineSep)`.
    pub fn get_selection(&self, line_sep: Option<&str>) -> String {
        from_u16(&self.get_selection_utf16(line_sep))
    }
    pub fn get_selection_utf16(&self, line_sep: Option<&str>) -> Text {
        let sel = self.inner().sel.clone();
        let mut lines: Vec<Text> = Vec::new();
        for r in &sel.ranges {
            lines.extend(self.get_between(r.from().get(), r.to().get()));
        }
        let sep = match line_sep {
            Some(s) if !s.is_empty() => to_u16(s),
            _ => self.line_separator_u16(),
        };
        join(&lines, &sep)
    }
    /// `getSelections(lineSep)`.
    pub fn get_selections(&self, line_sep: Option<&str>) -> Vec<String> {
        self.get_selections_utf16(line_sep)
            .iter()
            .map(|t| from_u16(t))
            .collect()
    }
    pub fn get_selections_utf16(&self, line_sep: Option<&str>) -> Vec<Text> {
        let sel = self.inner().sel.clone();
        let sep = match line_sep {
            Some(s) if !s.is_empty() => to_u16(s),
            _ => self.line_separator_u16(),
        };
        sel.ranges
            .iter()
            .map(|r| join(&self.get_between(r.from().get(), r.to().get()), &sep))
            .collect()
    }
    /// `replaceSelection(code, collapse, origin)`; origin defaults to `"+input"`.
    pub fn replace_selection(&self, code: &str, collapse: Option<&str>, origin: Option<&str>) {
        let n = self.inner().sel.ranges.len();
        let codes: Vec<&str> = vec![code; n];
        let origin = match origin {
            Some(o) if !o.is_empty() => o,
            _ => "+input",
        };
        self.replace_selections(&codes, collapse, Some(origin));
    }
    /// `replaceSelections(code, collapse, origin)`; collapse is `"around"`, `"start"`
    /// or `"end"`/`None`.
    pub fn replace_selections(&self, code: &[&str], collapse: Option<&str>, origin: Option<&str>) {
        self.in_op(|| {
            let sel = self.inner().sel.clone();
            let changes: Vec<Change> = sel
                .ranges
                .iter()
                .enumerate()
                .map(|(i, r)| Change {
                    from: r.from().get(),
                    to: r.to().get(),
                    text: Some(self.split_lines_u16(&to_u16(code[i]))),
                    origin: origin.map(str::to_string),
                    full: false,
                })
                .collect();
            let new_sel = match collapse {
                Some(c) if !c.is_empty() && c != "end" => {
                    Some(compute_replaced_sel(self, &changes, c))
                }
                _ => None,
            };
            for change in changes.into_iter().rev() {
                make_change(self, change, false);
            }
            if let Some(ns) = new_sel {
                set_selection_replace_history(self, ns, None);
            } else if self.has_editor() {
                // `else if (this.cm) ensureCursorVisible(this.cm)`, applied by the view
                // like the request of `setSelectionNoUndo` (see `take_scroll_requests`).
                self.request_cursor_visible();
            }
        })
    }

    // ----------------------------------------------------------------------------
    // History

    pub fn undo(&self) {
        self.in_op(|| make_change_from_history(self, true, false))
    }
    pub fn redo(&self) {
        self.in_op(|| make_change_from_history(self, false, false))
    }
    pub fn undo_selection(&self) {
        self.in_op(|| make_change_from_history(self, true, true))
    }
    pub fn redo_selection(&self) {
        self.in_op(|| make_change_from_history(self, false, true))
    }
    pub fn set_extending(&self, val: bool) {
        self.inner_mut().extend = val;
    }
    pub fn get_extending(&self) -> bool {
        self.inner().extend
    }
    /// `historySize()`.
    pub fn history_size(&self) -> HistorySize {
        let h = self.history_rc();
        let h = h.borrow();
        HistorySize {
            undo: h.done.iter().filter(|e| !e.is_sel()).count(),
            redo: h.undone.iter().filter(|e| !e.is_sel()).count(),
        }
    }
    /// `clearHistory()`: also resets the history of documents sharing it.
    pub fn clear_history(&self) {
        let new = {
            let old = self.history_rc();
            let o = old.borrow();
            Rc::new(RefCell::new(History::new(Some(&o))))
        };
        self.inner_mut().history = new.clone();
        for (d, _) in self.linked_docs(true) {
            d.inner_mut().history = new.clone();
        }
    }
    pub fn mark_clean(&self) {
        let g = self.change_generation(true);
        self.inner_mut().clean_generation = g;
    }
    pub fn change_generation(&self, force_split: bool) -> u64 {
        let h = self.history_rc();
        let mut h = h.borrow_mut();
        if force_split {
            h.last_op = None;
            h.last_sel_op = None;
            h.last_origin = None;
        }
        h.generation
    }
    /// `isClean(gen)`; `None` (or 0) uses the generation of the last `markClean`.
    pub fn is_clean(&self, generation: Option<u64>) -> bool {
        let g = match generation {
            Some(g) if g != 0 => g,
            _ => self.inner().clean_generation,
        };
        self.history_rc().borrow().generation == g
    }
    pub fn get_history(&self) -> HistoryData {
        let h = self.history_rc();
        let h = h.borrow();
        HistoryData {
            done: events_to_data(&h.done),
            undone: events_to_data(&h.undone),
        }
    }
    /// `setHistory(data)`. Like the JS, this gives *only this document* a new history
    /// object, even when it was shared with linked documents.
    pub fn set_history(&self, data: &HistoryData) {
        let new = {
            let old = self.history_rc();
            let o = old.borrow();
            History::new(Some(&o))
        };
        let new = Rc::new(RefCell::new(new));
        {
            let mut h = new.borrow_mut();
            h.done = data_to_events(&data.done);
            h.undone = data_to_events(&data.undone);
        }
        self.inner_mut().history = new;
    }
    /// `doc.history.undoDepth = n` (the `undoDepth` editor option).
    pub fn set_undo_depth(&self, depth: f64) {
        self.history_rc().borrow_mut().undo_depth = depth;
    }
    pub fn undo_depth(&self) -> f64 {
        self.history_rc().borrow().undo_depth
    }

    // ----------------------------------------------------------------------------
    // Markers

    /// `markText(from, to, options)`.
    pub fn mark_text(&self, from: Pos, to: Pos, options: MarkOptions) -> Result<Marker, CmError> {
        let (f, t) = (self.clip_pos(from), self.clip_pos(to));
        mark_text(self, f, t, &options, MarkerType::Range)
    }
    /// `setBookmark(pos, {insertLeft, shared})`. Widgets are view-only.
    pub fn set_bookmark(
        &self,
        pos: Pos,
        insert_left: bool,
        shared: bool,
    ) -> Result<Marker, CmError> {
        let opts = MarkOptions {
            insert_left,
            clear_when_empty: Some(false),
            shared,
            ..Default::default()
        };
        let p = self.clip_pos(pos);
        mark_text(self, p, p, &opts, MarkerType::Bookmark)
    }
    /// `findMarksAt(pos)`.
    pub fn find_marks_at(&self, pos: Pos) -> Vec<Marker> {
        let pos = self.clip_pos(pos);
        let spans = self
            .get_line_ref(pos.line)
            .borrow()
            .marked_spans
            .as_ref()
            .map(|a| a.get());
        let mut markers = Vec::new();
        for span in spans.iter().flatten() {
            if span.from.is_none_or(|f| f <= pos.ch) && span.to.is_none_or(|t| t >= pos.ch) {
                markers.push(Marker::outer(&span.marker));
            }
        }
        markers
    }
    /// `findMarks(from, to, filter)`.
    pub fn find_marks(
        &self,
        from: Pos,
        to: Pos,
        filter: Option<&dyn Fn(&TextMarker) -> bool>,
    ) -> Vec<Marker> {
        self.find_marks_filtered(from, to, filter)
    }
    /// `getAllMarks()`: note this returns the per-document markers, not their shared
    /// parents, and (CM5 bug, see `iter_all_js`) skips the first `first` lines.
    pub fn get_all_marks(&self) -> Vec<TextMarker> {
        let mut markers = Vec::new();
        for line in self.iter_all_js() {
            for s in line
                .borrow()
                .marked_spans
                .as_ref()
                .map(|a| a.get())
                .iter()
                .flatten()
            {
                if s.from.is_some() {
                    markers.push(s.marker.clone());
                }
            }
        }
        markers
    }

    // ----------------------------------------------------------------------------
    // Index <-> position

    /// `posFromIndex(off)`. Wrong for documents whose first line is not 0 (CM5 bug,
    /// see `iter_all_js`); kept for fidelity.
    pub fn pos_from_index(&self, off: i64) -> Pos {
        let mut off = off;
        let mut line_no = self.first_line();
        let sep_size = self.line_separator_u16().len() as i64;
        let mut ch = CH_END;
        for line in self.iter_all_js() {
            let sz = line.borrow().text.len() as i64 + sep_size;
            if sz > off {
                ch = off;
                break;
            }
            off -= sz;
            line_no += 1;
        }
        self.clip_pos(Pos::new(line_no, ch))
    }
    /// `indexFromPos(pos)`.
    pub fn index_from_pos(&self, coords: Pos) -> i64 {
        let coords = self.clip_pos(coords);
        let mut index = coords.ch;
        if coords.line < self.first_line() || coords.ch < 0 {
            return 0;
        }
        let sep_size = self.line_separator_u16().len() as i64;
        for line in self.iter_lines(self.first_line(), coords.line) {
            index += line.borrow().text.len() as i64 + sep_size;
        }
        index
    }

    // ----------------------------------------------------------------------------
    // Copies and linked documents

    /// `copy(copyHistory)`. The copy shares the selection object.
    pub fn copy(&self, copy_history: bool) -> Doc {
        let (first, size) = (self.first_line(), self.size() as i64);
        let lines: Vec<Text> = self
            .iter_lines(first, first + size)
            .iter()
            .map(|l| l.borrow().text.clone())
            .collect();
        let (mode, sep, dir) = {
            let i = self.inner();
            (i.mode_option.clone(), i.line_sep.clone(), i.direction)
        };
        let doc = Doc::construct(Some(lines), "", mode, first, sep, dir);
        {
            let sel = self.inner().sel.clone();
            let mut d = doc.inner_mut();
            d.sel = sel;
            d.extend = false;
        }
        if copy_history {
            doc.set_undo_depth(self.undo_depth());
            doc.set_history(&self.get_history());
        }
        doc
    }

    /// `linkedDoc(options)`. Errors only in the JS-`TypeError` situation described
    /// at `copySharedMarkers` (a shared bookmark, or a shared marker whose primary
    /// lost its text); the new document is linked even then.
    pub fn linked_doc(&self, options: LinkedDocOptions) -> Result<Doc, CmError> {
        let (mut from, mut to) = (self.first_line(), self.first_line() + self.size() as i64);
        if let Some(f) = options.from {
            if f > from {
                from = f;
            }
        }
        if let Some(t) = options.to {
            if t < to {
                to = t;
            }
        }
        let lines: Vec<Text> = self
            .iter_lines(from, to)
            .iter()
            .map(|l| l.borrow().text.clone())
            .collect();
        let (mode, sep, dir) = {
            let i = self.inner();
            (
                options.mode.clone().or_else(|| i.mode_option.clone()),
                i.line_sep.clone(),
                i.direction,
            )
        };
        let copy = Doc::construct(Some(lines), "", mode, from, sep, dir);
        if options.shared_hist {
            let h = self.history_rc();
            copy.inner_mut().history = h;
        }
        self.inner_mut().linked.push(Link {
            doc: copy.clone(),
            is_parent: false,
            shared_hist: options.shared_hist,
        });
        copy.inner_mut().linked = vec![Link {
            doc: self.clone(),
            is_parent: true,
            shared_hist: options.shared_hist,
        }];
        copy_shared_markers(&copy, &find_shared_markers(self))?;
        Ok(copy)
    }

    /// `unlinkDoc(other)`.
    pub fn unlink_doc(&self, other: &Doc) {
        let idx = self.inner().linked.iter().position(|l| l.doc.ptr_eq(other));
        if let Some(i) = idx {
            self.inner_mut().linked.remove(i);
            other.unlink_doc(self);
            detach_shared_markers(&find_shared_markers(self));
        }
        // If the histories were shared, split them again.
        if Rc::ptr_eq(&other.history_rc(), &self.history_rc()) {
            let mut split_ids = vec![other.id()];
            for (d, _) in other.linked_docs(true) {
                split_ids.push(d.id());
            }
            // Order matters when `other` is `self`: the JS assigns the new history
            // first and then copies from `this.history`, i.e. from the new, empty one.
            let new = Rc::new(RefCell::new(History::new(None)));
            other.inner_mut().history = new.clone();
            let src = self.history_rc();
            let (done, undone) = if Rc::ptr_eq(&src, &new) {
                (Vec::new(), Vec::new())
            } else {
                let mut h = src.borrow_mut();
                (
                    copy_history_array(&mut h.done, Some(&split_ids), false),
                    copy_history_array(&mut h.undone, Some(&split_ids), false),
                )
            };
            let mut n = new.borrow_mut();
            n.done = done;
            n.undone = undone;
        }
    }

    /// `iterLinkedDocs(f)`: `f(doc, sharedHist)`.
    pub fn iter_linked_docs<F: FnMut(&Doc, bool)>(&self, mut f: F) {
        for (d, s) in self.linked_docs(false) {
            f(&d, s);
        }
    }

    /// `splitLines(str)`.
    pub fn split_lines(&self, s: &str) -> Vec<String> {
        self.split_lines_u16(&to_u16(s))
            .iter()
            .map(|l| from_u16(l))
            .collect()
    }
    /// `lineSeparator()`.
    pub fn line_separator(&self) -> String {
        from_u16(&self.line_separator_u16())
    }
    pub(crate) fn line_separator_u16(&self) -> Text {
        match &self.inner().line_sep {
            Some(s) if !s.is_empty() => s.clone(),
            _ => vec![0x0a],
        }
    }
    /// The `lineSeparator` editor option.
    pub fn set_line_separator(&self, sep: Option<&str>) {
        self.inner_mut().line_sep = sep.map(to_u16);
    }
    /// `cm.setOption("lineSeparator", val)` (`edit/options.js`): besides setting the
    /// separator, the option handler splits every existing line at occurrences of
    /// the new separator, last to first, with plain `replaceRange` calls.
    pub fn set_line_separator_option(&self, sep: Option<&str>) {
        self.in_op(|| {
            self.set_line_separator(sep);
            let Some(val) = sep.map(to_u16).filter(|v| !v.is_empty()) else {
                return;
            };
            let mut breaks = Vec::new();
            let mut line_no = self.first_line();
            self.each_line(
                self.first_line(),
                self.first_line() + self.line_count() as i64,
                |h| {
                    let text = h.text_utf16();
                    let mut pos = 0;
                    while let Some(found) = crate::text::index_of(&text, &val, pos) {
                        pos = found + val.len();
                        breaks.push(Pos::new(line_no, found as i64));
                    }
                    line_no += 1;
                    false
                },
            );
            let val_s = from_u16(&val);
            for b in breaks.iter().rev() {
                self.replace_range(
                    &val_s,
                    *b,
                    Some(Pos::new(b.line, b.ch + val.len() as i64)),
                    None,
                );
            }
        })
    }
    pub fn mode_option(&self) -> Option<String> {
        self.inner().mode_option.clone()
    }
    pub fn direction(&self) -> Direction {
        self.inner().direction
    }
    /// `setDirection(dir)`. (Bidi order caches live in the view.)
    pub fn set_direction(&self, dir: Direction) {
        self.inner_mut().direction = dir;
    }
    /// `doc.cantEdit` — set when no cursor position outside atomic markers exists.
    pub fn cant_edit(&self) -> bool {
        self.inner().cant_edit
    }

    // ----------------------------------------------------------------------------
    // Line queries from `src/line/spans.js` that need no view

    /// `heightAtLine(line)` in line-height units (every visible line is 1 high).
    pub fn height_at_line(&self, line: i64) -> f64 {
        height_at_line(self, self.get_line_ref(line))
    }
    pub fn line_is_hidden(&self, line: i64) -> bool {
        line_is_hidden(self, &self.get_line_ref(line))
    }
    pub fn visual_line_no(&self, line: i64) -> i64 {
        visual_line_no(self, line)
    }
    pub fn visual_line_end_no(&self, line: i64) -> i64 {
        visual_line_end_no(self, line)
    }
    /// `lineLength(line)` taking collapsed ranges into account.
    pub fn line_length(&self, line: i64) -> i64 {
        line_length(&self.get_line_ref(line))
    }
    /// `lineAtHeight(doc, h)`.
    pub fn line_at_height(&self, h: f64) -> i64 {
        let i = self.inner();
        i.tree.line_at_height(h) as i64 + i.first
    }

    /// Debug aid: the marked spans of every line, as `(marker, from, to)`.
    #[doc(hidden)]
    pub fn debug_spans(&self) -> Vec<Vec<(TextMarker, Option<i64>, Option<i64>)>> {
        let (first, size) = (self.first_line(), self.size() as i64);
        self.iter_lines(first, first + size)
            .iter()
            .map(|l| {
                l.borrow()
                    .marked_spans
                    .as_ref()
                    .map(|a| a.get())
                    .iter()
                    .flatten()
                    .map(|s| (s.marker.clone(), s.from, s.to))
                    .collect()
            })
            .collect()
    }

    /// Debug aid: lines actually reachable in the tree (can differ from
    /// `line_count()` after CM5-faithful corruption).
    #[doc(hidden)]
    pub fn debug_actual_line_count(&self) -> usize {
        self.inner().tree.lines_range(0, usize::MAX / 2).len()
    }

    /// Debug aid: (lastOrigin, lastSelOrigin, generation, maxGeneration, done.len,
    /// undone.len, undoDepth).
    #[doc(hidden)]
    pub fn debug_history_state(
        &self,
    ) -> (Option<String>, Option<String>, u64, u64, usize, usize, f64) {
        let h = self.history_rc();
        let h = h.borrow();
        (
            h.last_origin.clone(),
            h.last_sel_origin.clone(),
            h.generation,
            h.max_generation,
            h.done.len(),
            h.undone.len(),
            h.undo_depth,
        )
    }

    #[doc(hidden)]
    pub fn check_tree(&self) -> Result<(), String> {
        self.inner().tree.check_invariants()
    }
    #[doc(hidden)]
    pub fn tree_depth(&self) -> usize {
        self.inner().tree.depth()
    }
    #[doc(hidden)]
    pub fn linked_count(&self) -> usize {
        self.inner().linked.len()
    }
}

pub(crate) fn dont_scroll() -> SelOptions {
    SelOptions {
        scroll: Some(false),
        ..Default::default()
    }
}
