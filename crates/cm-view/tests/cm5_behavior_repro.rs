//! Recorded CodeMirror 5 behavior for the differences found by the 2026-10-25 coverage
//! audit (08_COVERAGE_AUDIT.md, "Behavior differences").
//!
//! `oracle/cm5-behavior-repro.mjs` ran each scenario on a real CodeMirror 5 editor in
//! headless chromium (DejaVu Sans Mono 32px / 38px lines, 900x500, no wrapping). The
//! expected values below are copied from its output; each test names its scenario.

use cm_core::{Pos, SelOptions};
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};

/// A selection range as `[anchor.line, anchor.ch, head.line, head.ch]`.
type Sel = (i64, i64, i64, i64);

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}

/// The recording page: `lines(200)`, null mode, `Metrics::oracle()`, 900x500.
fn editor() -> Editor {
    let text: Vec<String> = (0..200).map(|i| format!("line {i}")).collect();
    let mut cm = Editor::new(
        &text.join("\n"),
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
    );
    cm.display.window_height = Some(900.0);
    cm.set_size(900.0, 500.0);
    cm
}

fn no_scroll() -> Option<SelOptions> {
    Some(SelOptions {
        scroll: Some(false),
        ..Default::default()
    })
}

/// Sets the cursor to 150:0 without scrolling, runs `act` in an operation, and
/// returns the scroll top before and after with the selections after.
fn far(act: impl FnOnce(&mut Editor)) -> (f64, f64, Vec<Sel>) {
    let mut cm = editor();
    cm.run_op(|cm| cm.doc.set_cursor(p(150, 0), no_scroll()));
    let before = cm.display.scroll_top;
    cm.run_op(act);
    (before, cm.display.scroll_top, sels(&cm))
}

fn sels(cm: &Editor) -> Vec<Sel> {
    cm.doc
        .list_selections()
        .iter()
        .map(|r| (r.anchor.line, r.anchor.ch, r.head.line, r.head.ch))
        .collect()
}

/// G13 #1, scenario `replaceSelection_noCollapse`: `replaceSelections` without a
/// collapse argument calls `ensureCursorVisible` (src/model/Doc.js:184).
#[test]
fn replace_selection_no_collapse_scrolls() {
    let r = far(|cm| cm.doc.replace_selection("x", None, None));
    assert_eq!(r, (0.0, 5242.0, vec![(150, 1, 150, 1)]));
}

/// Scenario `replaceSelection_end`: `"end"` takes the same branch as no collapse.
#[test]
fn replace_selection_end_scrolls() {
    let r = far(|cm| cm.doc.replace_selection("x", Some("end"), None));
    assert_eq!(r, (0.0, 5242.0, vec![(150, 1, 150, 1)]));
}

/// Scenario `replaceSelection_around` (`setSelectionReplaceHistory` scrolls).
#[test]
fn replace_selection_around_scrolls() {
    let r = far(|cm| cm.doc.replace_selection("x", Some("around"), None));
    assert_eq!(r, (0.0, 5242.0, vec![(150, 0, 150, 1)]));
}

/// Scenario `replaceSelection_start`.
#[test]
fn replace_selection_start_scrolls() {
    let r = far(|cm| cm.doc.replace_selection("x", Some("start"), None));
    assert_eq!(r, (0.0, 5242.0, vec![(150, 0, 150, 0)]));
}

/// Scenario `replaceSelections_multi`: two cursors, the primary one at 150:0.
#[test]
fn replace_selections_multi_scrolls() {
    let mut cm = editor();
    cm.run_op(|cm| {
        cm.doc
            .set_selections(&[(p(10, 0), None), (p(150, 0), None)], Some(1), no_scroll())
    });
    assert_eq!(cm.display.scroll_top, 0.0);
    cm.run_op(|cm| cm.doc.replace_selections(&["a", "b"], None, None));
    assert_eq!(cm.display.scroll_top, 5242.0);
    assert_eq!(sels(&cm), vec![(10, 1, 10, 1), (150, 1, 150, 1)]);
}

/// Scenario `replaceRange_control`: `replaceRange` does not scroll.
#[test]
fn replace_range_does_not_scroll() {
    let r = far(|cm| cm.doc.replace_range("x", p(150, 0), None, None));
    assert_eq!(r, (0.0, 0.0, vec![(150, 1, 150, 1)]));
}
