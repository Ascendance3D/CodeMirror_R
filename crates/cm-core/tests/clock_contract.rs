//! The time-source contract of `cm_core::clock` (L-B): host clocks, the system
//! default, unknown time, and what each does to history merging
//! (`model/history.js` `addChangeToHistory` / `addSelectionToHistory`).
//!
//! The clock is per thread and the test harness runs each test on its own thread, so
//! every test starts from the default (the system clock on native targets).

use std::cell::Cell;
use std::rc::Rc;

use cm_core::clock::{self, ClockStatus};
use cm_core::{Doc, Pos};

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}

/// Type `s` one character at a time at the end of line 0, origin `+input`.
fn type_chars(d: &Doc, s: &str, step: impl Fn()) {
    for c in s.chars() {
        let end = p(0, d.get_line(0).unwrap().len() as i64);
        d.replace_range(&c.to_string(), end, None, Some("+input"));
        step();
    }
}

#[test]
fn native_default_is_the_system_clock() {
    assert_eq!(clock::status(), ClockStatus::System);
    assert!(clock::is_available());
    let before = clock::missing_reads();
    let a = clock::try_now().expect("system time");
    let b = clock::now();
    assert!(a.is_finite() && a > 1.5e12, "epoch milliseconds, got {a}");
    assert!(
        b >= a,
        "system time does not run backward here: {a} then {b}"
    );
    assert_eq!(clock::missing_reads(), before);
}

#[test]
fn native_construction_and_edits_use_the_system_clock() {
    let d = Doc::new("");
    type_chars(&d, "abc", || {});
    // Typed within one 500 ms window: one event (more only if the machine stalls).
    assert!((1..=3).contains(&d.history_size().undo));
    d.undo();
    assert_eq!(d.get_value(None), "");
    d.redo();
    assert_eq!(d.get_value(None), "abc");
    assert_eq!(clock::missing_reads(), 0);
}

#[test]
fn deterministic_override_merges_inside_the_delay_only() {
    let t = clock::manual(1_000_000.0);
    assert_eq!(clock::status(), ClockStatus::Host);
    assert_eq!(clock::now(), 1_000_000.0);
    let d = Doc::new("");
    // 499 ms apart: `lastModTime > time - 500` holds, so the edits merge.
    type_chars(&d, "ab", || t.set(t.get() + 499.0));
    assert_eq!(d.history_size().undo, 1);
    // 500 ms apart: the comparison is strict, so a new event starts.
    t.set(t.get() + 1.0);
    type_chars(&d, "cd", || t.set(t.get() + 500.0));
    assert_eq!(d.history_size().undo, 3);
    d.undo();
    assert_eq!(d.get_value(None), "abc");
    d.undo();
    assert_eq!(d.get_value(None), "ab");
    d.undo();
    assert_eq!(d.get_value(None), "");
    assert_eq!(d.history_size().redo, 3);
}

#[test]
fn time_values_pass_through_exactly_and_backward_time_merges_like_cm5() {
    let t = clock::manual(1_234.5);
    assert_eq!(clock::now(), 1_234.5);
    assert_eq!(clock::try_now(), Some(1_234.5));
    let d = Doc::new("");
    type_chars(&d, "a", || {});
    // `Date` can step backward; CM5 then still sees `lastModTime > time - delay`.
    t.set(100.0);
    type_chars(&d, "b", || {});
    assert_eq!(d.history_size().undo, 1);
    // Far forward: no merge.
    t.set(1e9);
    type_chars(&d, "c", || {});
    assert_eq!(d.history_size().undo, 2);
}

#[test]
fn restoring_the_default_returns_to_the_system_clock() {
    clock::manual(5.0);
    assert_eq!(clock::now(), 5.0);
    clock::set_override(None);
    assert_eq!(clock::status(), ClockStatus::System);
    assert!(clock::now() > 1.5e12);
}

#[test]
fn missing_time_never_traps_and_never_merges() {
    // A host clock that has no time stands in for wasm32 without a host clock
    // (the wasm runtime test covers the real target).
    clock::set_host_clock(Some(Box::new(|| None)));
    assert_eq!(clock::try_now(), None);
    assert!(clock::now().is_nan());
    let reads = clock::missing_reads();
    assert!(reads >= 2);

    let d = Doc::new("x");
    assert!(
        clock::missing_reads() > reads,
        "construction read the clock"
    );
    type_chars(&d, "abc", || {});
    // Unknown time is outside the merge window: one event per keystroke, never fewer.
    assert_eq!(d.history_size().undo, 3);
    d.undo();
    assert_eq!(d.get_value(None), "xab");
    d.undo();
    d.undo();
    assert_eq!(d.get_value(None), "x");
    d.redo();
    assert_eq!(d.get_value(None), "xa");
    // Without an editor there is no operation id (CM5 passes NaN), so only time
    // could merge these two; unknown time keeps them separate.
    let d2 = Doc::new("");
    d2.operation(|| {
        d2.replace_range("a", p(0, 0), None, Some("+input"));
        d2.replace_range("b", p(0, 1), None, Some("+input"));
    });
    assert_eq!(d2.history_size().undo, 2);
}

#[test]
fn non_finite_host_time_is_unknown() {
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        clock::set_override(Some(Box::new(move || v)));
        let before = clock::missing_reads();
        assert_eq!(clock::try_now(), None, "{v}");
        assert_eq!(clock::missing_reads(), before + 1);
    }
}

#[test]
fn host_clock_can_lose_and_regain_time() {
    let t: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(Some(1_000.0)));
    let c = t.clone();
    clock::set_host_clock(Some(Box::new(move || c.get())));
    let d = Doc::new("");
    type_chars(&d, "ab", || {});
    assert_eq!(d.history_size().undo, 1);
    t.set(None);
    type_chars(&d, "c", || {});
    assert_eq!(d.history_size().undo, 2);
    t.set(Some(1_100.0));
    // The last event's time is unknown, so this one does not merge into it either.
    type_chars(&d, "d", || {});
    assert_eq!(d.history_size().undo, 3);
    type_chars(&d, "e", || {});
    assert_eq!(d.history_size().undo, 3);
    assert_eq!(d.get_value(None), "abcde");
}

struct DropFlag(Rc<Cell<u32>>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn host_callback_lifetime() {
    let drops = Rc::new(Cell::new(0));
    let flag = DropFlag(drops.clone());
    clock::set_override(Some(Box::new(move || {
        let _keep = &flag;
        7.0
    })));
    assert_eq!(clock::now(), 7.0);
    assert_eq!(
        drops.get(),
        0,
        "the thread owns the callback while installed"
    );
    clock::set_override(Some(Box::new(|| 8.0)));
    assert_eq!(drops.get(), 1, "replacing drops the old callback");
    assert_eq!(clock::now(), 8.0);

    // A callback whose destructor reads the clock (no re-entrant borrow panic).
    let seen = Rc::new(Cell::new(0.0));
    struct ReadOnDrop(Rc<Cell<f64>>);
    impl Drop for ReadOnDrop {
        fn drop(&mut self) {
            self.0.set(clock::now());
        }
    }
    let r = ReadOnDrop(seen.clone());
    clock::set_override(Some(Box::new(move || {
        let _keep = &r;
        9.0
    })));
    clock::set_override(Some(Box::new(|| 10.0)));
    assert_eq!(
        seen.get(),
        10.0,
        "the old callback drops after the new one is in"
    );

    // A callback that replaces itself while running.
    clock::set_override(Some(Box::new(|| {
        clock::set_override(Some(Box::new(|| 2.0)));
        1.0
    })));
    assert_eq!(clock::now(), 1.0);
    assert_eq!(clock::now(), 2.0);
    clock::set_override(None);
    assert_eq!(clock::status(), ClockStatus::System);
}

#[test]
fn repeated_construction_reads_one_clock() {
    let t = clock::manual(50_000.0);
    for i in 0..200 {
        let d = Doc::new("abc\ndef");
        d.replace_range("x", p(1, 0), None, Some("+input"));
        assert_eq!(d.get_value(None), "abc\nxdef", "iteration {i}");
        d.undo();
        assert_eq!(d.get_value(None), "abc\ndef");
        t.set(t.get() + 1.0);
    }
    assert_eq!(clock::missing_reads(), 0);
}

#[test]
fn clock_is_per_thread() {
    clock::manual(3.0);
    let other = std::thread::spawn(|| (clock::status(), clock::now() > 1.5e12))
        .join()
        .unwrap();
    assert_eq!(other, (ClockStatus::System, true));
    assert_eq!(clock::now(), 3.0);
}
