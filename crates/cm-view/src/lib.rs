//! Port of the CodeMirror 5 (5.65.21) view and input layer to a native editor.
//!
//! Copyright (C) 2017 by Marijn Haverbeke <marijn@haverbeke.berlin> and others
//! (original JavaScript, MIT). Rust port, MIT.
//!
//! * [`editor`] — the editor object: operations, scrolling, geometry, indentation,
//!   text input, clipboard, key dispatch (`src/edit/*`, `src/display/*`,
//!   `src/input/*`).
//! * [`commands`] — `CodeMirror.commands`.
//! * [`keymap`] — `keyMap.basic`, `pcDefault`, `macDefault`, `emacsy`; key names.
//! * [`movement`] — `findPosH`, `findPosV`, `moveVisually`, `endOfLine`.
//! * [`measure`] — the character box model standing in for DOM measurement.
//! * [`highlight`] — mode states, style arrays, frontiers, `getTokenAt`.
//! * [`mouse`] — clicks, drags, word/line/rectangle selection.
//! * `render` / `window` (feature `gpu`) — the wgpu renderer and the winit editor.
//! * [`native_clipboard`] — the system clipboard (feature `clipboard-native`, desktop
//!   targets only).

pub mod app;
pub mod commands;
pub mod editor;
pub mod editor_set;
pub mod events;
pub mod fileio;
pub mod highlight;
pub mod keymap;
pub mod measure;
pub mod mouse;
pub mod movement;
pub mod native_clipboard;
#[cfg(feature = "gpu")]
pub mod render;
#[cfg(feature = "gpu")]
pub mod window;

pub use editor::{CoordSys, Editor, EditorOptions, Indent, ScrollInfo, ScrollTarget};
pub use editor_set::EditorSet;
pub use events::{Cx, EditorEvents, EditorListeners, InputChange};
pub use keymap::KeyEvent;
pub use measure::Metrics;
pub use mouse::MouseEvent;
