//! Port of the model-level tests in CodeMirror 5 `test/test.js`. Each Rust test is
//! named after the JS `testCM("<name>", ...)` it reproduces (snake_case, the JS name
//! is quoted in the comment).
//!
//! `testCM` creates a `CodeMirror` editor, so every test runs against a document
//! with `attach_editor_semantics` (operations, `historyEventDelay` 1250, the editor
//! option defaults) and a manual clock frozen at one instant — the JS tests run
//! within milliseconds, so every timed merge is taken.
//!
//! Not ported because they need view, DOM, measurement, input, commands built on
//! `findPosH`/`findPosV`, line widgets, line classes or `change` events — none of
//! which exist in this model-only crate: everything not listed below, notably
//! `atomicMarker`, `beforeSelectionChange`, `selectionHistory`,
//! `selectionChangeReducesRedo`, `cursorMotionSplitsHistory`, `collapsedLines`,
//! `selectionHomeEnd`, `change_removedText`, `setValueClears` (the line class half;
//! the marker half is ported), `markTextCSS`, `bookmarkCursor`.
//! `readOnlyMarker` is ported except for its two `goCharLeft` commands.

use cm_core::{
    BeforeChange, CmError, Doc, DocOptions, EditorSemantics, HistoryData, MarkOptions, Marker,
    MarkerFind, Pos, SelOptions, TextMarker,
};
use std::cell::Cell;
use std::rc::Rc;

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}
fn pl(line: i64) -> Pos {
    Pos::line_end(line)
}

/// `CodeMirror(place, {value})`.
fn cm(value: &str) -> (Doc, Rc<Cell<f64>>) {
    cm_with(
        DocOptions {
            text: value.to_string(),
            ..Default::default()
        },
        EditorSemantics::default(),
    )
}
fn cm_with(o: DocOptions, sem: EditorSemantics) -> (Doc, Rc<Cell<f64>>) {
    let clock = cm_core::clock::manual(1_000_000.0);
    let d = Doc::with_options(o);
    d.attach_editor_semantics(sem);
    (d, clock)
}

/// `addDoc(cm, width, height)`.
fn add_doc(cm: &Doc, width: usize, height: usize) {
    let line = "x".repeat(width);
    cm.set_value(&vec![line; height].join("\n"));
}

fn eq_cursor_pos(a: Pos, b: Pos) {
    assert_eq!((a.line, a.ch, a.sticky), (b.line, b.ch, b.sticky));
}
fn range_of(m: &Marker) -> Option<(Pos, Pos)> {
    match m.find() {
        Some(MarkerFind::Range { from, to: Some(to) }) => Some((from, to)),
        None => None,
        other => panic!("unexpected find {other:?}"),
    }
}
fn point_of(m: &Marker) -> Option<Pos> {
    match m.find() {
        Some(MarkerFind::Point(q)) => Some(q),
        None => None,
        other => panic!("unexpected find {other:?}"),
    }
}
fn mark(cm: &Doc, from: Pos, to: Pos) -> Marker {
    cm.mark_text(from, to, MarkOptions::default()).unwrap()
}
fn bookmark(cm: &Doc, at: Pos) -> Marker {
    cm.set_bookmark(at, false, false).unwrap()
}

#[test]
fn get_range() {
    // testCM("getRange", ...)
    let (cm, _c) = cm("1234\n5678");
    assert_eq!(cm.get_line(0).as_deref(), Some("1234"));
    assert_eq!(cm.get_line(1).as_deref(), Some("5678"));
    assert_eq!(cm.get_line(2), None);
    assert_eq!(cm.get_line(-1), None);
    assert_eq!(cm.get_range(p(0, 0), p(0, 3), None), "123");
    assert_eq!(cm.get_range(p(0, -1), p(0, 200), None), "1234");
    assert_eq!(cm.get_range(p(0, 2), p(1, 2), None), "34\n56");
    assert_eq!(cm.get_range(p(1, 2), p(100, 0), None), "78");
}

#[test]
fn replace_range() {
    // testCM("replaceRange", ...)
    let (cm, _c) = cm("");
    assert_eq!(cm.get_value(None), "");
    cm.replace_range("foo\n", p(0, 0), None, None);
    assert_eq!(cm.get_value(None), "foo\n");
    cm.replace_range("a\nb", p(0, 1), None, None);
    assert_eq!(cm.get_value(None), "fa\nboo\n");
    assert_eq!(cm.line_count(), 3);
    cm.replace_range("xyzzy", p(0, 0), Some(p(1, 1)), None);
    assert_eq!(cm.get_value(None), "xyzzyoo\n");
    cm.replace_range("abc", p(0, 0), Some(p(10, 0)), None);
    assert_eq!(cm.get_value(None), "abc");
    assert_eq!(cm.line_count(), 1);
}

#[test]
fn selection() {
    // testCM("selection", ...)
    let (cm, _c) = cm("111111\n222222\n333333");
    cm.set_selection(p(0, 4), Some(p(2, 2)), None);
    assert!(cm.something_selected());
    assert_eq!(cm.get_selection(None), "11\n222222\n33");
    eq_cursor_pos(cm.get_cursor(Some("to")), p(2, 2)); // getCursor(false)
    eq_cursor_pos(cm.get_cursor(Some("from")), p(0, 4)); // getCursor(true)
    cm.set_selection(p(1, 0), None, None);
    assert!(!cm.something_selected());
    assert_eq!(cm.get_selection(None), "");
    eq_cursor_pos(cm.get_cursor(Some("from")), p(1, 0));
    cm.replace_selection("abc", Some("around"), None);
    assert_eq!(cm.get_selection(None), "abc");
    assert_eq!(cm.get_value(None), "111111\nabc222222\n333333");
    cm.replace_selection("def", Some("end"), None);
    assert_eq!(cm.get_selection(None), "");
    eq_cursor_pos(cm.get_cursor(Some("from")), p(1, 3));
    cm.set_cursor(p(2, 1), None);
    eq_cursor_pos(cm.get_cursor(Some("from")), p(2, 1));
    cm.set_cursor(p(1, 2), None); // cm.setCursor(1, 2)
    eq_cursor_pos(cm.get_cursor(Some("from")), p(1, 2));
}

#[test]
fn extend_selection() {
    // testCM("extendSelection", ...)
    let (cm, _c) = cm("");
    cm.set_extending(true);
    add_doc(&cm, 10, 10);
    cm.set_selection(p(3, 5), None, None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(3, 5));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(3, 5));
    cm.set_selection(p(2, 5), Some(p(5, 5)), None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(5, 5));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(2, 5));
    eq_cursor_pos(cm.get_cursor(Some("start")), p(2, 5));
    eq_cursor_pos(cm.get_cursor(Some("end")), p(5, 5));
    cm.set_selection(p(5, 5), Some(p(2, 5)), None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(2, 5));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(5, 5));
    eq_cursor_pos(cm.get_cursor(Some("start")), p(2, 5));
    eq_cursor_pos(cm.get_cursor(Some("end")), p(5, 5));
    cm.extend_selection(p(3, 2), None, None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(3, 2));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(5, 5));
    cm.extend_selection(p(6, 2), None, None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(6, 2));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(5, 5));
    cm.extend_selection(p(6, 3), Some(p(6, 4)), None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(6, 4));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(5, 5));
    cm.extend_selection(p(0, 3), Some(p(0, 4)), None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(0, 3));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(5, 5));
    cm.extend_selection(p(4, 5), Some(p(6, 5)), None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(6, 5));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(4, 5));
    cm.set_extending(false);
    cm.extend_selection(p(0, 3), Some(p(0, 4)), None);
    eq_cursor_pos(cm.get_cursor(Some("head")), p(0, 3));
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(0, 4));
}

#[test]
fn lines() {
    // testCM("lines", ...)
    let (cm, _c) = cm("111111\n222222\n333333");
    assert_eq!(cm.get_line(0).as_deref(), Some("111111"));
    assert_eq!(cm.get_line(1).as_deref(), Some("222222"));
    assert_eq!(cm.get_line(-1), None);
    cm.replace_range("", p(1, 0), Some(p(2, 0)), None);
    cm.replace_range("abc", p(1, 0), Some(pl(1)), None);
    assert_eq!(cm.get_value(None), "111111\nabc");
}

#[test]
fn pos_from_index() {
    // testCM("posFromIndex", ...)
    let (cm, _c) = cm("");
    cm.set_value("This function should\nconvert a zero based index\nto line and ch.");
    let examples = [
        (-1, 0, 0),
        (0, 0, 0),
        (10, 0, 10),
        (39, 1, 18),
        (55, 2, 7),
        (63, 2, 15),
        (64, 2, 15),
    ];
    for (index, line, ch) in examples {
        let pos = cm.pos_from_index(index);
        assert_eq!((pos.line, pos.ch), (line, ch), "index {index}");
        if (0..64).contains(&index) {
            assert_eq!(cm.index_from_pos(pos), index);
        }
    }
}

#[test]
fn undo() {
    // testCM("undo", ...)
    let (cm, _c) = cm("abc");
    cm.replace_range("def", p(0, 0), Some(pl(0)), None);
    assert_eq!(cm.history_size().undo, 1);
    cm.undo();
    assert_eq!(cm.get_value(None), "abc");
    assert_eq!(cm.history_size().undo, 0);
    assert_eq!(cm.history_size().redo, 1);
    cm.redo();
    assert_eq!(cm.get_value(None), "def");
    assert_eq!(cm.history_size().undo, 1);
    assert_eq!(cm.history_size().redo, 0);
    cm.set_value("1\n\n\n2");
    cm.clear_history();
    assert_eq!(cm.history_size().undo, 0);
    for _ in 0..20 {
        cm.replace_range("a", p(0, 0), None, None);
        cm.replace_range("b", p(3, 0), None, None);
    }
    assert_eq!(cm.history_size().undo, 40);
    for _ in 0..40 {
        cm.undo();
    }
    assert_eq!(cm.history_size().redo, 40);
    assert_eq!(cm.get_value(None), "1\n\n\n2");
}

#[test]
fn undo_depth() {
    // testCM("undoDepth", ..., {value: "abc", undoDepth: 4})
    let (cm, _c) = cm_with(
        DocOptions {
            text: "abc".into(),
            ..Default::default()
        },
        EditorSemantics {
            undo_depth: 4.0,
            ..EditorSemantics::default()
        },
    );
    cm.replace_range("d", pl(0), None, None);
    cm.replace_range("e", pl(0), None, None);
    cm.replace_range("f", pl(0), None, None);
    cm.undo();
    cm.undo();
    cm.undo();
    assert_eq!(cm.get_value(None), "abcd");
}

#[test]
fn undo_doesnt_clear_value() {
    // testCM("undoDoesntClearValue", ...)
    let (cm, _c) = cm("x");
    cm.undo();
    assert_eq!(cm.get_value(None), "x");
}

#[test]
fn undo_multi_line() {
    // testCM("undoMultiLine", ...)
    let (cm, _c) = cm("abc\ndef\nghi");
    cm.operation(|| {
        cm.replace_range("x", p(0, 0), None, None);
        cm.replace_range("y", p(1, 0), None, None);
    });
    cm.undo();
    assert_eq!(cm.get_value(None), "abc\ndef\nghi");
    cm.operation(|| {
        cm.replace_range("y", p(1, 0), None, None);
        cm.replace_range("x", p(0, 0), None, None);
    });
    cm.undo();
    assert_eq!(cm.get_value(None), "abc\ndef\nghi");
    cm.operation(|| {
        cm.replace_range("y", p(2, 0), None, None);
        cm.replace_range("x", p(1, 0), None, None);
        cm.replace_range("z", p(2, 0), None, None);
    });
    cm.undo();
    assert_eq!(cm.get_value(None), "abc\ndef\nghi");
}

#[test]
fn undo_composite() {
    // testCM("undoComposite", ...)
    let (cm, _c) = cm("a\nb\nc\n");
    cm.replace_range("y", pl(1), None, None);
    cm.operation(|| {
        cm.replace_range("x", pl(0), None, None);
        cm.replace_range("z", pl(2), None, None);
    });
    assert_eq!(cm.get_value(None), "ax\nby\ncz\n");
    cm.undo();
    assert_eq!(cm.get_value(None), "a\nby\nc\n");
    cm.undo();
    assert_eq!(cm.get_value(None), "a\nb\nc\n");
    cm.redo();
    cm.redo();
    assert_eq!(cm.get_value(None), "ax\nby\ncz\n");
}

#[test]
fn undo_selection() {
    // testCM("undoSelection", ...)
    let (cm, _c) = cm("abcdefgh\n");
    cm.set_selection(p(0, 2), Some(p(0, 4)), None);
    cm.replace_selection("", None, None);
    cm.set_cursor(p(1, 0), None);
    cm.undo();
    eq_cursor_pos(cm.get_cursor(Some("from")), p(0, 2));
    eq_cursor_pos(cm.get_cursor(Some("to")), p(0, 4));
    cm.set_cursor(p(1, 0), None);
    cm.redo();
    eq_cursor_pos(cm.get_cursor(Some("from")), p(0, 2));
    eq_cursor_pos(cm.get_cursor(Some("to")), p(0, 2));
}

#[test]
fn undo_selection_as_before() {
    // testCM("undoSelectionAsBefore", ...)
    let (cm, _c) = cm("");
    cm.replace_selection("abc", Some("around"), None);
    cm.undo();
    cm.redo();
    assert_eq!(cm.get_selection(None), "abc");
}

#[test]
fn selection_change_confuses_history() {
    // testCM("selectionChangeConfusesHistory", ...)
    let (cm, _c) = cm("");
    cm.replace_selection("abc", None, Some("dontmerge"));
    cm.operation(|| {
        cm.set_cursor(p(0, 0), None);
        cm.replace_selection("abc", None, Some("dontmerge"));
    });
    assert_eq!(cm.history_size().undo, 2);
}

#[test]
fn mark_text_single_line() {
    // testCM("markTextSingleLine", ...)
    let (cm, _c) = cm("");
    let cases: [(i64, i64, &str, Option<i64>, Option<i64>); 14] = [
        (0, 1, "", Some(2), Some(5)),
        (0, 4, "", Some(0), Some(2)),
        (1, 2, "x", Some(3), Some(6)),
        (4, 5, "", Some(3), Some(5)),
        (4, 5, "xx", Some(3), Some(7)),
        (2, 5, "", Some(2), Some(3)),
        (2, 5, "abcd", Some(6), Some(7)),
        (2, 6, "x", None, None),
        (3, 6, "", None, None),
        (0, 9, "hallo", None, None),
        (4, 6, "x", Some(3), Some(4)),
        (4, 8, "", Some(3), Some(4)),
        (6, 6, "a", Some(3), Some(6)),
        (8, 9, "", Some(3), Some(6)),
    ];
    for (a, b, c, f, t) in cases {
        cm.set_value("1234567890");
        let r = cm
            .mark_text(
                p(0, 3),
                p(0, 6),
                MarkOptions {
                    class_name: Some("foo".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        cm.replace_range(c, p(0, a), Some(p(0, b)), None);
        let found = range_of(&r);
        assert_eq!(found.map(|x| x.0.ch), f, "case {a} {b} {c:?}");
        assert_eq!(found.map(|x| x.1.ch), t, "case {a} {b} {c:?}");
    }
}

#[test]
fn mark_text_multi_line() {
    // testCM("markTextMultiLine", ...)
    let (cm, _c) = cm("");
    type C = (
        (i64, i64),
        (i64, i64),
        &'static str,
        Option<(i64, i64)>,
        Option<(i64, i64)>,
    );
    let cases: [C; 15] = [
        ((0, 0), (0, 5), "", Some((0, 0)), Some((2, 5))),
        ((0, 0), (0, 5), "foo\n", Some((1, 0)), Some((3, 5))),
        ((0, 1), (0, 10), "", Some((0, 1)), Some((2, 5))),
        ((0, 5), (0, 6), "x", Some((0, 6)), Some((2, 5))),
        ((0, 0), (1, 0), "", Some((0, 0)), Some((1, 5))),
        ((0, 6), (2, 4), "", Some((0, 5)), Some((0, 7))),
        ((0, 6), (2, 4), "aa", Some((0, 5)), Some((0, 9))),
        ((1, 2), (1, 8), "", Some((0, 5)), Some((2, 5))),
        ((0, 5), (2, 5), "xx", None, None),
        ((0, 0), (2, 10), "x", None, None),
        ((1, 5), (2, 5), "", Some((0, 5)), Some((1, 5))),
        ((2, 0), (2, 3), "", Some((0, 5)), Some((2, 2))),
        ((2, 5), (3, 0), "a\nb", Some((0, 5)), Some((2, 5))),
        ((2, 3), (3, 0), "x", Some((0, 5)), Some((2, 3))),
        ((1, 1), (1, 9), "1\n2\n3", Some((0, 5)), Some((4, 5))),
    ];
    for (a, b, c, f, t) in cases {
        cm.set_value("aaaaaaaaaa\nbbbbbbbbbb\ncccccccccc\ndddddddd\n");
        let r = cm
            .mark_text(
                p(0, 5),
                p(2, 5),
                MarkOptions {
                    class_name: Some("CodeMirror-matchingbracket".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        cm.replace_range(c, p(a.0, a.1), Some(p(b.0, b.1)), None);
        let found = range_of(&r);
        assert_eq!(
            found.map(|x| (x.0.line, x.0.ch)),
            f,
            "case {a:?} {b:?} {c:?}"
        );
        assert_eq!(
            found.map(|x| (x.1.line, x.1.ch)),
            t,
            "case {a:?} {b:?} {c:?}"
        );
    }
}

#[test]
fn mark_text_undo() {
    // testCM("markTextUndo", ...)
    let (cm, _c) = cm("1234\n56789\n00\n");
    let cls = || MarkOptions {
        class_name: Some("CodeMirror-matchingbracket".into()),
        ..Default::default()
    };
    let marker1 = cm.mark_text(p(0, 1), p(0, 3), cls()).unwrap();
    let marker2 = cm.mark_text(p(0, 0), p(2, 1), cls()).unwrap();
    let bm = bookmark(&cm, p(1, 5));
    cm.operation(|| {
        cm.replace_range("foo", p(0, 2), None, None);
        cm.replace_range("bar\nbaz\nbug\n", p(2, 0), Some(p(3, 0)), None);
    });
    let v1 = cm.get_value(None);
    cm.set_value("");
    assert!(marker1.find().is_none());
    assert!(marker2.find().is_none());
    assert!(bm.find().is_none());
    cm.undo();
    eq_cursor_pos(point_of(&bm).expect("still there"), p(1, 5));
    cm.undo();
    let m1 = range_of(&marker1).unwrap();
    let m2 = range_of(&marker2).unwrap();
    eq_cursor_pos(m1.0, p(0, 1));
    eq_cursor_pos(m1.1, p(0, 3));
    eq_cursor_pos(m2.0, p(0, 0));
    eq_cursor_pos(m2.1, p(2, 1));
    eq_cursor_pos(point_of(&bm).unwrap(), p(1, 5));
    cm.redo();
    cm.redo();
    assert!(bm.find().is_none());
    cm.undo();
    eq_cursor_pos(point_of(&bm).unwrap(), p(1, 5));
    assert_eq!(cm.get_value(None), v1);
}

#[test]
fn mark_text_stay_gone() {
    // testCM("markTextStayGone", ...)
    let (cm, _c) = cm("hello");
    let m1 = mark(&cm, p(0, 0), p(0, 1));
    cm.replace_range("hi", p(0, 2), None, None);
    m1.clear();
    cm.undo();
    assert!(m1.find().is_none());
}

#[test]
fn mark_text_allow_empty() {
    // testCM("markTextAllowEmpty", ...)
    let (cm, _c) = cm("abcde");
    let keep = |l: bool, r: bool| MarkOptions {
        clear_when_empty: Some(false),
        inclusive_left: l,
        inclusive_right: r,
        ..Default::default()
    };
    let m1 = cm.mark_text(p(0, 1), p(0, 2), keep(false, false)).unwrap();
    assert!(m1.find().is_some());
    cm.replace_range("x", p(0, 0), None, None);
    assert!(m1.find().is_some());
    cm.replace_range("y", p(0, 2), None, None);
    assert!(m1.find().is_some());
    cm.replace_range("z", p(0, 3), Some(p(0, 4)), None);
    assert!(m1.find().is_none());
    let m2 = cm.mark_text(p(0, 1), p(0, 2), keep(true, true)).unwrap();
    cm.replace_range("q", p(0, 1), Some(p(0, 2)), None);
    assert!(m2.find().is_some());
    cm.replace_range("", p(0, 0), Some(p(0, 3)), None);
    assert!(m2.find().is_none());
    let m3 = cm.mark_text(p(0, 1), p(0, 1), keep(false, false)).unwrap();
    cm.replace_range("a", p(0, 3), None, None);
    assert!(m3.find().is_some());
    cm.replace_range("b", p(0, 1), None, None);
    assert!(m3.find().is_none());
}

#[test]
fn mark_text_stacked() {
    // testCM("markTextStacked", ...)
    let (cm, _c) = cm("A");
    let keep = || MarkOptions {
        clear_when_empty: Some(false),
        ..Default::default()
    };
    let m1 = cm.mark_text(p(0, 0), p(0, 0), keep()).unwrap();
    let m2 = cm.mark_text(p(0, 0), p(0, 0), keep()).unwrap();
    cm.replace_range("B", p(0, 1), None, None);
    assert!(m1.find().is_some() && m2.find().is_some());
}

#[test]
fn undo_preserves_new_marks() {
    // testCM("undoPreservesNewMarks", ...)
    let (cm, _c) = cm("aaaa\nbbbb\ncccc\ndddd");
    mark(&cm, p(0, 3), p(0, 4));
    mark(&cm, p(1, 1), p(1, 3));
    cm.replace_range("", p(0, 3), Some(p(3, 1)), None);
    let m_before = mark(&cm, p(0, 0), p(0, 1));
    let m_after = mark(&cm, p(0, 5), p(0, 6));
    let m_around = mark(&cm, p(0, 2), p(0, 4));
    cm.undo();
    let b = range_of(&m_before).unwrap();
    eq_cursor_pos(b.0, p(0, 0));
    eq_cursor_pos(b.1, p(0, 1));
    let a = range_of(&m_after).unwrap();
    eq_cursor_pos(a.0, p(3, 3));
    eq_cursor_pos(a.1, p(3, 4));
    let r = range_of(&m_around).unwrap();
    eq_cursor_pos(r.0, p(0, 2));
    eq_cursor_pos(r.1, p(3, 2));
    let found = cm.find_marks_at(p(2, 2));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], m_around);
}

#[test]
fn mark_clear_between() {
    // testCM("markClearBetween", ...)
    let (cm, _c) = cm("");
    cm.set_value("aaa\nbbb\nccc\nddd\n");
    mark(&cm, p(0, 0), pl(2));
    cm.replace_range("aaa\nbbb\nccc", p(0, 0), Some(pl(2)), None);
    assert_eq!(cm.find_marks_at(p(1, 1)).len(), 0);
}

#[test]
fn find_marks_middle() {
    // testCM("findMarksMiddle", ...)
    let (cm, _c) = cm("line 0\nline 1\nline 2\nline 3");
    let m = mark(&cm, p(1, 1), p(3, 1));
    let found = cm.find_marks(p(2, 1), p(2, 2), None);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], m);
}

#[test]
fn delete_span_collapsed_inclusive_left() {
    // testCM("deleteSpanCollapsedInclusiveLeft", ...): must not throw.
    let (cm, _c) = cm("abc\nX\ndef");
    let (from, to) = (p(1, 0), p(1, 1));
    let _m = cm
        .mark_text(
            from,
            to,
            MarkOptions {
                collapsed: true,
                inclusive_left: true,
                ..Default::default()
            },
        )
        .unwrap();
    cm.replace_range("", from, Some(to), None);
    cm.check_tree().unwrap();
}

#[test]
fn bookmark_test() {
    // testCM("bookmark", ...)
    let (cm, _c) = cm("");
    type C = (
        Option<(i64, i64)>,
        (i64, i64),
        (i64, i64),
        &'static str,
        Option<(i64, i64)>,
    );
    let cases: [C; 8] = [
        (None, (1, 0), (1, 1), "", Some((1, 4))),
        (None, (1, 1), (1, 1), "xx", Some((1, 7))),
        (None, (1, 4), (1, 5), "ab", Some((1, 6))),
        (None, (1, 4), (1, 6), "", None),
        (None, (1, 5), (1, 6), "abc", Some((1, 5))),
        (None, (1, 6), (1, 8), "", Some((1, 5))),
        (None, (1, 4), (1, 4), "\n\n", Some((3, 1))),
        (Some((1, 9)), (1, 1), (1, 1), "\n", Some((2, 8))),
    ];
    for (bm, a, b, c, d) in cases {
        cm.set_value("1234567890\n1234567890\n1234567890");
        let at = bm.map(|x| p(x.0, x.1)).unwrap_or(p(1, 5));
        let m = bookmark(&cm, at);
        cm.replace_range(c, p(a.0, a.1), Some(p(b.0, b.1)), None);
        assert_eq!(
            point_of(&m).map(|q| (q.line, q.ch, q.sticky)),
            d.map(|x| (x.0, x.1, None)),
            "case {a:?} {b:?} {c:?}"
        );
    }
}

#[test]
fn bookmark_insert_left() {
    // testCM("bookmarkInsertLeft", ...)
    let (cm, _c) = cm("abcdef");
    let br = cm.set_bookmark(p(0, 2), false, false).unwrap();
    let bl = cm.set_bookmark(p(0, 2), true, false).unwrap();
    cm.set_cursor(p(0, 2), None);
    cm.replace_selection("hi", None, None);
    eq_cursor_pos(point_of(&br).unwrap(), p(0, 2));
    eq_cursor_pos(point_of(&bl).unwrap(), p(0, 4));
    cm.replace_range("", p(0, 4), Some(p(0, 5)), None);
    cm.replace_range("", p(0, 2), Some(p(0, 4)), None);
    cm.replace_range("", p(0, 1), Some(p(0, 2)), None);
    // Verify that deleting next to bookmarks doesn't kill them
    eq_cursor_pos(point_of(&br).unwrap(), p(0, 1));
    eq_cursor_pos(point_of(&bl).unwrap(), p(0, 1));
}

#[test]
fn get_all_marks() {
    // testCM("getAllMarks", ...)
    let (cm, _c) = cm("");
    add_doc(&cm, 10, 10);
    let m1 = bookmark(&cm, p(0, 2));
    let _m2 = mark(&cm, p(0, 2), p(3, 2));
    let m3 = mark(&cm, p(1, 2), p(1, 8));
    let _m4 = mark(&cm, p(8, 0), p(9, 0));
    assert_eq!(cm.get_all_marks().len(), 4);
    m1.clear();
    m3.clear();
    assert_eq!(cm.get_all_marks().len(), 2);
}

#[test]
fn set_value_clears() {
    // testCM("setValueClears", ...) — the marker half (line classes are view state).
    let (cm, _c) = cm("a\nb");
    let m = cm
        .mark_text(
            p(0, 0),
            p(1, 1),
            MarkOptions {
                inclusive_left: true,
                inclusive_right: true,
                ..Default::default()
            },
        )
        .unwrap();
    cm.set_value("foo");
    assert!(m.find().is_none());
}

#[test]
fn bug577() {
    // testCM("bug577", ...): must not throw.
    let (cm, _c) = cm("");
    cm.set_value("a\nb");
    cm.clear_history();
    cm.set_value("fooooo");
    cm.undo();
}

#[test]
fn restore_history() {
    // testCM("restoreHistory", ...)
    let (cm, _c) = cm("");
    cm.set_value("abc\ndef");
    cm.replace_range("hello", p(1, 0), Some(pl(1)), None);
    cm.replace_range("goop", p(0, 0), Some(pl(0)), None);
    cm.undo();
    let stored_val = cm.get_value(None);
    // JSON.parse(JSON.stringify(...)) — HistoryData is plain data already.
    let stored_hist: HistoryData = cm.get_history().clone();
    assert_eq!(stored_val, "abc\nhello");
    cm.set_value("");
    cm.clear_history();
    assert_eq!(cm.history_size().undo, 0);
    cm.set_value(&stored_val);
    cm.set_history(&stored_hist);
    cm.redo();
    assert_eq!(cm.get_value(None), "goop\nhello");
    cm.undo();
    cm.undo();
    assert_eq!(cm.get_value(None), "abc\ndef");
}

#[test]
fn weird_linebreaks() {
    // testCM("weirdLinebreaks", ...)
    let (cm, _c) = cm("");
    cm.set_value("foo\nbar\rbaz\r\nquux\n\rplop");
    assert_eq!(cm.get_value(None), "foo\nbar\nbaz\nquux\n\nplop");
    assert_eq!(cm.line_count(), 6);
    cm.set_value("\n\n");
    assert_eq!(cm.line_count(), 3);
}

#[test]
fn get_line_number() {
    // testCM("getLineNumber", ...)
    let (cm, _c) = cm("");
    add_doc(&cm, 2, 20);
    let h1 = cm.get_line_handle(1).unwrap();
    assert_eq!(cm.get_line_number(&h1), Some(1));
    cm.replace_range("hi\nbye\n", p(0, 0), None, None);
    assert_eq!(cm.get_line_number(&h1), Some(3));
    cm.set_value("");
    assert_eq!(cm.get_line_number(&h1), None);
}

#[test]
fn selection_bias() {
    // testCM("selectionBias", ...)
    let (cm, _c) = cm("12345");
    cm.mark_text(
        p(0, 1),
        p(0, 3),
        MarkOptions {
            atomic: true,
            ..Default::default()
        },
    )
    .unwrap();
    cm.set_cursor(p(0, 2), None);
    eq_cursor_pos(cm.get_cursor(None), p(0, 1));
    cm.set_cursor(p(0, 2), None);
    eq_cursor_pos(cm.get_cursor(None), p(0, 3));
    cm.set_cursor(p(0, 2), None);
    eq_cursor_pos(cm.get_cursor(None), p(0, 1));
    cm.set_cursor(
        p(0, 2),
        Some(SelOptions {
            bias: Some(-1),
            ..Default::default()
        }),
    );
    eq_cursor_pos(cm.get_cursor(None), p(0, 1));
    cm.set_cursor(p(0, 4), None);
    cm.set_cursor(
        p(0, 2),
        Some(SelOptions {
            bias: Some(1),
            ..Default::default()
        }),
    );
    eq_cursor_pos(cm.get_cursor(None), p(0, 3));
}

#[test]
fn read_only_marker() {
    // testCM("readOnlyMarker", ...) without the two `goCharLeft` commands and the
    // cursor assertion that follows them (commands are not part of the model).
    let (cm, _c) = cm("abcde\nfghij\nklmno\n");
    let mk = |ll, cl, lr, cr, at| {
        cm.mark_text(
            p(ll, cl),
            p(lr, cr),
            MarkOptions {
                read_only: true,
                atomic: at,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let m = mk(0, 1, 0, 4, false);
    cm.set_cursor(p(0, 2), None);
    cm.replace_selection("hi", Some("end"), None);
    eq_cursor_pos(cm.get_cursor(None), p(0, 2));
    assert_eq!(cm.get_line(0).as_deref(), Some("abcde"));
    cm.select_all(); // execCommand("selectAll")
    cm.replace_selection("oops", Some("around"), None);
    assert_eq!(cm.get_value(None), "oopsbcd");
    cm.undo();
    let r = range_of(&m).unwrap();
    eq_cursor_pos(r.0, p(0, 1));
    eq_cursor_pos(r.1, p(0, 4));
    m.clear();
    cm.set_cursor(p(0, 2), None);
    cm.replace_selection("hi", Some("around"), None);
    assert_eq!(cm.get_line(0).as_deref(), Some("abhicde"));
    eq_cursor_pos(cm.get_cursor(None), p(0, 4));
    let _m = mk(0, 2, 2, 2, true);
    cm.set_selection(p(1, 1), Some(p(2, 4)), None);
    cm.replace_selection("t", Some("end"), None);
    eq_cursor_pos(cm.get_cursor(None), p(2, 3));
    assert_eq!(cm.get_line(2).as_deref(), Some("klto"));
    // cm.execCommand("goCharLeft") x2; eqCursorPos(cm.getCursor(), Pos(0, 2)) — skipped
    cm.set_selection(p(0, 1), Some(p(0, 3)), None);
    cm.replace_selection("xx", Some("around"), None);
    eq_cursor_pos(cm.get_cursor(None), p(0, 3));
    assert_eq!(cm.get_line(0).as_deref(), Some("axxhicde"));
}

#[test]
fn dirty_bit() {
    // testCM("dirtyBit", ...)
    let (cm, _c) = cm("");
    assert!(cm.is_clean(None));
    cm.replace_selection("boo", None, Some("test"));
    assert!(!cm.is_clean(None));
    cm.undo();
    assert!(cm.is_clean(None));
    cm.replace_selection("boo", None, Some("test"));
    cm.replace_selection("baz", None, Some("test"));
    cm.undo();
    assert!(!cm.is_clean(None));
    cm.mark_clean();
    assert!(cm.is_clean(None));
    cm.undo();
    assert!(!cm.is_clean(None));
    cm.redo();
    assert!(cm.is_clean(None));
}

#[test]
fn change_generation() {
    // testCM("changeGeneration", ...)
    let (cm, _c) = cm("");
    cm.replace_selection("x", None, None);
    let soft_gen = cm.change_generation(false);
    cm.replace_selection("x", None, None);
    cm.undo();
    assert_eq!(cm.get_value(None), "");
    assert!(!cm.is_clean(Some(soft_gen)));
    cm.replace_selection("x", None, None);
    let hard_gen = cm.change_generation(true);
    cm.replace_selection("x", None, None);
    cm.undo();
    assert_eq!(cm.get_value(None), "x");
    assert!(cm.is_clean(Some(hard_gen)));
}

#[test]
fn before_change() {
    // testCM("beforeChange", ...)
    let (cm, _c) = cm("abcdefghijk");
    cm.on_before_change(|_, change: &mut BeforeChange| {
        let text = change
            .text
            .iter()
            .map(|t| {
                t.chars()
                    .map(|c| if c.is_whitespace() { '_' } else { c })
                    .collect()
            })
            .collect();
        change.update(None, None, Some(text), None);
    });
    cm.set_value("hello, i am a\nnew document\n");
    assert_eq!(cm.get_value(None), "hello,_i_am_a\nnew_document\n");
    cm.on_before_change(|_, change: &mut BeforeChange| {
        if change.from.line == 0 {
            change.cancel();
        }
    });
    cm.set_value("oops"); // Canceled
    assert_eq!(cm.get_value(None), "hello,_i_am_a\nnew_document\n");
    cm.replace_range("hey hey hey", p(1, 0), Some(p(2, 0)), None);
    assert_eq!(cm.get_value(None), "hello,_i_am_a\nhey_hey_hey");
}

#[test]
fn before_change_undo() {
    // testCM("beforeChangeUndo", ...)
    let (cm, _c) = cm("one\ntwo");
    cm.replace_range("hi", p(0, 0), Some(pl(0)), None);
    cm.replace_range("bye", p(0, 0), Some(pl(0)), None);
    assert_eq!(cm.history_size().undo, 2);
    cm.on_before_change(|_, change: &mut BeforeChange| {
        assert!(!change.can_update());
        change.cancel();
    });
    cm.undo();
    assert_eq!(cm.history_size().undo, 0);
    assert_eq!(cm.get_value(None), "bye\ntwo");
}

#[test]
fn selection_history_non_overlapping() {
    // testCM("selectionHistoryNonOverlapping", ...)
    let (cm, _c) = cm("1234");
    cm.set_selection(p(0, 0), Some(p(0, 1)), None);
    cm.set_selection(p(0, 2), Some(p(0, 3)), None);
    cm.undo_selection(); // execCommand("undoSelection")
    eq_cursor_pos(cm.get_cursor(Some("anchor")), p(0, 0));
    eq_cursor_pos(cm.get_cursor(Some("head")), p(0, 1));
}

#[test]
fn always_merge_sel_event_with_change_origin() {
    // testCM("alwaysMergeSelEventWithChangeOrigin", ...)
    let (cm, _c) = cm("a");
    let origin = |o: &str| {
        Some(SelOptions {
            origin: Some(o.into()),
            ..Default::default()
        })
    };
    cm.replace_selection("U", None, Some("foo"));
    cm.set_selection(p(0, 0), Some(p(0, 1)), origin("foo"));
    cm.undo_selection();
    assert_eq!(cm.get_value(None), "a");
    cm.replace_selection("V", None, Some("foo"));
    cm.set_selection(p(0, 0), Some(p(0, 1)), origin("bar"));
    cm.undo_selection();
    assert_eq!(cm.get_value(None), "Va");
}

#[test]
fn sel_change_in_operation_does_not_split() {
    // testCM("selChangeInOperationDoesNotSplit", ...)
    let (cm, _c) = cm("a");
    for _ in 0..4 {
        cm.operation(|| {
            cm.replace_selection("x", None, None);
            cm.set_cursor(p(0, cm.get_cursor(None).ch - 1), None);
        });
    }
    eq_cursor_pos(cm.get_cursor(None), p(0, 0));
    assert_eq!(cm.get_value(None), "xxxxa");
    cm.undo();
    assert_eq!(cm.get_value(None), "a");
}

#[test]
fn line_separator() {
    // testCM("lineSeparator", ..., {value: "foo\nbar\r\nbaz\rquux", lineSeparator: "\n"})
    let (cm, _c) = cm_with(
        DocOptions {
            text: "foo\nbar\r\nbaz\rquux".into(),
            line_sep: Some("\n".into()),
            ..Default::default()
        },
        EditorSemantics::default(),
    );
    assert_eq!(cm.line_count(), 3);
    assert_eq!(cm.get_line(1).as_deref(), Some("bar\r"));
    assert_eq!(cm.get_line(2).as_deref(), Some("baz\rquux"));
    cm.set_line_separator_option(Some("\r"));
    assert_eq!(cm.line_count(), 5);
    assert_eq!(cm.get_line(4).as_deref(), Some("quux"));
    assert_eq!(cm.get_value(None), "foo\rbar\r\rbaz\rquux");
    assert_eq!(cm.get_value(Some("\n")), "foo\nbar\n\nbaz\nquux");
    cm.set_line_separator_option(None);
    cm.set_value("foo\nbar\r\nbaz\rquux");
    assert_eq!(cm.line_count(), 4);
}

#[test]
fn hidden_lines_select_all() {
    // testCM("hiddenLinesSelectAll", ...) — Issue #484, with `foldLines`.
    let (cm, _c) = cm("");
    add_doc(&cm, 4, 20);
    let fold = |start: i64, end: i64| {
        cm.mark_text(
            p(start, 0),
            pl(end - 1),
            MarkOptions {
                inclusive_left: true,
                inclusive_right: true,
                collapsed: true,
                ..Default::default()
            },
        )
        .unwrap()
    };
    fold(0, 10);
    fold(11, 20);
    cm.select_all(); // CodeMirror.commands.selectAll(cm)
    eq_cursor_pos(cm.get_cursor(Some("from")), p(10, 0));
    eq_cursor_pos(cm.get_cursor(Some("to")), p(10, 4));
}

#[test]
fn random_collapsed_ranges() {
    // testCM("randomCollapsedRanges", ...) with a seeded generator instead of
    // Math.random; the JS only checks that nothing but "overlapping" throws. The
    // B-tree and marker invariants are checked as well.
    let (cm, _c) = cm("");
    add_doc(&cm, 20, 500);
    let mut seed: u32 = 577;
    let mut rnd = move || {
        seed = seed.wrapping_add(0x6D2B_79F5);
        let mut t = seed;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    };
    let mut overlaps = 0;
    cm.operation(|| {
        for i in 0..200 {
            let start = p(
                (rnd() * 500.0).floor() as i64,
                (rnd() * 20.0).floor() as i64,
            );
            if i % 4 != 0 {
                match cm.mark_text(
                    start,
                    p(start.line + 2, 1),
                    MarkOptions {
                        collapsed: true,
                        ..Default::default()
                    },
                ) {
                    Ok(_) => {}
                    Err(CmError::CollapsedOverlap) => overlaps += 1,
                    Err(e) => panic!("{e:?}"),
                }
            } else {
                cm.mark_text(
                    start,
                    p(start.line, start.ch + 4),
                    MarkOptions {
                        class_name: Some("foo".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
            }
        }
    });
    cm.check_tree().unwrap();
    assert!(
        overlaps > 0,
        "the generator should produce some overlapping folds"
    );
    let all: Vec<TextMarker> = cm.get_all_marks();
    assert!(all.len() > 50);
}

/// Not from test.js: a B-tree stress test (25/50/10 chunk thresholds) checking the
/// tree invariants and the text against a plain `Vec<String>` model.
#[test]
fn btree_stress() {
    let (cm, _c) = cm("");
    // Keep every event so the whole run can be undone (the editor default is 200).
    cm.set_undo_depth(f64::INFINITY);
    let mut model: Vec<String> = vec![String::new()];
    let mut seed: u64 = 0x5eed;
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n.max(1)
    };
    for step in 0..3000 {
        let n = model.len() as u64;
        let l1 = rnd(n) as i64;
        let span = if rnd(4) == 0 { 120 } else { 3 };
        let l2 = (l1 + rnd(span) as i64).min(n as i64 - 1);
        let c1 = rnd(model[l1 as usize].len() as u64 + 1) as i64;
        let c2 = if l2 == l1 {
            c1 + rnd(model[l1 as usize].len() as u64 - c1 as u64 + 1) as i64
        } else {
            rnd(model[l2 as usize].len() as u64 + 1) as i64
        };
        let ins_lines = if rnd(3) == 0 {
            rnd(80) as usize
        } else {
            rnd(3) as usize
        };
        let text: Vec<String> = (0..=ins_lines)
            .map(|i| {
                format!("s{step}l{i}")
                    .chars()
                    .take(rnd(6) as usize)
                    .collect()
            })
            .collect();
        cm.replace_range(&text.join("\n"), p(l1, c1), Some(p(l2, c2)), None);
        // model
        let head = model[l1 as usize][..c1 as usize].to_string();
        let tail = model[l2 as usize][c2 as usize..].to_string();
        let mut repl = text.clone();
        repl[0] = head + &repl[0];
        let last = repl.len() - 1;
        repl[last] += &tail;
        model.splice(l1 as usize..=l2 as usize, repl);
        if step % 50 == 0 {
            cm.check_tree().unwrap();
            assert_eq!(cm.get_value(None), model.join("\n"), "step {step}");
        }
    }
    cm.check_tree().unwrap();
    assert_eq!(cm.get_value(None), model.join("\n"));
    assert!(
        cm.tree_depth() >= 2,
        "the stress run should grow an inner level"
    );
    while cm.history_size().undo > 0 {
        cm.undo();
    }
    cm.check_tree().unwrap();
    assert_eq!(cm.get_value(None), "");
}
