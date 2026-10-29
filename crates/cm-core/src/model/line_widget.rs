//! Line widgets (src/model/line_widget.js): `LineWidget`, `clear`, `changed` and
//! `addLineWidget`. A widget is a block above or below a line. CM5 measures its DOM
//! node (`widgetHeight`, src/measurement/widgets.js:4); here the node is opaque and the
//! host gives its height in px. The view adds widget heights to line heights; the model
//! keeps the widgets on the line and tells the attached editor through
//! [`crate::EditorSink::line_widget`].

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use super::doc::Doc;
use super::line::{Line, LineHandle, LineRef};
use super::line_class::LineSel;
use super::spans::line_is_hidden;
use crate::operation_group::LineChange;

/// The `options` of `addLineWidget(line, node, options)`.
#[derive(Clone, Debug, Default)]
pub struct LineWidgetOptions {
    /// `above`: drawn above the line instead of below.
    pub above: bool,
    /// `coverGutter`: extends over the gutters.
    pub cover_gutter: bool,
    /// `noHScroll`: does not scroll horizontally.
    pub no_h_scroll: bool,
    /// `handleMouseEvents`: the editor handles mouse events on the widget.
    pub handle_mouse_events: bool,
    /// `insertAt`: index among the line's widgets (default: last).
    pub insert_at: Option<i64>,
    /// `className`: added to `CodeMirror-linewidget`.
    pub class_name: Option<String>,
}

/// What happened to a widget, for [`crate::EditorSink::line_widget`] (the editor's
/// `lineWidgetAdded`/`lineWidgetCleared`/`lineWidgetChanged` events).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineWidgetEvent {
    Added,
    Cleared,
    Changed,
}

pub(crate) struct WidgetData {
    pub node: Rc<dyn Any>,
    pub opts: LineWidgetOptions,
    /// `widget.height`: the node's measured height (host-provided).
    pub height: f64,
    /// `widget.line` (weak: the line owns the widget).
    pub line: Weak<RefCell<Line>>,
}

/// A line widget. Compares by identity.
#[derive(Clone)]
pub struct LineWidget(pub(crate) Rc<RefCell<WidgetData>>);

impl PartialEq for LineWidget {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
}

impl std::fmt::Debug for LineWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineWidget")
            .field("height", &self.height())
            .field("opts", &self.0.borrow().opts)
            .finish()
    }
}

impl LineWidget {
    /// `widget.node`.
    pub fn node(&self) -> Rc<dyn Any> {
        self.0.borrow().node.clone()
    }
    /// `widgetHeight(widget)`.
    pub fn height(&self) -> f64 {
        self.0.borrow().height
    }
    pub fn options(&self) -> LineWidgetOptions {
        self.0.borrow().opts.clone()
    }
    pub fn above(&self) -> bool {
        self.0.borrow().opts.above
    }
    /// `widget.line`, `None` before it was added or when that line was removed.
    pub fn line(&self) -> Option<LineHandle> {
        self.0.borrow().line.upgrade().map(LineHandle)
    }

    /// `widget.clear()`: remove the widget from its line. The line gets shorter by the
    /// widget's height; the editor redraws the line and keeps the visible text in place
    /// when the line is above the viewport. `lineWidgetCleared` is signaled after that
    /// operation, so it is delivered with the orphan callbacks (line_widget.js:33).
    pub fn clear(&self, doc: &Doc) {
        if let Some(no) = self.detach(doc)
            && let Some(sink) = doc.sink()
        {
            sink.signal_line_widget(self, no, LineWidgetEvent::Cleared);
        }
    }

    /// The part of `clear()` inside its `runInOp`: returns the line number when the
    /// widget was on a line of `doc`. Like CM5, clearing a widget that is already off
    /// a line which still has other widgets reports the line again.
    pub fn detach(&self, doc: &Doc) -> Option<i64> {
        let line = self.0.borrow().line.upgrade()?;
        let no = doc.line_no(&line)?;
        {
            let mut l = line.borrow_mut();
            let ws = l.widgets.as_mut()?;
            ws.retain(|w| w != self);
            if ws.is_empty() {
                l.widgets = None;
            }
        }
        let height = self.height();
        if let Some(sink) = doc.sink() {
            doc.in_op(|| {
                sink.line_widget(
                    doc,
                    &LineHandle(line.clone()),
                    no,
                    -height,
                    self,
                    LineWidgetEvent::Cleared,
                );
                sink.line_changed(doc, no, LineChange::Widget);
            });
        }
        Some(no)
    }

    /// `widget.changed()` after the node's height became `height`: when it differs,
    /// the line height follows (unless the line is hidden), the editor forces an
    /// update, keeps the visible text in place and signals `lineWidgetChanged`.
    pub fn changed(&self, doc: &Doc, height: f64) {
        let old = self.height();
        self.0.borrow_mut().height = height;
        let diff = height - old;
        if diff == 0.0 {
            return;
        }
        let Some(line) = self.0.borrow().line.upgrade() else {
            return;
        };
        if let (Some(sink), Some(no)) = (doc.sink(), doc.line_no(&line)) {
            let hidden = line_is_hidden(doc, &line);
            doc.in_op(|| {
                sink.line_widget(
                    doc,
                    &LineHandle(line.clone()),
                    no,
                    if hidden { 0.0 } else { diff },
                    self,
                    LineWidgetEvent::Changed,
                );
                sink.signal_line_widget(self, no, LineWidgetEvent::Changed);
            });
        }
    }
}

impl Doc {
    /// `addLineWidget(handle, node, options)`; `height` is the node's height in px.
    /// Returns the widget, also when the line was removed (it is then on no line).
    pub fn add_line_widget(
        &self,
        line: impl Into<LineSel>,
        node: Rc<dyn Any>,
        height: f64,
        opts: LineWidgetOptions,
    ) -> LineWidget {
        let widget = LineWidget(Rc::new(RefCell::new(WidgetData {
            node,
            opts,
            height,
            line: Weak::new(),
        })));
        let sel = line.into();
        let sink = self.sink();
        self.in_op(|| {
            let mut added: Option<(LineRef, bool)> = None;
            let done = self.change_line(&sel, LineChange::Widget, |l| {
                let ws = l.widgets.get_or_insert_with(Vec::new);
                match widget.0.borrow().opts.insert_at {
                    None => ws.push(widget.clone()),
                    Some(at) => {
                        let at = at.clamp(0, ws.len() as i64) as usize;
                        ws.insert(at, widget.clone());
                    }
                }
                Ok::<_, std::convert::Infallible>(true)
            });
            if let Ok(Some(h)) = done {
                widget.0.borrow_mut().line = Rc::downgrade(&h.0);
                let hidden = line_is_hidden(self, &h.0);
                added = Some((h.0, hidden));
            }
            if let (Some(sink), Some((line, hidden))) = (&sink, added) {
                let no = self.line_no(&line).unwrap_or(0);
                let diff = if hidden { 0.0 } else { height };
                sink.line_widget(
                    self,
                    &LineHandle(line),
                    no,
                    diff,
                    &widget,
                    LineWidgetEvent::Added,
                );
            }
            // `if (cm) signalLater(cm, "lineWidgetAdded", cm, widget, lineNo)`: a number
            // is passed on as given, a handle gives its line number.
            if let Some(sink) = &sink {
                let no = match &sel {
                    LineSel::No(n) => Some(*n),
                    LineSel::Handle(h) => self.line_no(&h.0),
                };
                if let Some(no) = no {
                    sink.signal_line_widget(&widget, no, LineWidgetEvent::Added);
                }
            }
        });
        widget
    }
}

/// `line.widgets`.
impl LineHandle {
    pub fn widgets(&self) -> Vec<LineWidget> {
        self.0.borrow().widgets.clone().unwrap_or_default()
    }
}
