//! The model half of `src/line/line_data.js`: `Line` and `cleanUpLine`. `updateLine`
//! is in `document_data.rs`. (Rendering — `buildLineContent` and friends — belongs to
//! the view and is not here.)

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use super::spans::{SpanArr, attach_marked_spans, detach_marked_spans};
use super::tree::NodeId;
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
    }));
    attach_marked_spans(&line, spans);
    line
}

/// An opaque line handle (`getLineHandle`). Compares by identity.
#[derive(Clone)]
pub struct LineHandle(pub(crate) LineRef);

impl LineHandle {
    pub fn text(&self) -> String {
        crate::text::from_u16(&self.0.borrow().text)
    }
    pub fn text_utf16(&self) -> Text {
        self.0.borrow().text.clone()
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
