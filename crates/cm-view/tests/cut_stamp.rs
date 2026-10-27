//! C-CLOCK-STAMP (R5): the cut stamp (`cm.state.cutIncoming`) and the origin of the
//! input that follows a cut. Expected values come from the pinned CM5 run in a
//! browser (`.reports/codemirror-r/2026-10-27-c-clock-stamp-01/repro/cm5-cut-*.json`):
//! a read-only cut fires `cut`, stamps `cutIncoming` and changes nothing; the next
//! input is `cut` inside 200 ms and `+input` after it. When the time is unknown
//! (`cm_core::clock`), a stamp can never expire, so a blocked cut must leave none.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cm_core::Pos;
use cm_core::clock;
use cm_modes::Spec;
use cm_view::app::{App, AppOptions, Clipboard, Input};
use cm_view::editor::last_copied;
use cm_view::keymap::KeyEvent;
use cm_view::{Editor, EditorOptions, Metrics};

fn editor(text: &str) -> Editor {
    Editor::new(
        text,
        Spec::from("text/plain"),
        EditorOptions::default(),
        Metrics::test_grid(),
    )
}

fn origins(ed: &Editor) -> Rc<RefCell<Vec<String>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    ed.on_change(move |_cx, ch| {
        s.borrow_mut()
            .push(ch.origin.clone().unwrap_or_else(|| "(none)".into()))
    });
    seen
}

fn unknown_clock() {
    clock::set_host_clock(Some(Box::new(|| None)));
}

/// A host clock that can fail: `Some(t)` while `up` is set, `None` otherwise.
fn flaky_clock(start: f64) -> (Rc<Cell<f64>>, Rc<Cell<bool>>) {
    let t = Rc::new(Cell::new(start));
    let up = Rc::new(Cell::new(true));
    let (t2, up2) = (t.clone(), up.clone());
    clock::set_host_clock(Some(Box::new(move || up2.get().then(|| t2.get()))));
    (t, up)
}

fn select(ed: &mut Editor, a: (i64, i64), h: (i64, i64)) {
    ed.run_op(|cm| {
        cm.doc
            .set_selection(Pos::new(a.0, a.1), Some(Pos::new(h.0, h.1)), None)
    });
}

fn cursor(ed: &mut Editor, line: i64, ch: i64) {
    ed.run_op(|cm| cm.doc.set_cursor(Pos::new(line, ch), None));
}

fn sel(ed: &Editor) -> Vec<(i64, i64, i64, i64)> {
    ed.doc
        .list_selections()
        .iter()
        .map(|r| (r.anchor.line, r.anchor.ch, r.head.line, r.head.ch))
        .collect()
}

fn hist(ed: &Editor) -> (usize, usize) {
    let h = ed.doc.history_size();
    (h.undo, h.redo)
}

/// Read-only cut of "ab" in "abc\ndef"; returns the editor and its origin log.
fn blocked_cut() -> (Editor, Rc<RefCell<Vec<String>>>) {
    let mut ed = editor("abc\ndef");
    let log = origins(&ed);
    ed.options.read_only = true;
    select(&mut ed, (0, 0), (0, 2));
    assert_eq!(ed.cut().as_deref(), Some("ab"), "the clipboard text");
    assert_eq!(
        ed.get_value(),
        "abc\ndef",
        "a read-only cut changes nothing"
    );
    assert_eq!(sel(&ed), [(0, 0, 0, 2)]);
    assert_eq!(hist(&ed), (0, 0));
    assert!(log.borrow().is_empty());
    ed.options.read_only = false;
    cursor(&mut ed, 1, 3);
    (ed, log)
}

// ---- successful cuts ----

#[test]
fn successful_cut_valid_clock() {
    let t = clock::manual(1e12);
    let mut ed = editor("abc\ndef");
    let log = origins(&ed);
    select(&mut ed, (0, 0), (0, 2));
    assert_eq!(ed.cut().as_deref(), Some("ab"));
    assert_eq!(ed.get_value(), "c\ndef");
    assert_eq!(ed.cut_incoming, -1.0, "consumed by the cut's own input");
    t.set(t.get() + 100.0);
    cursor(&mut ed, 1, 3);
    ed.type_text("z");
    // CM5 `editable_cut_then_type`.
    assert_eq!(*log.borrow(), ["cut", "+input"]);
    assert_eq!(ed.get_value(), "c\ndefz");
    assert_eq!(hist(&ed), (2, 0));
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "c\ndef");
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "abc\ndef");
    assert!(ed.exec_command("redo"));
    assert!(ed.exec_command("redo"));
    assert_eq!(ed.get_value(), "c\ndefz");
    clock::set_host_clock(None);
}

#[test]
fn successful_cut_unknown_time() {
    unknown_clock();
    let mut ed = editor("abc\ndef");
    let log = origins(&ed);
    select(&mut ed, (0, 0), (0, 2));
    assert_eq!(ed.cut().as_deref(), Some("ab"));
    assert_eq!(ed.cut_incoming, -1.0);
    cursor(&mut ed, 1, 3);
    ed.type_text("z");
    // The L-B rule keeps the cut origin when the time is unknown.
    assert_eq!(*log.borrow(), ["cut", "+input"]);
    assert_eq!(hist(&ed), (2, 0), "unknown time never merges");
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "c\ndef");
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "abc\ndef");
    assert!(ed.exec_command("redo"));
    assert_eq!(ed.get_value(), "c\ndef");
    clock::set_host_clock(None);
}

// ---- blocked (read-only) cuts ----

#[test]
fn blocked_cut_valid_clock_keeps_the_cm5_stamp() {
    let t = clock::manual(1e12);
    let (mut ed, log) = blocked_cut();
    // CM5 `readonly_cut_shortcut`: the stamp is set and nothing else changes.
    assert_eq!(ed.cut_incoming, 1e12);
    t.set(t.get() + 100.0);
    ed.type_text("z");
    // CM5 `readonly_cut_then_type_fast`: inside 200 ms the input is "cut".
    assert_eq!(*log.borrow(), ["cut"]);
    assert_eq!(ed.cut_incoming, -1.0);
    clock::set_host_clock(None);
}

#[test]
fn blocked_cut_valid_clock_expires_after_200ms() {
    let t = clock::manual(1e12);
    let (mut ed, log) = blocked_cut();
    t.set(t.get() + 300.0);
    ed.type_text("z");
    // CM5 `readonly_cut_then_type_300ms`.
    assert_eq!(*log.borrow(), ["+input"]);
    assert_eq!(ed.get_value(), "abc\ndefz");
    assert_eq!(hist(&ed), (1, 0));
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "abc\ndef");
    assert!(ed.exec_command("redo"));
    assert_eq!(ed.get_value(), "abc\ndefz");
    clock::set_host_clock(None);
}

/// R5: the original reproduction (`p13_1_stale_cut_stamp_unknown_clock`).
#[test]
fn blocked_cut_unknown_time_then_typing_is_input() {
    unknown_clock();
    let (mut ed, log) = blocked_cut();
    assert_eq!(ed.cut_incoming, -1.0, "no stamp that can never expire");
    assert_eq!(ed.paste_incoming, -1.0);
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input"]);
    assert_eq!(ed.get_value(), "abc\ndefz");
    assert_eq!(sel(&ed), [(1, 4, 1, 4)]);
    assert_eq!(hist(&ed), (1, 0));
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "abc\ndef");
    assert_eq!(hist(&ed), (0, 1));
    assert!(ed.exec_command("redo"));
    assert_eq!(ed.get_value(), "abc\ndefz");
    clock::set_host_clock(None);
}

#[test]
fn blocked_cut_unknown_time_then_typing_does_not_merge() {
    unknown_clock();
    let (mut ed, log) = blocked_cut();
    ed.type_text("y");
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input", "+input"]);
    assert_eq!(hist(&ed), (2, 0), "unknown time: separate undo steps");
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "abc\ndefy");
    clock::set_host_clock(None);
}

#[test]
fn blocked_cut_valid_clock_typing_still_merges() {
    let t = clock::manual(1e12);
    let (mut ed, log) = blocked_cut();
    t.set(t.get() + 300.0);
    ed.type_text("y");
    t.set(t.get() + 100.0);
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input", "+input"]);
    assert_eq!(hist(&ed), (1, 0), "+input inside the delay merges");
    clock::set_host_clock(None);
}

#[test]
fn blocked_cut_then_paste() {
    for unknown in [true, false] {
        let t = clock::manual(1e12);
        if unknown {
            unknown_clock();
        }
        let (mut ed, log) = blocked_cut();
        t.set(t.get() + 50.0);
        ed.paste("P");
        assert_eq!(*log.borrow(), ["paste"], "unknown={unknown}");
        assert_eq!(ed.get_value(), "abc\ndefP");
        assert_eq!((ed.cut_incoming, ed.paste_incoming), (-1.0, -1.0));
        assert!(ed.exec_command("undo"));
        assert_eq!(ed.get_value(), "abc\ndef");
        clock::set_host_clock(None);
    }
}

#[test]
fn blocked_cut_then_another_cut() {
    unknown_clock();
    let (mut ed, log) = blocked_cut();
    select(&mut ed, (1, 0), (1, 2));
    assert_eq!(ed.cut().as_deref(), Some("de"));
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["cut", "+input"]);
    assert_eq!(ed.get_value(), "abc\nzf");
    clock::set_host_clock(None);
}

#[test]
fn multiple_blocked_cuts() {
    unknown_clock();
    let mut ed = editor("abc\ndef");
    let log = origins(&ed);
    ed.options.read_only = true;
    select(&mut ed, (0, 0), (0, 2));
    for _ in 0..5 {
        assert_eq!(ed.cut().as_deref(), Some("ab"));
        assert_eq!(ed.cut_incoming, -1.0);
    }
    assert_eq!(ed.get_value(), "abc\ndef");
    ed.options.read_only = false;
    cursor(&mut ed, 1, 3);
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input"]);
    clock::set_host_clock(None);
}

#[test]
fn copy_then_typing() {
    for unknown in [true, false] {
        let t = clock::manual(1e12);
        if unknown {
            unknown_clock();
        }
        let mut ed = editor("abc\ndef");
        let log = origins(&ed);
        select(&mut ed, (0, 0), (0, 2));
        assert_eq!(ed.copy().as_deref(), Some("ab"));
        assert_eq!(ed.cut_incoming, -1.0, "copy makes no stamp");
        assert_eq!(hist(&ed), (0, 0));
        t.set(t.get() + 10.0);
        cursor(&mut ed, 1, 3);
        ed.type_text("z");
        // CM5 `copy_then_type`.
        assert_eq!(*log.borrow(), ["+input"], "unknown={unknown}");
        clock::set_host_clock(None);
    }
}

// ---- no or empty selection ----

#[test]
fn cut_with_no_selection_line_wise() {
    for read_only in [true, false] {
        unknown_clock();
        let mut ed = editor("abc\ndef");
        let log = origins(&ed);
        ed.options.read_only = read_only;
        cursor(&mut ed, 0, 1);
        assert_eq!(ed.cut().as_deref(), Some("abc\n"));
        assert_eq!(last_copied().map(|l| l.line_wise), Some(true));
        if read_only {
            // CM5 `cut_no_selection_linewise_readonly`.
            assert_eq!(ed.get_value(), "abc\ndef");
            assert_eq!(sel(&ed), [(0, 0, 1, 0)]);
            assert_eq!(ed.cut_incoming, -1.0);
            ed.options.read_only = false;
            cursor(&mut ed, 1, 3);
            ed.type_text("z");
            assert_eq!(*log.borrow(), ["+input"]);
        } else {
            // CM5 `cut_no_selection_linewise_shortcut`.
            assert_eq!(ed.get_value(), "def");
            assert_eq!(*log.borrow(), ["cut"]);
            assert!(ed.exec_command("undo"));
            assert_eq!(ed.get_value(), "abc\ndef");
            assert!(ed.exec_command("redo"));
            assert_eq!(ed.get_value(), "def");
        }
        clock::set_host_clock(None);
    }
}

#[test]
fn cut_with_no_selection_not_line_wise() {
    for unknown in [true, false] {
        clock::manual(1e12);
        if unknown {
            unknown_clock();
        }
        let mut ed = editor("abc\ndef");
        let log = origins(&ed);
        ed.options.line_wise_copy_cut = false;
        cursor(&mut ed, 0, 1);
        // CM5 `cut_no_selection_no_linewise`: prepareCopyCut returns before the stamp.
        assert_eq!(ed.cut(), None);
        assert_eq!(ed.cut_incoming, -1.0);
        assert_eq!(ed.get_value(), "abc\ndef");
        ed.type_text("z");
        assert_eq!(*log.borrow(), ["+input"]);
        clock::set_host_clock(None);
    }
}

#[test]
fn cut_of_empty_selections() {
    unknown_clock();
    let mut ed = editor("abc\ndef\nghi");
    let log = origins(&ed);
    ed.options.read_only = true;
    ed.run_op(|cm| {
        cm.doc.set_selections(
            &[
                (Pos::new(0, 1), Some(Pos::new(0, 1))),
                (Pos::new(2, 2), Some(Pos::new(2, 2))),
            ],
            None,
            None,
        )
    });
    assert_eq!(ed.cut().as_deref(), Some("abc\n\nghi"));
    assert_eq!(ed.get_value(), "abc\ndef\nghi");
    assert_eq!(ed.cut_incoming, -1.0);
    ed.options.read_only = false;
    cursor(&mut ed, 1, 0);
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input"]);
    clock::set_host_clock(None);
}

// ---- around undo and redo ----

#[test]
fn cut_after_undo_and_redo() {
    unknown_clock();
    let mut ed = editor("abc\ndef");
    let log = origins(&ed);
    cursor(&mut ed, 1, 3);
    ed.type_text("x");
    assert!(ed.exec_command("undo"));
    select(&mut ed, (0, 0), (0, 2));
    ed.options.read_only = true;
    assert_eq!(ed.cut().as_deref(), Some("ab"));
    assert_eq!(hist(&ed), (0, 1), "a blocked cut keeps the redo stack");
    ed.options.read_only = false;
    assert!(ed.exec_command("redo"));
    assert_eq!(ed.get_value(), "abc\ndefx");
    select(&mut ed, (0, 0), (0, 2));
    ed.options.read_only = true;
    assert_eq!(ed.cut().as_deref(), Some("ab"));
    ed.options.read_only = false;
    cursor(&mut ed, 1, 4);
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input", "undo", "redo", "+input"]);
    // A successful cut after undo.
    assert!(ed.exec_command("undo"));
    select(&mut ed, (0, 0), (0, 2));
    assert_eq!(ed.cut().as_deref(), Some("ab"));
    assert_eq!(ed.get_value(), "c\ndefx");
    assert_eq!(hist(&ed), (2, 0));
    assert!(ed.exec_command("undo"));
    assert_eq!(ed.get_value(), "abc\ndefx");
    assert!(ed.exec_command("redo"));
    assert_eq!(ed.get_value(), "c\ndefx");
    clock::set_host_clock(None);
}

// ---- instances and lifecycle ----

#[test]
fn multiple_editor_instances() {
    unknown_clock();
    let (mut a, log_a) = blocked_cut();
    let mut b = editor("xyz");
    let log_b = origins(&b);
    cursor(&mut b, 0, 3);
    b.type_text("!");
    a.type_text("z");
    assert_eq!(*log_a.borrow(), ["+input"]);
    assert_eq!(*log_b.borrow(), ["+input"]);
    // A pending stamp, valid clock: per instance.
    let t = clock::manual(1e12);
    let mut c = editor("abc");
    c.options.read_only = true;
    select(&mut c, (0, 0), (0, 1));
    c.cut();
    let mut d = editor("abc");
    let log_d = origins(&d);
    cursor(&mut d, 0, 3);
    t.set(t.get() + 10.0);
    d.type_text("z");
    assert_eq!(*log_d.borrow(), ["+input"], "a stamp stays in its editor");
    // Destroy the editor with the pending stamp; a new one starts clean.
    drop(c);
    let e = editor("abc");
    assert_eq!((e.cut_incoming, e.paste_incoming), (-1.0, -1.0));
    clock::set_host_clock(None);
}

// ---- clock failure and recovery ----

#[test]
fn clock_failure_during_a_cut() {
    let (t, up) = flaky_clock(1e12);
    // Blocked cut while the clock is down; typing after it is back.
    let (mut ed, log) = {
        let mut ed = editor("abc\ndef");
        let log = origins(&ed);
        ed.options.read_only = true;
        select(&mut ed, (0, 0), (0, 2));
        up.set(false);
        assert_eq!(ed.cut().as_deref(), Some("ab"));
        up.set(true);
        assert_eq!(ed.cut_incoming, -1.0);
        ed.options.read_only = false;
        cursor(&mut ed, 1, 3);
        (ed, log)
    };
    t.set(t.get() + 10.0);
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input"]);
    // A successful cut while the clock is down keeps its origin.
    select(&mut ed, (0, 0), (0, 2));
    up.set(false);
    assert_eq!(ed.cut().as_deref(), Some("ab"));
    up.set(true);
    ed.type_text("y");
    assert_eq!(*log.borrow(), ["+input", "cut", "+input"]);
    clock::set_host_clock(None);
}

#[test]
fn clock_recovery_after_a_blocked_cut() {
    unknown_clock();
    let (mut ed, log) = blocked_cut();
    let t = clock::manual(1e12);
    ed.type_text("y");
    t.set(t.get() + 100.0);
    ed.type_text("z");
    assert_eq!(*log.borrow(), ["+input", "+input"]);
    assert_eq!(
        hist(&ed),
        (1, 0),
        "with the clock back, typing merges again"
    );
    clock::set_host_clock(None);
}

#[test]
fn valid_stamp_then_clock_failure() {
    let (_t, up) = flaky_clock(1e12);
    let (mut ed, log) = blocked_cut();
    assert_eq!(ed.cut_incoming, 1e12);
    up.set(false);
    ed.type_text("z");
    // `recent` is unknown: a measured stamp is not recent (CM5 `NaN` compare).
    assert_eq!(*log.borrow(), ["+input"]);
    clock::set_host_clock(None);
}

// ---- through the App clipboard ----

#[derive(Clone, Default)]
struct SharedClip(Rc<RefCell<Option<String>>>);

impl Clipboard for SharedClip {
    fn get(&mut self) -> Option<String> {
        self.0.borrow().clone()
    }
    fn set(&mut self, text: &str) {
        *self.0.borrow_mut() = Some(text.to_string());
    }
}

fn ctrl(k: &str) -> Input {
    Input::Key {
        key: KeyEvent {
            ctrl: true,
            ..KeyEvent::new(k)
        },
        text: None,
    }
}

#[test]
fn app_read_only_cut_then_typing() {
    unknown_clock();
    let clip = SharedClip::default();
    let opts = AppOptions {
        read_only: true,
        ..Default::default()
    };
    let mut app = App::new(
        "abc\ndef",
        None,
        Metrics::test_grid(),
        &opts,
        Box::new(clip.clone()),
    );
    let log = origins(&app.editor);
    select(&mut app.editor, (0, 0), (0, 2));
    app.handle(ctrl("X"));
    assert_eq!(
        clip.0.borrow().as_deref(),
        Some("ab"),
        "a blocked cut still copies"
    );
    assert_eq!(app.editor.get_value(), "abc\ndef");
    assert_eq!(app.editor.cut_incoming, -1.0);
    app.editor.options.read_only = false;
    cursor(&mut app.editor, 1, 3);
    app.handle(Input::Text("z".into()));
    app.handle(ctrl("V"));
    assert_eq!(app.editor.get_value(), "abc\ndefzab");
    assert_eq!(*log.borrow(), ["+input", "paste"]);
    clock::set_host_clock(None);
}

#[test]
fn repeated_cuts() {
    unknown_clock();
    let mut ed = editor(&"line\n".repeat(200));
    ed.options.read_only = true;
    select(&mut ed, (0, 0), (0, 4));
    for _ in 0..2000 {
        ed.cut();
    }
    assert_eq!(ed.cut_incoming, -1.0);
    ed.options.read_only = false;
    let log = origins(&ed);
    for i in 0..100 {
        select(&mut ed, (i, 0), (i, 1));
        ed.cut();
    }
    assert_eq!(log.borrow().len(), 100);
    assert!(log.borrow().iter().all(|o| o == "cut"));
    clock::set_host_clock(None);
}
