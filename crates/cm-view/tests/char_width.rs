//! R4: the longest line when the host reports a character width of 0, as a browser
//! does for an editor in a hidden or not yet laid out element.
//!
//! `updateHeightsInViewport` (update_lines.js:39-45) counts a rendered line wider
//! than `sizerWidth` as `Math.ceil(width / charWidth(display))` characters, and
//! `charWidth` returns `width || 10` (position_measurement.js:614), so a width of 0
//! never divides. These metrics give normal characters no width and a CJK
//! character 20px, so a line of CJK text is wider than `sizerWidth` while
//! `char_width` is 0. A rendered line is `paddingH + text - 1` wide, so `n` CJK
//! characters count as `ceil((3 + 20n) / 10)`.

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};

const CJK: &str = "\u{4e00}";

/// `char_width` 0, a CJK character 20px wide.
fn zero() -> Metrics {
    let mut m = Metrics::monospace(0.0, 13.0);
    m.wide_width = 20.0;
    m
}

/// `char_width` 10, a CJK character 20px wide.
fn valid() -> Metrics {
    Metrics::monospace(10.0, 13.0)
}

fn editor(text: &str, m: Metrics) -> Editor {
    Editor::new(text, Spec::Null, EditorOptions::default(), m)
}

fn cjk(n: usize) -> String {
    CJK.repeat(n)
}

fn replace(cm: &mut Editor, text: &str, from: Pos, to: Option<Pos>) {
    cm.run_op(|cm| cm.doc.replace_range(text, from, to, None));
}

/// The `triage` probe `p12_4_zero_char_width_wide_text`, with the finite length
/// CM5 computes: `ceil(63 / 10)`, where it was `i64::MAX`.
#[test]
fn zero_char_width_wide_text_is_not_saturated() {
    let mut cm = editor(&format!("ab\n{}", cjk(3)), zero());
    assert_eq!(cm.max_line(), Some((1, 7)));
    replace(&mut cm, "x", Pos::new(0, 0), None);
    assert_eq!(cm.max_line(), Some((1, 7)));
    cm.set_size(400.0, 300.0);
    replace(&mut cm, "x", Pos::new(0, 0), None);
    assert_eq!(cm.max_line(), Some((1, 7)));
}

#[test]
fn zero_char_width_short_lines() {
    // Each line is 3px wide: `ceil(3 / 10)` = 1.
    assert_eq!(editor("a\nb", zero()).max_line(), Some((0, 1)));
}

#[test]
fn zero_char_width_empty_document() {
    assert_eq!(editor("", zero()).max_line(), Some((0, 1)));
}

#[test]
fn zero_char_width_long_line() {
    let cm = editor(&format!("{}\nab", cjk(200)), zero());
    assert_eq!(cm.max_line(), Some((0, 401)));
}

/// Insertion, deletion and replacement keep tracking the longest line, through the
/// change bookkeeping (`lineLength`), `findMaxLine` and the width check.
#[test]
fn zero_char_width_tracks_edits() {
    let mut cm = editor(&format!("ab\n{}", cjk(3)), zero());
    assert_eq!(cm.max_line(), Some((1, 7)));
    // A line longer than `maxLineLength` becomes the longest line.
    replace(&mut cm, "\nabcdefghijklmnop", Pos::new(1, 3), None);
    assert_eq!(cm.max_line(), Some((2, 16)));
    // Deleting it finds the longest line again; the width check counts the CJK line.
    replace(&mut cm, "", Pos::new(1, 3), Some(Pos::new(2, 16)));
    assert_eq!(cm.max_line(), Some((1, 7)));
    assert_eq!(cm.get_value(), format!("ab\n{}", cjk(3)));
    // Replacing the longest line: `findMaxLine` gives 5, the width check 11.
    replace(&mut cm, &cjk(5), Pos::new(1, 0), Some(Pos::new(1, 3)));
    assert_eq!(cm.max_line(), Some((1, 11)));
    // A new, wider line is found by the width check alone.
    replace(&mut cm, &format!("\n{}", cjk(8)), Pos::new(1, 5), None);
    assert_eq!(cm.max_line(), Some((2, 17)));
    // Removing the longest line: `findMaxLine` picks line 1, which is no wider
    // than the measured `sizerWidth`.
    replace(&mut cm, "", Pos::new(1, 5), Some(Pos::new(2, 8)));
    assert_eq!(cm.max_line(), Some((1, 5)));
}

/// A host that measures 0 after a valid measurement (the element was hidden).
#[test]
fn char_width_becomes_zero_after_a_valid_measurement() {
    let mut cm = editor(&format!("ab\n{}", cjk(3)), valid());
    assert_eq!(cm.max_line(), Some((1, 7)));
    cm.metrics.char_width = 0.0;
    // `findMaxLine` picks line 0, whose measured width (3 + 4) makes the 43px CJK
    // line count: `ceil(43 / 10)`.
    replace(&mut cm, &cjk(2), Pos::new(1, 0), Some(Pos::new(1, 3)));
    assert_eq!(cm.max_line(), Some((1, 5)));
}

/// A host that measures a valid width after 0 (the element was shown).
#[test]
fn char_width_recovers_after_a_zero_measurement() {
    let mut cm = editor(&format!("ab\n{}", cjk(3)), zero());
    assert_eq!(cm.max_line(), Some((1, 7)));
    cm.metrics = valid();
    // `lineLength` 20, then the width check before the line is measured:
    // `ceil(203 / 10)`.
    replace(&mut cm, "\nabcdefghijklmnopqrst", Pos::new(1, 3), None);
    assert_eq!(cm.max_line(), Some((2, 21)));
    replace(&mut cm, "", Pos::new(1, 3), Some(Pos::new(2, 20)));
    assert_eq!(cm.max_line(), Some((1, 3)));
    replace(
        &mut cm,
        &format!("\n{}", "y".repeat(30)),
        Pos::new(1, 3),
        None,
    );
    assert_eq!(cm.max_line(), Some((2, 31)));
}

#[test]
fn repeated_measurements_keep_the_longest_line() {
    let mut cm = editor(&format!("ab\n{}", cjk(3)), zero());
    for i in 0..5 {
        cm.set_size(300.0 + 50.0 * i as f64, 200.0 + 20.0 * i as f64);
        cm.run_op(|_| {});
        assert_eq!(cm.max_line(), Some((1, 7)), "{i}");
    }
}

/// Valid widths: the values the base commit (322d5f0) computes.
#[test]
fn valid_char_widths_are_unchanged() {
    let text = format!("ab\n{}", cjk(3));
    assert_eq!(editor(&text, valid()).max_line(), Some((1, 7)));
    // DejaVu Sans Mono 32px: `ceil(99 / 19.265625)`.
    assert_eq!(editor(&text, Metrics::oracle()).max_line(), Some((1, 6)));
    // Small but not 0: the measured width is used, not 10.
    for (cw, want) in [(0.5, 126), (1e-3, 63_000)] {
        let mut m = Metrics::monospace(cw, 13.0);
        m.wide_width = 20.0;
        assert_eq!(editor(&text, m).max_line(), Some((1, want)), "{cw}");
    }
    let mut cm = editor("ab\ncdef\ng", valid());
    assert_eq!(cm.max_line(), Some((1, 5)));
    replace(&mut cm, "\nhijklmnop", Pos::new(2, 1), None);
    // The width check counts the 93px line before it is measured: `ceil(93 / 10)`.
    assert_eq!(cm.max_line(), Some((3, 10)));
    // `findMaxLine` picks "cdef"; no line is wider than the measured `sizerWidth`.
    replace(&mut cm, "", Pos::new(2, 1), Some(Pos::new(3, 9)));
    assert_eq!(cm.max_line(), Some((1, 4)));
}

#[test]
fn editors_keep_their_own_longest_line() {
    let text = format!("ab\n{}", cjk(3));
    let mut a = editor(&text, zero());
    let mut b = editor(&text, valid());
    let mut c = editor(&text, zero());
    replace(&mut a, "\nabcdefghijklmnop", Pos::new(1, 3), None);
    b.metrics.char_width = 0.0;
    replace(&mut b, &cjk(2), Pos::new(1, 0), Some(Pos::new(1, 3)));
    replace(&mut c, "x", Pos::new(0, 0), None);
    assert_eq!(a.max_line(), Some((2, 16)));
    assert_eq!(b.max_line(), Some((1, 5)));
    assert_eq!(c.max_line(), Some((1, 7)));
}

/// The width of a character changes only the longest-line state: the text, the
/// selection and the history are those of the same edits with a valid width.
#[test]
fn zero_char_width_keeps_document_selection_and_history() {
    let text = format!("ab\n{}", cjk(3));
    let run = |m: Metrics| {
        let mut cm = editor(&text, m);
        replace(&mut cm, "\nabcdefghijklmnop", Pos::new(1, 3), None);
        replace(&mut cm, &cjk(5), Pos::new(1, 0), Some(Pos::new(1, 3)));
        replace(&mut cm, "", Pos::new(0, 0), Some(Pos::new(0, 1)));
        let edited = (cm.get_value(), cm.list_selections(), cm.doc.history_size());
        cm.run_op(|cm| {
            cm.doc.undo();
            cm.doc.undo();
        });
        let undone = (cm.get_value(), cm.list_selections(), cm.doc.history_size());
        cm.run_op(|cm| cm.doc.redo());
        let redone = (cm.get_value(), cm.list_selections(), cm.doc.history_size());
        (edited, undone, redone)
    };
    let (z, v) = (run(zero()), run(valid()));
    assert_eq!(z, v);
    assert_eq!(z.0.0, format!("b\n{}\nabcdefghijklmnop", cjk(5)));
    assert_eq!(z.1.0, format!("ab\n{}\nabcdefghijklmnop", cjk(3)));
}
