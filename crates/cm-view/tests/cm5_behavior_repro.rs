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

// P0 review of the G13 #1 fix (scenarios recorded 2026-10-26).

/// Scenario `replaceSelection_inOp_thenSetCursorNoScroll`: `ensureCursorVisible`
/// captures the cursor when it is called (scrolling.js:119-123), so a later
/// non-scrolling `setCursor` in the same operation does not move the target.
#[test]
fn replace_selection_in_op_then_set_cursor_no_scroll() {
    let r = far(|cm| {
        cm.doc.replace_selection("x", None, None);
        cm.doc.set_cursor(p(10, 0), no_scroll());
    });
    assert_eq!(r, (0.0, 5242.0, vec![(10, 0, 10, 0)]));
}

/// Scenario `replaceSelection_inOp_thenSetCursor`: the second request resolves the
/// first into a scroll top (5242), then scrolls 10:0 into view from there.
#[test]
fn replace_selection_in_op_then_set_cursor() {
    let r = far(|cm| {
        cm.doc.replace_selection("x", None, None);
        cm.doc.set_cursor(p(10, 0), None);
    });
    assert_eq!(r, (0.0, 384.0, vec![(10, 0, 10, 0)]));
}

/// Scenario `replaceSelection_inOp_twice`.
#[test]
fn replace_selection_in_op_twice() {
    let r = far(|cm| {
        cm.doc.replace_selection("x", None, None);
        cm.doc.set_cursor(p(40, 0), no_scroll());
        cm.doc.replace_selection("y", None, None);
    });
    assert_eq!(r, (0.0, 1524.0, vec![(40, 1, 40, 1)]));
}

/// Scenario `replaceSelection_largeDocEnd`: 5000 lines, a multi-line insert at the end.
#[test]
fn replace_selection_large_doc_end() {
    let text: Vec<String> = (0..5000).map(|i| format!("line {i}")).collect();
    let mut cm = Editor::new(
        &text.join("\n"),
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
    );
    cm.display.window_height = Some(900.0);
    cm.set_size(900.0, 500.0);
    cm.run_op(|cm| cm.doc.set_cursor(p(4999, 9), no_scroll()));
    assert_eq!(cm.display.scroll_top, 0.0);
    cm.run_op(|cm| cm.doc.replace_selection("x\ny\nz", None, None));
    assert_eq!(cm.display.scroll_top, 189584.0);
    assert_eq!(sels(&cm), vec![(5001, 1, 5001, 1)]);
}

/// Scenario `replaceSelection_readOnlySpan`: the change is cancelled, the cursor is
/// still scrolled into view.
#[test]
fn replace_selection_read_only_span_still_scrolls() {
    let r = far(|cm| {
        cm.doc
            .mark_text(
                p(150, 0),
                p(150, 4),
                cm_core::MarkOptions {
                    read_only: true,
                    ..Default::default()
                },
            )
            .unwrap();
        cm.doc.set_cursor(p(150, 2), no_scroll());
        cm.doc.replace_selection("x", None, None);
    });
    assert_eq!(r.1, 5242.0);
    assert_eq!(r.2, vec![(150, 2, 150, 2)]);
}

/// Scenario `replaceSelection_history`: the fix adds no selection event.
#[test]
fn replace_selection_history_unchanged() {
    let mut cm = editor();
    cm.run_op(|cm| cm.doc.set_cursor(p(150, 0), no_scroll()));
    cm.doc.clear_history();
    cm.run_op(|cm| cm.doc.replace_selection("x", None, None));
    let kinds = |cm: &Editor| -> Vec<&'static str> {
        cm.doc
            .get_history()
            .done
            .iter()
            .map(|e| match e {
                cm_core::HistoryEventData::Selection(_) => "selection",
                cm_core::HistoryEventData::Changes(_) => "changes",
            })
            .collect()
    };
    assert_eq!(kinds(&cm), vec!["selection", "changes", "selection"]);
    let size = cm.doc.history_size();
    assert_eq!((size.undo, size.redo), (1, 0));
    cm.run_op(|cm| cm.doc.undo_selection());
    assert_eq!(sels(&cm), vec![(150, 0, 150, 0)]);
    assert_eq!(cm.display.scroll_top, 5242.0);
    assert_eq!(kinds(&cm), vec!["selection"]);
}

/// Scenario `replaceSelection_detached`: a Doc without an editor edits normally and
/// requests no scroll.
#[test]
fn replace_selection_detached_doc() {
    let text: Vec<String> = (0..200).map(|i| format!("line {i}")).collect();
    let doc = cm_core::Doc::new(&text.join("\n"));
    doc.set_cursor(p(150, 0), None);
    doc.replace_selection("x", None, None);
    assert_eq!(doc.get_line(150).as_deref(), Some("xline 150"));
    assert_eq!(doc.get_cursor(None), p(150, 1));
    assert_eq!(doc.history_size().undo, 1);
    assert!(doc.take_scroll_requests().is_empty());
}
