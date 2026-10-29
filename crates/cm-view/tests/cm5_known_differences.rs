//! Known differences from CodeMirror 5, pinned by characterization tests.
//!
//! Each test replays a scenario that `oracle/cm5-behavior-repro.mjs` recorded on the
//! pinned CM5 (headless chromium, DejaVu Sans Mono 32px / 38px lines, 900x500), and
//! asserts what the Rust port does *today* next to the recorded CM5 value. The
//! numbers refer to the 2026-10-25 coverage audit ("Behavior differences", G13).
//!
//! These tests record a difference; they do not approve it. When a fix lane changes
//! the behavior, the test fails: replace it with a test of the CM5 value.

use std::rc::Rc;

use cm_core::{HistoryEventData, Pos};
use cm_modes::Spec;
use cm_view::app::{App, AppOptions, Input, MemClipboard};
use cm_view::keymap::{Binding, KeyMap, KeyMapRef};
use cm_view::{Editor, EditorOptions, KeyEvent, Metrics, MouseEvent};

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The recording page: `lines(200)`, null mode, `Metrics::oracle()`, 900x500.
fn editor() -> Editor {
    let mut cm = Editor::new(
        &lines(200),
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
    );
    cm.display.window_height = Some(900.0);
    cm.set_size(900.0, 500.0);
    cm
}

type Sel = (i64, i64, i64, i64);

fn sels(cm: &Editor) -> Vec<Sel> {
    cm.doc
        .list_selections()
        .iter()
        .map(|r| (r.anchor.line, r.anchor.ch, r.head.line, r.head.ch))
        .collect()
}

fn cursor(cm: &Editor) -> (i64, i64) {
    let c = cm.doc.get_cursor(None);
    (c.line, c.ch)
}

/// The recording's mouse targets: client x inside column `ch`, client y in the middle
/// of `line`. Client coordinates are div coordinates plus the 4px top padding.
fn x_at(ch: i64) -> f64 {
    4.0 + ch as f64 * 19.265625 + 1.0
}
fn y_at(line: i64) -> f64 {
    line as f64 * 38.0 + 19.0
}

fn mouse(x: f64, y: f64, alt: bool) -> MouseEvent {
    MouseEvent {
        x,
        y,
        button: 1,
        shift: false,
        ctrl: false,
        alt,
        meta: false,
        time: 0.0,
    }
}

/// G13 #4, scenario `key_sequence_timeout` (src/edit/key_events.js:48-60). After a
/// completed sequence CM5 keeps `keySeq` until a 50 ms timer clears it, so `Ctrl-X`
/// 120 ms later runs its own binding (goDocStart -> 0:0). Rust keeps the sequence
/// until the next key and runs `Ctrl-K Ctrl-X` again (goDocEnd -> 199:8).
#[test]
fn g13_4_key_sequence_not_cleared_by_timer() {
    let mut cm = editor();
    let mut m = KeyMap::default();
    m.bind("Ctrl-K Ctrl-X", Binding::Command("goDocEnd".into()))
        .unwrap();
    m.bind("Ctrl-X", Binding::Command("goDocStart".into()))
        .unwrap();
    cm.extra_keys = Some(KeyMapRef::Map(Rc::new(m)));
    cm.run_op(|cm| cm.doc.set_cursor(p(5, 0), None));
    cm.key_down(&KeyEvent::parse("Ctrl-K"));
    assert_eq!(cursor(&cm), (5, 0));
    cm.key_down(&KeyEvent::parse("Ctrl-X"));
    assert_eq!(cursor(&cm), (199, 8), "CM5: 199:8");
    // Within 50 ms CM5 also repeats the sequence.
    cm.run_op(|cm| cm.doc.set_cursor(p(5, 0), None));
    cm.key_down(&KeyEvent::parse("Ctrl-X"));
    assert_eq!(cursor(&cm), (199, 8), "CM5: 199:8");
    // "After 120 ms": Rust has no timer, so the elapsed time cannot matter.
    cm.run_op(|cm| cm.doc.set_cursor(p(5, 0), None));
    cm.key_down(&KeyEvent::parse("Ctrl-X"));
    const CM5_AFTER_TIMEOUT: (i64, i64) = (0, 0);
    assert_eq!(cursor(&cm), (199, 8));
    assert_ne!(cursor(&cm), CM5_AFTER_TIMEOUT);
}

/// G13 #6, scenario `drop_history` (src/edit/drop_events.js:77). An internal
/// drag-move of `line` (1:0-1:4) to 5:6. CM5 moves the cursor with
/// `setSelectionNoUndo`; Rust with `set_cursor`, which adds a selection event. In this
/// scenario the event is replaced inside the same operation (`addSelectionToHistory`,
/// `opId == hist.lastSelOp`), so the recorded history, text and undo results all
/// match CM5: no observed difference. A drop whose selection event is not merged
/// could still differ; no such scenario is recorded.
#[test]
fn g13_6_drop_history_matches_in_one_operation() {
    let mut cm = editor();
    cm.run_op(|cm| cm.doc.set_selection(p(1, 0), Some(p(1, 4)), None));
    cm.doc.clear_history();
    cm.mouse_down(&mouse(x_at(2), y_at(1), false));
    cm.mouse_move(&mouse(x_at(6), y_at(5), false));
    cm.mouse_up(Some(&mouse(x_at(6), y_at(5), false)));
    assert_eq!(cm.doc.get_line(1).as_deref(), Some(" 1"));
    assert_eq!(cm.doc.get_line(5).as_deref(), Some("line 5line"));
    assert_eq!(sels(&cm), vec![(5, 6, 5, 10)]);
    let kinds = |cm: &Editor| -> Vec<&'static str> {
        cm.doc
            .get_history()
            .done
            .iter()
            .map(|e| match e {
                HistoryEventData::Selection(_) => "selection",
                HistoryEventData::Changes(_) => "changes",
            })
            .collect()
    };
    let sel_events = |cm: &Editor| -> Vec<Vec<Sel>> {
        cm.doc
            .get_history()
            .done
            .iter()
            .filter_map(|e| match e {
                HistoryEventData::Selection(s) => Some(
                    s.ranges
                        .iter()
                        .map(|r| (r.anchor.line, r.anchor.ch, r.head.line, r.head.ch))
                        .collect(),
                ),
                HistoryEventData::Changes(_) => None,
            })
            .collect()
    };
    assert_eq!(kinds(&cm), ["selection", "changes", "selection"]);
    assert_eq!(
        sel_events(&cm),
        vec![vec![(5, 6, 5, 6)], vec![(5, 6, 5, 10)]]
    );
    cm.run_op(|cm| cm.doc.undo_selection());
    assert_eq!(sels(&cm), vec![(5, 6, 5, 6)]);
    assert_eq!(kinds(&cm), ["selection"]);
    cm.run_op(|cm| cm.doc.undo_selection());
    assert_eq!(sels(&cm), vec![(5, 6, 5, 6)]);
    cm.run_op(|cm| cm.doc.undo());
    assert_eq!(sels(&cm), vec![(5, 6, 5, 6)]);
    assert_eq!(cm.doc.get_line(1).as_deref(), Some("line 1"));
    assert_eq!(cm.doc.get_line(5).as_deref(), Some("line 5"));
}

/// G13 #7, scenario `rectangle_select_scroll` (src/edit/mouse_events.js:265). Alt-drag
/// from 2:2 to the partly visible line 13: CM5 calls `cm.scrollIntoView(pos)` and
/// scrolls to 36, the same as a plain drag (`..._control_noAlt`). Rust selects the
/// same rectangle but does not scroll.
#[test]
fn g13_7_rectangle_select_does_not_scroll() {
    let rect: Vec<Sel> = (2..=13).map(|l| (l, 2, l, 4)).collect();
    let mut cm = editor();
    cm.mouse_down(&mouse(x_at(2), y_at(2), true));
    cm.mouse_move(&mouse(x_at(4), 495.0, true));
    assert_eq!(sels(&cm), rect);
    const CM5_SCROLL_TOP: f64 = 36.0;
    assert_eq!(cm.display.scroll_top, 0.0);
    assert_ne!(cm.display.scroll_top, CM5_SCROLL_TOP);
    cm.mouse_up(None);

    // The control: a plain drag scrolls to the recorded 36 in both.
    let mut cm = editor();
    cm.mouse_down(&mouse(x_at(2), y_at(2), false));
    cm.mouse_move(&mouse(x_at(4), 495.0, false));
    assert_eq!(sels(&cm), vec![(2, 2, 13, 4)]);
    assert_eq!(cm.display.scroll_top, CM5_SCROLL_TOP);
}

/// G13 #8, scenario `drag_autoscroll_held` (src/edit/mouse_events.js:297-307). A drag
/// held still below the editor: CM5 keeps scrolling on timers (recorded scroll tops
/// 74, 74, 150, 226, 454 at 0, 100, 200, 400, 800 ms; timing dependent). Rust scrolls
/// once per move event and never while the mouse is still.
#[test]
fn g13_8_drag_autoscroll_needs_move_events() {
    let mut app = App::new(
        &lines(200),
        None,
        Metrics::oracle(),
        &AppOptions {
            no_line_numbers: true,
            ..Default::default()
        },
        Box::new(MemClipboard::default()),
    );
    app.handle(Input::Resize {
        width: 900.0,
        height: 500.0,
    });
    let (x, y) = (x_at(2), 4.0 + y_at(2));
    app.handle(Input::MouseDown {
        x,
        y,
        button: 1,
        mods: KeyEvent::default(),
        time: 0.0,
    });
    let below = Input::MouseMove {
        x,
        y: 560.0,
        mods: KeyEvent::default(),
    };
    app.handle(below.clone());
    let first = app.editor.display.scroll_top;
    assert!(first > 0.0);
    // Holding still: no input, so nothing changes, whatever the elapsed time.
    assert_eq!(app.editor.display.scroll_top, first);
    // Each further move event scrolls again (here the selection extension and the
    // one-line step of app.rs together), not 20px per 50ms on a timer.
    app.handle(below);
    assert!(app.editor.display.scroll_top > first);
}

#[test]
fn test_get_state_after() {
    let opts = cm_view::EditorOptions::default();
    let metrics = cm_view::Metrics {
        char_width: 10.0,
        text_height: 15.0,
        padding_h: 0.0,
        padding_top: 0.0,
        wide_width: 20.0,
        astral_width: 20.0,
    };
    let mut e = cm_view::Editor::new(
        "let a = 1;\nlet b = 2;",
        cm_modes::Spec::from("javascript"),
        opts,
        metrics,
    );

    // We run get_state_after(Some(0), true) -> it returns a state
    let state = e.get_state_after(Some(0), true);
    // state shouldn't be state::True for js
    assert!(matches!(state, cm_modes::State::Value(_)));

    // test that non-precise does the same initially
    let state2 = e.get_state_after(Some(0), false);
    assert!(matches!(state2, cm_modes::State::Value(_)));
}
