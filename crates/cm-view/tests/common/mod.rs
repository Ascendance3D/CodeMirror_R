//! Test helpers mirroring `test/driver.js`, the top of `test/test.js` and the
//! helpers in `test/multi_test.js`.
#![allow(dead_code)]

use cm_core::Pos;
use cm_core::pos::Sticky;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};

/// `Pos(line, ch)`.
pub fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}
/// `Pos(line, ch, "before")`.
pub fn pb(line: i64, ch: i64) -> Pos {
    Pos::with_sticky(line, ch, Some(Sticky::Before))
}
/// `Pos(line, ch, "after")`.
pub fn pa(line: i64, ch: i64) -> Pos {
    Pos::with_sticky(line, ch, Some(Sticky::After))
}
/// `Pos(line)`.
pub fn pl(line: i64) -> Pos {
    Pos::line_end(line)
}

/// `testCM(name, run, opts)`: an editor with the test page's defaults. The test
/// page (`test/index.html`) loads `mode/css/css.js` first, so `CodeMirror.defaults.mode`
/// is `"css"`; `test.js` sets `CodeMirror.defaults.rtlMoveVisually = true`. The
/// default `.CodeMirror` height is 300px. The clock is frozen (the JS tests run
/// within milliseconds, so every timed history merge is taken).
pub fn cm_opts(value: &str, mode: Option<&str>, f: impl FnOnce(&mut EditorOptions)) -> Editor {
    cm_core::clock::manual(1_000_000.0);
    let mut o = EditorOptions {
        rtl_move_visually: true,
        mac: false,
        ..Default::default()
    };
    f(&mut o);
    let spec = Spec::from(mode.unwrap_or("css"));
    let mut ed = Editor::new(value, spec, o, Metrics::test_grid());
    ed.set_size(600.0, 300.0);
    ed
}
pub fn cm(value: &str) -> Editor {
    cm_opts(value, None, |_| {})
}
pub fn cm_mode(value: &str, mode: &str) -> Editor {
    cm_opts(value, Some(mode), |_| {})
}

/// `addDoc(cm, width, height)`.
pub fn add_doc(cm: &mut Editor, width: usize, height: usize) {
    let line = "x".repeat(width);
    let content = vec![line; height].join("\n");
    cm.set_value(&content);
}

/// `eqCharPos(a, b)`.
#[track_caller]
pub fn eq_char_pos(a: Pos, b: Pos) {
    assert_eq!((a.line, a.ch), (b.line, b.ch), "{a:?} != {b:?}");
}
/// `eqCursorPos(a, b)`: also compares `sticky`.
#[track_caller]
pub fn eq_cursor_pos(a: Pos, b: Pos) {
    eq_char_pos(a, b);
    assert_eq!(a.sticky, b.sticky, "sticky of {a:?} != {b:?}");
}

/// A `select(cm, ...)` argument: a cursor or an `{anchor, head}` range.
#[derive(Clone, Copy)]
pub enum S {
    C(Pos),
    R(Pos, Pos),
}

/// `select(cm, ...)` (multi_test.js): primary is the last one.
pub fn select(cm: &mut Editor, sels: &[S]) {
    let rs: Vec<(Pos, Option<Pos>)> = sels
        .iter()
        .map(|s| match *s {
            S::C(p) => (p, Some(p)),
            S::R(a, h) => (a, Some(h)),
        })
        .collect();
    let n = rs.len();
    cm.run_op(|cm| cm.doc.set_selections(&rs, Some(n - 1), None));
}

/// `hasSelections(cm, al, ac, hl, hc, ...)`.
#[track_caller]
pub fn has_selections(cm: &Editor, s: &[(i64, i64, i64, i64)]) {
    let sels = cm.doc.list_selections();
    assert_eq!(
        sels.len(),
        s.len(),
        "expected {} selections, found {}: {sels:?}",
        s.len(),
        sels.len()
    );
    for (i, (r, e)) in sels.iter().zip(s).enumerate() {
        assert_eq!(
            (r.anchor.line, r.anchor.ch),
            (e.0, e.1),
            "anchor of selection {i}"
        );
        assert_eq!(
            (r.head.line, r.head.ch),
            (e.2, e.3),
            "head of selection {i}"
        );
    }
}

/// `hasCursors(cm, l, c, ...)`.
#[track_caller]
pub fn has_cursors(cm: &Editor, s: &[(i64, i64)]) {
    let sels = cm.doc.list_selections();
    assert_eq!(
        sels.len(),
        s.len(),
        "expected {} selections, found {}: {sels:?}",
        s.len(),
        sels.len()
    );
    for (i, (r, e)) in sels.iter().zip(s).enumerate() {
        eq_cursor_pos(r.anchor, r.head);
        assert_eq!((r.head.line, r.head.ch), (e.0, e.1), "selection {i}");
    }
}

/// `cm.getCursor()`.
pub fn cur(cm: &Editor) -> Pos {
    cm.doc.get_cursor(None)
}
