//! `src/model/mark_text.js`: `TextMarker`, `SharedTextMarker`, `markText`, and the
//! shared-marker plumbing for linked documents.
//!
//! Not ported: `replacedWith`/widget nodes, `changed()`, the `attributes` option, and
//! the editor events `markerAdded`/`markerCleared`/`markerChanged`. `className`,
//! `startStyle`, `endStyle`, `css` and `title` are stored but nothing draws them.
//! The marker `clear` handlers (`on_clear`) run synchronously at the end of
//! `clear()`; the JS defers them with `signalLater`. There are no user
//! `beforeCursorEnter`/`hide`/`unhide` listeners (only `clearOnEnter` is honored).

use std::cell::{Ref, RefCell, RefMut};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use super::changes::Change;
use super::doc::{Doc, DocInner};
use super::history::add_change_to_history;
use super::line::LineRef;
use super::selection_updates::re_check_selection;
use super::spans::{
    MarkedSpan, add_marked_span, conflicting_collapsed_range, get_marked_span_for, line_is_hidden,
    remove_marked_span,
};
use crate::CmError;
use crate::pos::{Pos, cmp};

/// `nextMarkerId` — module-global in the JS.
static NEXT_MARKER_ID: AtomicU64 = AtomicU64::new(0);

fn next_marker_id() -> u64 {
    NEXT_MARKER_ID.fetch_add(1, Ordering::Relaxed) + 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerType {
    Range,
    Bookmark,
}

/// Options accepted by `markText` / `setBookmark`. `Option<bool>` fields are the ones
/// the JS tests for presence or for `!== false`.
#[derive(Clone, Debug, Default)]
pub struct MarkOptions {
    pub class_name: Option<String>,
    pub start_style: Option<String>,
    pub end_style: Option<String>,
    pub css: Option<String>,
    pub title: Option<String>,
    pub inclusive_left: bool,
    pub inclusive_right: bool,
    pub atomic: bool,
    pub read_only: bool,
    /// Default (None) behaves like `true`: empty markers are dropped.
    pub clear_when_empty: Option<bool>,
    pub add_to_history: bool,
    pub collapsed: bool,
    pub clear_on_enter: bool,
    /// `"selectLeft" in marker` decides which flag `skipAtomic` consults.
    pub select_left: Option<bool>,
    pub select_right: Option<bool>,
    pub insert_left: bool,
    pub shared: bool,
    pub handle_mouse_events: bool,
}

type ClearHandler = Rc<dyn Fn(Option<(Pos, Option<Pos>)>)>;

pub(crate) struct MarkerData {
    pub id: u64,
    pub kind: MarkerType,
    pub doc: Weak<RefCell<DocInner>>,
    pub lines: Vec<Weak<RefCell<super::line::Line>>>,
    pub opts: MarkOptions,
    pub atomic: bool,
    pub collapsed: bool,
    pub explicitly_cleared: bool,
    pub parent: Option<SharedTextMarker>,
    pub clear_handlers: Vec<ClearHandler>,
}

/// A marker in one document (`TextMarker`). Clones share the marker.
#[derive(Clone)]
pub struct TextMarker(pub(crate) Rc<RefCell<MarkerData>>);

/// Result of [`TextMarker::find_lines`] (`find(side, lineObj = true)`).
pub(crate) enum FindL {
    None,
    Point(LineRef, i64),
    /// `{from, to}`; `to` is `undefined` when no span ends.
    Range(LineRef, i64, Option<(LineRef, i64)>),
}

/// Result of `find(-1)` / `find(1)`.
pub(crate) enum SideFind {
    Pos(Pos),
    Object,
    None,
}

/// What `marker.find()` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerFind {
    /// `to` is `None` only for a marker whose end span got lost (the JS then
    /// returns `{from, to: undefined}`).
    Range {
        from: Pos,
        to: Option<Pos>,
    },
    Point(Pos),
}

impl TextMarker {
    pub(crate) fn data(&self) -> Ref<'_, MarkerData> {
        self.0.borrow()
    }
    pub(crate) fn data_mut(&self) -> RefMut<'_, MarkerData> {
        self.0.borrow_mut()
    }
    pub fn ptr_eq(&self, other: &TextMarker) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub fn id(&self) -> u64 {
        self.data().id
    }
    pub fn kind(&self) -> MarkerType {
        self.data().kind
    }
    pub fn options(&self) -> MarkOptions {
        self.data().opts.clone()
    }
    pub fn is_atomic(&self) -> bool {
        self.data().atomic
    }
    pub fn is_collapsed(&self) -> bool {
        self.data().collapsed
    }
    pub fn explicitly_cleared(&self) -> bool {
        self.data().explicitly_cleared
    }
    pub fn parent(&self) -> Option<SharedTextMarker> {
        self.data().parent.clone()
    }
    pub fn doc(&self) -> Option<Doc> {
        self.data().doc.upgrade().map(Doc)
    }

    /// `CodeMirror.on(marker, "clear", f)`. Called synchronously at the end of
    /// `clear()` with the range the marker covered (the JS defers it with
    /// `signalLater`).
    pub fn on_clear<F: Fn(Option<(Pos, Option<Pos>)>) + 'static>(&self, f: F) {
        self.data_mut().clear_handlers.push(Rc::new(f));
    }

    /// Debug aid: line numbers of `marker.lines` (None for a detached line).
    #[doc(hidden)]
    pub fn debug_line_numbers(&self) -> Vec<Option<i64>> {
        let lines: Vec<_> = self.data().lines.clone();
        let doc = self.doc();
        lines
            .iter()
            .map(|l| {
                l.upgrade()
                    .and_then(|l| doc.as_ref().and_then(|d| d.line_no(&l)))
            })
            .collect()
    }

    pub(crate) fn attach_line(&self, line: &LineRef) {
        self.data_mut().lines.push(Rc::downgrade(line));
    }

    /// `detachLine(line)`. Keeps the JS quirk: `splice(indexOf(...), 1)` with a
    /// missing line removes the *last* entry.
    pub(crate) fn detach_line(&self, line: &LineRef) {
        let mut d = self.data_mut();
        let idx = d
            .lines
            .iter()
            .position(|l| l.upgrade().is_some_and(|l| Rc::ptr_eq(&l, line)));
        match idx {
            Some(i) => {
                d.lines.remove(i);
            }
            None => {
                d.lines.pop();
            }
        }
    }

    fn live_lines(&self) -> Vec<LineRef> {
        self.data()
            .lines
            .iter()
            .filter_map(|l| l.upgrade())
            .collect()
    }

    /// `find(side, true)` with line objects. `side`: 0 both, -1 left, 1 right.
    pub(crate) fn find_lines(&self, side: i32) -> FindL {
        let mut from: Option<(LineRef, i64)> = None;
        let mut to: Option<(LineRef, i64)> = None;
        for line in self.live_lines() {
            let span = get_marked_span_for(line.borrow().marked_spans.as_ref(), self);
            let Some(span) = span else { continue };
            if let Some(f) = span.from {
                from = Some((line.clone(), f));
                if side == -1 {
                    return FindL::Point(line, f);
                }
            }
            if let Some(t) = span.to {
                to = Some((line.clone(), t));
                if side == 1 {
                    return FindL::Point(line, t);
                }
            }
        }
        match from {
            Some((l, f)) => FindL::Range(l, f, to),
            None => FindL::None,
        }
    }

    fn line_no(&self, line: &LineRef) -> i64 {
        let doc = self.doc().expect("marker document alive");
        doc.line_no(line).expect("marked line attached")
    }

    /// `find(side)` with line numbers, for side -1/1. With side 1 and no span end the
    /// JS falls through and returns a `{from, to}` object instead of a position
    /// ([`SideFind::Object`]); callers that then read `.line` get `undefined`.
    pub(crate) fn find_side(&self, side: i32) -> SideFind {
        match self.find_lines(side) {
            FindL::Point(l, ch) => SideFind::Pos(Pos::new(self.line_no(&l), ch)),
            FindL::Range(..) => SideFind::Object,
            FindL::None => SideFind::None,
        }
    }

    /// `find(0)`: `{from, to}` with a possibly undefined `to`.
    pub(crate) fn find_range_raw(&self) -> Option<(Pos, Option<Pos>)> {
        match self.find_lines(0) {
            FindL::Range(fl, fch, to) => Some((
                Pos::new(self.line_no(&fl), fch),
                to.map(|(tl, tch)| Pos::new(self.line_no(&tl), tch)),
            )),
            _ => None,
        }
    }

    /// `find(0)` where the caller dereferences `to` (TypeError in the JS if missing).
    pub(crate) fn find_range(&self) -> Option<(Pos, Pos)> {
        self.find_range_raw().map(|(f, t)| {
            (
                f,
                t.expect("JS TypeError: Cannot read properties of undefined (reading 'line')"),
            )
        })
    }

    /// `marker.find()`: bookmarks report a single position, ranges `{from, to}`.
    pub fn find(&self) -> Option<MarkerFind> {
        if self.kind() == MarkerType::Bookmark {
            match self.find_lines(1) {
                FindL::Point(l, ch) => Some(MarkerFind::Point(Pos::new(self.line_no(&l), ch))),
                FindL::Range(fl, fch, to) => Some(MarkerFind::Range {
                    from: Pos::new(self.line_no(&fl), fch),
                    to: to.map(|(tl, tch)| Pos::new(self.line_no(&tl), tch)),
                }),
                FindL::None => None,
            }
        } else {
            self.find_range_raw()
                .map(|(from, to)| MarkerFind::Range { from, to })
        }
    }

    /// `marker.clear()`.
    pub fn clear(&self) {
        if self.data().explicitly_cleared {
            return;
        }
        let found = if self.data().clear_handlers.is_empty() {
            None
        } else {
            self.find_range_raw()
        };
        for line in self.live_lines() {
            let spans = line.borrow().marked_spans.clone();
            if let Some(spans) = spans {
                line.borrow_mut().marked_spans = remove_marked_span(&spans, self);
            }
        }
        let doc = self.doc();
        let (atomic, parent, handlers) = {
            let mut d = self.data_mut();
            d.lines.clear();
            d.explicitly_cleared = true;
            (d.atomic, d.parent.clone(), d.clear_handlers.clone())
        };
        if atomic {
            if let Some(doc) = &doc {
                if doc.inner().cant_edit {
                    doc.inner_mut().cant_edit = false;
                    if doc.has_editor() {
                        re_check_selection(doc);
                    }
                }
            }
        }
        for h in handlers {
            h(found);
        }
        if let Some(p) = parent {
            p.clear();
        }
    }
}

impl PartialEq for TextMarker {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl std::fmt::Debug for TextMarker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextMarker#{}", self.data().id)
    }
}

pub(crate) struct SharedData {
    pub markers: Vec<TextMarker>,
    pub primary: TextMarker,
    pub explicitly_cleared: bool,
    pub clear_handlers: Vec<Rc<dyn Fn()>>,
}

/// A marker spanning linked documents (`SharedTextMarker`).
#[derive(Clone)]
pub struct SharedTextMarker(pub(crate) Rc<RefCell<SharedData>>);

impl SharedTextMarker {
    pub fn ptr_eq(&self, other: &SharedTextMarker) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub fn markers(&self) -> Vec<TextMarker> {
        self.0.borrow().markers.clone()
    }
    pub fn primary(&self) -> TextMarker {
        self.0.borrow().primary.clone()
    }
    pub fn on_clear<F: Fn() + 'static>(&self, f: F) {
        self.0.borrow_mut().clear_handlers.push(Rc::new(f));
    }
    pub fn clear(&self) {
        if self.0.borrow().explicitly_cleared {
            return;
        }
        self.0.borrow_mut().explicitly_cleared = true;
        let markers = self.markers();
        for m in markers {
            m.clear();
        }
        let handlers = self.0.borrow().clear_handlers.clone();
        for h in handlers {
            h();
        }
    }
    pub fn find(&self) -> Option<MarkerFind> {
        self.primary().find()
    }
}

impl PartialEq for SharedTextMarker {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl std::fmt::Debug for SharedTextMarker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharedTextMarker(primary #{})", self.primary().id())
    }
}

/// Either kind of marker, as returned by `markText` and the `findMarks*` queries.
#[derive(Clone, Debug, PartialEq)]
pub enum Marker {
    Text(TextMarker),
    Shared(SharedTextMarker),
}

impl Marker {
    pub fn clear(&self) {
        match self {
            Marker::Text(m) => m.clear(),
            Marker::Shared(m) => m.clear(),
        }
    }
    pub fn find(&self) -> Option<MarkerFind> {
        match self {
            Marker::Text(m) => m.find(),
            Marker::Shared(m) => m.find(),
        }
    }
    /// The underlying per-document marker (the primary one for shared markers).
    pub fn text_marker(&self) -> TextMarker {
        match self {
            Marker::Text(m) => m.clone(),
            Marker::Shared(m) => m.primary(),
        }
    }
    /// `span.marker.parent || span.marker`.
    pub(crate) fn outer(m: &TextMarker) -> Marker {
        match m.parent() {
            Some(p) => Marker::Shared(p),
            None => Marker::Text(m.clone()),
        }
    }
}

/// `markText(doc, from, to, options, type)`; `from`/`to` are already clipped.
pub(crate) fn mark_text(
    doc: &Doc,
    from: Pos,
    to: Pos,
    options: &MarkOptions,
    kind: MarkerType,
) -> Result<Marker, CmError> {
    if options.shared {
        return mark_text_shared(doc, from, to, options, kind).map(Marker::Shared);
    }
    doc.in_op(|| mark_text_inner(doc, from, to, options.clone(), options.atomic, kind))
        .map(Marker::Text)
}

pub(crate) fn mark_text_inner(
    doc: &Doc,
    from: Pos,
    to: Pos,
    opts: MarkOptions,
    atomic: bool,
    kind: MarkerType,
) -> Result<TextMarker, CmError> {
    let marker = TextMarker(Rc::new(RefCell::new(MarkerData {
        id: next_marker_id(),
        kind,
        doc: Rc::downgrade(&doc.0),
        lines: Vec::new(),
        collapsed: opts.collapsed,
        opts,
        atomic,
        explicitly_cleared: false,
        parent: None,
        clear_handlers: Vec::new(),
    })));
    let diff = cmp(&from, &to);
    // Don't connect empty markers unless clearWhenEmpty is false.
    if diff.is_gt() || diff.is_eq() && marker.data().opts.clear_when_empty != Some(false) {
        return Ok(marker);
    }
    let (collapsed, il, ir) = {
        let d = marker.data();
        (d.collapsed, d.opts.inclusive_left, d.opts.inclusive_right)
    };
    if collapsed
        && (conflicting_collapsed_range(doc, from.line, &from, &to, il, ir)
            || from.line != to.line
                && conflicting_collapsed_range(doc, to.line, &from, &to, il, ir))
    {
        return Err(CmError::CollapsedOverlap);
    }

    if marker.data().opts.add_to_history {
        let change = Change {
            from,
            to,
            text: None,
            origin: Some("markText".into()),
            full: false,
        };
        let sel = doc.inner().sel.clone();
        add_change_to_history(doc, &change, sel, None);
    }

    let lines = doc.iter_lines(from.line, to.line + 1);
    let mut cur_line = from.line;
    for line in &lines {
        if collapsed && cur_line != from.line {
            doc.inner_mut().tree.update_line_height(line, 0.0);
        }
        add_marked_span(
            doc,
            line,
            MarkedSpan {
                marker: marker.clone(),
                from: if cur_line == from.line {
                    Some(from.ch)
                } else {
                    None
                },
                to: if cur_line == to.line {
                    Some(to.ch)
                } else {
                    None
                },
            },
        );
        cur_line += 1;
    }
    // lineIsHidden depends on the presence of the spans, so needs a second pass.
    if collapsed {
        for line in &lines {
            if line_is_hidden(doc, line) {
                doc.inner_mut().tree.update_line_height(line, 0.0);
            }
        }
    }

    if marker.data().opts.read_only {
        let non_empty = {
            let h = doc.history_rc();
            let h = h.borrow();
            !h.done.is_empty() || !h.undone.is_empty()
        };
        if non_empty {
            doc.clear_history();
        }
    }
    if collapsed {
        let mut d = marker.data_mut();
        d.id = next_marker_id();
        d.atomic = true;
    }
    if doc.has_editor() && marker.data().atomic {
        re_check_selection(doc);
    }
    Ok(marker)
}

/// `markTextShared(doc, from, to, options, type)`.
fn mark_text_shared(
    doc: &Doc,
    from: Pos,
    to: Pos,
    options: &MarkOptions,
    kind: MarkerType,
) -> Result<SharedTextMarker, CmError> {
    let mut options = options.clone();
    options.shared = false;
    let first =
        doc.in_op(|| mark_text_inner(doc, from, to, options.clone(), options.atomic, kind))?;
    let mut markers = vec![first.clone()];
    let mut primary = first;
    for (d, _) in doc.linked_docs(false) {
        let f = d.clip_pos(from);
        let t = d.clip_pos(to);
        let m = d.in_op(|| mark_text_inner(&d, f, t, options.clone(), options.atomic, kind))?;
        markers.push(m.clone());
        if d.inner().linked.iter().any(|l| l.is_parent) {
            continue;
        }
        primary = m;
    }
    let shared = SharedTextMarker(Rc::new(RefCell::new(SharedData {
        markers: markers.clone(),
        primary,
        explicitly_cleared: false,
        clear_handlers: Vec::new(),
    })));
    for m in &markers {
        m.data_mut().parent = Some(shared.clone());
    }
    Ok(shared)
}

/// `findSharedMarkers(doc)`.
pub(crate) fn find_shared_markers(doc: &Doc) -> Vec<SharedTextMarker> {
    let last = doc.last_line();
    let end = doc.clip_pos(Pos::line_end(last));
    doc.find_marks_filtered(
        Pos::new(doc.first_line(), 0),
        end,
        Some(&|m: &TextMarker| m.parent().is_some()),
    )
    .into_iter()
    .filter_map(|m| match m {
        Marker::Shared(s) => Some(s),
        Marker::Text(_) => None,
    })
    .collect()
}

/// `copySharedMarkers(doc, markers)`. The JS passes the primary marker object itself
/// as the options, so the sub-marker inherits all of its options (and `atomic`).
///
/// `marker.find()` must return `{from, to}`; for a bookmark primary it returns a bare
/// position, and for a primary that lost its text it returns `undefined`. Both make
/// the JS throw a `TypeError` out of `linkedDoc` (after the new document has already
/// been linked); that is reported as [`CmError::TypeError`].
pub(crate) fn copy_shared_markers(doc: &Doc, markers: &[SharedTextMarker]) -> Result<(), CmError> {
    for marker in markers {
        let primary = marker.primary();
        let found = if primary.kind() == MarkerType::Bookmark {
            None
        } else {
            primary.find_range_raw()
        };
        let Some((pf, Some(pt))) = found else {
            return Err(CmError::TypeError(
                "Cannot read properties of undefined (reading 'line')".into(),
            ));
        };
        let m_from = doc.clip_pos(pf);
        let m_to = doc.clip_pos(pt);
        if !cmp(&m_from, &m_to).is_eq() {
            let (opts, atomic, kind) = {
                let d = primary.data();
                let mut o = d.opts.clone();
                o.shared = false;
                (o, d.atomic, d.kind)
            };
            let sub = mark_text_inner(doc, m_from, m_to, opts, atomic, kind)?;
            marker.0.borrow_mut().markers.push(sub.clone());
            sub.data_mut().parent = Some(marker.clone());
        }
    }
    Ok(())
}

/// `detachSharedMarkers(markers)`.
pub(crate) fn detach_shared_markers(markers: &[SharedTextMarker]) {
    for marker in markers {
        let Some(pdoc) = marker.primary().doc() else {
            continue;
        };
        let mut linked = vec![pdoc.clone()];
        for (d, _) in pdoc.linked_docs(false) {
            linked.push(d);
        }
        let subs = marker.markers();
        let mut keep = Vec::new();
        for sub in subs {
            let in_linked = sub
                .doc()
                .is_some_and(|d| linked.iter().any(|l| l.ptr_eq(&d)));
            if in_linked {
                keep.push(sub);
            } else {
                sub.data_mut().parent = None;
            }
        }
        marker.0.borrow_mut().markers = keep;
    }
}
