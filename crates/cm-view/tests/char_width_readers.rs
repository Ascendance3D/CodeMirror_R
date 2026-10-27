//! R4b: the other callers of `charWidth(display)`, which returns `width || 10`
//! (position_measurement.js:607-615), so a width of 0, -0 or NaN, as a hidden or
//! not yet laid out element measures, counts as 10 and every other value is used
//! as measured:
//!
//! * `defaultCharWidth()` (src/edit/methods.js:220);
//! * `posFromMouse(cm, e, liberal, forRect)` (position_measurement.js:683-687), the
//!   column of a rectangular selection past the end of a line;
//! * `estimateCoords(cm, pos)` (position_measurement.js:399-406), which
//!   `resolveScrollToPos` (src/display/scrolling.js:140-146) uses when another
//!   scroll request follows a queued one in the same operation.
//!
//! The expected values are CM5 5.65.21 in headless Chromium 151, Firefox 153 and
//! WebKit 26.5, with the `charWidth` measurement span at `font-size: 0`
//! (C-CHARWIDTH-02 lane recording; the three engines agree). Text layout keeps the
//! host's widths: these metrics give normal characters `char_width` and a CJK
//! character 20px.

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics, MouseEvent};

const CJK: &str = "\u{4e00}";

fn metrics(char_width: f64) -> Metrics {
    let mut m = Metrics::monospace(char_width, 13.0);
    m.wide_width = 20.0;
    m
}

/// A 400x200 editor, as in the recording.
fn editor(text: &str, char_width: f64, wrap: bool) -> Editor {
    let options = EditorOptions {
        line_wrapping: wrap,
        ..EditorOptions::default()
    };
    let mut cm = Editor::new(text, Spec::Null, options, metrics(char_width));
    cm.set_size(400.0, 200.0);
    cm
}

fn alt_mouse(x: f64, y: f64) -> MouseEvent {
    MouseEvent {
        x,
        y,
        button: 1,
        shift: false,
        ctrl: false,
        alt: true,
        meta: false,
        time: 0.0,
    }
}

type Sel = (i64, i64, i64, i64);

fn sels(cm: &Editor) -> Vec<Sel> {
    cm.doc
        .list_selections()
        .iter()
        .map(|s| (s.anchor.line, s.anchor.ch, s.head.line, s.head.ch))
        .collect()
}

/// `w || 10` for the widths the recording checked (0, -0 and NaN give 10).
const FALLBACK: [f64; 3] = [0.0, -0.0, f64::NAN];
/// Widths CM5 returns unchanged.
const UNCHANGED: [f64; 6] = [0.5, 1e-3, -1.0, f64::INFINITY, 10.0, 7.828125];

#[test]
fn default_char_width_counts_a_zero_or_nan_width_as_10() {
    for w in FALLBACK {
        assert_eq!(editor("ab", w, false).default_char_width(), 10.0, "{w:?}");
    }
    for w in UNCHANGED {
        assert_eq!(editor("ab", w, false).default_char_width(), w, "{w:?}");
    }
    // The host metric itself is kept.
    assert_eq!(editor("ab", 0.0, false).metrics.char_width, 0.0);
    assert_eq!(editor("", 0.0, false).default_char_width(), 10.0);
}

/// CM5 does not cache a 0 (only `width > 2`), so a later measurement is used:
/// hidden 10, shown again 7.828125 (Chromium), repeatedly.
#[test]
fn default_char_width_follows_metric_changes() {
    let mut cm = editor("ab", 7.828125, false);
    for _ in 0..3 {
        assert_eq!(cm.default_char_width(), 7.828125);
        cm.set_metrics(metrics(0.0));
        assert_eq!(cm.default_char_width(), 10.0);
        cm.set_metrics(metrics(7.828125));
    }
    assert_eq!(cm.default_char_width(), 7.828125);
}

/// `posFromMouse(..., forRect)` past the end of line 0 (`ab`): column
/// `round((x - paddingH) / charWidth)`. Recorded: x 104, 204, 255 give 10, 20, 25.
#[test]
fn rectangle_column_past_the_line_end_matches_cm5() {
    let text = format!("ab\n{}", "x".repeat(60));
    for w in FALLBACK.into_iter().filter(|w| !w.is_nan()) {
        let cm = editor(&text, w, false);
        let y = cm.metrics.text_height / 2.0;
        for (x, col) in [(104.0, 10), (204.0, 20), (255.0, 25)] {
            assert_eq!(cm.pos_from_mouse(x, y, true).ch, col, "{w:?} {x}");
            // Not a rectangle: the end of the line, as before.
            assert_eq!(cm.pos_from_mouse(x, y, false).ch, 2, "{w:?} {x}");
        }
    }
    // An empty document.
    let cm = editor("", 0.0, false);
    assert_eq!(cm.pos_from_mouse(204.0, 6.5, true).ch, 20);
}

/// Measured widths keep their result: `round(100 / 0.5)`, `round(100 / 1e-3)`, a
/// negative or infinite width gives column 0 (`Math.max(0, ...)`).
#[test]
fn rectangle_column_with_a_measured_width_is_unchanged() {
    for (w, col) in [
        (0.5, 200),
        (1e-3, 100_000),
        (-1.0, 0),
        (f64::INFINITY, 0),
        (10.0, 10),
    ] {
        let cm = editor("ab", w, false);
        assert_eq!(cm.pos_from_mouse(104.0, 6.5, true).ch, col, "{w:?}");
    }
}

/// An Alt-drag from line 1 ch 0 to x 104, 204, 255 past the end of line 0. CM5
/// recorded `[[0,0,0,2],[1,0,1,col]]`. Line 1 is CJK so the press lands on ch 0
/// with zero-width normal characters.
#[test]
fn rectangle_drag_selects_the_cm5_columns() {
    for w in [0.0, -0.0, 10.0] {
        for (x, col) in [(104.0, 10), (204.0, 20), (255.0, 25)] {
            let mut cm = editor(&format!("ab\n{}", CJK.repeat(60)), w, false);
            let (lh, pt) = (cm.metrics.text_height, cm.metrics.padding_top);
            cm.mouse_down(&alt_mouse(cm.metrics.padding_h + 1.0, pt + 1.5 * lh));
            cm.mouse_move(&alt_mouse(x, pt + 0.5 * lh));
            cm.mouse_up(None);
            assert_eq!(sels(&cm), vec![(0, 0, 0, 2), (1, 0, 1, col)], "{w:?} {x}");
        }
    }
}

/// `setCursor` then `scrollTo(null, 0)` in one operation (Rust: `add_to_scroll_top`,
/// the same `resolveScrollToPos`): the estimate puts the cursor at
/// `charWidth * ch`, so scrollLeft is `10 * ch + 10 - 400`. Recorded: ch 0, 100,
/// 150 give 0, 610, 1110 without wrapping and 0 with wrapping.
#[test]
fn scroll_estimate_matches_cm5() {
    let text = format!("{}\nab", CJK.repeat(400));
    for w in [0.0, -0.0, f64::NAN, 10.0] {
        for (wrap, expect) in [(false, [0.0, 610.0, 1110.0]), (true, [0.0; 3])] {
            for (ch, left) in [0, 100, 150].into_iter().zip(expect) {
                let mut cm = editor(&text, w, wrap);
                cm.run_op(|cm| {
                    cm.doc.set_cursor(Pos::new(0, ch), None);
                    cm.add_to_scroll_top(0.0);
                });
                assert_eq!(cm.display.scroll_left, left, "{w:?} wrap {wrap} ch {ch}");
            }
        }
    }
}

/// A measured small width keeps its estimate (`0.5 * 150` is on screen), and a
/// short line needs no scroll.
#[test]
fn scroll_estimate_with_a_measured_width_is_unchanged() {
    let mut cm = editor(&format!("{}\nab", CJK.repeat(400)), 0.5, false);
    cm.run_op(|cm| {
        cm.doc.set_cursor(Pos::new(0, 150), None);
        cm.add_to_scroll_top(0.0);
    });
    assert_eq!(cm.display.scroll_left, 0.0);
    let mut cm = editor("ab", 0.0, false);
    cm.run_op(|cm| {
        cm.doc.set_cursor(Pos::new(0, 2), None);
        cm.add_to_scroll_top(0.0);
    });
    assert_eq!(cm.display.scroll_left, 0.0);
}

/// Two editors: each reads its own metrics.
#[test]
fn editors_use_their_own_char_width() {
    let (zero, valid) = (editor("ab", 0.0, false), editor("ab", 4.0, false));
    assert_eq!(zero.pos_from_mouse(204.0, 6.5, true).ch, 20);
    assert_eq!(valid.pos_from_mouse(204.0, 6.5, true).ch, 50);
    assert_eq!(zero.default_char_width(), 10.0);
    assert_eq!(valid.default_char_width(), 4.0);
}

/// A metric change through `set_metrics` (the `refresh()` port): valid, hidden
/// (every width 0), an edit, valid again, an edit, the longest line edited and
/// removed, then repeated changes. As in CM5 (`hidden + refresh` ... `toggle`
/// in the recording), a hidden editor keeps its longest line, `defaultCharWidth`
/// is 10 while hidden, and tracking continues once the width is valid.
#[test]
fn longest_line_recovers_after_a_hidden_measurement() {
    let hidden = || {
        let mut m = Metrics::monospace(0.0, 13.0);
        m.wide_width = 0.0;
        m.astral_width = 0.0;
        m
    };
    let mut cm = Editor::new(
        &format!("ab\n{}", CJK.repeat(3)),
        Spec::Null,
        EditorOptions::default(),
        metrics(10.0),
    );
    let insert = |cm: &mut Editor, text: &str, at: Pos| {
        cm.run_op(|cm| cm.doc.replace_range(text, at, None, None))
    };
    assert_eq!(cm.max_line(), Some((1, 7)));
    cm.set_metrics(hidden());
    assert_eq!(
        (cm.max_line(), cm.default_char_width()),
        (Some((1, 7)), 10.0)
    );
    insert(&mut cm, "x", Pos::new(0, 0));
    assert_eq!(cm.max_line(), Some((1, 7)));
    cm.set_metrics(metrics(10.0));
    assert_eq!(
        (cm.max_line(), cm.default_char_width()),
        (Some((1, 7)), 10.0)
    );
    insert(&mut cm, "x", Pos::new(0, 0));
    assert_eq!(cm.max_line(), Some((1, 7)));
    insert(&mut cm, &CJK.repeat(2), Pos::new(1, 3));
    assert_eq!(cm.max_line(), Some((1, 11)));
    cm.run_op(|cm| {
        cm.doc
            .replace_range("", Pos::new(0, 4), Some(Pos::new(1, 5)), None)
    });
    assert_eq!(cm.max_line(), Some((0, 4)));
    cm.set_value(&format!("ab\n{}", CJK.repeat(3)));
    for _ in 0..3 {
        cm.set_metrics(hidden());
        assert_eq!(cm.max_line(), Some((1, 7)));
        cm.set_metrics(metrics(10.0));
        assert_eq!(cm.max_line(), Some((1, 7)));
    }
}
