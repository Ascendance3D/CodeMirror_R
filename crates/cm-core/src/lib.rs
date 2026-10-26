//! Port of the CodeMirror 5 (5.65.21) core *without* any UI.
//!
//! Copyright (C) 2017 by Marijn Haverbeke <marijn@haverbeke.berlin> and others
//! (original JavaScript, MIT). Rust port, MIT.
//!
//! * [`util`] — `src/util/StringStream.js`, `src/util/bidi.js` and the pure helpers
//!   from `src/util/misc.js`.
//! * [`model`] — `src/model/*` plus the parts of `src/line/*` the model needs: the
//!   chunk B-tree, changes, history (undo/redo/selection history and event merging),
//!   selections, text markers and linked documents.
//!
//! Text is stored as UTF-16 code units, and every `ch` offset is a UTF-16 code unit
//! offset, exactly as in the JavaScript original. The `&str`/`String` API converts at
//! the boundary; `*_utf16` variants exist where lossless round trips matter.

pub mod clock;
pub mod events;
pub mod model;
pub mod pos;
pub mod text;
pub mod util;

pub use events::{ListenerId, Listeners};
pub use model::changes::{BeforeChange, BeforeSelectionChange};
pub use model::doc::{ChangeInfo, Doc, DocOptions, EditorSemantics, LinkedDocOptions};
pub use model::history::{HistoryChangeData, HistoryData, HistoryEventData, HistorySize};
pub use model::line::LineHandle;
pub use model::mark_text::{
    MarkOptions, Marker, MarkerFind, MarkerType, SharedTextMarker, TextMarker,
};
pub use model::selection::{SelRange, Selection};
pub use model::selection_updates::SelOptions;
pub use pos::{Pos, Sticky};
pub use util::string_stream::StringStream;

/// Errors the JavaScript original reports by throwing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmError {
    /// `getLine`: "There is no line N in the document."
    NoSuchLine(i64),
    /// `markText`: "Inserting collapsed marker partially overlapping an existing one"
    CollapsedOverlap,
    /// A JS `TypeError` the original raises on a reachable path (message as in V8).
    TypeError(String),
}

impl std::fmt::Display for CmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CmError::NoSuchLine(n) => write!(f, "There is no line {n} in the document."),
            CmError::CollapsedOverlap => {
                write!(
                    f,
                    "Inserting collapsed marker partially overlapping an existing one"
                )
            }
            CmError::TypeError(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CmError {}
