//! The model half of `src/line/line_data.js`: `Line` and `cleanUpLine`. `updateLine`
//! is in `document_data.rs`. (Rendering — `buildLineContent` and friends — belongs to
//! the view and is not here.)

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use super::changes::ChangeObj;
use super::spans::{SpanArr, attach_marked_spans, detach_marked_spans};
use super::tree::NodeId;
use crate::events::{Args, Entry, ListenerId, Listeners, NamedHandler, NamedListeners, removed};
use crate::operation_group::{Delayed, signal_later};
use crate::text::Text;

pub(crate) struct Line {
    pub text: Text,
    pub marked_spans: Option<SpanArr>,
    pub height: f64,
    /// The leaf chunk holding this line, `None` once removed (`line.parent = null`).
    pub parent: Option<NodeId>,
    /// `line.stateAfter` (src/line/highlight.js), owned by a view; `updateLine` clears it.
    pub state_after: Option<Rc<dyn Any>>,
    /// `line.styles`, owned by a view; `updateLine` clears it.
    pub styles: Option<Rc<dyn Any>>,
    /// `line.textClass`, `bgClass`, `wrapClass`, `gutterClass` (`addLineClass`).
    pub text_class: Option<String>,
    pub bg_class: Option<String>,
    pub wrap_class: Option<String>,
    pub gutter_class: Option<String>,
    /// `line.gutterMarkers` (`setGutterMarker`): `None` is JS `null`.
    pub gutter_markers: Option<GutterMarkers>,
    /// Line handle listeners (`change`, `delete`), allocated on first use.
    pub events: Option<Box<LineEvents>>,
    /// `line.widgets` (`addLineWidget`).
    pub widgets: Option<Vec<super::line_widget::LineWidget>>,
}

/// A gutter marker value. CM5 stores any truthy value, in practice a DOM node; here it is
/// opaque and compared by identity. The JS falsy values (`null`, `undefined`, `0`, `""`)
/// are all `None` in a [`GutterMarkers`] entry; a `Some` marker is always truthy.
#[derive(Clone)]
pub struct GutterMarker(pub Rc<dyn Any>);

impl GutterMarker {
    pub fn new<T: Any>(v: T) -> GutterMarker {
        GutterMarker(Rc::new(v))
    }
    /// The marker as a `T`, when it is one.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl PartialEq for GutterMarker {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
}

impl std::fmt::Debug for GutterMarker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GutterMarker")
    }
}

/// The `line.gutterMarkers` object: gutter id to marker, in insertion order. An entry
/// whose marker was cleared stays as `None` while another entry is set, as in JS where
/// `setGutterMarker(line, id, null)` assigns `null` and only an all-falsy object is
/// dropped (`isEmpty`, util/misc.js:135). (JS orders integer-like ids first; this keeps
/// insertion order.)
pub type GutterMarkers = Vec<(String, Option<GutterMarker>)>;

pub(crate) type LineChangeHandler = dyn Fn(&LineHandle, &ChangeObj);

/// The listeners of one line handle.
#[derive(Default)]
pub(crate) struct LineEvents {
    pub change: Listeners<LineChangeHandler>,
    pub delete: Listeners<dyn Fn()>,
    /// String-named and custom events.
    pub named: NamedListeners<NamedHandler>,
}

/// `signalLater(line, "change", line, change)` (document_data.js:27).
pub(crate) fn signal_line_change(line: &LineRef, change: &Rc<ChangeObj>) {
    let snap = match &line.borrow().events {
        Some(e) if !e.change.is_empty() => e.change.snapshot(),
        _ => return,
    };
    for e in snap.iter() {
        let (f, h, c) = (e.f.clone(), LineHandle(line.clone()), change.clone());
        signal_later(Delayed::Free(Box::new(move || f(&h, &c))));
    }
}

/// `signalLater(line, "delete")` (chunk.js:38).
pub(crate) fn signal_line_delete(line: &LineRef) {
    let snap = match &line.borrow().events {
        Some(e) if !e.delete.is_empty() => e.delete.snapshot(),
        _ => return,
    };
    for e in snap.iter() {
        let f = e.f.clone();
        signal_later(Delayed::Free(Box::new(move || f())));
    }
}

/// Whether the line has `delete` listeners.
pub(crate) fn has_delete_handlers(line: &LineRef) -> bool {
    line.borrow()
        .events
        .as_ref()
        .is_some_and(|e| !e.delete.is_empty())
}

pub(crate) type LineRef = Rc<RefCell<Line>>;

/// `new Line(text, markedSpans, estimateHeight)`. Without an editor the height
/// estimate is always 1.
pub(crate) fn new_line(text: Text, spans: Option<SpanArr>) -> LineRef {
    let line = Rc::new(RefCell::new(Line {
        text,
        marked_spans: None,
        height: 1.0,
        parent: None,
        state_after: None,
        styles: None,
        text_class: None,
        bg_class: None,
        wrap_class: None,
        gutter_class: None,
        gutter_markers: None,
        events: None,
        widgets: None,
    }));
    attach_marked_spans(&line, spans);
    line
}

/// An opaque line handle (`getLineHandle`). Compares by identity.
#[derive(Clone)]
pub struct LineHandle(pub(crate) LineRef);

impl LineHandle {
    /// `line.textClass`: the classes `addLineClass(line, "text", ...)` set.
    pub fn text_class(&self) -> Option<String> {
        self.0.borrow().text_class.clone()
    }
    /// `line.bgClass` (`addLineClass(line, "background", ...)`).
    pub fn bg_class(&self) -> Option<String> {
        self.0.borrow().bg_class.clone()
    }
    /// `line.wrapClass` (`addLineClass(line, "wrap", ...)`).
    pub fn wrap_class(&self) -> Option<String> {
        self.0.borrow().wrap_class.clone()
    }
    /// `line.gutterClass` (`addLineClass(line, "gutter", ...)`).
    pub fn gutter_class(&self) -> Option<String> {
        self.0.borrow().gutter_class.clone()
    }
    /// `line.gutterMarkers`.
    pub fn gutter_markers(&self) -> Option<GutterMarkers> {
        self.0.borrow().gutter_markers.clone()
    }
    /// `on("change", f)`: the line's text or markers changed (delayed).
    pub fn on_change<F: Fn(&LineHandle, &ChangeObj) + 'static>(&self, f: F) -> ListenerId {
        let mut l = self.0.borrow_mut();
        l.events.get_or_insert_default().change.on_rc(Rc::new(f))
    }
    /// `on("delete", f)`: the line was removed from the document (delayed).
    pub fn on_delete<F: Fn() + 'static>(&self, f: F) -> ListenerId {
        let mut l = self.0.borrow_mut();
        l.events.get_or_insert_default().delete.on_rc(Rc::new(f))
    }
    /// `on(line, name, f)` for a string-named or custom event. Remove it with
    /// [`LineHandle::off`].
    pub fn on_event<F: Fn(Args<'_>) + 'static>(&self, name: &str, f: F) -> ListenerId {
        let mut l = self.0.borrow_mut();
        l.events
            .get_or_insert_default()
            .named
            .on_rc(name, Rc::new(f))
    }
    /// `getHandlers(line, name)` for the named table.
    pub fn get_handlers(&self, name: &str) -> Rc<[Entry<NamedHandler>]> {
        match &self.0.borrow().events {
            Some(e) => e.named.get_handlers(name),
            None => Rc::from(Vec::<Entry<NamedHandler>>::new()),
        }
    }
    /// `signal(line, name, ...values)`.
    pub fn signal_event(&self, name: &str, args: Args<'_>) {
        let hs = self.get_handlers(name);
        for e in hs.iter() {
            (e.f)(args);
        }
    }
    /// Remove the line handler registered as `id`.
    pub fn off(&self, id: ListenerId) -> bool {
        let gone = {
            let mut l = self.0.borrow_mut();
            l.events.as_mut().and_then(|e| {
                removed(e.change.remove(id))
                    .or_else(|| removed(e.delete.remove(id)))
                    .or_else(|| removed(e.named.remove(id)))
            })
        };
        gone.is_some()
    }
    pub fn text(&self) -> String {
        crate::text::from_u16(&self.0.borrow().text)
    }
    pub fn text_utf16(&self) -> Text {
        self.0.borrow().text.clone()
    }
    /// `lineLength(line)`: the length in UTF-16 units, taking collapsed ranges into
    /// account (0 for a hidden line).
    pub fn line_length(&self) -> i64 {
        super::spans::line_length(&self.0)
    }
    pub fn height(&self) -> f64 {
        self.0.borrow().height
    }
    /// `line.stateAfter` (view-owned mode state cache).
    pub fn state_after(&self) -> Option<Rc<dyn Any>> {
        self.0.borrow().state_after.clone()
    }
    pub fn set_state_after(&self, v: Option<Rc<dyn Any>>) {
        self.0.borrow_mut().state_after = v;
    }
    /// `line.styles` (view-owned highlighting cache).
    pub fn styles(&self) -> Option<Rc<dyn Any>> {
        self.0.borrow().styles.clone()
    }
    pub fn set_styles(&self, v: Option<Rc<dyn Any>>) {
        self.0.borrow_mut().styles = v;
    }
    /// Identity key for side tables (stable while the line lives).
    pub fn id(&self) -> usize {
        Rc::as_ptr(&self.0) as *const () as usize
    }
}

impl PartialEq for LineHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for LineHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LineHandle({:?})", self.text())
    }
}

/// `cleanUpLine(line)`: detach from the tree and from its markers.
pub(crate) fn clean_up_line(line: &LineRef) {
    line.borrow_mut().parent = None;
    detach_marked_spans(line);
}
