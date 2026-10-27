//! The shared-engine entry points a browser host (cm-web) uses: `keyNames` for DOM
//! key codes, `refresh()` after font measurement (`Editor::set_metrics`), and a
//! save requested by Ctrl-S when keys reach the editor without `App::handle`.

use std::path::Path;

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::app::{App, AppOptions, MemClipboard};
use cm_view::fileio::MemFiles;
use cm_view::keymap::{key_event_from_dom, key_name};
use cm_view::{CoordSys, Editor, EditorOptions, Metrics};

#[test]
fn dom_key_codes_reach_the_key_maps() {
    let mut cm = Editor::new(
        "abc",
        Spec::from("text/plain"),
        EditorOptions::default(),
        Metrics::test_grid(),
    );
    cm.doc.set_cursor(Pos::new(0, 3), None);
    // keyCode 8 Backspace, 36 Home, 65 "A" with Ctrl (selectAll on pcDefault).
    let bs = key_event_from_dom(8, Some("Backspace"), [false; 4], false).unwrap();
    assert!(cm.key_down(&bs));
    assert_eq!(cm.get_value(), "ab");
    let home = key_event_from_dom(36, Some("Home"), [false; 4], false).unwrap();
    assert!(cm.key_down(&home));
    let c = cm.doc.get_cursor(None);
    assert_eq!((c.line, c.ch), (0, 0));
    let ctrl_a = key_event_from_dom(65, Some("KeyA"), [false, true, false, false], false).unwrap();
    assert_eq!(key_name(&ctrl_a, false, false).as_deref(), Some("Ctrl-A"));
    assert!(cm.key_down(&ctrl_a));
    assert_eq!(cm.doc.get_selection(None), "ab");
    // Numpad 4 (keyCode 100) is "4", which no default map binds.
    let num4 = key_event_from_dom(100, Some("Numpad4"), [false; 4], false).unwrap();
    assert_eq!(num4.key, "4");
    assert!(!cm.key_down(&num4));
}

#[test]
fn set_metrics_refreshes_geometry() {
    let opts = EditorOptions {
        line_numbers: true,
        ..Default::default()
    };
    let mut cm = Editor::new(
        &"x\n".repeat(120),
        Spec::from("text/plain"),
        opts,
        Metrics::test_grid(),
    );
    cm.set_size(400.0, 200.0);
    let g0 = cm.gutter_width;
    assert_eq!(cm.visible_lines(), (0, 9));
    assert_eq!(cm.cursor_coords(Pos::new(3, 1), CoordSys::Div).top, 60.0);
    assert_eq!(cm.doc_height(), 121.0 * 20.0);
    let m = Metrics::monospace(9.6328125, 19.0);
    cm.set_metrics(m.clone());
    assert_eq!(cm.metrics, m);
    let c = cm.cursor_coords(Pos::new(3, 1), CoordSys::Div);
    assert_eq!((c.left, c.top, c.bottom), (4.0 + 9.6328125, 57.0, 76.0));
    assert_eq!(cm.doc_height(), 121.0 * 19.0);
    // updateGutterSpace: three digits at the new char width.
    assert_eq!(cm.gutter_width, (3.0 * 9.6328125 + 9.0f64).ceil());
    assert_ne!(cm.gutter_width, g0);
    // The visible range follows the new line height.
    assert_eq!(cm.visible_lines(), (0, 10));
}

#[test]
fn save_requested_by_a_key_binding_without_app_handle() {
    let path = Path::new("/a.txt");
    let mut files = MemFiles::default();
    files.0.insert(path.to_path_buf(), b"one\r\n".to_vec());
    let mut app = App::open_with(
        Box::new(files),
        path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
    .unwrap();
    app.editor.doc.set_cursor(Pos::new(0, 3), None);
    app.editor.type_text("!");
    let ctrl_s = key_event_from_dom(83, Some("KeyS"), [false, true, false, false], false).unwrap();
    assert!(app.editor.key_down(&ctrl_s));
    assert!(
        !app.editor.doc.is_clean(None),
        "the binding only requests the save"
    );
    app.flush_save();
    assert!(app.editor.doc.is_clean(None));
    assert_eq!(app.files.read(path).unwrap().unwrap(), b"one!\r\n");
    // A second flush without a request writes nothing new.
    app.editor.type_text("?");
    app.flush_save();
    assert!(!app.editor.doc.is_clean(None));
}

#[test]
fn mac_command_keys_from_dom() {
    let opts = EditorOptions {
        mac: true,
        ..Default::default()
    };
    let mut cm = Editor::new("abc", Spec::from("text/plain"), opts, Metrics::test_grid());
    cm.doc.set_cursor(Pos::new(0, 3), None);
    cm.type_text("d");
    assert_eq!(cm.get_value(), "abcd");
    // keyCode 90 "Z" with metaKey: Cmd-Z (macDefault undo).
    let cmd_z = key_event_from_dom(90, Some("KeyZ"), [false, false, false, true], false).unwrap();
    assert_eq!(key_name(&cmd_z, false, false).as_deref(), Some("Cmd-Z"));
    assert!(cm.key_down(&cmd_z));
    assert_eq!(cm.get_value(), "abc");
    // Cmd-Home (goDocStart) and Shift-Cmd-Z (redo).
    let cmd_home =
        key_event_from_dom(36, Some("Home"), [false, false, false, true], false).unwrap();
    assert!(cm.key_down(&cmd_home));
    let c = cm.doc.get_cursor(None);
    assert_eq!((c.line, c.ch), (0, 0));
    let redo = key_event_from_dom(90, Some("KeyZ"), [true, false, false, true], false).unwrap();
    assert!(cm.key_down(&redo));
    assert_eq!(cm.get_value(), "abcd");
}
