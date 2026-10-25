use cm_core::Pos;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};

#[test]
fn smoke() {
    let mut cm = Editor::new(
        "foo bar\n  baz\nqux",
        Spec::Null,
        EditorOptions::default(),
        Metrics::test_grid(),
    );
    cm.doc.set_cursor(Pos::new(0, 0), None);
    cm.exec_command("goWordRight");
    assert_eq!(cm.doc.get_cursor(None).ch, 3);
    cm.exec_command("goLineDown");
    assert_eq!(
        (cm.doc.get_cursor(None).line, cm.doc.get_cursor(None).ch),
        (1, 3)
    );
    cm.exec_command("goLineStartSmart");
    assert_eq!(cm.doc.get_cursor(None).ch, 2);
    cm.type_text("x");
    assert_eq!(cm.get_value(), "foo bar\n  xbaz\nqux");
    cm.exec_command("newlineAndIndent");
    assert_eq!(cm.get_value(), "foo bar\n  x\n  baz\nqux");
    cm.exec_command("undo");
    assert_eq!(cm.get_value(), "foo bar\n  baz\nqux");
}
