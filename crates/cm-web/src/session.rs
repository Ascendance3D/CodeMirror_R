//! The browser input rules, independent of the DOM: which browser events reach
//! the shared editor, in what order, and which default actions the adapter
//! cancels so that no edit is applied twice.
//!
//! CM5 (`src/edit/key_events.js`, `src/input/TextareaInput.js`, `src/input/input.js`)
//! reads typed text by polling a hidden textarea and diffing it against
//! `prevInput`. The adapter keeps the textarea empty instead and takes text from
//! `beforeinput`, which every target browser fires before it edits the field:
//!
//! 1. `keydown` → [`Session::key_down`] → `onKeyDown` (`Editor::key_down`). A
//!    handled key cancels the default action, so no `beforeinput` follows.
//! 2. `beforeinput` → [`Session::before_input`] → `onKeyPress` (char bindings) and
//!    `applyTextInput` (`Editor::key_press`). The adapter always cancels it, so the
//!    textarea stays empty and the browser applies nothing.
//! 3. `input` → [`Session::poll`]: a fallback for text that reached the textarea
//!    anyway (an input type that cannot be cancelled). Like `poll`, it applies the
//!    field's text once, and the DOM layer then clears the field.
//! 4. Composition (IME): the preedit text stays out of the document
//!    ([`Session::preedit`]); `compositionend` commits the final text once. CM5's
//!    textarea instead writes the preedit into the document with the `*compose`
//!    origin; the shared editor has no composition model yet (`KNOWN_GAPS.md`).
//! 5. `copy`/`cut`/`paste` events → `prepareCopyCut`/`handlePaste`
//!    ([`Session::copy`], [`Session::cut`], [`Session::paste`]). Ctrl-C/X/V are not
//!    key bindings, so `keydown` leaves them to these events.

use cm_view::KeyEvent;
use cm_view::app::{App, Input};

/// A `beforeinput` event by `inputType`.
#[derive(Clone, Debug, PartialEq)]
pub enum BeforeInput {
    /// `insertText`, `insertReplacementText`.
    Text(String),
    /// `insertLineBreak`, `insertParagraph`.
    LineBreak,
    /// `deleteContentBackward`.
    DeleteBackward,
    /// `deleteContentForward`.
    DeleteForward,
    /// `deleteWordBackward`.
    DeleteWordBackward,
    /// `deleteWordForward`.
    DeleteWordForward,
    /// `historyUndo`.
    Undo,
    /// `historyRedo`.
    Redo,
    /// `insertFromPaste`, `insertFromDrop`, `deleteByCut`, `deleteByDrag`: the
    /// clipboard events own these (drag and drop is unsupported).
    Clipboard,
    /// `insertCompositionText` and friends, while an IME composes.
    Composition,
    /// Anything else: cancelled and ignored.
    Other(String),
}

impl BeforeInput {
    /// Classify `InputEvent.inputType` with its `data`.
    pub fn from_dom(input_type: &str, data: Option<String>) -> BeforeInput {
        match input_type {
            "insertText" | "insertReplacementText" => BeforeInput::Text(data.unwrap_or_default()),
            "insertLineBreak" | "insertParagraph" => BeforeInput::LineBreak,
            "deleteContentBackward" => BeforeInput::DeleteBackward,
            "deleteContentForward" => BeforeInput::DeleteForward,
            "deleteWordBackward" => BeforeInput::DeleteWordBackward,
            "deleteWordForward" => BeforeInput::DeleteWordForward,
            "historyUndo" => BeforeInput::Undo,
            "historyRedo" => BeforeInput::Redo,
            "insertFromPaste"
            | "insertFromPasteAsQuotation"
            | "insertFromDrop"
            | "deleteByCut"
            | "deleteByDrag" => BeforeInput::Clipboard,
            t if t.contains("Composition") => BeforeInput::Composition,
            t => BeforeInput::Other(t.to_string()),
        }
    }
}

/// One editor's input state on top of the shared [`App`].
pub struct Session {
    pub app: App,
    /// The IME preedit text while a composition runs. Never document text.
    composing: Option<String>,
    /// Text that `compositionend` committed, until the next key: a later
    /// `beforeinput`/`input` with the same text is the browser's echo of it.
    committed: Option<String>,
    /// Input events the adapter cancelled without an editor equivalent.
    pub ignored: Vec<String>,
}

impl Session {
    pub fn new(app: App) -> Session {
        Session {
            app,
            composing: None,
            committed: None,
            ignored: Vec::new(),
        }
    }

    /// The host's `setTimeout(fireOrphanDelayed, 0)` after each browser event,
    /// plus a save the `save` command asked for.
    fn after_event(&mut self) {
        self.app.flush_save();
        self.app.deliver_pending();
    }

    /// `keydown`. `key` is `None` when `keyName` has no name for the event.
    /// Returns whether to cancel the default action (a binding handled the key).
    pub fn key_down(&mut self, key: Option<KeyEvent>, is_composing: bool) -> bool {
        if is_composing || self.composing.is_some() {
            return false;
        }
        self.committed = None;
        let Some(key) = key else { return false };
        let handled = self.app.editor.key_down(&key);
        self.after_event();
        handled
    }

    /// `beforeinput`. Returns whether to cancel the default action.
    pub fn before_input(&mut self, ev: BeforeInput) -> bool {
        let ed = &mut self.app.editor;
        match ev {
            BeforeInput::Composition => return false,
            BeforeInput::Text(t) => {
                if self.committed.take().is_some_and(|c| c == t) {
                    return true;
                }
                if !t.is_empty() {
                    ed.key_press(&t, &KeyEvent::default());
                }
            }
            BeforeInput::LineBreak => {
                ed.exec_command("newlineAndIndent");
            }
            BeforeInput::DeleteBackward => {
                ed.exec_command("delCharBefore");
            }
            BeforeInput::DeleteForward => {
                ed.exec_command("delCharAfter");
            }
            BeforeInput::DeleteWordBackward => {
                ed.exec_command("delGroupBefore");
            }
            BeforeInput::DeleteWordForward => {
                ed.exec_command("delGroupAfter");
            }
            BeforeInput::Undo => {
                ed.exec_command("undo");
            }
            BeforeInput::Redo => {
                ed.exec_command("redo");
            }
            BeforeInput::Clipboard => {}
            BeforeInput::Other(t) => self.ignored.push(t),
        }
        self.after_event();
        true
    }

    /// `input` (`TextareaInput.poll`): text that reached the field although
    /// `beforeinput` was cancelled. Applied once; the caller clears the field.
    /// Returns whether the document changed.
    pub fn poll(&mut self, field: &str) -> bool {
        if field.is_empty() || self.composing.is_some() {
            return false;
        }
        if self.committed.take().is_some_and(|c| c == field) {
            return false;
        }
        let r = self.app.editor.type_text(field);
        self.after_event();
        r
    }

    /// `compositionstart`.
    pub fn composition_start(&mut self) {
        self.composing = Some(String::new());
        self.committed = None;
    }

    /// `compositionupdate`: the preedit changes; the document does not.
    pub fn composition_update(&mut self, data: &str) {
        self.composing = Some(data.to_string());
    }

    /// `compositionend`: commit the final text once, as typed input.
    pub fn composition_end(&mut self, data: &str) {
        self.composing = None;
        if !data.is_empty() {
            self.app.editor.type_text(data);
            self.committed = Some(data.to_string());
        }
        self.after_event();
    }

    /// The preedit text of a running composition.
    pub fn preedit(&self) -> Option<&str> {
        self.composing.as_deref()
    }

    /// The `copy` event (`onCopyCut`): the text for `clipboardData`, or `None`
    /// to leave the event alone (nothing selected, no line-wise copy).
    pub fn copy(&mut self) -> Option<String> {
        let t = self.app.editor.copy()?;
        self.app.clipboard.set(&t);
        self.after_event();
        Some(t)
    }

    /// The `cut` event.
    pub fn cut(&mut self) -> Option<String> {
        let t = self.app.editor.cut()?;
        self.app.clipboard.set(&t);
        self.after_event();
        Some(t)
    }

    /// The `paste` event (`handlePaste`) with the event's plain text.
    pub fn paste(&mut self, text: &str) {
        self.app.editor.paste(text);
        self.after_event();
    }

    /// Mouse, wheel, resize and command input through the shared [`App`].
    pub fn handle(&mut self, input: Input) -> bool {
        let r = self.app.handle(input);
        self.app.flush_save();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm_view::Metrics;
    use cm_view::app::{AppOptions, MemClipboard};

    fn session(text: &str) -> Session {
        let app = App::new(
            text,
            None,
            Metrics::test_grid(),
            &AppOptions::default(),
            Box::new(MemClipboard::default()),
        );
        Session::new(app)
    }

    fn key(desc: &str) -> Option<KeyEvent> {
        Some(KeyEvent::parse(desc))
    }

    #[test]
    fn typed_text_is_applied_once() {
        let mut s = session("");
        // A printable key: keydown is not bound, beforeinput inserts and is cancelled.
        assert!(!s.key_down(key("A"), false));
        assert!(s.before_input(BeforeInput::Text("a".into())));
        // The field stays empty, so the `input` fallback has nothing to apply.
        assert!(!s.poll(""));
        assert_eq!(s.app.editor.get_value(), "a");
    }

    #[test]
    fn bound_keys_cancel_the_default() {
        let mut s = session("ab");
        s.app.editor.doc.set_cursor(cm_core::Pos::new(0, 2), None);
        assert!(s.key_down(key("Backspace"), false));
        assert!(s.key_down(key("Enter"), false));
        assert_eq!(s.app.editor.get_value(), "a\n");
        // Ctrl-C/X/V are left to the clipboard events.
        assert!(!s.key_down(key("Ctrl-C"), false));
        assert!(!s.key_down(key("Ctrl-X"), false));
        assert!(!s.key_down(key("Ctrl-V"), false));
        assert_eq!(s.app.editor.get_value(), "a\n");
        // No key name (IME keyCode 229): nothing happens.
        assert!(!s.key_down(None, false));
    }

    #[test]
    fn composition_stays_out_of_the_document() {
        let mut s = session("x");
        s.app.editor.doc.set_cursor(cm_core::Pos::new(0, 1), None);
        s.composition_start();
        assert!(!s.key_down(key("A"), true));
        assert!(!s.before_input(BeforeInput::Composition));
        s.composition_update("に");
        assert_eq!(s.preedit(), Some("に"));
        assert_eq!(s.app.editor.get_value(), "x");
        s.composition_update("日本");
        assert!(!s.poll("日本"), "the field holds the preedit; no commit");
        assert_eq!(s.app.editor.get_value(), "x");
        s.composition_end("日本");
        assert_eq!(s.preedit(), None);
        assert_eq!(s.app.editor.get_value(), "x日本");
        // Echoes of the commit (Firefox fires `input` after compositionend).
        assert!(!s.poll("日本"));
        assert_eq!(s.app.editor.get_value(), "x日本");
        assert_eq!(s.app.editor.doc.history_size().undo, 1);
    }

    #[test]
    fn clipboard_events() {
        let mut s = session("one\ntwo");
        assert_eq!(s.copy().as_deref(), Some("one\n"), "line-wise copy");
        s.app.editor.doc.set_selection(
            cm_core::Pos::new(1, 0),
            Some(cm_core::Pos::new(1, 3)),
            None,
        );
        assert_eq!(s.cut().as_deref(), Some("two"));
        assert_eq!(s.app.editor.get_value(), "one\n");
        assert_eq!(s.app.clipboard.get().as_deref(), Some("two"));
        s.paste("2");
        assert_eq!(s.app.editor.get_value(), "one\n2");
        let mut lw = session("one");
        lw.app.editor.options.line_wise_copy_cut = false;
        assert_eq!(lw.copy(), None);
    }

    #[test]
    fn before_input_types() {
        let mut s = session("ab cd");
        s.app.editor.doc.set_cursor(cm_core::Pos::new(0, 5), None);
        assert!(s.before_input(BeforeInput::from_dom("deleteWordBackward", None)));
        assert_eq!(s.app.editor.get_value(), "ab ");
        assert!(s.before_input(BeforeInput::from_dom("insertParagraph", None)));
        assert_eq!(s.app.editor.get_value(), "ab \n");
        assert!(s.before_input(BeforeInput::from_dom("historyUndo", None)));
        assert_eq!(s.app.editor.get_value(), "ab ");
        assert!(s.before_input(BeforeInput::from_dom("insertFromPaste", None)));
        assert_eq!(s.app.editor.get_value(), "ab ");
        assert!(s.before_input(BeforeInput::from_dom("formatBold", None)));
        assert_eq!(s.ignored, ["formatBold"]);
        assert_eq!(
            BeforeInput::from_dom("insertCompositionText", Some("x".into())),
            BeforeInput::Composition
        );
    }
}
