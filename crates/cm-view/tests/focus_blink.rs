//! `onFocus` / `onBlur` (src/display/focus.js:21,40) and `restartBlink`
//! (src/display/selection.js:160) with the `cursorBlinkRate` option, on a host
//! clock (`cm_core::clock::set_override`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, KeyEvent, Metrics};

thread_local! {
    static NOW: Cell<f64> = const { Cell::new(1000.0) };
}

fn set_now(t: f64) {
    NOW.with(|n| n.set(t));
}

fn editor(rate: f64) -> Editor {
    cm_core::clock::set_override(Some(Box::new(|| NOW.with(|n| n.get()))));
    set_now(1000.0);
    Editor::new(
        "abc\ndef",
        Spec::Null,
        EditorOptions {
            cursor_blink_rate: rate,
            ..Default::default()
        },
        Metrics::test_grid(),
    )
}

/// Focus restarts the blink: the cursor shows at once and toggles every
/// `cursorBlinkRate` ms; blurred, no cursor shows.
#[test]
fn blink_follows_focus_and_rate() {
    let mut cm = editor(530.0);
    assert!(!cm.cursor_blink_visible(), "not focused");
    set_now(5000.0);
    cm.set_focused(true);
    assert!(cm.cursor_blink_visible());
    assert_eq!(cm.next_blink_in(), Some(530.0));
    set_now(5529.0);
    assert!(cm.cursor_blink_visible());
    set_now(5530.0);
    assert!(!cm.cursor_blink_visible());
    assert_eq!(cm.next_blink_in(), Some(530.0));
    set_now(6060.0);
    assert!(cm.cursor_blink_visible());
    // An operation that changed the selection restarts it (endOperation_finish).
    set_now(6600.0);
    assert!(!cm.cursor_blink_visible());
    cm.run_op(|cm| cm.doc.set_cursor(Pos::new(1, 1), None));
    assert!(cm.cursor_blink_visible());
    set_now(6700.0);
    // One that did not, does not.
    cm.run_op(|_| {});
    set_now(7130.0);
    assert!(!cm.cursor_blink_visible());
    // A handled key restarts it (dispatchKeyInner).
    let right = KeyEvent {
        key: "Right".into(),
        ..Default::default()
    };
    cm.run_op(|cm| cm.doc.set_cursor(Pos::new(1, 3), None));
    set_now(7700.0);
    assert!(!cm.cursor_blink_visible());
    assert!(cm.key_down(&right));
    assert!(cm.cursor_blink_visible());
    cm.set_focused(false);
    assert!(!cm.cursor_blink_visible());
    assert_eq!(cm.next_blink_in(), None);
    cm_core::clock::set_override(None);
}

/// `cursorBlinkRate` 0: the cursor shows without blinking; negative: hidden.
#[test]
fn blink_rate_zero_and_negative() {
    let mut cm = editor(0.0);
    cm.set_focused(true);
    set_now(100_000.0);
    assert!(cm.cursor_blink_visible());
    assert_eq!(cm.next_blink_in(), None);
    let mut cm = editor(-1.0);
    cm.set_focused(true);
    assert!(!cm.cursor_blink_visible());
    assert_eq!(cm.next_blink_in(), None);
    // Changing the option takes effect at the next restart.
    cm.options.cursor_blink_rate = 200.0;
    cm.restart_blink();
    assert!(cm.cursor_blink_visible());
    assert_eq!(cm.next_blink_in(), Some(200.0));
    cm_core::clock::set_override(None);
}

/// `onBlur` resets `display.shift` 150 ms later, unless the editor was focused
/// again by then; `focus` / `blur` fire only on a state change, and a repeated
/// focus still restarts the blink.
#[test]
fn blur_resets_shift_after_150ms() {
    let mut cm = editor(530.0);
    let log = Rc::new(RefCell::new(Vec::<&str>::new()));
    let l = log.clone();
    cm.on_focus(move |_| l.borrow_mut().push("focus"));
    let l = log.clone();
    cm.on_blur(move |_| l.borrow_mut().push("blur"));
    cm.set_focused(true);
    cm.display.shift = true;
    set_now(2000.0);
    cm.set_focused(false);
    assert!(cm.display.shift);
    assert_eq!(cm.next_timer_in(), Some(150.0));
    set_now(2149.0);
    cm.run_timers();
    assert!(cm.display.shift);
    set_now(2150.0);
    cm.run_timers();
    assert!(!cm.display.shift);
    assert_eq!(cm.next_timer_in(), None);
    // Focused again before the timer: shift stays.
    cm.set_focused(true);
    cm.display.shift = true;
    set_now(3000.0);
    cm.set_focused(false);
    cm.set_focused(true);
    set_now(3200.0);
    cm.run_timers();
    assert!(cm.display.shift);
    // A repeated focus restarts the blink without a second `focus` event.
    set_now(3600.0);
    assert!(!cm.cursor_blink_visible());
    cm.set_focused(true);
    assert!(cm.cursor_blink_visible());
    assert_eq!(*log.borrow(), ["focus", "blur", "focus", "blur", "focus"]);
    cm_core::clock::set_override(None);
}
