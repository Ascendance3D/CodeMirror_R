//! Port of the view/command/input tests in CodeMirror 5 `test/test.js`. Each Rust
//! test names the JS `testCM("<name>")` it reproduces (snake_case; the JS name is
//! quoted in its doc comment). The model-only tests of test.js live in
//! `crates/cm-core/tests/upstream_test_js.rs`; `test/multi_test.js` is ported in
//! `upstream_multi_test.rs`.
//!
//! Measurement uses `Metrics::test_grid()` (10px cells, 20px lines, 4px padding):
//! where the JS compares DOM rects the port checks the same relations on the
//! box model. The editor is 600x300 (the test page's `.CodeMirror` is 300px high).
//! The test page's default mode is `css` (the first mode `test/index.html` loads).
//!
//! ## Not ported (and why)
//!
//! Model-only, already in cm-core's port: getRange, replaceRange, selection,
//! extendSelection, lines, posFromIndex, undo, undoDepth, undoDoesntClearValue,
//! undoMultiLine, undoComposite, undoSelection, undoSelectionAsBefore,
//! selectionChangeConfusesHistory, markText* (SingleLine, MultiLine, Undo, StayGone,
//! AllowEmpty, Stacked), undoPreservesNewMarks, markClearBetween, findMarksMiddle,
//! deleteSpanCollapsedInclusiveLeft, bookmark, bookmarkInsertLeft, getAllMarks,
//! setValueClears, bug577, restoreHistory, weirdLinebreaks, getLineNumber,
//! selectionBias, dirtyBit, changeGeneration, beforeChange, beforeChangeUndo,
//! selectionHistoryNonOverlapping, selChangeInOperationDoesNotSplit,
//! alwaysMergeSelEventWithChangeOrigin, lineSeparator, hiddenLinesSelectAll,
//! randomCollapsedRanges.
//!
//! DOM / textarea / CSS only: core_fromTextArea, core_defaults (option table and
//! `CodeMirror.defaults` mutation), lineInfo (gutter markers), markTextCSS,
//! markTextWithAttributes, bookmarkCursor, multiBookmarkCursor (inline widget
//! rects), selectionPos (selection div rects), doubleScrollbar, setSize (DOM size
//! strings), scrollVerticallyAndHorizontally (scrollbar elements),
//! collapsedRangeBetweenLinesSelected (selection div widths), splitSpaces_nonspecial
//! (`cm-invalidchar` spans), core_rmClass, core_addClass, combinedOperations (line
//! classes), "should have translate=no attribute", lineStyleFromBlankLine (the
//! ported Mode trait's `blankLine` cannot return a style).
//!
//! Not implemented in the port: line widgets and inline widgets (wrappingInlineWidget,
//! showEmptyWidgetSpan, changedInlineWidget, changedBookmark, inlineWidget,
//! lineWidgets, lineWidgetFocus, lineWidgetCautiousRedraw, lineWidgetChanged,
//! lineWidgetIssue5486, resizeLineWidget, the widget half of jumpTheGap), collapsed
//! ranges in layout/measurement (collapsedLines, collapsedRangeCoordsChar, clickFold,
//! everythingFolded, structuredFold, nestedFold, badNestedFold, nestedFoldOnSide,
//! editInFold), line classes (addLineClass), helpers (`registerHelper`),
//! wrapped-line measurement against a real browser
//! (wrappingAndResizing, measureEndOfLine, measureWrappedEndOfLine,
//! measureWrappedBeginOfLine, scrollEntirelyToRight).
//!
//! Bidi (the box model lays every line out left to right, so visual-order
//! measurement is a gap): coordsCharBidi, badBidiOptimization, rtlMovement,
//! measureEndOfLineBidi, measureWrappedBidiLevel2, the generated move_bidi_* /
//! move_end_bidi_* / coords_wrapped_bidi_* tests, rtl_wrapped_selection,
//! bidi_wrapped_selection, issue_4878.

mod common;
use common::*;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cm_core::text::to_u16;
use cm_core::util::string_stream::{RegexMatch, StringStream};
use cm_core::{BeforeSelectionChange, Doc, MarkOptions, Marker, MarkerFind, Pos, SelRange};
use cm_modes::{Mode, Registry, Spec, State};
use cm_view::keymap::{Binding, KeyMap, KeyMapRef};
use cm_view::movement::UnitH;
use cm_view::{CoordSys, Editor, EditorOptions, Indent, KeyEvent, Metrics};

fn exec(cm: &mut Editor, c: &str) {
    cm.exec_command(c);
}

/// `charCoords(pos, "page")`: window coordinates of the editor (the page is not
/// scrolled), i.e. local minus the scroll offsets plus the gutter.
fn page_coords(cm: &Editor, pos: Pos) -> cm_view::measure::Rect {
    // `cm.charCoords` clips its argument.
    let mut r = cm.char_coords(cm.doc.clip_pos(pos), CoordSys::Local);
    let dx = cm.gutter_width - cm.display.scroll_left;
    let dy = -cm.display.scroll_top;
    r.left += dx;
    r.right += dx;
    r.top += dy;
    r.bottom += dy;
    r
}
/// `coordsChar({left, top}, "page")`.
fn coords_char_page(cm: &Editor, x: f64, y: f64) -> Pos {
    let (dx, dy) = cm.div_from_window(x, y);
    cm.coords_char(dx, dy).pos()
}

/// test.js "indent"
#[test]
fn indent() {
    let mut cm = cm_opts("if (x) {\nblah();\n}", None, |o| {
        o.indent_unit = 3;
        o.indent_with_tabs = true;
        o.tab_size = 8;
    });
    cm.indent_line(1, None, false);
    assert_eq!(cm.doc.get_line(1).unwrap(), "   blah();");
    // setOption("indentUnit", 8): loadMode.
    cm.options.indent_unit = 8;
    let spec = cm.mode_spec.clone();
    cm.set_mode(spec);
    cm.indent_line(1, None, false);
    assert_eq!(cm.doc.get_line(1).unwrap(), "\tblah();");
    cm.options.indent_unit = 10;
    let spec = cm.mode_spec.clone();
    cm.set_mode(spec);
    cm.set_tab_size(4);
    cm.indent_line(1, None, false);
    assert_eq!(cm.doc.get_line(1).unwrap(), "\t\t  blah();");
}

/// test.js "indentByNumber"
#[test]
fn indent_by_number() {
    let mut cm = cm("foo\nbar\nbaz");
    cm.indent_line(0, Some(Indent::By(2)), false);
    assert_eq!(cm.doc.get_line(0).unwrap(), "  foo");
    cm.indent_line(0, Some(Indent::By(-200)), false);
    assert_eq!(cm.doc.get_line(0).unwrap(), "foo");
    cm.run_op(|cm| cm.doc.set_selection(p(0, 0), Some(p(1, 2)), None));
    cm.indent_selection(Indent::By(3));
    assert_eq!(cm.get_value(), "   foo\n   bar\nbaz");
}

/// test.js "coords"
#[test]
fn coords() {
    let mut cm = cm("");
    let w = cm.display.width;
    cm.set_size(w, 100.0);
    add_doc(&mut cm, 32, 200);
    let top = page_coords(&cm, p(0, 0));
    let bot = page_coords(&cm, p(200, 30));
    assert!(top.left < bot.left);
    assert!(top.top < bot.top);
    assert!(top.top < top.bottom);
    cm.run_op(|cm| cm.scroll_to(None, Some(100.0)));
    let top2 = page_coords(&cm, p(0, 0));
    assert!(top.top > top2.top);
    assert_eq!(top.left, top2.left);
}

/// test.js "coordsChar" (`lineNumbers: true`), in the "page" and "local" systems.
#[test]
fn coords_char() {
    let mut cm = cm_opts("", None, |o| o.line_numbers = true);
    add_doc(&mut cm, 35, 70);
    for i in 0..2 {
        for ch in (0..=35).step_by(5) {
            for line in (0..70).step_by(5) {
                cm.run_op(|cm| cm.doc.set_cursor(p(line, ch), None));
                let pos = if i == 0 {
                    let c = page_coords(&cm, p(line, ch));
                    coords_char_page(&cm, c.left + 1.0, c.top + 1.0)
                } else {
                    let c = cm.char_coords(p(line, ch), CoordSys::Local);
                    cm.coords_char_local(c.left + 1.0, c.top + 1.0).pos()
                };
                eq_char_pos(pos, p(line, ch));
            }
        }
    }
}

/// test.js "scrollSnap"
#[test]
fn scroll_snap() {
    let mut cm = cm("");
    cm.set_size(100.0, 100.0);
    add_doc(&mut cm, 200, 200);
    cm.run_op(|cm| cm.doc.set_cursor(p(100, 180), None));
    assert!(cm.display.scroll_left > 0.0 && cm.display.scroll_top > 0.0);
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 0), None));
    assert!(
        cm.display.scroll_left == 0.0 && cm.display.scroll_top == 0.0,
        "scrolled clean to top"
    );
    cm.run_op(|cm| cm.doc.set_cursor(p(100, 180), None));
    cm.run_op(|cm| cm.doc.set_cursor(p(199, 0), None));
    let (max_top, _) = cm.max_scroll();
    assert!(
        cm.display.scroll_left == 0.0 && cm.display.scroll_top + 2.0 > max_top,
        "scrolled clean to bottom"
    );
}

/// test.js "scrollIntoView"
#[test]
fn scroll_into_view() {
    let mut cm = cm("");
    add_doc(&mut cm, 200, 200);
    let check = |cm: &mut Editor, line: i64, ch: i64, msg: &str| {
        let pos = p(line, ch);
        cm.scroll_into_view(pos, pos, 0.0);
        let b = page_coords(cm, pos);
        assert!(b.left >= 0.0, "{msg} (left)");
        assert!(b.right <= cm.display.width, "{msg} (right)");
        assert!(b.top >= 0.0, "{msg} (top)");
        assert!(b.bottom <= cm.display.height, "{msg} (bottom)");
    };
    check(&mut cm, 199, 199, "bottom right");
    check(&mut cm, 0, 0, "top left");
    check(&mut cm, 100, 100, "center");
    check(&mut cm, 199, 0, "bottom left");
    check(&mut cm, 0, 199, "top right");
    check(&mut cm, 100, 100, "center again");
}

/// test.js "scrollBackAndForth"
#[test]
fn scroll_back_and_forth() {
    let mut cm = cm("");
    add_doc(&mut cm, 1, 200);
    cm.run_op(|cm| {
        cm.scroll_into_view(p(199, 0), p(199, 0), 0.0);
        cm.scroll_into_view(p(4, 0), p(4, 0), 0.0);
    });
    assert!(cm.display.scroll_top > 0.0);
}

/// test.js "selectAllNoScroll"
#[test]
fn select_all_no_scroll() {
    let mut cm = cm("");
    add_doc(&mut cm, 1, 200);
    exec(&mut cm, "selectAll");
    assert_eq!(cm.display.scroll_top, 0.0);
    cm.run_op(|cm| cm.doc.set_cursor(pl(199), None));
    exec(&mut cm, "selectAll");
    assert!(cm.display.scroll_top > 0.0);
}

/// test.js "moveVstuck" (`lineWrapping: true`).
#[test]
fn move_v_stuck() {
    let mut cm = cm_opts("", None, |o| o.line_wrapping = true);
    let h0 = cm.doc_height();
    let val = "fooooooooooooooooooooooooo baaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaar\n";
    cm.set_value(val);
    let mut w = cm.char_coords(p(0, 26), CoordSys::Div).right * 2.8;
    loop {
        let h = cm.display.height;
        cm.set_size(w, h);
        if cm.doc_height() <= 3.5 * h0 {
            break;
        }
        w += 5.0;
    }
    let len = to_u16(val).len() as i64;
    cm.run_op(|cm| cm.doc.set_cursor(p(0, len - 1), None));
    cm.move_v(-1, cm_view::movement::UnitV::Line);
    eq_cursor_pos(cur(&cm), pb(0, 27));
    assert!(
        cm.cursor_coords(cur(&cm), CoordSys::Local).top < h0,
        "cursor is in first visual line"
    );
}

/// test.js "collapseOnMove"
#[test]
fn collapse_on_move() {
    let mut cm = cm("aaaaa\nb\nccccc");
    cm.run_op(|cm| cm.doc.set_selection(p(0, 1), Some(p(2, 4)), None));
    exec(&mut cm, "goLineUp");
    assert!(!cm.doc.something_selected());
    eq_char_pos(cur(&cm), p(0, 1));
    cm.run_op(|cm| cm.doc.set_selection(p(0, 1), Some(p(2, 4)), None));
    exec(&mut cm, "goPageDown");
    assert!(!cm.doc.something_selected());
    eq_char_pos(cur(&cm), p(2, 4));
    exec(&mut cm, "goLineUp");
    exec(&mut cm, "goLineUp");
    eq_char_pos(cur(&cm), p(0, 4));
    cm.run_op(|cm| cm.doc.set_selection(p(0, 1), Some(p(2, 4)), None));
    exec(&mut cm, "goCharLeft");
    assert!(!cm.doc.something_selected());
    eq_char_pos(cur(&cm), p(0, 1));
}

/// test.js "clickTab" (`lineWrapping: true, tabSize: 8`).
#[test]
fn click_tab() {
    let cm = cm_opts("\t\n\n", None, |o| {
        o.line_wrapping = true;
        o.tab_size = 8;
    });
    let p0 = cm.char_coords(p(0, 0), CoordSys::Local);
    eq_char_pos(
        cm.coords_char_local(p0.left + 5.0, p0.top + 5.0).pos(),
        p(0, 0),
    );
    eq_char_pos(
        cm.coords_char_local(p0.right - 5.0, p0.top + 5.0).pos(),
        p(0, 1),
    );
}

/// test.js "verticalScroll": `scrollWidth` follows the widest line.
#[test]
fn vertical_scroll() {
    let mut cm = cm("");
    cm.set_size(100.0, 200.0);
    cm.set_value("foo\nbar\nbaz\n");
    let base = cm.max_scroll().1;
    let long_a = format!("{}h", "a".repeat(97));
    cm.doc.replace_range(&long_a, p(0, 0), Some(pl(0)), None);
    assert!(cm.max_scroll().1 > base, "scrollbar present");
    cm.doc.replace_range("foo", p(0, 0), Some(pl(0)), None);
    assert_eq!(cm.max_scroll().1, base, "scrollbar gone");
    cm.doc.replace_range(&long_a, p(0, 0), Some(pl(0)), None);
    cm.doc
        .replace_range(&format!("{}h", "b".repeat(81)), p(1, 0), Some(pl(1)), None);
    assert!(cm.max_scroll().1 > base, "present again");
    let cur_w = cm.max_scroll().1;
    cm.doc.replace_range("foo", p(0, 0), Some(pl(0)), None);
    assert!(cm.max_scroll().1 < cur_w, "scrollbar smaller");
    assert!(cm.max_scroll().1 > base, "but still present");
}

/// test.js "extraKeys"
#[test]
fn extra_keys() {
    let mut cm = cm("");
    let outcome: Rc<RefCell<Option<&'static str>>> = Rc::new(RefCell::new(None));
    let set = |s: &'static str| {
        let o = outcome.clone();
        Rc::new(move |_: &mut Editor| {
            *o.borrow_mut() = Some(s);
            true
        }) as cm_view::editor::CommandFn
    };
    cm.commands.insert("testCommand".into(), set("tc"));
    cm.commands.insert("goTestCommand".into(), set("gtc"));
    let mut m = KeyMap::default();
    m.bind("Shift-X", Binding::Fn(set("sx"))).unwrap();
    m.bind("X", Binding::Fn(set("x"))).unwrap();
    m.bind("Ctrl-Alt-U", Binding::Fn(set("cau"))).unwrap();
    m.bind("End", Binding::Command("testCommand".into()))
        .unwrap();
    m.bind("Home", Binding::Command("goTestCommand".into()))
        .unwrap();
    m.bind("Tab", Binding::Nothing).unwrap();
    cm.extra_keys = Some(KeyMapRef::Map(Rc::new(m)));
    let fake = |cm: &mut Editor, expected: Option<&str>, key: &str| {
        *outcome.borrow_mut() = None;
        cm.key_down(&KeyEvent::parse(key));
        assert_eq!(*outcome.borrow(), expected, "{key}");
    };
    fake(&mut cm, None, "U");
    fake(&mut cm, Some("cau"), "Ctrl-Alt-U");
    fake(&mut cm, None, "Shift-Ctrl-Alt-U");
    fake(&mut cm, Some("x"), "X");
    fake(&mut cm, Some("sx"), "Shift-X");
    fake(&mut cm, Some("tc"), "End");
    fake(&mut cm, None, "Shift-End");
    fake(&mut cm, Some("gtc"), "Home");
    fake(&mut cm, Some("gtc"), "Shift-Home");
    fake(&mut cm, None, "Tab");
}

/// test.js "wordMovementCommands"
#[test]
fn word_movement_commands() {
    let mut cm = cm("this is (the) firstline.\na foo12\u{e9}\u{f8}\u{d7}bar\n");
    exec(&mut cm, "goWordLeft");
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goWordRight");
    exec(&mut cm, "goWordRight");
    eq_cursor_pos(cur(&cm), pb(0, 7));
    exec(&mut cm, "goWordLeft");
    eq_cursor_pos(cur(&cm), pa(0, 5));
    exec(&mut cm, "goWordRight");
    exec(&mut cm, "goWordRight");
    eq_cursor_pos(cur(&cm), pb(0, 12));
    exec(&mut cm, "goWordLeft");
    eq_cursor_pos(cur(&cm), pa(0, 9));
    for _ in 0..3 {
        exec(&mut cm, "goWordRight");
    }
    eq_cursor_pos(cur(&cm), pb(0, 24));
    exec(&mut cm, "goWordRight");
    exec(&mut cm, "goWordRight");
    eq_cursor_pos(cur(&cm), pb(1, 9));
    exec(&mut cm, "goWordRight");
    eq_cursor_pos(cur(&cm), pb(1, 13));
    exec(&mut cm, "goWordRight");
    exec(&mut cm, "goWordRight");
    eq_char_pos(cur(&cm), p(2, 0));
}

/// test.js "groupMovementCommands"
#[test]
fn group_movement_commands() {
    let mut cm = cm("booo ba---quux. ffff\n  abc d");
    exec(&mut cm, "goGroupLeft");
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goGroupRight");
    eq_cursor_pos(cur(&cm), pb(0, 4));
    exec(&mut cm, "goGroupRight");
    eq_cursor_pos(cur(&cm), pb(0, 7));
    exec(&mut cm, "goGroupRight");
    eq_cursor_pos(cur(&cm), pb(0, 10));
    exec(&mut cm, "goGroupLeft");
    eq_cursor_pos(cur(&cm), pa(0, 7));
    for _ in 0..3 {
        exec(&mut cm, "goGroupRight");
    }
    eq_cursor_pos(cur(&cm), pb(0, 15));
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 17), None));
    exec(&mut cm, "goGroupLeft");
    eq_cursor_pos(cur(&cm), pa(0, 16));
    exec(&mut cm, "goGroupLeft");
    eq_cursor_pos(cur(&cm), pa(0, 14));
    exec(&mut cm, "goGroupRight");
    exec(&mut cm, "goGroupRight");
    eq_cursor_pos(cur(&cm), pb(0, 20));
    exec(&mut cm, "goGroupRight");
    eq_cursor_pos(cur(&cm), pa(1, 0));
    exec(&mut cm, "goGroupRight");
    eq_cursor_pos(cur(&cm), pb(1, 2));
    exec(&mut cm, "goGroupRight");
    eq_cursor_pos(cur(&cm), pb(1, 5));
    exec(&mut cm, "goGroupLeft");
    exec(&mut cm, "goGroupLeft");
    eq_cursor_pos(cur(&cm), pa(1, 0));
    exec(&mut cm, "goGroupLeft");
    eq_cursor_pos(cur(&cm), pa(0, 20));
    exec(&mut cm, "goGroupLeft");
    eq_cursor_pos(cur(&cm), pa(0, 16));
}

/// test.js "groupsAndWhitespace"
#[test]
fn groups_and_whitespace() {
    let mut cm = cm("  foo +++  \n  bar");
    let positions = [
        p(0, 0),
        p(0, 2),
        p(0, 5),
        p(0, 9),
        p(0, 11),
        p(1, 0),
        p(1, 2),
        p(1, 5),
    ];
    for pos in &positions[1..] {
        exec(&mut cm, "goGroupRight");
        eq_char_pos(cur(&cm), *pos);
    }
    for i in (0..positions.len() - 1).rev() {
        exec(&mut cm, "goGroupLeft");
        eq_char_pos(cur(&cm), if i == 2 { pb(0, 6) } else { positions[i] });
    }
}

/// test.js "charMovementCommands"
#[test]
fn char_movement_commands() {
    let mut cm = cm("line1\n ine2\n");
    exec(&mut cm, "goCharLeft");
    exec(&mut cm, "goColumnLeft");
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goCharRight");
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), pb(0, 2));
    cm.run_op(|cm| cm.doc.set_cursor(p(1, 0), None));
    exec(&mut cm, "goColumnLeft");
    eq_cursor_pos(cur(&cm), p(1, 0));
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), pb(0, 5));
    exec(&mut cm, "goColumnRight");
    eq_cursor_pos(cur(&cm), pb(0, 5));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), pa(1, 0));
    exec(&mut cm, "goLineEnd");
    eq_cursor_pos(cur(&cm), pb(1, 5));
    exec(&mut cm, "goLineStartSmart");
    eq_cursor_pos(cur(&cm), pa(1, 1));
    exec(&mut cm, "goLineStartSmart");
    eq_cursor_pos(cur(&cm), pa(1, 0));
    cm.run_op(|cm| cm.doc.set_cursor(p(2, 0), None));
    exec(&mut cm, "goCharRight");
    exec(&mut cm, "goColumnRight");
    eq_cursor_pos(cur(&cm), p(2, 0));
}

/// test.js "verticalMovementCommands"
#[test]
fn vertical_movement_commands() {
    let mut cm = cm("line1\nlong long line2\nline3\n\nline5\n");
    exec(&mut cm, "goLineUp");
    eq_char_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goLineDown");
    eq_char_pos(cur(&cm), p(1, 0));
    cm.run_op(|cm| cm.doc.set_cursor(p(1, 12), None));
    exec(&mut cm, "goLineDown");
    eq_char_pos(cur(&cm), p(2, 5));
    exec(&mut cm, "goLineDown");
    eq_char_pos(cur(&cm), p(3, 0));
    exec(&mut cm, "goLineUp");
    eq_char_pos(cur(&cm), p(2, 5));
    exec(&mut cm, "goLineUp");
    eq_char_pos(cur(&cm), p(1, 12));
    exec(&mut cm, "goPageDown");
    eq_char_pos(cur(&cm), p(5, 0));
    exec(&mut cm, "goPageDown");
    exec(&mut cm, "goLineDown");
    eq_char_pos(cur(&cm), p(5, 0));
    exec(&mut cm, "goPageUp");
    eq_char_pos(cur(&cm), p(0, 0));
}

/// test.js "verticalMovementCommandsWrapping" (`lineWrapping: true`, width 120px).
#[test]
fn vertical_movement_commands_wrapping() {
    let mut cm = cm_opts(
        "a very long line that wraps around somehow so that we can test cursor movement\nshortone\nk",
        None,
        |o| o.line_wrapping = true,
    );
    let h = cm.display.height;
    cm.set_size(120.0, h);
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 5), None));
    exec(&mut cm, "goLineDown");
    assert_eq!(cur(&cm).line, 0);
    assert!(cur(&cm).ch > 5, "moved beyond wrap");
    let mut i = 0;
    loop {
        assert!(i < 20, "no endless loop");
        exec(&mut cm, "goLineDown");
        let c = cur(&cm);
        if c.line == 1 {
            assert_eq!(c.ch, 5);
        }
        if c.line == 2 {
            assert_eq!(c.ch, 1);
            break;
        }
        i += 1;
    }
}

/// test.js "verticalMovementCommandsSingleLine" (wrapper height "auto": one line).
#[test]
fn vertical_movement_commands_single_line() {
    let mut cm = cm("single line");
    let (w, h) = (cm.display.width, cm.doc_height() + 8.0);
    cm.set_size(w, h);
    exec(&mut cm, "goLineUp");
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goLineDown");
    eq_cursor_pos(cur(&cm), p(0, 11));
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 5), None));
    exec(&mut cm, "goLineDown");
    eq_cursor_pos(cur(&cm), p(0, 11));
    exec(&mut cm, "goLineDown");
    eq_cursor_pos(cur(&cm), p(0, 11));
    exec(&mut cm, "goLineUp");
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goLineUp");
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goPageDown");
    eq_cursor_pos(cur(&cm), p(0, 11));
    exec(&mut cm, "goPageDown");
    exec(&mut cm, "goLineDown");
    eq_cursor_pos(cur(&cm), p(0, 11));
    exec(&mut cm, "goPageUp");
    eq_cursor_pos(cur(&cm), p(0, 0));
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 5), None));
    exec(&mut cm, "goPageUp");
    eq_cursor_pos(cur(&cm), p(0, 0));
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 5), None));
    exec(&mut cm, "goPageDown");
    eq_cursor_pos(cur(&cm), p(0, 11));
}

/// test.js "bidiUpdate": updating a line clears its bidi ordering.
#[test]
fn bidi_update() {
    let mut cm = cm("abcd\n");
    cm.run_op(|cm| cm.doc.set_cursor(pb(0, 2), None));
    cm.run_op(|cm| cm.doc.replace_selection("خحج", Some("start"), None));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), pb(0, 6));
}

/// test.js "movebyTextUnit"
#[test]
fn moveby_text_unit() {
    let mut cm = cm("");
    cm.set_value("\u{5d1}\u{5b0}\u{5bc}\u{5e8}\u{5b5}\u{5d0}\u{5e9}\u{5b4}\ne\u{301}e\u{301}e\u{301}\u{301}\n");
    exec(&mut cm, "goLineStart");
    for _ in 0..4 {
        exec(&mut cm, "goCharRight");
    }
    eq_cursor_pos(cur(&cm), pa(0, 0));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), pa(1, 0));
    exec(&mut cm, "goCharRight");
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), pb(1, 4));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), pb(1, 7));
}

/// test.js "jumpTheGap" (`lineWrapping: true`, width 200px, `line-height: 2`). The
/// line-widget half is not ported (no line widgets).
#[test]
fn jump_the_gap() {
    cm_core::clock::manual(1_000_000.0);
    let o = EditorOptions {
        line_wrapping: true,
        rtl_move_visually: true,
        mac: false,
        ..Default::default()
    };
    let mut cm = Editor::new(
        "abc\ndef\nghi\njkl\n",
        Spec::from("css"),
        o,
        Metrics::monospace(10.0, 40.0),
    );
    cm.set_size(600.0, 300.0);
    let mut long = "abcdef ghiklmnop qrstuvw xyz ".to_string();
    for _ in 0..3 {
        long = format!("{long}{long}");
    }
    cm.doc.replace_range(&long, p(2, 0), Some(pl(2)), None);
    cm.set_size(200.0, 300.0);
    cm.run_op(|cm| cm.doc.set_cursor(p(0, 1), None));
    exec(&mut cm, "goLineDown");
    eq_char_pos(cur(&cm), p(1, 1));
    exec(&mut cm, "goLineDown");
    eq_char_pos(cur(&cm), p(2, 1));
    exec(&mut cm, "goLineDown");
    assert_eq!(cur(&cm).line, 2);
    assert!(cur(&cm).ch > 1);
    exec(&mut cm, "goLineUp");
    eq_char_pos(cur(&cm), p(2, 1));
    exec(&mut cm, "goLineUp");
    eq_char_pos(cur(&cm), p(1, 1));
}

fn mark(cm: &Editor, from: Pos, to: Pos, o: MarkOptions) -> Marker {
    cm.doc.mark_text(from, to, o).unwrap()
}
fn set_cursor(cm: &mut Editor, pos: Pos) {
    cm.run_op(|cm| cm.doc.set_cursor(pos, None));
}

/// test.js "atomicMarker"
#[test]
fn atomic_marker() {
    let mut cm = cm("");
    add_doc(&mut cm, 10, 10);
    let atom =
        |cm: &Editor, ll, cl, lr, cr, li: bool, ri: bool, ls: Option<bool>, rs: Option<bool>| {
            mark(
                cm,
                p(ll, cl),
                p(lr, cr),
                MarkOptions {
                    atomic: true,
                    inclusive_left: li,
                    inclusive_right: ri,
                    select_left: ls,
                    select_right: rs,
                    ..Default::default()
                },
            )
        };
    // Can cursor to the left and right of a normal marker by jumping across it.
    let m = atom(&cm, 0, 1, 0, 5, false, false, None, None);
    set_cursor(&mut cm, p(0, 1));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), p(0, 5));
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), p(0, 1));
    m.clear();
    // Can't cursor to the left of a marker when inclusiveLeft=true.
    let m = atom(&cm, 0, 0, 0, 5, true, false, None, None);
    eq_cursor_pos(cur(&cm), p(0, 5));
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), p(0, 5));
    m.clear();
    // Can't cursor to the left of a marker when inclusiveLeft=false and selectLeft=false.
    let m = atom(&cm, 0, 0, 0, 5, false, false, Some(false), None);
    set_cursor(&mut cm, p(0, 5));
    eq_cursor_pos(cur(&cm), p(0, 5));
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), p(0, 5));
    m.clear();
    // Can cursor to the left of a marker when inclusiveLeft=false and selectLeft=true.
    let m = atom(&cm, 0, 0, 0, 5, false, false, Some(true), None);
    set_cursor(&mut cm, p(0, 5));
    eq_cursor_pos(cur(&cm), p(0, 5));
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), p(0, 0));
    m.clear();
    // Can't cursor to the right of a marker when inclusiveRight=true.
    let m = atom(&cm, 0, 0, 0, 5, false, true, None, None);
    set_cursor(&mut cm, p(0, 0));
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), p(0, 6));
    m.clear();
    // Can't cursor to the right of a marker when inclusiveRight=false and selectRight=false.
    let m = atom(&cm, 0, 0, 0, 5, false, false, Some(true), Some(false));
    set_cursor(&mut cm, p(0, 0));
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), p(0, 6));
    m.clear();
    // Can cursor to the right of a marker when inclusiveRight=false and selectRight=true.
    let m = atom(&cm, 0, 0, 0, 5, false, false, Some(true), Some(true));
    set_cursor(&mut cm, p(0, 0));
    eq_cursor_pos(cur(&cm), p(0, 0));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), p(0, 5));
    m.clear();
    // Can't cursor to the right of a multiline marker when inclusiveRight=true.
    let m = atom(&cm, 8, 4, 9, 10, false, true, None, None);
    set_cursor(&mut cm, p(9, 8));
    eq_cursor_pos(cur(&cm), p(8, 4));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), p(8, 4));
    exec(&mut cm, "goLineDown");
    eq_cursor_pos(cur(&cm), p(8, 4));
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), pa(8, 3));
    m.clear();
    // Cursor jumps across a multiline atomic marker, and backspace deletes it.
    let m = atom(&cm, 1, 1, 3, 8, false, false, None, None);
    set_cursor(&mut cm, p(0, 0));
    set_cursor(&mut cm, p(2, 0));
    eq_cursor_pos(cur(&cm), p(3, 8));
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), p(1, 1));
    exec(&mut cm, "goCharRight");
    eq_cursor_pos(cur(&cm), p(3, 8));
    exec(&mut cm, "goLineUp");
    eq_cursor_pos(cur(&cm), p(1, 1));
    exec(&mut cm, "goLineDown");
    eq_cursor_pos(cur(&cm), p(3, 8));
    exec(&mut cm, "delCharBefore");
    assert_eq!(to_u16(&cm.get_value()).len(), 80, "del chunk");
    m.clear();
    add_doc(&mut cm, 10, 10);
    // Delete before an atomic marker deletes the entire marker.
    let m = atom(&cm, 3, 0, 5, 5, false, false, None, None);
    set_cursor(&mut cm, p(3, 0));
    exec(&mut cm, "delWordAfter");
    assert_eq!(to_u16(&cm.get_value()).len(), 82, "del chunk");
    m.clear();
}

/// test.js "selectionHomeEnd"
#[test]
fn selection_home_end() {
    let mut cm = cm("ab\ncdef\ngh");
    mark(
        &cm,
        p(1, 0),
        p(1, 1),
        MarkOptions {
            atomic: true,
            inclusive_left: true,
            ..Default::default()
        },
    );
    mark(
        &cm,
        p(1, 3),
        p(1, 4),
        MarkOptions {
            atomic: true,
            inclusive_right: true,
            ..Default::default()
        },
    );
    set_cursor(&mut cm, p(1, 2));
    exec(&mut cm, "goLineStart");
    eq_cursor_pos(cur(&cm), p(1, 1));
    exec(&mut cm, "goLineEnd");
    eq_cursor_pos(cur(&cm), p(1, 3));
}

/// test.js "readOnlyMarker" (the cm-core port skips its `goCharLeft` half).
#[test]
fn read_only_marker() {
    let mut cm = cm("abcde\nfghij\nklmno\n");
    let mk = |cm: &Editor, ll, cl, lr, cr, at: bool| {
        mark(
            cm,
            p(ll, cl),
            p(lr, cr),
            MarkOptions {
                read_only: true,
                atomic: at,
                ..Default::default()
            },
        )
    };
    let m = mk(&cm, 0, 1, 0, 4, false);
    set_cursor(&mut cm, p(0, 2));
    cm.run_op(|cm| cm.doc.replace_selection("hi", Some("end"), None));
    eq_cursor_pos(cur(&cm), p(0, 2));
    assert_eq!(cm.doc.get_line(0).unwrap(), "abcde");
    exec(&mut cm, "selectAll");
    cm.run_op(|cm| cm.doc.replace_selection("oops", Some("around"), None));
    assert_eq!(cm.get_value(), "oopsbcd");
    exec(&mut cm, "undo");
    match m.find() {
        Some(MarkerFind::Range { from, to }) => {
            eq_cursor_pos(from, p(0, 1));
            eq_cursor_pos(to.unwrap(), p(0, 4));
        }
        other => panic!("{other:?}"),
    }
    m.clear();
    set_cursor(&mut cm, p(0, 2));
    cm.run_op(|cm| cm.doc.replace_selection("hi", Some("around"), None));
    assert_eq!(cm.doc.get_line(0).unwrap(), "abhicde");
    eq_cursor_pos(cur(&cm), p(0, 4));
    let _m = mk(&cm, 0, 2, 2, 2, true);
    cm.run_op(|cm| cm.doc.set_selection(p(1, 1), Some(p(2, 4)), None));
    cm.run_op(|cm| cm.doc.replace_selection("t", Some("end"), None));
    eq_cursor_pos(cur(&cm), p(2, 3));
    assert_eq!(cm.doc.get_line(2).unwrap(), "klto");
    exec(&mut cm, "goCharLeft");
    exec(&mut cm, "goCharLeft");
    eq_cursor_pos(cur(&cm), p(0, 2));
    cm.run_op(|cm| cm.doc.set_selection(p(0, 1), Some(p(0, 3)), None));
    cm.run_op(|cm| cm.doc.replace_selection("xx", Some("around"), None));
    eq_cursor_pos(cur(&cm), p(0, 3));
    assert_eq!(cm.doc.get_line(0).unwrap(), "axxhicde");
}

/// test.js "hiddenLinesAutoUnfold" (`foldLines(cm, 1, 3, true)`: a collapsed,
/// inclusive, clearOnEnter marker).
#[test]
fn hidden_lines_auto_unfold() {
    let mut cm = cm("abc\ndef\nghi\njkl");
    let cleared = Rc::new(Cell::new(0));
    let fold = |cm: &Editor| {
        let m = mark(
            cm,
            p(1, 0),
            pl(2),
            MarkOptions {
                inclusive_left: true,
                inclusive_right: true,
                collapsed: true,
                clear_on_enter: true,
                ..Default::default()
            },
        );
        let c = cleared.clone();
        match &m {
            Marker::Text(t) => t.on_clear(move |_| c.set(c.get() + 1)),
            Marker::Shared(_) => unreachable!(),
        };
        m
    };
    let _r = fold(&cm);
    set_cursor(&mut cm, p(3, 0));
    assert_eq!(cleared.get(), 0);
    exec(&mut cm, "goCharLeft");
    assert_eq!(cleared.get(), 1);
    let _r = fold(&cm);
    eq_cursor_pos(cur(&cm), p(3, 0));
    set_cursor(&mut cm, p(0, 3));
    exec(&mut cm, "goCharRight");
    assert_eq!(cleared.get(), 2);
}

/// test.js "addKeyMap"
#[test]
fn add_key_map() {
    let mut cm = cm("abc");
    let right = KeyEvent::new("Right");
    cm.key_down(&right);
    eq_cursor_pos(cur(&cm), pb(0, 1));
    let test = Rc::new(Cell::new(0));
    let mk = |f: Rc<dyn Fn()>| {
        let mut m = KeyMap::default();
        m.bind(
            "Right",
            Binding::Fn(Rc::new(move |_| {
                f();
                true
            })),
        )
        .unwrap();
        Rc::new(m)
    };
    let t1 = test.clone();
    let map1 = mk(Rc::new(move || t1.set(t1.get() + 1)));
    let t2 = test.clone();
    let map2 = mk(Rc::new(move || t2.set(t2.get() + 10)));
    cm.add_key_map_rc(map1.clone(), false);
    cm.key_down(&right);
    eq_cursor_pos(cur(&cm), pb(0, 1));
    assert_eq!(test.get(), 1);
    cm.add_key_map_rc(map2.clone(), true);
    cm.key_down(&right);
    assert_eq!(test.get(), 2);
    cm.remove_key_map_rc(&map1);
    cm.key_down(&right);
    assert_eq!(test.get(), 12);
    cm.remove_key_map_rc(&map2);
    cm.key_down(&right);
    assert_eq!(test.get(), 12);
    eq_cursor_pos(cur(&cm), pb(0, 2));
    let t3 = test.clone();
    let mut m3 = KeyMap {
        name: Some("mymap".into()),
        ..Default::default()
    };
    m3.bind(
        "Right",
        Binding::Fn(Rc::new(move |_| {
            t3.set(55);
            true
        })),
    )
    .unwrap();
    cm.add_key_map(m3, false);
    cm.key_down(&right);
    assert_eq!(test.get(), 55);
    cm.remove_key_map("mymap");
    cm.key_down(&right);
    eq_cursor_pos(cur(&cm), pb(0, 3));
}

/// test.js "findPosH"
#[test]
fn find_pos_h() {
    let cm = cm("line one\nline two.something.other\n");
    let cases: &[(Pos, Pos, i64, &str, bool)] = &[
        (p(0, 0), pb(0, 1), 1, "char", false),
        (p(0, 0), p(0, 0), -1, "char", true),
        (p(0, 0), pb(0, 4), 1, "word", false),
        (p(0, 0), pb(0, 8), 2, "word", false),
        (p(0, 0), pa(2, 0), 20, "word", true),
        (p(0, 7), pa(0, 5), -1, "word", false),
        (p(0, 4), pb(0, 8), 1, "word", false),
        (p(1, 0), pb(1, 18), 3, "word", false),
        (p(1, 22), pa(1, 5), -3, "word", false),
        (p(1, 15), pa(1, 10), -5, "char", false),
        (p(1, 15), pa(1, 10), -5, "column", false),
        (p(1, 15), pa(1, 0), -50, "column", true),
        (p(1, 15), pb(1, 24), 50, "column", true),
        (p(1, 15), pa(2, 0), 50, "char", true),
    ];
    for (from, to, by, unit, hit) in cases {
        // `cm.findPosH(from, by, unit)`: `visually` is undefined.
        let (r, h) = cm.find_pos_h_api(*from, *by, unit, false);
        eq_cursor_pos(r, *to);
        assert_eq!(h, *hit, "hitSide for {from:?} by {by} {unit}");
    }
    let _ = UnitH::Char;
}

/// test.js "beforeSelectionChange"
#[test]
fn before_selection_change() {
    let mut cm = cm("");
    fn not_at_end(doc: &Doc, pos: Pos) -> Pos {
        let len = to_u16(&doc.get_line(pos.line).unwrap_or_default()).len() as i64;
        if len == 0 || pos.ch == len {
            return p(pos.line, pos.ch - 1);
        }
        pos
    }
    cm.doc
        .on_before_selection_change(|doc: &Doc, obj: &mut BeforeSelectionChange| {
            let r = obj.ranges[0];
            obj.update(vec![SelRange::new(
                not_at_end(doc, r.anchor),
                not_at_end(doc, r.head),
            )]);
        });
    add_doc(&mut cm, 10, 10);
    exec(&mut cm, "goLineEnd");
    eq_cursor_pos(cur(&cm), p(0, 9));
    exec(&mut cm, "selectAll");
    eq_cursor_pos(cm.doc.get_cursor(Some("start")), p(0, 0));
    eq_cursor_pos(cm.doc.get_cursor(Some("end")), p(9, 9));
}

/// A mode written in a test (`CodeMirror.defineMode` in test.js).
struct FnMode {
    name: &'static str,
    token: fn(&mut StringStream<'_>) -> Option<String>,
}
impl Mode for FnMode {
    fn name(&self) -> &str {
        self.name
    }
    fn token(&self, stream: &mut StringStream<'_>, _state: &mut State) -> Option<String> {
        (self.token)(stream)
    }
}
fn editor_with_mode(
    value: &str,
    name: &'static str,
    token: fn(&mut StringStream<'_>) -> Option<String>,
) -> Editor {
    cm_core::clock::manual(1_000_000.0);
    let mut reg = Registry::with_all_modes();
    reg.define_mode(
        name,
        Rc::new(move |_, _, _| Rc::new(FnMode { name, token }) as Rc<dyn Mode>),
    );
    let o = EditorOptions {
        rtl_move_visually: true,
        mac: false,
        ..Default::default()
    };
    Editor::with_registry(
        value,
        Spec::from(name),
        o,
        Metrics::test_grid(),
        Rc::new(reg),
    )
}

/// Match `open [^close]* close` at the stream position.
fn match_delim(stream: &mut StringStream<'_>, open: u16, close: u16) -> bool {
    stream
        .match_with(
            |s| {
                if s.first() != Some(&open) {
                    return None;
                }
                s.iter()
                    .skip(1)
                    .position(|&c| c == close)
                    .map(|i| RegexMatch {
                        index: 0,
                        len: i + 2,
                    })
            },
            true,
        )
        .is_some()
}

/// test.js "lineStyleFromMode": line classes from `line-` / `line-background-`
/// styles, checked on the style arrays (`line.styleClasses`) instead of the DOM.
#[test]
fn line_style_from_mode() {
    fn token(stream: &mut StringStream<'_>) -> Option<String> {
        if match_delim(stream, b'[' as u16, b']' as u16) {
            return Some("  line-brackets  ".into());
        }
        if match_delim(stream, b'(' as u16, b')' as u16) {
            return Some("  line-background-parens  ".into());
        }
        if match_delim(stream, b'<' as u16, b'>' as u16) {
            return Some("  span  line-line  line-background-bg  ".into());
        }
        // stream.match(/^\s+|^\S+/)
        let ws = cm_core::text::is_js_space(stream.peek().unwrap());
        stream.eat_while(|c| cm_core::text::is_js_space(c) == ws);
        None
    }
    let cm = editor_with_mode(
        "line1: [br] [br]\nline2: (par) (par)\nline3: <tag> <tag>",
        "test_mode",
        token,
    );
    let s0 = cm.line_styles(0);
    assert_eq!(s0.classes, Some((None, Some("brackets".into()))));
    let s1 = cm.line_styles(1);
    assert_eq!(s1.classes, Some((Some("parens".into()), None)));
    let s2 = cm.line_styles(2);
    assert_eq!(s2.classes, Some((Some("bg".into()), Some("line".into()))));
    let spans: Vec<&str> = s2
        .spans
        .iter()
        .filter_map(|(_, s)| s.as_deref())
        .filter(|s| s.contains("span"))
        .collect();
    assert_eq!(spans.len(), 2);
    assert!(spans.iter().all(|s| s.trim() == "span"));
}

/// test.js "selectionHistory"
#[test]
fn selection_history() {
    let mut cm = cm("a b c d");
    for _ in 0..3 {
        cm.doc.set_extending(true);
        exec(&mut cm, "goCharRight");
        cm.doc.set_extending(false);
        exec(&mut cm, "goCharRight");
        exec(&mut cm, "goCharRight");
    }
    exec(&mut cm, "undoSelection");
    assert_eq!(cm.doc.get_selection(None), "c");
    exec(&mut cm, "undoSelection");
    assert_eq!(cm.doc.get_selection(None), "");
    eq_cursor_pos(cur(&cm), pb(0, 4));
    exec(&mut cm, "undoSelection");
    assert_eq!(cm.doc.get_selection(None), "b");
    exec(&mut cm, "redoSelection");
    assert_eq!(cm.doc.get_selection(None), "");
    eq_cursor_pos(cur(&cm), pb(0, 4));
    exec(&mut cm, "redoSelection");
    assert_eq!(cm.doc.get_selection(None), "c");
    exec(&mut cm, "redoSelection");
    assert_eq!(cm.doc.get_selection(None), "");
    eq_cursor_pos(cur(&cm), pb(0, 6));
}

/// test.js "selectionChangeReducesRedo"
#[test]
fn selection_change_reduces_redo() {
    let mut cm = cm("abc");
    cm.run_op(|cm| cm.doc.replace_selection("X", None, None));
    exec(&mut cm, "goCharRight");
    cm.run_op(|cm| cm.doc.undo_selection());
    exec(&mut cm, "selectAll");
    cm.run_op(|cm| cm.doc.undo_selection());
    assert_eq!(cm.get_value(), "Xabc");
    eq_cursor_pos(cur(&cm), p(0, 1));
    cm.run_op(|cm| cm.doc.undo_selection());
    assert_eq!(cm.get_value(), "abc");
}

/// test.js "cursorMotionSplitsHistory"
#[test]
fn cursor_motion_splits_history() {
    let mut cm = cm("1234");
    cm.run_op(|cm| cm.doc.replace_selection("a", None, None));
    exec(&mut cm, "goCharRight");
    cm.run_op(|cm| cm.doc.replace_selection("b", None, None));
    cm.run_op(|cm| cm.doc.replace_selection("c", None, None));
    exec(&mut cm, "undo");
    assert_eq!(cm.get_value(), "a1234");
    eq_cursor_pos(cur(&cm), pb(0, 2));
    exec(&mut cm, "undo");
    assert_eq!(cm.get_value(), "1234");
    eq_cursor_pos(cur(&cm), p(0, 0));
}

/// test.js "getTokenAt" (`mode: "javascript"`).
#[test]
fn get_token_at() {
    let cm = cm_mode("1+2", "javascript");
    let tok = cm.get_token_at(p(0, 2), false);
    assert_eq!(tok.ty.as_deref(), Some("operator"));
    assert_eq!(tok.string, "+");
    let toks = cm.get_line_tokens(0, false);
    assert_eq!(toks.len(), 3);
    for (t, (ty, s)) in toks
        .iter()
        .zip([("number", "1"), ("operator", "+"), ("number", "2")])
    {
        assert_eq!(t.ty.as_deref(), Some(ty));
        assert_eq!(t.string, s);
    }
}

/// An overlay mode object of test.js: `{token: f}` (no `name`, no `startState`).
struct FnOverlay(fn(&mut StringStream<'_>) -> Option<String>);

impl Mode for FnOverlay {
    fn name(&self) -> &str {
        ""
    }
    fn token(&self, stream: &mut StringStream<'_>, _state: &mut State) -> Option<String> {
        (self.0)(stream)
    }
}

/// The text of every rendered token with class `cm-<class>` (`byClassName`): the
/// spans of each line whose style has the word `class`.
fn tokens_with_class(cm: &Editor, class: &str) -> Vec<String> {
    let mut out = Vec::new();
    for n in 0..cm.doc.line_count() as i64 {
        let text = cm.line_text(n);
        let mut start = 0;
        for (end, style) in &cm.line_styles(n).spans {
            if style
                .as_deref()
                .is_some_and(|s| s.split(' ').any(|w| w == class))
            {
                out.push(String::from_utf16_lossy(&text[start..*end]));
            }
            start = *end;
        }
    }
    out
}

/// test.js "getTokenTypeAt" (`mode: "javascript"`): an overlay style shows as a
/// token class, and `getTokenTypeAt` cuts it off.
#[test]
fn get_token_type_at() {
    let mut cm = cm_mode("1 + 'foo'", "javascript");
    assert_eq!(cm.get_token_type_at(p(0, 0)).as_deref(), Some("number"));
    assert_eq!(cm.get_token_type_at(p(0, 6)).as_deref(), Some("string"));
    let ov = FnOverlay(|stream| {
        if stream.match_str("foo", true, false) {
            return Some("foo".into());
        }
        stream.next();
        None
    });
    cm.add_overlay(
        cm_view::highlight::OverlaySpec::Mode(Rc::new(ov)),
        false,
        0.0,
    )
    .unwrap();
    assert_eq!(tokens_with_class(&cm, "foo").len(), 1);
    assert_eq!(cm.get_token_type_at(p(0, 6)).as_deref(), Some("string"));
}

/// test.js "addOverlay" (`mode: "javascript"`): the overlay reads the base token
/// with `stream.baseToken()`, and an edit re-runs it.
#[test]
fn add_overlay() {
    let mut cm = cm_mode("foo /* 100 */\nbar + 233;\nbaz", "javascript");
    let ov = FnOverlay(|stream| {
        let base = stream.base_token().expect("baseToken");
        let comment = base.0.as_deref().is_some_and(|t| t.contains("comment"));
        if !comment
            && stream
                .match_regexp(
                    &cm_core::util::regexp::RegExp::new("\\d+", "").unwrap(),
                    true,
                )
                .is_some()
        {
            return Some("x".into());
        }
        stream.next();
        None
    });
    cm.add_overlay(
        cm_view::highlight::OverlaySpec::Mode(Rc::new(ov)),
        false,
        0.0,
    )
    .unwrap();
    let x = tokens_with_class(&cm, "x");
    assert_eq!(x, vec!["233".to_string()]);
    cm.run_op(|cm| cm.doc.replace_range("", p(0, 4), Some(p(0, 6)), None));
    assert_eq!(tokens_with_class(&cm, "x").len(), 2);
}

/// test.js "delete_wrapped" (`lineWrapping: true`, `makeItWrapAfter(cm, Pos(0, 2))`).
#[test]
fn delete_wrapped() {
    let mut cm = cm_opts("12345", None, |o| o.line_wrapping = true);
    // makeItWrapAfter: grow from 0 until char 2 is on the first visual line.
    let first_top = cm.cursor_coords(p(0, 0), CoordSys::Local).top;
    let h = cm.display.height;
    let mut w = 0.0;
    loop {
        cm.set_size(w, h);
        if cm.char_coords(p(0, 2), CoordSys::Local).top == first_top {
            break;
        }
        w += 1.0;
    }
    cm.run_op(|cm| cm.doc.set_cursor(pa(0, 3), None));
    cm.delete_h(-1, UnitH::Char);
    assert_eq!(cm.doc.get_line(0).unwrap(), "1245");
}

/// test.js "mode_lookahead" (`lookahead_mode`: atom when the line two down has an x).
#[test]
fn mode_lookahead() {
    fn token(stream: &mut StringStream<'_>) -> Option<String> {
        stream.skip_to_end();
        // /x/.test(stream.lookAhead(2)): undefined stringifies to "undefined".
        let la = stream.look_ahead(2);
        if la.is_some_and(|l| l.contains(&(b'x' as u16))) {
            Some("atom".into())
        } else {
            None
        }
    }
    let mut cm = editor_with_mode("foo\na\nx\nx\n", "lookahead_mode", token);
    assert_eq!(cm.get_token_at(p(0, 1), false).ty.as_deref(), Some("atom"));
    assert_eq!(cm.get_token_at(p(1, 1), false).ty.as_deref(), Some("atom"));
    assert_eq!(cm.get_token_at(p(2, 1), false).ty, None);
    cm.run_op(|cm| cm.doc.replace_range("\n", p(2, 0), None, None));
    assert_eq!(cm.get_token_at(p(0, 1), false).ty, None);
    assert_eq!(cm.get_token_at(p(1, 1), false).ty.as_deref(), Some("atom"));
}

/// test.js `mouseDown(cm, button, pos, mods)`: a press at `charCoords(pos)`.
fn mouse_down_at(cm: &mut Editor, button: u8, pos: Pos, shift: bool, ctrl: bool, time: f64) {
    let c = cm.char_coords(pos, cm_view::CoordSys::Div);
    cm.mouse_down(&cm_view::MouseEvent {
        x: c.left,
        y: c.top,
        button,
        shift,
        ctrl,
        alt: false,
        meta: false,
        time,
    });
    cm.mouse_up(None);
}

/// test.js:2048 mouseBinding.
#[test]
fn mouse_binding() {
    let mut cm = cm("foo\nbar\nbaz");
    let fired: Rc<RefCell<Vec<&str>>> = Rc::default();
    let mut map = cm_view::keymap::KeyMap::default();
    for (key, tag) in [
        ("Shift-LeftClick", "a"),
        ("Shift-LeftDoubleClick", "b"),
        ("Shift-LeftTripleClick", "c"),
    ] {
        let f = fired.clone();
        map.bind(
            key,
            cm_view::keymap::Binding::Mouse(Rc::new(move |_, pos| {
                if tag == "a" {
                    eq_char_pos(pos.unwrap(), p(1, 2));
                }
                f.borrow_mut().push(tag);
                true
            })),
        );
    }
    cm.add_key_map(map, false);
    let mut t = 1000.0;
    for (button, shift, ctrl) in [
        (1, true, false),
        (1, true, false),
        (1, true, false),
        (1, false, false),
        (2, false, true),
        (2, false, true),
    ] {
        mouse_down_at(&mut cm, button, p(1, 2), shift, ctrl, t);
        t += 10.0;
    }
    assert_eq!(fired.borrow().join(" "), "a b c");
}

/// test.js:2069 configureMouse.
#[test]
fn configure_mouse() {
    let mut cm = cm("foo bar baz");
    cm.set_configure_mouse(Some(Rc::new(|_, _, _| cm_view::mouse::MouseConfig {
        unit: Some(cm_view::mouse::MouseUnit::Word),
        ..Default::default()
    })));
    mouse_down_at(&mut cm, 1, p(0, 5), false, false, 1000.0);
    eq_char_pos(cm.doc.selection().primary().from(), p(0, 4));
    eq_char_pos(cm.doc.selection().primary().to(), p(0, 7));
    cm.set_configure_mouse(Some(Rc::new(|_, _, _| cm_view::mouse::MouseConfig {
        extend: Some(true),
        ..Default::default()
    })));
    mouse_down_at(&mut cm, 1, p(0, 0), false, false, 3000.0);
    eq_char_pos(cm.doc.selection().primary().from(), p(0, 0));
    eq_char_pos(cm.doc.selection().primary().to(), p(0, 4));
}

/// `rangeForUnit` with a `unit` function from `configureMouse` (mouse_events.js:190):
/// the function's range is selected.
#[test]
fn configure_mouse_custom_unit() {
    let mut cm = cm("foo bar baz");
    cm.set_configure_mouse(Some(Rc::new(|_, _, _| cm_view::mouse::MouseConfig {
        unit: Some(cm_view::mouse::MouseUnit::Custom(Rc::new(|_, pos| {
            (Pos::new(pos.line, 1), Pos::new(pos.line, 9))
        }))),
        ..Default::default()
    })));
    mouse_down_at(&mut cm, 1, p(0, 5), false, false, 1000.0);
    eq_char_pos(cm.doc.selection().primary().from(), p(0, 1));
    eq_char_pos(cm.doc.selection().primary().to(), p(0, 9));
}
