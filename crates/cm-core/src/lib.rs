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
pub mod operation_group;
pub mod pos;
pub mod text;
pub mod util;

pub use events::{Args, Emitter, ListenerId, Listeners, NamedHandler, NamedListeners};
pub use model::changes::{BeforeChange, BeforeSelectionChange, ChangeObj};
pub use model::doc::{Doc, DocOptions, EditorSemantics, LinkedDocOptions};
pub use model::gutter::LineInfo;
pub use model::history::{HistoryChangeData, HistoryData, HistoryEventData, HistorySize};
pub use model::line::{GutterMarker, GutterMarkers, LineHandle};
pub use model::line_class::{LineSel, LineWhere};
pub use model::line_widget::{LineWidget, LineWidgetEvent, LineWidgetOptions};
pub use model::mark_text::{
    MarkOptions, Marker, MarkerFind, MarkerType, SharedTextMarker, TextMarker,
};
pub use model::selection::{SelRange, Selection};
pub use model::selection_updates::SelOptions;
pub use operation_group::{Delayed, EdCall, EditorId, EditorSink, Editors, LineChange, NoEditors};
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
    /// `attachDoc`: "This document is already in use." (the document has an editor).
    DocInUse,
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
            CmError::DocInUse => write!(f, "This document is already in use."),
        }
    }
}

impl std::error::Error for CmError {}
