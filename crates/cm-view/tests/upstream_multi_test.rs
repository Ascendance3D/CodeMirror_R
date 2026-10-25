//! Port of CodeMirror 5 `test/multi_test.js` (namespace `multi_`). Every test of
//! that file is ported; each Rust test names the JS `testCM("<name>")` it reproduces.

mod common;
use common::*;

use cm_core::EditorSemantics;
use cm_view::Indent;

/// multi_test.js "getSelection"
#[test]
fn multi_get_selection() {
    let mut cm = cm("1234\n5678\n90");
    select(&mut cm, &[S::R(p(0, 0), p(1, 2)), S::R(p(2, 2), p(2, 0))]);
    assert_eq!(cm.doc.get_selection(None), "1234\n56\n90");
    // getSelection(false) returns the line array.
    assert_eq!(
        cm.doc
            .get_selections(None)
            .join("\n")
            .split('\n')
            .collect::<Vec<_>>()
            .join("|"),
        "1234|56|90"
    );
    assert_eq!(cm.doc.get_selections(None).join("|"), "1234\n56|90");
}

/// multi_test.js "setSelection"
#[test]
fn multi_set_selection() {
    let mut cm = cm("abcde\nabcde\nabcde\n");
    select(
        &mut cm,
        &[S::C(p(3, 0)), S::C(p(0, 0)), S::R(p(2, 5), p(1, 0))],
    );
    has_selections(&cm, &[(0, 0, 0, 0), (2, 5, 1, 0), (3, 0, 3, 0)]);
    cm.doc.set_selection(p(1, 2), Some(p(1, 1)), None);
    has_selections(&cm, &[(1, 2, 1, 1)]);
    select(
        &mut cm,
        &[
            S::R(p(1, 1), p(2, 4)),
            S::R(p(0, 0), p(1, 3)),
            S::C(p(3, 0)),
            S::C(p(2, 2)),
        ],
    );
    has_selections(&cm, &[(0, 0, 2, 4), (3, 0, 3, 0)]);
    cm.doc.set_selections(
        &[
            (p(0, 1), Some(p(0, 2))),
            (p(1, 1), Some(p(1, 2))),
            (p(2, 1), Some(p(2, 2))),
        ],
        Some(1),
        None,
    );
    eq_char_pos(cm.doc.get_cursor(Some("head")), p(1, 2));
    eq_char_pos(cm.doc.get_cursor(Some("anchor")), p(1, 1));
    eq_char_pos(cm.doc.get_cursor(Some("from")), p(1, 1));
    eq_char_pos(cm.doc.get_cursor(Some("to")), p(1, 2));
    cm.doc.set_cursor(p(1, 1), None);
    has_cursors(&cm, &[(1, 1)]);
}

/// multi_test.js "somethingSelected"
#[test]
fn multi_something_selected() {
    let mut cm = cm("123456789");
    select(&mut cm, &[S::C(p(0, 1)), S::R(p(0, 3), p(0, 5))]);
    assert!(cm.doc.something_selected());
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(0, 3)), S::C(p(0, 5))]);
    assert!(!cm.doc.something_selected());
}

/// multi_test.js "extendSelection"
#[test]
fn multi_extend_selection() {
    let mut cm = cm("1234\n1234\n1234");
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(1, 1)), S::C(p(2, 1))]);
    cm.doc.set_extending(true);
    cm.doc.extend_selections(&[p(0, 2), p(1, 0), p(2, 3)], None);
    has_selections(&cm, &[(0, 1, 0, 2), (1, 1, 1, 0), (2, 1, 2, 3)]);
    cm.doc.extend_selection(p(2, 4), Some(p(2, 0)), None);
    has_selections(&cm, &[(2, 4, 2, 0)]);
}

/// multi_test.js "addSelection"
#[test]
fn multi_add_selection() {
    let mut cm = cm("1234\n1234\n1234");
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(1, 1))]);
    cm.doc.add_selection(p(0, 0), Some(p(0, 4)), None);
    has_selections(&cm, &[(0, 0, 0, 4), (1, 1, 1, 1)]);
    cm.doc.add_selection(p(2, 2), None, None);
    has_selections(&cm, &[(0, 0, 0, 4), (1, 1, 1, 1), (2, 2, 2, 2)]);
}

/// multi_test.js "replaceSelection"
#[test]
fn multi_replace_selection() {
    let mut cm = cm("");
    let selections = [
        (p(0, 0), Some(p(0, 1))),
        (p(0, 2), Some(p(0, 3))),
        (p(0, 4), Some(p(0, 5))),
        (p(2, 1), Some(p(2, 4))),
        (p(2, 5), Some(p(2, 6))),
    ];
    let val = "123456\n123456\n123456";
    cm.set_value(val);
    cm.doc.set_selections(&selections, None, None);
    cm.doc.replace_selection("ab", Some("around"), None);
    assert_eq!(cm.get_value(), "ab2ab4ab6\n123456\n1ab5ab");
    has_selections(
        &cm,
        &[
            (0, 0, 0, 2),
            (0, 3, 0, 5),
            (0, 6, 0, 8),
            (2, 1, 2, 3),
            (2, 4, 2, 6),
        ],
    );
    cm.set_value(val);
    cm.doc.set_selections(&selections, None, None);
    cm.doc.replace_selection("", Some("around"), None);
    assert_eq!(cm.get_value(), "246\n123456\n15");
    has_selections(
        &cm,
        &[
            (0, 0, 0, 0),
            (0, 1, 0, 1),
            (0, 2, 0, 2),
            (2, 1, 2, 1),
            (2, 2, 2, 2),
        ],
    );
    cm.set_value(val);
    cm.doc.set_selections(&selections, None, None);
    cm.doc.replace_selection("X\nY\nZ", Some("around"), None);
    has_selections(
        &cm,
        &[
            (0, 0, 2, 1),
            (2, 2, 4, 1),
            (4, 2, 6, 1),
            (8, 1, 10, 1),
            (10, 2, 12, 1),
        ],
    );
    cm.doc.replace_selection("a", Some("around"), None);
    has_selections(
        &cm,
        &[
            (0, 0, 0, 1),
            (0, 2, 0, 3),
            (0, 4, 0, 5),
            (2, 1, 2, 2),
            (2, 3, 2, 4),
        ],
    );
    cm.doc.replace_selection("xy", Some("start"), None);
    has_selections(
        &cm,
        &[
            (0, 0, 0, 0),
            (0, 3, 0, 3),
            (0, 6, 0, 6),
            (2, 1, 2, 1),
            (2, 4, 2, 4),
        ],
    );
    cm.doc.replace_selection("z\nf", None, None);
    has_selections(
        &cm,
        &[
            (1, 1, 1, 1),
            (2, 1, 2, 1),
            (3, 1, 3, 1),
            (6, 1, 6, 1),
            (7, 1, 7, 1),
        ],
    );
    assert_eq!(
        cm.get_value(),
        "z\nfxy2z\nfxy4z\nfxy6\n123456\n1z\nfxy5z\nfxy"
    );
}

/// multi_test.js "indentSelection"
#[test]
fn multi_indent_selection() {
    let mut cm = cm("foo\nbar\nbaz");
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(1, 1))]);
    cm.indent_selection(Indent::By(4));
    assert_eq!(cm.get_value(), "    foo\n    bar\nbaz");
    select(&mut cm, &[S::C(p(0, 2)), S::C(p(0, 3)), S::C(p(0, 4))]);
    cm.indent_selection(Indent::By(-2));
    assert_eq!(cm.get_value(), "  foo\n    bar\nbaz");
    select(&mut cm, &[S::R(p(0, 0), p(1, 2)), S::R(p(1, 3), p(2, 0))]);
    cm.indent_selection(Indent::By(-2));
    assert_eq!(cm.get_value(), "foo\n  bar\nbaz");
}

/// multi_test.js "killLine"
#[test]
fn multi_kill_line() {
    let mut cm = cm("foo\nbar\nbaz");
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(0, 2)), S::C(p(1, 1))]);
    cm.exec_command("killLine");
    assert_eq!(cm.get_value(), "f\nb\nbaz");
    cm.exec_command("killLine");
    assert_eq!(cm.get_value(), "fbbaz");
    cm.set_value("foo\nbar\nbaz");
    select(&mut cm, &[S::C(p(0, 1)), S::R(p(0, 2), p(2, 1))]);
    cm.exec_command("killLine");
    assert_eq!(cm.get_value(), "faz");
}

/// multi_test.js "deleteLine"
#[test]
fn multi_delete_line() {
    let mut cm = cm("1\n2\n3\n4\n5\n6\n7");
    select(
        &mut cm,
        &[S::C(p(0, 0)), S::R(p(2, 0), p(0, 1)), S::C(p(4, 0))],
    );
    cm.exec_command("deleteLine");
    assert_eq!(cm.get_value(), "4\n6\n7");
    select(&mut cm, &[S::C(p(2, 1))]);
    cm.exec_command("deleteLine");
    assert_eq!(cm.get_value(), "4\n6\n");
}

/// multi_test.js "deleteH"
#[test]
fn multi_delete_h() {
    let mut cm = cm("foo bar baz\nabc def ghi\n");
    select(&mut cm, &[S::C(p(0, 4)), S::R(p(1, 4), p(1, 5))]);
    cm.exec_command("delWordAfter");
    assert_eq!(cm.get_value(), "foo bar baz\nabc ef ghi\n");
    cm.exec_command("delWordAfter");
    assert_eq!(cm.get_value(), "foo  baz\nabc  ghi\n");
    cm.exec_command("delCharBefore");
    cm.exec_command("delCharBefore");
    assert_eq!(cm.get_value(), "fo baz\nab ghi\n");
    select(&mut cm, &[S::C(p(0, 3)), S::C(p(0, 4)), S::C(p(0, 5))]);
    cm.exec_command("delWordAfter");
    assert_eq!(cm.get_value(), "fo \nab ghi\n");
}

/// multi_test.js "goLineStart"
#[test]
fn multi_go_line_start() {
    let mut cm = cm("foo\nbar\nbaz");
    select(&mut cm, &[S::C(p(0, 2)), S::C(p(0, 3)), S::C(p(1, 1))]);
    cm.exec_command("goLineStart");
    has_cursors(&cm, &[(0, 0), (1, 0)]);
    select(&mut cm, &[S::C(p(1, 1)), S::C(p(0, 1))]);
    cm.doc.set_extending(true);
    cm.exec_command("goLineStart");
    has_selections(&cm, &[(0, 1, 0, 0), (1, 1, 1, 0)]);
}

/// multi_test.js "moveV"
#[test]
fn multi_move_v() {
    let mut cm = cm("12345\n12345\n12345");
    select(&mut cm, &[S::C(p(0, 2)), S::C(p(1, 2))]);
    cm.exec_command("goLineDown");
    has_cursors(&cm, &[(1, 2), (2, 2)]);
    cm.exec_command("goLineUp");
    has_cursors(&cm, &[(0, 2), (1, 2)]);
    cm.exec_command("goLineUp");
    has_cursors(&cm, &[(0, 0), (0, 2)]);
    cm.exec_command("goLineUp");
    has_cursors(&cm, &[(0, 0)]);
    select(&mut cm, &[S::C(p(0, 2)), S::C(p(1, 2))]);
    cm.doc.set_extending(true);
    cm.exec_command("goLineDown");
    has_selections(&cm, &[(0, 2, 2, 2)]);
}

/// multi_test.js "moveH"
#[test]
fn multi_move_h() {
    let mut cm = cm("12345\n12345\n12345");
    select(
        &mut cm,
        &[S::C(p(0, 1)), S::C(p(0, 3)), S::C(p(0, 5)), S::C(p(2, 3))],
    );
    cm.exec_command("goCharRight");
    has_cursors(&cm, &[(0, 2), (0, 4), (1, 0), (2, 4)]);
    cm.exec_command("goCharLeft");
    has_cursors(&cm, &[(0, 1), (0, 3), (0, 5), (2, 3)]);
    for _ in 0..15 {
        cm.exec_command("goCharRight");
    }
    has_cursors(&cm, &[(2, 4), (2, 5)]);
}

/// multi_test.js "newlineAndIndent"
#[test]
fn multi_newline_and_indent() {
    let mut cm = cm_mode("x = [1];\ny = [2];", "javascript");
    select(&mut cm, &[S::C(p(0, 5)), S::C(p(1, 5))]);
    cm.exec_command("newlineAndIndent");
    has_cursors(&cm, &[(1, 2), (3, 2)]);
    assert_eq!(cm.get_value(), "x = [\n  1];\ny = [\n  2];");
    cm.exec_command("undo");
    assert_eq!(cm.get_value(), "x = [1];\ny = [2];");
    has_cursors(&cm, &[(0, 5), (1, 5)]);
    select(&mut cm, &[S::C(p(0, 5)), S::C(p(0, 6))]);
    cm.exec_command("newlineAndIndent");
    has_cursors(&cm, &[(1, 2), (2, 0)]);
    assert_eq!(cm.get_value(), "x = [\n  1\n];\ny = [2];");
}

/// multi_test.js "goDocStartEnd"
#[test]
fn multi_go_doc_start_end() {
    let mut cm = cm("abc\ndef");
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(1, 1))]);
    cm.exec_command("goDocStart");
    has_cursors(&cm, &[(0, 0)]);
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(1, 1))]);
    cm.exec_command("goDocEnd");
    has_cursors(&cm, &[(1, 3)]);
    select(&mut cm, &[S::C(p(0, 1)), S::C(p(1, 1))]);
    cm.doc.set_extending(true);
    cm.exec_command("goDocEnd");
    has_selections(&cm, &[(1, 1, 1, 3)]);
}

/// multi_test.js "selectionHistory"
#[test]
fn multi_selection_history() {
    let mut cm = cm("1 2 3");
    for i in 0..3 {
        cm.run_op(|cm| {
            cm.doc
                .add_selection(p(0, i * 2), Some(p(0, i * 2 + 1)), None)
        });
    }
    cm.exec_command("undoSelection");
    assert_eq!(cm.doc.get_selection(None), "1\n2");
    cm.exec_command("undoSelection");
    assert_eq!(cm.doc.get_selection(None), "1");
    cm.exec_command("undoSelection");
    assert_eq!(cm.doc.get_selection(None), "");
    eq_char_pos(cur(&cm), p(0, 0));
    cm.exec_command("redoSelection");
    assert_eq!(cm.doc.get_selection(None), "1");
    cm.exec_command("redoSelection");
    assert_eq!(cm.doc.get_selection(None), "1\n2");
    cm.exec_command("redoSelection");
    assert_eq!(cm.doc.get_selection(None), "1\n2\n3");
}

/// multi_test.js "selectionsMayTouch" (option `selectionsMayTouch: true`).
#[test]
fn multi_selections_may_touch() {
    let mut cm = cm("1234");
    cm.doc.attach_editor_semantics(EditorSemantics {
        selections_may_touch: true,
        ..Default::default()
    });
    select(&mut cm, &[S::C(p(0, 0)), S::C(p(0, 2))]);
    cm.doc.set_extending(true);
    cm.doc.extend_selections(&[p(0, 2), p(0, 4)], None);
    has_selections(&cm, &[(0, 0, 0, 2), (0, 2, 0, 4)]);
    cm.doc.extend_selections(&[p(0, 3), p(0, 4)], None);
    has_selections(&cm, &[(0, 0, 0, 4)]);
}
